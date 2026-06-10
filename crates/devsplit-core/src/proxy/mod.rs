//! Motor de proxy transparente.
//!
//! - Termina TLS local (rustls) na :443.
//! - Roteia por Host + PathPrefix (prefixo mais especifico ganha).
//! - Match local -> encaminha p/ 127.0.0.1:porta (HTTP/1.1 claro).
//! - Sem match -> passthrough p/ o gateway remoto: conecta no IP REAL pinado e
//!   valida o cert contra `sni` (FQDN). NUNCA desabilita verificacao.
//! - WebSocket: passthrough transparente via `hyper::upgrade::on` +
//!   `copy_bidirectional` (byte-copy bruto apos o 101).
//! - CORS: responde o preflight (OPTIONS) na borda.
//! - Hot-reload: a config vem de um `ArcSwap`; conexoes em voo nao caem.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Once};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use std::sync::atomic::{AtomicU64, Ordering};

use arc_swap::ArcSwap;
use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Empty, Full};
use hyper::body::Incoming;
use hyper::header::{
    HeaderName, HeaderValue, ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_HEADERS,
    ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_REQUEST_HEADERS,
    CONNECTION, CONTENT_LENGTH, HOST, ORIGIN, UPGRADE,
};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::types::{Decision, HeaderPair, PassthroughTarget, ProxyConfig, TrafficEvent, Upstream};

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

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Lê o Content-Length, se presente e numerico.
fn content_length(headers: &hyper::HeaderMap) -> Option<u64> {
    headers.get(CONTENT_LENGTH)?.to_str().ok()?.parse().ok()
}

/// Converte headers em pares (nome, valor), REDIGINDO os sensiveis (cookie,
/// set-cookie, x-api-key, *secret*, *token*). `authorization` fica visivel de
/// proposito — e o que o painel de Sessao/JWT inspeciona.
fn redact_headers(headers: &hyper::HeaderMap) -> Vec<HeaderPair> {
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
    let client_config = build_client_config();

    let task = tokio::spawn(async move {
        loop {
            let (tcp, _peer) = match listener.accept().await {
                Ok(x) => x,
                Err(_) => continue,
            };
            let acceptor = acceptor.clone();
            let config = config.clone();
            let client_config = client_config.clone();
            let traffic_tx = traffic_tx.clone();
            tokio::spawn(async move {
                let tls = match acceptor.accept(tcp).await {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let io = TokioIo::new(tls);
                let service = hyper::service::service_fn(move |req| {
                    handle(
                        req,
                        config.clone(),
                        client_config.clone(),
                        traffic_tx.clone(),
                    )
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .with_upgrades()
                    .await;
            });
        }
    });

    Ok(ProxyHandle { task, local_addr })
}

/// Handler por requisicao. Escolhe o host (multi-host), decide local vs
/// passthrough, captura headers/body (redatado, so pequenos), cronometra a
/// latencia e emite o [`TrafficEvent`] completo.
async fn handle(
    req: Request<Incoming>,
    config: SharedConfig,
    client_config: Arc<ClientConfig>,
    traffic_tx: Option<mpsc::Sender<TrafficEvent>>,
) -> Result<Response<BoxedBody>, Infallible> {
    let started = Instant::now();
    let cfg = config.load_full();

    let host = req
        .headers()
        .get(HOST)
        .and_then(|h| h.to_str().ok())
        .map(strip_port)
        .unwrap_or(&cfg.intercept_host)
        .to_string();
    let path = req.uri().path().to_string();
    let method = req.method().clone();
    let origin = req
        .headers()
        .get(ORIGIN)
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);

    // Anti-loop e preflight (early returns, sem evento).
    if req.headers().contains_key("x-devsplit-hop") {
        return Ok(loop_detected());
    }
    if method == Method::OPTIONS {
        return Ok(cors_preflight(&req, origin.as_deref()));
    }

    // Multi-host: escolhe rotas + passthrough pelo Host (primario ou extra).
    let (routes, passthrough) = if host == cfg.intercept_host {
        (&cfg.routes, &cfg.passthrough)
    } else if let Some(hc) = cfg.extra_hosts.iter().find(|h| h.host == host) {
        (&hc.routes, &hc.passthrough)
    } else {
        (&cfg.routes, &cfg.passthrough)
    };
    let matched = routes.match_route(&host, &path).map(|r| r.upstream.clone());

    // --- captura da request (headers sempre; body so se pequeno e nao-upgrade) ---
    let is_upgrade = is_upgrade(req.headers().get(CONNECTION), req.headers().get(UPGRADE));
    let req_headers = redact_headers(req.headers());
    let req_clen = content_length(req.headers());
    let (parts, incoming) = req.into_parts();
    let (req_body, req_body_truncated, req_size, fwd_body): (Option<String>, bool, Option<u64>, BoxedBody) =
        if !is_upgrade && req_clen.is_some_and(|n| n <= CAP) {
            match incoming.collect().await {
                Ok(c) => {
                    let bytes = c.to_bytes();
                    let (p, t) = preview(&bytes);
                    (Some(p), t, Some(bytes.len() as u64), full_body(bytes))
                }
                Err(_) => (None, false, req_clen, empty_body()),
            }
        } else {
            (None, false, req_clen, incoming.boxed())
        };
    let fwd_req = Request::from_parts(parts, fwd_body);

    // --- encaminhamento ---
    let (decision, result) = match matched {
        Some(Upstream::Local { host: lh, port }) => (
            Decision::Local(format!("{lh}:{port}")),
            forward_local(fwd_req, &lh, port).await,
        ),
        _ => (
            Decision::Passthrough,
            forward_passthrough(fwd_req, passthrough, client_config).await,
        ),
    };

    let upstream_resp = match result {
        Ok(r) => r,
        Err(e) => bad_gateway(&e),
    };
    let status = upstream_resp.status().as_u16();

    // --- captura da response ---
    let resp_headers = redact_headers(upstream_resp.headers());
    let resp_clen = content_length(upstream_resp.headers());
    let switching = upstream_resp.status() == StatusCode::SWITCHING_PROTOCOLS;
    let (resp_body, resp_body_truncated, resp_size, mut resp): (Option<String>, bool, Option<u64>, Response<BoxedBody>) =
        if !switching && resp_clen.is_some_and(|n| n <= CAP) {
            let (parts, body) = upstream_resp.into_parts();
            match body.collect().await {
                Ok(c) => {
                    let bytes = c.to_bytes();
                    let (p, t) = preview(&bytes);
                    (Some(p), t, Some(bytes.len() as u64), Response::from_parts(parts, full_body(bytes)))
                }
                Err(_) => (None, false, resp_clen, Response::from_parts(parts, empty_body())),
            }
        } else {
            (None, false, resp_clen, upstream_resp)
        };
    add_cors(resp.headers_mut(), origin.as_deref());

    if let Some(tx) = &traffic_tx {
        let _ = tx.try_send(TrafficEvent {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            ts: now_ms(),
            method: method.to_string(),
            host,
            path,
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
        });
    }

    Ok(resp)
}

/// Encaminha p/ um servico local (HTTP/1.1 claro). Suporta upgrade de WS.
async fn forward_local(
    req: Request<BoxedBody>,
    host: &str,
    port: u16,
) -> anyhow::Result<Response<BoxedBody>> {
    let stream = TcpStream::connect((host, port)).await?;
    let io = TokioIo::new(stream);
    let (sender, conn) = hyper::client::conn::http1::handshake(io).await?;
    tokio::spawn(async move {
        let _ = conn.with_upgrades().await;
    });
    send_maybe_upgrade(req, sender).await
}

/// Passthrough p/ o gateway remoto: TCP no IP pinado + TLS validando o cert
/// contra o SNI (FQDN). Reescreve o Host p/ o FQDN. Suporta upgrade de WS.
async fn forward_passthrough(
    mut req: Request<BoxedBody>,
    pt: &PassthroughTarget,
    client_config: Arc<ClientConfig>,
) -> anyhow::Result<Response<BoxedBody>> {
    let ip = pt
        .fixed_ip
        .ok_or_else(|| anyhow::anyhow!("passthrough sem IP resolvido (rode o DNS direto antes)"))?;
    if ip.is_loopback() {
        return Err(anyhow::anyhow!(
            "passthrough resolveu p/ loopback ({ip}) — possivel loop, abortado"
        ));
    }

    let tcp = TcpStream::connect((ip, pt.port)).await?;
    let connector = TlsConnector::from(client_config);
    let server_name = ServerName::try_from(pt.sni.clone())
        .map_err(|_| anyhow::anyhow!("SNI invalido: {}", pt.sni))?;
    let tls = connector.connect(server_name, tcp).await?;
    let io = TokioIo::new(tls);
    let (sender, conn) = hyper::client::conn::http1::handshake(io).await?;
    tokio::spawn(async move {
        let _ = conn.with_upgrades().await;
    });

    // O gateway remoto espera o Host = FQDN (estamos conectados por IP).
    req.headers_mut()
        .insert(HOST, HeaderValue::from_str(&pt.sni)?);
    // Marca o salto p/ detectar loop caso o trafego reentre no devsplit.
    req.headers_mut().insert(
        HeaderName::from_static("x-devsplit-hop"),
        HeaderValue::from_static("1"),
    );

    send_maybe_upgrade(req, sender).await
}

/// Resposta 508 quando a requisicao ja passou pelo devsplit (loop detectado).
fn loop_detected() -> Response<BoxedBody> {
    Response::builder()
        .status(StatusCode::LOOP_DETECTED)
        .body(full_body(
            "devsplit: loop detectado (a requisicao reentrou no proxy)",
        ))
        .unwrap_or_else(|_| Response::new(empty_body()))
}

/// Envia a request; se for um upgrade (WebSocket), faz passthrough transparente
/// do tunel apos o 101.
async fn send_maybe_upgrade(
    mut req: Request<BoxedBody>,
    mut sender: hyper::client::conn::http1::SendRequest<BoxedBody>,
) -> anyhow::Result<Response<BoxedBody>> {
    if !is_upgrade(req.headers().get(CONNECTION), req.headers().get(UPGRADE)) {
        let resp = sender.send_request(req).await?;
        return Ok(resp.map(|b| b.boxed()));
    }

    // Captura o upgrade do lado do cliente ANTES de mover a request.
    let client_upgrade = hyper::upgrade::on(&mut req);
    let mut resp = sender.send_request(req).await?;

    if resp.status() == StatusCode::SWITCHING_PROTOCOLS {
        let upstream_upgrade = hyper::upgrade::on(&mut resp);
        tokio::spawn(async move {
            match tokio::try_join!(client_upgrade, upstream_upgrade) {
                Ok((client, upstream)) => {
                    let mut a = TokioIo::new(client);
                    let mut b = TokioIo::new(upstream);
                    let _ = tokio::io::copy_bidirectional(&mut a, &mut b).await;
                }
                Err(_) => {}
            }
        });
    }

    Ok(resp.map(|b| b.boxed()))
}

// ---- helpers ----

/// `host:port` -> `host`.
fn strip_port(host: &str) -> &str {
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

/// Responde o preflight OPTIONS espelhando origin + headers pedidos.
fn cors_preflight(req: &Request<Incoming>, origin: Option<&str>) -> Response<BoxedBody> {
    let mut builder = Response::builder().status(StatusCode::NO_CONTENT);
    if let Some(headers) = builder.headers_mut() {
        add_cors(headers, origin);
        if let Some(reqh) = req.headers().get(ACCESS_CONTROL_REQUEST_HEADERS).cloned() {
            headers.insert(ACCESS_CONTROL_ALLOW_HEADERS, reqh);
        }
    }
    builder.body(empty_body()).unwrap_or_else(|_| Response::new(empty_body()))
}

/// Adiciona headers CORS espelhando a origin (ou `*` quando ausente).
fn add_cors(headers: &mut hyper::HeaderMap, origin: Option<&str>) {
    let allow_origin = origin
        .and_then(|o| HeaderValue::from_str(o).ok())
        .unwrap_or_else(|| HeaderValue::from_static("*"));
    headers.insert(ACCESS_CONTROL_ALLOW_ORIGIN, allow_origin);
    headers.insert(
        ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET,POST,PUT,PATCH,DELETE,OPTIONS,HEAD"),
    );
    headers.insert(
        ACCESS_CONTROL_ALLOW_CREDENTIALS,
        HeaderValue::from_static("true"),
    );
    // Vary: Origin p/ caches nao misturarem origins.
    headers.insert(
        HeaderName::from_static("vary"),
        HeaderValue::from_static("Origin"),
    );
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
                verify: true,
            },
            extra_hosts: Vec::new(),
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
}
