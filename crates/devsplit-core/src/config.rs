//! Parsing/validacao do `devsplit.yaml` + conversao p/ [`crate::types::ProxyConfig`].
//!
//! Os DTOs aqui sao o espelho serde do arquivo de config; [`to_proxy_config`]
//! resolve perfis (com `extends`) e ambientes p/ o snapshot runtime do motor.

use std::collections::HashMap;
use std::fs;
use std::net::IpAddr;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use crate::types::{HostConfig, PassthroughTarget, ProxyConfig, Route, RouteTable, Upstream};

fn default_true() -> bool {
    true
}

/// Raiz do `devsplit.yaml`.
#[derive(Debug, Clone, Deserialize)]
pub struct DevsplitConfig {
    #[serde(default)]
    pub version: u32,
    pub upstream: UpstreamSpec,
    /// Hosts extras interceptados ao mesmo tempo (passthrough-only). Opcional.
    #[serde(default)]
    pub extra_upstreams: Option<Vec<UpstreamSpec>>,
    #[serde(default)]
    pub tls: Option<TlsSpec>,
    #[serde(default)]
    pub profiles: HashMap<String, ProfileSpec>,
    #[serde(default)]
    pub environments: HashMap<String, EnvironmentSpec>,
    #[serde(default)]
    pub cors: Option<CorsSpec>,
    #[serde(default)]
    pub defaults: Option<DefaultsSpec>,
}

/// Gateway remoto a interceptar.
#[derive(Debug, Clone, Deserialize)]
pub struct UpstreamSpec {
    pub host: String,
    pub passthrough: PassthroughSpec,
    #[serde(default)]
    pub kind: Option<String>,
}

/// Destino do passthrough (catch-all).
#[derive(Debug, Clone, Deserialize)]
pub struct PassthroughSpec {
    pub resolve: String,
    /// `ip:port` opcional; quando presente dispensa a resolucao DNS.
    #[serde(default)]
    pub address: Option<String>,
    pub sni: String,
    #[serde(default = "default_true")]
    pub verify: bool,
}

/// Bloco `tls`.
#[derive(Debug, Clone, Deserialize)]
pub struct TlsSpec {
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub leaf_max_days: Option<u32>,
}

/// Um perfil de roteamento. `extends` herda rotas de outro perfil.
#[derive(Debug, Clone, Deserialize)]
pub struct ProfileSpec {
    #[serde(default)]
    pub extends: Option<String>,
    #[serde(default)]
    pub routes: Vec<RouteSpec>,
}

/// Uma rota: prefixo -> target local; `also` adiciona prefixos extras p/ o
/// MESMO target.
#[derive(Debug, Clone, Deserialize)]
pub struct RouteSpec {
    pub prefix: String,
    /// Ex.: `http://127.0.0.1:3000` (o esquema e ignorado).
    pub target: String,
    #[serde(default)]
    pub also: Vec<String>,
}

/// Override de ambiente: substitui o upstream.
#[derive(Debug, Clone, Deserialize)]
pub struct EnvironmentSpec {
    pub upstream: UpstreamSpec,
}

/// Bloco `cors`.
#[derive(Debug, Clone, Deserialize)]
pub struct CorsSpec {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub allow_origins: Vec<String>,
    #[serde(default)]
    pub allow_credentials: bool,
}

/// Bloco `defaults`.
#[derive(Debug, Clone, Deserialize)]
pub struct DefaultsSpec {
    /// Endereco de bind, ex.: `0.0.0.0:443`.
    #[serde(default)]
    pub listen: Option<String>,
}

/// Faz o parse de uma string YAML.
pub fn parse(s: &str) -> Result<DevsplitConfig> {
    serde_yaml::from_str(s).context("parsear devsplit.yaml")
}

/// Le e faz o parse de um arquivo `devsplit.yaml`.
pub fn load(path: &Path) -> Result<DevsplitConfig> {
    let s = fs::read_to_string(path).with_context(|| format!("ler {path:?}"))?;
    parse(&s)
}

/// Nomes dos perfis definidos, com `default` primeiro (se existir) e o resto
/// em ordem alfabetica — p/ a UI listar de forma estavel.
pub fn profile_names(cfg: &DevsplitConfig) -> Vec<String> {
    let mut names: Vec<String> = cfg.profiles.keys().cloned().collect();
    names.sort();
    if let Some(pos) = names.iter().position(|n| n == "default") {
        let d = names.remove(pos);
        names.insert(0, d);
    }
    names
}

/// Divide um `host:port` (ou `[ipv6]:port`) em (host, port).
fn split_host_port(s: &str) -> Result<(String, u16)> {
    let (host, port) = s
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("endereco sem porta: `{s}`"))?;
    let port: u16 = port
        .parse()
        .with_context(|| format!("porta invalida em `{s}`"))?;
    // Remove colchetes de IPv6 literal, se houver.
    let host = host.trim_start_matches('[').trim_end_matches(']');
    Ok((host.to_string(), port))
}

/// Faz o parse de um target tipo `http://127.0.0.1:3000` -> (host, port),
/// ignorando o esquema.
fn parse_target(target: &str) -> Result<(String, u16)> {
    let authority = target.split_once("://").map(|(_, a)| a).unwrap_or(target);
    // descarta path/query eventuais
    let authority = authority
        .split(['/', '?'])
        .next()
        .unwrap_or(authority);
    split_host_port(authority)
}

/// Monta o [`PassthroughTarget`] a partir de um [`PassthroughSpec`].
fn build_passthrough(pt: &PassthroughSpec) -> Result<PassthroughTarget> {
    let (fixed_ip, port) = match &pt.address {
        Some(addr) => {
            let (ip_s, port) = split_host_port(addr)
                .with_context(|| format!("address de passthrough invalido: `{addr}`"))?;
            let ip: IpAddr = ip_s
                .parse()
                .with_context(|| format!("IP invalido no passthrough.address: `{ip_s}`"))?;
            (Some(ip), port)
        }
        None => (None, 443),
    };
    Ok(PassthroughTarget {
        sni: pt.sni.clone(),
        resolve_host: pt.resolve.clone(),
        fixed_ip,
        port,
        verify: pt.verify,
    })
}

/// Resolve as rotas de um perfil aplicando `extends` recursivamente (pai
/// primeiro, depois filho). Detecta ciclos.
fn collect_routes<'a>(
    cfg: &'a DevsplitConfig,
    name: &str,
    chain: &mut Vec<String>,
) -> Result<Vec<RouteSpec>> {
    if chain.iter().any(|n| n == name) {
        bail!("ciclo de `extends` envolvendo o perfil `{name}`");
    }
    let profile = cfg
        .profiles
        .get(name)
        .ok_or_else(|| anyhow!("perfil `{name}` nao encontrado"))?;
    chain.push(name.to_string());

    let mut routes = Vec::new();
    if let Some(parent) = &profile.extends {
        routes.extend(collect_routes(cfg, parent, chain)?);
    }
    routes.extend(profile.routes.clone());
    chain.pop();
    Ok(routes)
}

/// Converte a config para o snapshot runtime [`ProxyConfig`].
///
/// - resolve `profile` (com `extends` recursivo);
/// - cada rota (e cada item de `also`) vira uma [`Route`] p/ o MESMO
///   [`Upstream::Local`] derivado do `target`;
/// - monta o [`PassthroughTarget`] do `upstream.passthrough`;
/// - `listen_host`/`listen_port` vem de `defaults.listen` (default
///   `0.0.0.0:443`); `intercept_host` = `upstream.host`;
/// - se `environment` for dado, o upstream do ambiente sobrescreve o base.
pub fn to_proxy_config(
    cfg: &DevsplitConfig,
    profile: &str,
    environment: Option<&str>,
) -> Result<ProxyConfig> {
    // Upstream efetivo (override por ambiente).
    let upstream = match environment {
        Some(env) => {
            &cfg.environments
                .get(env)
                .ok_or_else(|| anyhow!("ambiente `{env}` nao encontrado"))?
                .upstream
        }
        None => &cfg.upstream,
    };

    // Rotas resolvidas.
    let mut chain = Vec::new();
    let route_specs = collect_routes(cfg, profile, &mut chain)?;

    let mut routes: Vec<Route> = Vec::new();
    for rs in &route_specs {
        let (thost, tport) = parse_target(&rs.target)
            .with_context(|| format!("target invalido na rota `{}`", rs.prefix))?;
        let make = || Upstream::Local {
            host: thost.clone(),
            port: tport,
        };
        routes.push(Route {
            host: upstream.host.clone(),
            prefix: rs.prefix.clone(),
            upstream: make(),
        });
        for also in &rs.also {
            routes.push(Route {
                host: upstream.host.clone(),
                prefix: also.clone(),
                upstream: make(),
            });
        }
    }

    // Passthrough.
    // Passthrough do host primario.
    let passthrough = build_passthrough(&upstream.passthrough)?;

    // Hosts adicionais interceptados (passthrough-only por enquanto).
    let extra_hosts = cfg
        .extra_upstreams
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|u| {
            Ok::<_, anyhow::Error>(HostConfig {
                host: u.host.clone(),
                routes: RouteTable::new(Vec::new()),
                passthrough: build_passthrough(&u.passthrough)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    // Bind.
    let listen = cfg
        .defaults
        .as_ref()
        .and_then(|d| d.listen.clone())
        .unwrap_or_else(|| "0.0.0.0:443".to_string());
    let (listen_host, listen_port) =
        split_host_port(&listen).with_context(|| format!("defaults.listen invalido: `{listen}`"))?;

    Ok(ProxyConfig {
        listen_host,
        listen_port,
        intercept_host: upstream.host.clone(),
        routes: RouteTable::new(routes),
        passthrough,
        extra_hosts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const YAML: &str = r#"
version: 1
upstream:
  host: api.stage.acme.com
  passthrough: { resolve: api.stage.acme.com, address: 203.0.113.40:443, sni: api.stage.acme.com, verify: true }
  kind: stage
tls: { provider: mkcert, leaf_max_days: 825 }
profiles:
  default:
    routes:
      - { prefix: /transporte, target: "http://127.0.0.1:3000", also: [/socket.io] }
      - { prefix: /auth, target: "http://127.0.0.1:3001" }
cors: { enabled: true, allow_origins: ["https://app.stage.acme.com"], allow_credentials: true }
"#;

    fn local(host: &str, port: u16) -> Upstream {
        Upstream::Local {
            host: host.to_string(),
            port,
        }
    }

    #[test]
    fn parse_and_convert_default_profile() {
        let cfg = parse(YAML).unwrap();
        let pc = to_proxy_config(&cfg, "default", None).unwrap();

        assert_eq!(pc.intercept_host, "api.stage.acme.com");
        assert_eq!(pc.listen_host, "0.0.0.0");
        assert_eq!(pc.listen_port, 443);

        let host = "api.stage.acme.com";
        let find = |prefix: &str| {
            pc.routes
                .entries()
                .iter()
                .find(|r| r.prefix == prefix && r.host == host)
                .map(|r| r.upstream.clone())
        };

        assert_eq!(find("/transporte"), Some(local("127.0.0.1", 3000)));
        // `also` -> mesmo upstream
        assert_eq!(find("/socket.io"), Some(local("127.0.0.1", 3000)));
        assert_eq!(find("/auth"), Some(local("127.0.0.1", 3001)));

        // passthrough
        assert_eq!(pc.passthrough.sni, "api.stage.acme.com");
        assert_eq!(pc.passthrough.resolve_host, "api.stage.acme.com");
        assert_eq!(
            pc.passthrough.fixed_ip,
            Some("203.0.113.40".parse().unwrap())
        );
        assert_eq!(pc.passthrough.port, 443);
        assert!(pc.passthrough.verify);
    }

    #[test]
    fn extends_merges_routes() {
        let yaml = r#"
upstream:
  host: api.example.com
  passthrough: { resolve: api.example.com, sni: api.example.com }
profiles:
  base:
    routes:
      - { prefix: /auth, target: "http://127.0.0.1:3001" }
  full:
    extends: base
    routes:
      - { prefix: /app, target: "http://127.0.0.1:3000" }
"#;
        let cfg = parse(yaml).unwrap();
        let pc = to_proxy_config(&cfg, "full", None).unwrap();
        let prefixes: Vec<&str> = pc.routes.entries().iter().map(|r| r.prefix.as_str()).collect();
        assert!(prefixes.contains(&"/auth"), "herda rota do pai");
        assert!(prefixes.contains(&"/app"), "mantem rota propria");
        // passthrough sem address -> sem fixed_ip, porta 443 default
        assert_eq!(pc.passthrough.fixed_ip, None);
        assert_eq!(pc.passthrough.port, 443);
    }

    #[test]
    fn environment_overrides_upstream() {
        let yaml = r#"
upstream:
  host: api.stage.acme.com
  passthrough: { resolve: api.stage.acme.com, sni: api.stage.acme.com }
environments:
  prod:
    upstream:
      host: api.prod.acme.com
      passthrough: { resolve: api.prod.acme.com, address: 198.51.100.10:8443, sni: api.prod.acme.com }
profiles:
  default:
    routes:
      - { prefix: /auth, target: "http://127.0.0.1:3001" }
"#;
        let cfg = parse(yaml).unwrap();
        let pc = to_proxy_config(&cfg, "default", Some("prod")).unwrap();
        assert_eq!(pc.intercept_host, "api.prod.acme.com");
        assert_eq!(pc.passthrough.sni, "api.prod.acme.com");
        assert_eq!(pc.passthrough.port, 8443);
        assert_eq!(pc.passthrough.fixed_ip, Some("198.51.100.10".parse().unwrap()));
        // rota mantem host do upstream efetivo
        assert!(pc.routes.entries().iter().all(|r| r.host == "api.prod.acme.com"));
    }
}
