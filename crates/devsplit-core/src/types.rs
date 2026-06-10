//! Tipos compartilhados do nucleo do devsplit.
//!
//! Este modulo e o CONTRATO entre o motor de proxy, a config, o DNS e a casca
//! Tauri. Tudo que cruza fronteiras de modulo vive aqui.

use std::cmp::Reverse;
use std::net::IpAddr;

use serde::Serialize;

/// Destino resolvido de uma requisicao que casou uma rota.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Upstream {
    /// Servico rodando localmente, ex.: 127.0.0.1:3000.
    Local { host: String, port: u16 },
    /// Passthrough p/ o gateway remoto real. O destino concreto (IP pinado +
    /// SNI) vem do [`PassthroughTarget`] do [`ProxyConfig`], nao daqui.
    Passthrough,
}

/// Uma regra de roteamento: casa Host + prefixo de path -> upstream.
#[derive(Clone, Debug)]
pub struct Route {
    /// FQDN a casar contra o header Host.
    pub host: String,
    /// Prefixo de path (ex.: "/transporte").
    pub prefix: String,
    /// Para onde vai quando casa.
    pub upstream: Upstream,
}

/// Tabela de rotas imutavel e pre-ordenada (prefixo mais longo primeiro).
/// Trocada atomicamente no reload via `ArcSwap` no motor.
#[derive(Clone, Debug, Default)]
pub struct RouteTable {
    entries: Vec<Route>,
}

impl RouteTable {
    /// Constroi a tabela ja ordenada por especificidade (prefixo mais longo
    /// primeiro), de forma que o PRIMEIRO match seja o mais especifico.
    pub fn new(mut entries: Vec<Route>) -> Self {
        entries.sort_by_key(|r| Reverse(r.prefix.len()));
        Self { entries }
    }

    pub fn entries(&self) -> &[Route] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Primeiro match vence (a tabela esta ordenada do mais especifico p/ o
    /// menos). Retorna `None` quando nenhuma rota local casa -> catch-all
    /// (passthrough) decide no chamador.
    pub fn match_route(&self, host: &str, path: &str) -> Option<&Route> {
        self.entries
            .iter()
            .find(|r| r.host == host && path.starts_with(r.prefix.as_str()))
    }
}

/// Alvo do passthrough (catch-all): conecta no IP REAL pinado e valida o cert
/// remoto contra `sni` (FQDN). Nunca desabilita verificacao.
#[derive(Clone, Debug)]
pub struct PassthroughTarget {
    /// FQDN usado como SNI E alvo da validacao do cert remoto.
    pub sni: String,
    /// FQDN a resolver via DNS DIRETO (ignora /etc/hosts) p/ achar o IP real.
    pub resolve_host: String,
    /// IP fixo opcional; se `Some`, dispensa a resolucao DNS.
    pub fixed_ip: Option<IpAddr>,
    /// Porta do gateway remoto (tipicamente 443).
    pub port: u16,
    /// Validar o cert remoto. DEVE ser `true` em stage real.
    pub verify: bool,
}

/// Snapshot de configuracao runtime que o motor de proxy consome. Trocado
/// atomicamente no reload.
#[derive(Clone, Debug)]
pub struct ProxyConfig {
    /// Endereco de bind (ex.: "0.0.0.0").
    pub listen_host: String,
    /// Porta de bind (ex.: 443).
    pub listen_port: u16,
    /// FQDN interceptado (o que o front aponta).
    pub intercept_host: String,
    /// Rotas locais (o catch-all = passthrough fica implicito).
    pub routes: RouteTable,
    /// Destino do passthrough.
    pub passthrough: PassthroughTarget,
    /// Hosts adicionais interceptados ao mesmo tempo (ex.: ws.stage, cdn.stage),
    /// cada um com rotas + passthrough proprios. Vazio = single-host (padrao).
    pub extra_hosts: Vec<HostConfig>,
}

/// Config de um host adicional interceptado (multi-host).
#[derive(Clone, Debug)]
pub struct HostConfig {
    pub host: String,
    pub routes: RouteTable,
    pub passthrough: PassthroughTarget,
}

/// Decisao tomada para uma requisicao (p/ o log/UI).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "target", rename_all = "lowercase")]
pub enum Decision {
    /// Encaminhada p/ um servico local (string = "127.0.0.1:3000").
    Local(String),
    /// Passada adiante p/ o gateway remoto real.
    Passthrough,
}

/// Par header (nome, valor) — ja redatado quando sensivel.
#[derive(Clone, Debug, Serialize)]
pub struct HeaderPair {
    pub name: String,
    pub value: String,
}

/// Evento de trafego: registro completo de uma requisicao (consumido pela UI).
/// Bodies sao capturados so quando pequenos e nao-streaming; senao ficam `None`.
#[derive(Clone, Debug, Serialize)]
pub struct TrafficEvent {
    /// Id incremental (chave do detalhe no ring buffer).
    pub id: u64,
    /// Epoch millis.
    pub ts: u64,
    pub method: String,
    pub host: String,
    pub path: String,
    pub decision: Decision,
    /// Status HTTP da resposta, quando conhecido.
    pub status: Option<u16>,
    /// Latencia em ms (ate a resposta), quando conhecida.
    pub latency_ms: Option<u64>,
    pub req_headers: Vec<HeaderPair>,
    /// Preview do corpo da request (utf8 lossy), ou None se nao capturado.
    pub req_body: Option<String>,
    pub req_body_truncated: bool,
    pub req_size: Option<u64>,
    pub resp_headers: Vec<HeaderPair>,
    pub resp_body: Option<String>,
    pub resp_body_truncated: bool,
    pub resp_size: Option<u64>,
}
