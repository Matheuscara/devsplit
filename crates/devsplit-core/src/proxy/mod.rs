//! Motor de proxy transparente.
//!
//! - Termina TLS local (rustls) na :443.
//! - Roteia por Host + PathPrefix (prefixo mais especifico ganha, em fronteira
//!   de segmento). Host desconhecido -> 421; sem Host -> 400.
//! - Match local -> encaminha p/ 127.0.0.1:porta (HTTP/1.1 claro).
//! - Sem match -> passthrough p/ o gateway remoto: conecta no IP REAL pinado e
//!   valida o cert contra `sni` (FQDN). NUNCA desabilita verificacao.
//! - Conexoes upstream reaproveitadas via pool simples (por destino).
//! - WebSocket: passthrough transparente via `hyper::upgrade::on` +
//!   `copy_bidirectional` (byte-copy bruto apos o 101).
//! - CORS: so quando `cfg.cors.enabled` (allowlist); preflight respondido na borda.
//! - Hot-reload: a config vem de um `ArcSwap`; conexoes em voo nao caem.

use std::collections::HashMap;
use std::convert::Infallible;
use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use arc_swap::ArcSwap;
use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::body::Incoming;
use hyper::client::conn::http1::SendRequest;
use hyper::header::{
    HeaderName, HeaderValue, ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_HEADERS,
    ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_REQUEST_HEADERS,
    ACCESS_CONTROL_REQUEST_METHOD, CONNECTION, CONTENT_ENCODING, CONTENT_LENGTH, HOST, ORIGIN,
    UPGRADE, VARY,
};
use hyper::{HeaderMap, Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::types::{
    CorsPolicy, Decision, HeaderPair, PassthroughTarget, ProxyConfig, TrafficEvent, Upstream,
};

/// Config compartilhada com troca atomica (hot-reload).
pub type SharedConfig = Arc<ArcSwap<ProxyConfig>>;

type BoxedBody = BoxBody<Bytes, hyper::Error>;

static CRYPTO: Once = Once::new();

/// Instala o provider cripto `ring` como default do rustls (idempotente).
/// Deve rodar antes de qualquer `ServerConfig`/`ClientConfig::builder()`.
pub fn ensure_crypto_provider() {
    CRYPTO.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn empty_body() -> BoxedBody {
    Empty::<Bytes>::new().map_err(|never| match never {}).boxed()
}

fn full_body<T: Into<Bytes>>(b: T) -> BoxedBody {
    Full::new(b.into()).map_err(|never| match never {}).boxed()
}

/// Teto p/ bufferizar um corpo (request ou response) e captura-lo. Acima disso,
/// ou sem Content-Length (streaming/SSE), o corpo passa direto sem captura.
const CAP: u64 = 256 * 1024;
/// Teto do preview guardado (evita eventos gigantes mesmo dentro do CAP).
const PREVIEW_CAP: usize = 64 * 1024;
/// Teto p/ conectar (TCP + TLS + handshake HTTP) num upstream.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Quanto uma conexao devolvida ao pool pode esperar p/ ficar pronta.
const CHECKIN_TIMEOUT: Duration = Duration::from_secs(30);
/// Maximo de conexoes ociosas por destino.
const POOL_MAX_IDLE: usize = 8;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static DROPPED: AtomicU64 = AtomicU64::new(0);

/// Total de [`TrafficEvent`]s descartados (canal cheio/fechado) desde o start.
pub fn dropped_events() -> u64 {
    DROPPED.load(Ordering::Relaxed)
}

fn emit(tx: &Option<mpsc::Sender<TrafficEvent>>, ev: TrafficEvent) {
    if let Some(tx) = tx {
        if tx.try_send(ev).is_err() {
            let n = DROPPED.fetch_add(1, Ordering::Relaxed) + 1;
            tracing::warn!(dropped_total = n, "devsplit: evento de trafego descartado");
        }
    }
}

/// Lê o Content-Length, se presente e numerico.
fn content_length(headers: &HeaderMap) -> Option<u64> {
    headers.get(CONTENT_LENGTH)?.to_str().ok()?.parse().ok()
}

/// Converte headers em pares (nome, valor), REDIGINDO os sensiveis (cookie,
/// set-cookie, x-api-key, *secret*, *token*). `authorization` fica visivel de
/// proposito — e o que o painel de Sessao/JWT inspeciona.
fn redact_headers(headers: &HeaderMap) -> Vec<HeaderPair> {
    headers
        .iter()
        .map(|(name, value)| {
            let n = name.as_str();
            let sensitive = matches!(n, "cookie" | "set-cookie" | "x-api-key" | "proxy-authorization")
                || n.contains("secret")
                || n.contains("token");
            let v = if sensitive {
                "<redacted>".to_string()
            } else {
                value.to_str().unwrap_or("<binario>").to_string()
            };
            HeaderPair { name: n.to_string(), value: v }
        })
        .collect()
}

/// Preview utf8-lossy de um corpo, truncado em [`PREVIEW_CAP`].
fn preview(bytes: &[u8]) -> (String, bool) {
    if bytes.len() > PREVIEW_CAP {
        (String::from_utf8_lossy(&bytes[..PREVIEW_CAP]).into_owned(), true)
    } else {
        (String::from_utf8_lossy(bytes).into_owned(), false)
    }
}

/// Preview respeitando `Content-Encoding`: gzip/deflate/br sao decodificados
/// (so ate PREVIEW_CAP+1 bytes, evitando bomba de descompressao). Encoding
/// desconhecido ou falha de decode -> `None` (nunca despeja bytes comprimidos).
fn body_preview(headers: &HeaderMap, bytes: &[u8]) -> Option<(String, bool)> {
    let enc = headers
        .get(CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if enc.is_empty() || enc == "identity" {
        return Some(preview(bytes));
    }
    let limit = PREVIEW_CAP as u64 + 1;
    let mut out = Vec::new();
    let ok = match enc.as_str() {
        "gzip" | "x-gzip" => flate2::read::MultiGzDecoder::new(bytes)
            .take(limit)
            .read_to_end(&mut out)
            .is_ok(),
        "deflate" => {
            // RFC diz zlib; alguns servidores mandam deflate cru.
            flate2::read::ZlibDecoder::new(bytes).take(limit).read_to_end(&mut out).is_ok() || {
                out.clear();
                flate2::read::DeflateDecoder::new(bytes)
                    .take(limit)
                    .read_to_end(&mut out)
                    .is_ok()
            }
        }
        "br" => brotli_decompressor::Decompressor::new(bytes, 4096)
            .take(limit)
            .read_to_end(&mut out)
            .is_ok(),
        _ => false,
    };
    ok.then(|| preview(&out))
}

/// `ClientConfig` rustls p/ o passthrough: valida o cert remoto contra o SNI
/// usando os roots da Mozilla (webpki-roots). Sem `dangerous()`, SNI ligado.
pub fn build_client_config() -> Arc<ClientConfig> {
    ensure_crypto_provider();
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(cfg)
}

// ---- pool de conexoes upstream ----

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum PoolKey {
    Local(String, u16),
    Passthrough(IpAddr, u16, String),
}

/// Pool minimo de `SendRequest` HTTP/1 ociosos por destino.
#[derive(Default)]
struct Pool {
    idle: Mutex<HashMap<PoolKey, Vec<SendRequest<BoxedBody>>>>,
}

impl Pool {
    /// Retira uma conexao viva e pronta, descartando as mortas.
    async fn checkout(&self, key: &PoolKey) -> Option<SendRequest<BoxedBody>> {
        let mut idle = self.idle.lock().await;
        let list = idle.get_mut(key)?;
        while let Some(s) = list.pop() {
            if !s.is_closed() && s.is_ready() {
                return Some(s);
            }
        }
        None
    }

    /// Devolve a conexao quando ela ficar pronta de novo (resposta anterior
    /// consumida). Conexao com erro/fechada ou que nao fica pronta e dropada.
    fn checkin(self: &Arc<Self>, key: PoolKey, mut sender: SendRequest<BoxedBody>) {
        let pool = self.clone();
        tokio::spawn(async move {
            match tokio::time::timeout(CHECKIN_TIMEOUT, sender.ready()).await {
                Ok(Ok(())) if !sender.is_closed() => {
                    let mut idle = pool.idle.lock().await;
                    let list = idle.entry(key).or_default();
                    if list.len() < POOL_MAX_IDLE {
                        list.push(sender);
                    }
                }
                _ => {}
            }
        });
    }
}

/// Handle do servidor em execucao. `abort()` derruba o accept loop (libera :443).
pub struct ProxyHandle {
    task: tokio::task::JoinHandle<()>,
    pub local_addr: SocketAddr,
}

impl ProxyHandle {
    /// Aborta o accept loop (nao espera). Mantido por conveniencia.
    pub fn abort(&self) {
        self.task.abort();
    }

    /// Derruba o servidor e ESPERA a task encerrar, garantindo que o listener
    /// foi dropado e a :443 liberada antes de retornar (essencial p/ religar).
    pub async fn shutdown(self) {
        self.task.abort();
        let _ = self.task.await;
    }
}

/// Estado compartilhado por todas as requisicoes de um `serve`.
struct Ctx {
    config: SharedConfig,
    client_config: Arc<ClientConfig>,
    traffic_tx: Option<mpsc::Sender<TrafficEvent>>,
    pool: Arc<Pool>,
}

/// Sobe o proxy: binda TLS em `listen`, serve ate `abort()`. `config` pode ser
/// trocada a quente (`ArcSwap::store`). `traffic_tx` recebe eventos p/ a UI.
pub async fn serve(
    listen: SocketAddr,
    server_config: Arc<ServerConfig>,
    config: SharedConfig,
    traffic_tx: Option<mpsc::Sender<TrafficEvent>>,
) -> std::io::Result<ProxyHandle> {
    ensure_crypto_provider();
    let listener = TcpListener::bind(listen).await?;
    let local_addr = listener.local_addr()?;
    let acceptor = TlsAcceptor::from(server_config);
    let ctx = Arc::new(Ctx {
        config,
        client_config: build_client_config(),
        traffic_tx,
        pool: Arc::new(Pool::default()),
    });

    let task = tokio::spawn(async move {
        loop {
            let (tcp, peer) = match listener.accept().await {
                Ok(x) => x,
                Err(e) => {
                    // Ex.: EMFILE. Evita spin de CPU enquanto o erro persiste.
                    tracing::warn!("devsplit: accept falhou: {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
            let acceptor = acceptor.clone();
            let ctx = ctx.clone();
            tokio::spawn(async move {
                let tls = match acceptor.accept(tcp).await {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let io = TokioIo::new(tls);
                let service =
                    hyper::service::service_fn(move |req| handle(req, peer, ctx.clone()));
                let _ = hyper::server::conn::http1::Builder::new()
                    .timer(TokioTimer::new())
                    .serve_connection(io, service)
                    .with_upgrades()
                    .await;
            });
        }
    });

    Ok(ProxyHandle { task, local_addr })
}

/// Evento leve p/ respostas geradas na borda (sem upstream, sem body).
fn edge_event(
    req: &Request<Incoming>,
    host: &str,
    status: StatusCode,
    started: Instant,
) -> TrafficEvent {
    TrafficEvent {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        ts: now_ms(),
        method: req.method().to_string(),
        host: host.to_string(),
        path: req.uri().path().to_string(),
        query: req.uri().query().map(String::from),
        decision: Decision::Edge,
        status: Some(status.as_u16()),
        latency_ms: Some(started.elapsed().as_millis() as u64),
        req_headers: redact_headers(req.headers()),
        req_body: None,
        req_body_truncated: false,
        req_size: content_length(req.headers()),
        resp_headers: Vec::new(),
        resp_body: None,
        resp_body_truncated: false,
        resp_size: None,
    }
}

fn text_response(status: StatusCode, msg: &'static str) -> Response<BoxedBody> {
    Response::builder()
        .status(status)
        .body(full_body(msg))
        .unwrap_or_else(|_| Response::new(empty_body()))
}

/// Handler por requisicao. Valida o Host (multi-host), decide local vs
/// passthrough, captura headers/body (redatado, so pequenos), cronometra a
/// latencia e emite o [`TrafficEvent`] completo.
async fn handle(
    req: Request<Incoming>,
    peer: SocketAddr,
    ctx: Arc<Ctx>,
) -> Result<Response<BoxedBody>, Infallible> {
    let started = Instant::now();
    let cfg = ctx.config.load_full();
    let tx = &ctx.traffic_tx;

    // Host obrigatorio (HTTP/1.1). Ausente/ilegivel -> 400 na borda.
    let host = match req
        .headers()
        .get(HOST)
        .and_then(|h| h.to_str().ok())
        .map(|h| strip_port(h.trim()).to_ascii_lowercase())
        .filter(|h| !h.is_empty())
    {
        Some(h) => h,
        None => {
            tracing::warn!("devsplit: requisicao sem Host valido -> 400");
            emit(tx, edge_event(&req, "", StatusCode::BAD_REQUEST, started));
            return Ok(text_response(StatusCode::BAD_REQUEST, "devsplit: header Host ausente ou invalido"));
        }
    };

    // Anti-loop.
    if req.headers().contains_key("x-devsplit-hop") {
        tracing::warn!(%host, "devsplit: loop detectado -> 508");
        emit(tx, edge_event(&req, &host, StatusCode::LOOP_DETECTED, started));
        return Ok(text_response(
            StatusCode::LOOP_DETECTED,
            "devsplit: loop detectado (a requisicao reentrou no proxy)",
        ));
    }

    // Multi-host: escolhe rotas + passthrough pelo Host (primario ou extra).
    // Host fora da config -> 421 (nunca cai silenciosamente no primario).
    let (routes, passthrough) = if host.eq_ignore_ascii_case(&cfg.intercept_host) {
        (&cfg.routes, &cfg.passthrough)
    } else if let Some(hc) = cfg.extra_hosts.iter().find(|h| h.host.eq_ignore_ascii_case(&host)) {
        (&hc.routes, &hc.passthrough)
    } else {
        tracing::warn!(%host, "devsplit: host desconhecido -> 421");
        emit(tx, edge_event(&req, &host, StatusCode::MISDIRECTED_REQUEST, started));
        return Ok(text_response(
            StatusCode::MISDIRECTED_REQUEST,
            "devsplit: host nao interceptado por esta instancia",
        ));
    };

    let origin = req
        .headers()
        .get(ORIGIN)
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);

    // Preflight CORS na borda so com politica ligada; senao vai ao upstream.
    if cfg.cors.enabled && is_preflight(&req) {
        let resp = cors_preflight(&req, origin.as_deref(), &cfg.cors);
        emit(tx, edge_event(&req, &host, resp.status(), started));
        return Ok(resp);
    }

    let path = req.uri().path().to_string();
    let query = req.uri().query().map(String::from);
    let method = req.method().clone();
    let matched = routes.match_route(&host, &path).map(|r| r.upstream.clone());

    // --- captura da request (headers sempre; body so se pequeno e nao-upgrade) ---
    let upgrade = is_upgrade(req.headers().get(CONNECTION), req.headers().get(UPGRADE));
    let req_headers = redact_headers(req.headers());
    let req_clen = content_length(req.headers());
    let (mut parts, incoming) = req.into_parts();
    let (req_body, req_body_truncated, req_size, fwd_body): (Option<String>, bool, Option<u64>, BoxedBody) =
        if !upgrade && req_clen.is_some_and(|n| n <= CAP) {
            match incoming.collect().await {
                Ok(c) => {
                    let bytes = c.to_bytes();
                    let (p, t) = body_preview(&parts.headers, &bytes).unzip();
                    (p, t.unwrap_or(false), Some(bytes.len() as u64), full_body(bytes))
                }
                Err(_) => {
                    // Nunca encaminhar Content-Length antigo com corpo vazio.
                    let resp = bad_gateway_msg("falha lendo corpo da requisicao");
                    let mut ev = TrafficEvent {
                        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
                        ts: now_ms(),
                        method: method.to_string(),
                        host,
                        path,
                        query,
                        decision: Decision::Edge,
                        status: Some(resp.status().as_u16()),
                        latency_ms: Some(started.elapsed().as_millis() as u64),
                        req_headers,
                        req_body: None,
                        req_body_truncated: false,
                        req_size: req_clen,
                        resp_headers: Vec::new(),
                        resp_body: None,
                        resp_body_truncated: false,
                        resp_size: None,
                    };
                    ev.resp_headers = redact_headers(resp.headers());
                    emit(tx, ev);
                    return Ok(resp);
                }
            }
        } else {
            (None, false, req_clen, incoming.boxed())
        };

    let fwd_host = if host.eq_ignore_ascii_case(&cfg.intercept_host) {
        cfg.intercept_host.clone()
    } else {
        host.clone()
    };
    prepare_forward_headers(&mut parts.headers, upgrade, peer.ip(), &fwd_host);
    let fwd_req = Request::from_parts(parts, fwd_body);

    // --- encaminhamento ---
    let (decision, result) = match matched {
        Some(Upstream::Local { host: lh, port }) => (
            Decision::Local(format!("{lh}:{port}")),
            forward_local(fwd_req, upgrade, &lh, port, &ctx.pool).await,
        ),
        _ => (
            Decision::Passthrough,
            forward_passthrough(fwd_req, upgrade, passthrough, ctx.client_config.clone(), &ctx.pool)
                .await,
        ),
    };

    let upstream_resp = match result {
        Ok(r) => r,
        Err(e) => bad_gateway(&e),
    };

    // --- captura da response ---
    let resp_clen = content_length(upstream_resp.headers());
    let switching = upstream_resp.status() == StatusCode::SWITCHING_PROTOCOLS;
    let (resp_body, resp_body_truncated, resp_size, mut resp): (Option<String>, bool, Option<u64>, Response<BoxedBody>) =
        if !switching && resp_clen.is_some_and(|n| n <= CAP) {
            let (parts, body) = upstream_resp.into_parts();
            match body.collect().await {
                Ok(c) => {
                    let bytes = c.to_bytes();
                    let (p, t) = body_preview(&parts.headers, &bytes).unzip();
                    (p, t.unwrap_or(false), Some(bytes.len() as u64), Response::from_parts(parts, full_body(bytes)))
                }
                // Headers parciais + corpo vazio corromperiam o framing: 502 inteiro.
                Err(_) => (None, false, None, bad_gateway_msg("falha lendo corpo da resposta")),
            }
        } else {
            (None, false, resp_clen, upstream_resp)
        };
    if cfg.cors.enabled {
        add_cors(resp.headers_mut(), origin.as_deref(), &cfg.cors);
    }
    let status = resp.status().as_u16();
    let resp_headers = redact_headers(resp.headers());

    emit(
        tx,
        TrafficEvent {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            ts: now_ms(),
            method: method.to_string(),
            host,
            path,
            query,
            decision,
            status: Some(status),
            latency_ms: Some(started.elapsed().as_millis() as u64),
            req_headers,
            req_body,
            req_body_truncated,
            req_size,
            resp_headers,
            resp_body,
            resp_body_truncated,
            resp_size,
        },
    );

    Ok(resp)
}

/// Headers hop-by-hop fixos (RFC 7230 §6.1 + legados). `Transfer-Encoding`
/// fica: o hyper usa p/ o framing do corpo em streaming.
const HOP_BY_HOP: &[&str] = &["connection", "keep-alive", "proxy-connection", "te", "trailer", "upgrade"];

/// Prepara os headers de saida: remove hop-by-hop (incl. os nomeados em
/// `Connection`), descarta `X-Forwarded-*`/`Forwarded` do cliente (anti-spoof)
/// e seta os nossos. Em upgrade, restaura `Connection: upgrade` + `Upgrade`.
fn prepare_forward_headers(headers: &mut HeaderMap, upgrade: bool, client_ip: IpAddr, fwd_host: &str) {
    let upgrade_value = headers.get(UPGRADE).cloned();
    let named: Vec<String> = headers
        .get_all(CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty() && t != "transfer-encoding")
        .collect();
    for n in named.iter().map(String::as_str).chain(HOP_BY_HOP.iter().copied()) {
        headers.remove(n);
    }
    let spoofable: Vec<HeaderName> = headers
        .keys()
        .filter(|k| k.as_str().starts_with("x-forwarded-") || k.as_str() == "forwarded")
        .cloned()
        .collect();
    for k in spoofable {
        headers.remove(k);
    }
    if let Ok(v) = HeaderValue::from_str(&client_ip.to_string()) {
        headers.insert(HeaderName::from_static("x-forwarded-for"), v);
    }
    headers.insert(HeaderName::from_static("x-forwarded-proto"), HeaderValue::from_static("https"));
    if let Ok(v) = HeaderValue::from_str(fwd_host) {
        headers.insert(HeaderName::from_static("x-forwarded-host"), v);
    }
    if upgrade {
        if let Some(u) = upgrade_value {
            headers.insert(CONNECTION, HeaderValue::from_static("upgrade"));
            headers.insert(UPGRADE, u);
        }
    }
}

async fn with_connect_timeout<T, F>(what: &str, fut: F) -> anyhow::Result<T>
where
    F: std::future::Future<Output = anyhow::Result<T>>,
{
    tokio::time::timeout(CONNECT_TIMEOUT, fut)
        .await
        .map_err(|_| anyhow::anyhow!("timeout ({}s) conectando em {what}", CONNECT_TIMEOUT.as_secs()))?
}

/// Encaminha p/ um servico local (HTTP/1.1 claro). Suporta upgrade de WS.
async fn forward_local(
    req: Request<BoxedBody>,
    upgrade: bool,
    host: &str,
    port: u16,
    pool: &Arc<Pool>,
) -> anyhow::Result<Response<BoxedBody>> {
    let key = PoolKey::Local(host.to_string(), port);
    let pooled = if upgrade { None } else { pool.checkout(&key).await };
    let sender = match pooled {
        Some(s) => s,
        None => {
            with_connect_timeout(&format!("{host}:{port}"), async {
                let stream = TcpStream::connect((host, port)).await?;
                let (sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
                tokio::spawn(async move {
                    let _ = conn.with_upgrades().await;
                });
                Ok(sender)
            })
            .await?
        }
    };
    send_maybe_upgrade(req, upgrade, sender, pool, key).await
}

/// Passthrough p/ o gateway remoto: TCP no IP pinado + TLS validando o cert
/// contra o SNI (FQDN). Reescreve o Host p/ o FQDN. Suporta upgrade de WS.
async fn forward_passthrough(
    mut req: Request<BoxedBody>,
    upgrade: bool,
    pt: &PassthroughTarget,
    client_config: Arc<ClientConfig>,
    pool: &Arc<Pool>,
) -> anyhow::Result<Response<BoxedBody>> {
    let ip = pt
        .fixed_ip
        .ok_or_else(|| anyhow::anyhow!("passthrough sem IP resolvido (rode o DNS direto antes)"))?;
    if ip.is_loopback() {
        return Err(anyhow::anyhow!(
            "passthrough resolveu p/ loopback ({ip}) — possivel loop, abortado"
        ));
    }

    let key = PoolKey::Passthrough(ip, pt.port, pt.sni.clone());
    let pooled = if upgrade { None } else { pool.checkout(&key).await };
    let sender = match pooled {
        Some(s) => s,
        None => {
            let server_name = ServerName::try_from(pt.sni.clone())
                .map_err(|_| anyhow::anyhow!("SNI invalido: {}", pt.sni))?;
            with_connect_timeout(&format!("{ip}:{} ({})", pt.port, pt.sni), async {
                let tcp = TcpStream::connect((ip, pt.port)).await?;
                let tls = TlsConnector::from(client_config).connect(server_name, tcp).await?;
                let (sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(tls)).await?;
                tokio::spawn(async move {
                    let _ = conn.with_upgrades().await;
                });
                Ok(sender)
            })
            .await?
        }
    };

    // O gateway remoto espera o Host = FQDN (estamos conectados por IP).
    let host_value = if pt.port == 443 {
        HeaderValue::from_str(&pt.sni)?
    } else {
        HeaderValue::from_str(&format!("{}:{}", pt.sni, pt.port))?
    };
    req.headers_mut().insert(HOST, host_value);
    // Marca o salto p/ detectar loop caso o trafego reentre no devsplit.
    req.headers_mut().insert(
        HeaderName::from_static("x-devsplit-hop"),
        HeaderValue::from_static("1"),
    );

    send_maybe_upgrade(req, upgrade, sender, pool, key).await
}

/// Envia a request; se for um upgrade (WebSocket), faz passthrough transparente
/// do tunel apos o 101. Requests normais devolvem a conexao ao pool.
async fn send_maybe_upgrade(
    mut req: Request<BoxedBody>,
    upgrade: bool,
    mut sender: SendRequest<BoxedBody>,
    pool: &Arc<Pool>,
    key: PoolKey,
) -> anyhow::Result<Response<BoxedBody>> {
    if !upgrade {
        // Erro aqui dropa o sender (conexao descartada, nao volta ao pool).
        let resp = sender.send_request(req).await?;
        pool.checkin(key, sender);
        return Ok(resp.map(|b| b.boxed()));
    }

    // Captura o upgrade do lado do cliente ANTES de mover a request.
    let client_upgrade = hyper::upgrade::on(&mut req);
    let mut resp = sender.send_request(req).await?;

    if resp.status() == StatusCode::SWITCHING_PROTOCOLS {
        let upstream_upgrade = hyper::upgrade::on(&mut resp);
        tokio::spawn(async move {
            if let Ok((client, upstream)) = tokio::try_join!(client_upgrade, upstream_upgrade) {
                let mut a = TokioIo::new(client);
                let mut b = TokioIo::new(upstream);
                let _ = tokio::io::copy_bidirectional(&mut a, &mut b).await;
            }
        });
    }
    // Sem 101: `client_upgrade` (OnUpgrade) e dropado aqui; o hyper do lado do
    // cliente segue a conexao como HTTP normal ao enviar esta resposta. O corpo
    // da request (stream `Incoming`) ja foi movido p/ o upstream, que o consome
    // ou o dropa junto com a conexao dedicada (upgrades nao usam o pool) —
    // nada fica preso.
    Ok(resp.map(|b| b.boxed()))
}

// ---- helpers ----

/// `host:port` -> `host` (IPv6 `[::1]:443` -> `[::1]`).
fn strip_port(host: &str) -> &str {
    if host.starts_with('[') {
        return host.find(']').map(|i| &host[..=i]).unwrap_or(host);
    }
    host.split(':').next().unwrap_or(host)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Detecta `Connection: Upgrade` + header `Upgrade` presente.
fn is_upgrade(connection: Option<&HeaderValue>, upgrade: Option<&HeaderValue>) -> bool {
    let has_conn_upgrade = connection
        .and_then(|v| v.to_str().ok())
        .map(|v| v.to_ascii_lowercase().split(',').any(|t| t.trim() == "upgrade"))
        .unwrap_or(false);
    has_conn_upgrade && upgrade.is_some()
}

fn bad_gateway(err: &anyhow::Error) -> Response<BoxedBody> {
    Response::builder()
        .status(StatusCode::BAD_GATEWAY)
        .body(full_body(format!("devsplit: erro de upstream: {err}")))
        .unwrap_or_else(|_| Response::new(empty_body()))
}

fn bad_gateway_msg(msg: &str) -> Response<BoxedBody> {
    bad_gateway(&anyhow::anyhow!("{msg}"))
}

/// Preflight CORS = OPTIONS + `Access-Control-Request-Method`.
fn is_preflight<B>(req: &Request<B>) -> bool {
    req.method() == Method::OPTIONS && req.headers().contains_key(ACCESS_CONTROL_REQUEST_METHOD)
}

/// Responde o preflight na borda. Origin fora da allowlist -> 204 sem headers
/// de permissao (o browser bloqueia).
fn cors_preflight<B>(req: &Request<B>, origin: Option<&str>, policy: &CorsPolicy) -> Response<BoxedBody> {
    let mut builder = Response::builder().status(StatusCode::NO_CONTENT);
    if let Some(headers) = builder.headers_mut() {
        if add_cors(headers, origin, policy) {
            headers.insert(
                ACCESS_CONTROL_ALLOW_METHODS,
                HeaderValue::from_static("GET,POST,PUT,PATCH,DELETE,OPTIONS,HEAD"),
            );
            if let Some(reqh) = req.headers().get(ACCESS_CONTROL_REQUEST_HEADERS).cloned() {
                headers.insert(ACCESS_CONTROL_ALLOW_HEADERS, reqh);
            }
        }
    }
    builder.body(empty_body()).unwrap_or_else(|_| Response::new(empty_body()))
}

/// Aplica a politica CORS: sempre acrescenta `Vary: Origin`; espelha a origin
/// so se `policy.allows`. Retorna `true` se a origin foi aceita.
fn add_cors(headers: &mut HeaderMap, origin: Option<&str>, policy: &CorsPolicy) -> bool {
    if !policy.enabled {
        return false;
    }
    let already_varies = headers
        .get_all(VARY)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .any(|t| t.trim().eq_ignore_ascii_case("origin") || t.trim() == "*");
    if !already_varies {
        headers.append(VARY, HeaderValue::from_static("Origin"));
    }
    let Some(o) = origin.filter(|o| policy.allows(o)) else {
        return false;
    };
    let Ok(v) = HeaderValue::from_str(o) else {
        return false;
    };
    headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, v);
    if policy.allow_credentials {
        headers.insert(ACCESS_CONTROL_ALLOW_CREDENTIALS, HeaderValue::from_static("true"));
    } else {
        headers.remove(ACCESS_CONTROL_ALLOW_CREDENTIALS);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Route, RouteTable};

    #[test]
    fn longest_prefix_wins() {
        let table = RouteTable::new(vec![
            Route {
                host: "api.stage.acme.com".into(),
                prefix: "/auth".into(),
                upstream: Upstream::Local { host: "127.0.0.1".into(), port: 3001 },
            },
            Route {
                host: "api.stage.acme.com".into(),
                prefix: "/auth/admin".into(),
                upstream: Upstream::Local { host: "127.0.0.1".into(), port: 3999 },
            },
        ]);
        let r = table.match_route("api.stage.acme.com", "/auth/admin/users").unwrap();
        assert_eq!(r.upstream, Upstream::Local { host: "127.0.0.1".into(), port: 3999 });
        let r = table.match_route("api.stage.acme.com", "/auth/login").unwrap();
        assert_eq!(r.upstream, Upstream::Local { host: "127.0.0.1".into(), port: 3001 });
    }

    #[test]
    fn no_match_falls_through() {
        let table = RouteTable::new(vec![Route {
            host: "api.stage.acme.com".into(),
            prefix: "/transporte".into(),
            upstream: Upstream::Local { host: "127.0.0.1".into(), port: 3000 },
        }]);
        assert!(table.match_route("api.stage.acme.com", "/financeiro").is_none());
        assert!(table.match_route("outro.host", "/transporte").is_none());
    }

    #[test]
    fn strip_port_works() {
        assert_eq!(strip_port("api.stage.acme.com:443"), "api.stage.acme.com");
        assert_eq!(strip_port("api.stage.acme.com"), "api.stage.acme.com");
    }

    #[test]
    fn upgrade_detection() {
        let conn = HeaderValue::from_static("keep-alive, Upgrade");
        let upg = HeaderValue::from_static("websocket");
        assert!(is_upgrade(Some(&conn), Some(&upg)));
        let conn2 = HeaderValue::from_static("keep-alive");
        assert!(!is_upgrade(Some(&conn2), Some(&upg)));
        assert!(!is_upgrade(Some(&conn), None));
    }

    /// E2E: cliente TLS real -> proxy (cert local) -> backend HTTP local.
    /// Prova terminacao TLS + roteamento Host+PathPrefix + forward local.
    #[tokio::test]
    async fn e2e_local_forward_over_tls() {
        use http_body_util::Empty;
        ensure_crypto_provider();

        // backend HTTP local que responde "LOCAL OK"
        let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_port = backend.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let (s, _) = match backend.accept().await {
                    Ok(x) => x,
                    Err(_) => break,
                };
                tokio::spawn(async move {
                    let svc = hyper::service::service_fn(|_req: Request<Incoming>| async {
                        Ok::<_, std::convert::Infallible>(Response::new(Full::new(
                            Bytes::from_static(b"LOCAL OK"),
                        )))
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(s), svc)
                        .await;
                });
            }
        });

        // CA + leaf locais p/ "localhost"
        let dir = tempfile::tempdir().unwrap();
        let ca = crate::tlsca::ensure_ca(dir.path()).unwrap();
        let leaf = crate::tlsca::issue_leaf(&ca, &["localhost".to_string()], 825).unwrap();
        let server_config = crate::tlsca::build_server_config(&leaf).unwrap();

        // proxy: /transporte -> backend local; resto = passthrough (nao exercitado)
        let cfg = ProxyConfig {
            listen_host: "127.0.0.1".into(),
            listen_port: 0,
            intercept_host: "localhost".into(),
            routes: RouteTable::new(vec![Route {
                host: "localhost".into(),
                prefix: "/transporte".into(),
                upstream: Upstream::Local { host: "127.0.0.1".into(), port: backend_port },
            }]),
            passthrough: PassthroughTarget {
                sni: "localhost".into(),
                resolve_host: "localhost".into(),
                fixed_ip: None,
                port: 443,
            },
            extra_hosts: Vec::new(),
            cors: Default::default(),
        };
        let shared = Arc::new(ArcSwap::from_pointee(cfg));
        let handle = serve("127.0.0.1:0".parse().unwrap(), server_config, shared, None)
            .await
            .unwrap();
        let proxy_addr = handle.local_addr;

        // cliente TLS que confia na CA local
        let mut roots = RootCertStore::empty();
        let mut rd = ca.cert_pem.as_bytes();
        for c in rustls_pemfile::certs(&mut rd) {
            roots.add(c.unwrap()).unwrap();
        }
        let client_config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(client_config));
        let tcp = TcpStream::connect(proxy_addr).await.unwrap();
        let sni = ServerName::try_from("localhost").unwrap();
        let tls = connector.connect(sni, tcp).await.unwrap();
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = conn.await;
        });

        let req = Request::builder()
            .uri("/transporte/coletas")
            .header(HOST, "localhost")
            .body(Empty::<Bytes>::new())
            .unwrap();
        let resp = sender.send_request(req).await.unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"LOCAL OK");

        handle.abort();
    }

    #[test]
    fn redacts_sensitive_headers() {
        use hyper::header::{HeaderMap, HeaderName, HeaderValue};
        let mut h = HeaderMap::new();
        h.insert("authorization", HeaderValue::from_static("Bearer abc.def.ghi"));
        h.insert("cookie", HeaderValue::from_static("session=segredo123"));
        h.insert("x-api-key", HeaderValue::from_static("key-abc"));
        h.insert(HeaderName::from_static("x-refresh-token"), HeaderValue::from_static("rt-zzz"));
        h.insert(HeaderName::from_static("x-client-secret"), HeaderValue::from_static("sh-zzz"));
        h.insert("content-type", HeaderValue::from_static("application/json"));

        let pairs = redact_headers(&h);
        let val = |name: &str| pairs.iter().find(|p| p.name == name).map(|p| p.value.as_str());

        // sensiveis -> redatados
        assert_eq!(val("cookie"), Some("<redacted>"));
        assert_eq!(val("x-api-key"), Some("<redacted>"));
        assert_eq!(val("x-refresh-token"), Some("<redacted>")); // contem "token"
        assert_eq!(val("x-client-secret"), Some("<redacted>")); // contem "secret"
        // authorization fica VISIVEL de proposito (inspector de JWT/sessao)
        assert_eq!(val("authorization"), Some("Bearer abc.def.ghi"));
        // nao-sensivel passa
        assert_eq!(val("content-type"), Some("application/json"));
        // invariante de seguranca: nenhum valor sensivel vaza no payload
        assert!(!pairs.iter().any(|p| p.value.contains("segredo123")));
        assert!(!pairs.iter().any(|p| p.value.contains("key-abc")));
    }

    // ---- helpers de teste ----

    fn route(host: &str, prefix: &str, port: u16) -> Route {
        Route {
            host: host.into(),
            prefix: prefix.into(),
            upstream: Upstream::Local { host: "127.0.0.1".into(), port },
        }
    }

    #[test]
    fn prefix_requires_segment_boundary() {
        let t = RouteTable::new(vec![route("h", "/auth", 1)]);
        assert!(t.match_route("h", "/auth").is_some());
        assert!(t.match_route("h", "/auth/x").is_some());
        assert!(t.match_route("h", "/authors").is_none());
        let root = RouteTable::new(vec![route("h", "/", 1)]);
        assert!(root.match_route("h", "/qualquer").is_some());
    }

    fn policy() -> CorsPolicy {
        CorsPolicy {
            enabled: true,
            allow_origins: vec!["https://app.acme.com".into()],
            allow_origins_regex: vec![regex::Regex::new(r"^https://.*\.dev\.acme\.com$").unwrap()],
            allow_credentials: true,
        }
    }

    #[test]
    fn cors_reflects_only_allowed_origins() {
        let p = policy();
        let mut h = HeaderMap::new();
        h.insert(VARY, HeaderValue::from_static("Accept-Encoding"));
        assert!(add_cors(&mut h, Some("https://app.acme.com"), &p));
        assert_eq!(h.get(ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(), "https://app.acme.com");
        assert_eq!(h.get(ACCESS_CONTROL_ALLOW_CREDENTIALS).unwrap(), "true");
        let vary: Vec<_> = h.get_all(VARY).iter().collect();
        assert_eq!(vary.len(), 2, "Vary deve ser acrescentado, nao sobrescrito");

        let mut h = HeaderMap::new();
        assert!(add_cors(&mut h, Some("https://x.dev.acme.com"), &p));
        let mut h = HeaderMap::new();
        assert!(!add_cors(&mut h, Some("https://evil.com"), &p));
        assert!(h.get(ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
        assert!(h.get(ACCESS_CONTROL_ALLOW_CREDENTIALS).is_none());

        // desligado: nao toca em nada
        let mut h = HeaderMap::new();
        assert!(!add_cors(&mut h, Some("https://app.acme.com"), &CorsPolicy::default()));
        assert!(h.is_empty());
    }

    #[test]
    fn forward_headers_strip_hop_by_hop_and_spoofing() {
        let mut h = HeaderMap::new();
        h.insert(CONNECTION, HeaderValue::from_static("keep-alive, x-secret-hop"));
        h.insert("x-secret-hop", HeaderValue::from_static("1"));
        h.insert("keep-alive", HeaderValue::from_static("timeout=5"));
        h.insert("x-forwarded-for", HeaderValue::from_static("6.6.6.6"));
        h.insert("forwarded", HeaderValue::from_static("for=6.6.6.6"));
        h.insert("content-type", HeaderValue::from_static("text/plain"));
        prepare_forward_headers(&mut h, false, "10.0.0.7".parse().unwrap(), "api.acme.com");
        assert!(h.get(CONNECTION).is_none());
        assert!(h.get("x-secret-hop").is_none());
        assert!(h.get("keep-alive").is_none());
        assert!(h.get("forwarded").is_none());
        assert_eq!(h.get("x-forwarded-for").unwrap(), "10.0.0.7");
        assert_eq!(h.get("x-forwarded-proto").unwrap(), "https");
        assert_eq!(h.get("x-forwarded-host").unwrap(), "api.acme.com");
        assert_eq!(h.get("content-type").unwrap(), "text/plain");
    }

    #[test]
    fn compressed_preview_is_decoded() {
        use std::io::Write;
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(b"{\"ok\":true}").unwrap();
        let gz = enc.finish().unwrap();
        let mut h = HeaderMap::new();
        h.insert(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        assert_eq!(body_preview(&h, &gz).unwrap().0, "{\"ok\":true}");
        assert!(body_preview(&h, b"lixo nao gzip").is_none());
        h.insert(CONTENT_ENCODING, HeaderValue::from_static("zstd"));
        assert!(body_preview(&h, &gz).is_none());
    }

    /// Sobe backend HTTP (ecoa X-Forwarded-For no corpo) + proxy TLS p/ "localhost".
    async fn start_stack(
        cors: CorsPolicy,
        tx: Option<mpsc::Sender<TrafficEvent>>,
    ) -> (ProxyHandle, crate::tlsca::Ca) {
        let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_port = backend.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((s, _)) = backend.accept().await {
                tokio::spawn(async move {
                    let svc = hyper::service::service_fn(|req: Request<Incoming>| async move {
                        let xff = req
                            .headers()
                            .get("x-forwarded-for")
                            .map(|v| v.to_str().unwrap().to_string())
                            .unwrap_or_default();
                        Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(format!(
                            "{} xff={xff}",
                            req.method()
                        )))))
                    });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(s), svc)
                        .await;
                });
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let ca = crate::tlsca::ensure_ca(dir.path()).unwrap();
        let leaf = crate::tlsca::issue_leaf(&ca, &["localhost".to_string()], 825).unwrap();
        let server_config = crate::tlsca::build_server_config(&leaf).unwrap();
        let cfg = ProxyConfig {
            listen_host: "127.0.0.1".into(),
            listen_port: 0,
            intercept_host: "localhost".into(),
            routes: RouteTable::new(vec![route("localhost", "/api", backend_port)]),
            passthrough: PassthroughTarget {
                sni: "localhost".into(),
                resolve_host: "localhost".into(),
                fixed_ip: None,
                port: 443,
            },
            extra_hosts: Vec::new(),
            cors,
        };
        let shared = Arc::new(ArcSwap::from_pointee(cfg));
        let handle = serve("127.0.0.1:0".parse().unwrap(), server_config, shared, tx)
            .await
            .unwrap();
        (handle, ca)
    }

    async fn client(
        addr: SocketAddr,
        ca: &crate::tlsca::Ca,
    ) -> hyper::client::conn::http1::SendRequest<Empty<Bytes>> {
        let mut roots = RootCertStore::empty();
        let mut rd = ca.cert_pem.as_bytes();
        for c in rustls_pemfile::certs(&mut rd) {
            roots.add(c.unwrap()).unwrap();
        }
        let cc = ClientConfig::builder().with_root_certificates(roots).with_no_client_auth();
        let tcp = TcpStream::connect(addr).await.unwrap();
        let tls = TlsConnector::from(Arc::new(cc))
            .connect(ServerName::try_from("localhost").unwrap(), tcp)
            .await
            .unwrap();
        let (sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(tls)).await.unwrap();
        tokio::spawn(async move {
            let _ = conn.await;
        });
        sender
    }

    #[tokio::test]
    async fn unknown_host_is_421_and_missing_host_is_400() {
        ensure_crypto_provider();
        let (tx, mut rx) = mpsc::channel(16);
        let (handle, ca) = start_stack(CorsPolicy::default(), Some(tx)).await;
        let mut s = client(handle.local_addr, &ca).await;
        let req = Request::builder()
            .uri("/api/x")
            .header(HOST, "outro.host")
            .body(Empty::new())
            .unwrap();
        assert_eq!(s.send_request(req).await.unwrap().status(), 421);
        let ev = rx.recv().await.unwrap();
        assert_eq!(ev.decision, Decision::Edge);
        assert_eq!(ev.status, Some(421));

        let mut s = client(handle.local_addr, &ca).await;
        let req = Request::builder().uri("/api/x").body(Empty::new()).unwrap();
        assert_eq!(s.send_request(req).await.unwrap().status(), 400);
        assert_eq!(rx.recv().await.unwrap().status, Some(400));
        handle.abort();
    }

    #[tokio::test]
    async fn pooled_requests_forward_xff_query_and_cors() {
        ensure_crypto_provider();
        let (tx, mut rx) = mpsc::channel(16);
        let (handle, ca) = start_stack(policy(), Some(tx)).await;
        let mut s = client(handle.local_addr, &ca).await;
        for _ in 0..3 {
            let req = Request::builder()
                .uri("/api/x?a=1")
                .header(HOST, "localhost")
                .header(ORIGIN, "https://app.acme.com")
                .header("x-forwarded-for", "6.6.6.6")
                .body(Empty::new())
                .unwrap();
            let resp = s.send_request(req).await.unwrap();
            assert_eq!(resp.status(), 200);
            assert_eq!(resp.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).unwrap(), "https://app.acme.com");
            let body = resp.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(&body[..], b"GET xff=127.0.0.1");
            let ev = rx.recv().await.unwrap();
            assert_eq!(ev.query.as_deref(), Some("a=1"));
            assert!(matches!(ev.decision, Decision::Local(_)));
        }

        // preflight na borda; origin rejeitada nao recebe Allow-Origin
        let req = Request::builder()
            .method(Method::OPTIONS)
            .uri("/api/x")
            .header(HOST, "localhost")
            .header(ORIGIN, "https://evil.com")
            .header(ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .body(Empty::new())
            .unwrap();
        let resp = s.send_request(req).await.unwrap();
        assert_eq!(resp.status(), 204);
        assert!(resp.headers().get(ACCESS_CONTROL_ALLOW_ORIGIN).is_none());
        assert_eq!(rx.recv().await.unwrap().decision, Decision::Edge);

        // OPTIONS sem Access-Control-Request-Method vai ao upstream
        let req = Request::builder()
            .method(Method::OPTIONS)
            .uri("/api/x")
            .header(HOST, "localhost")
            .body(Empty::new())
            .unwrap();
        let body = s.send_request(req).await.unwrap().into_body().collect().await.unwrap().to_bytes();
        assert!(body.starts_with(b"OPTIONS"));
        handle.abort();
    }
}
