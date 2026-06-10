//! Casca Tauri do devsplit: cola o nucleo (`devsplit-core`) na UI React.
//!
//! Responsabilidades (mínimas — o trabalho pesado vive no core, ja testado):
//! - manter o estado runtime (config carregada, perfil ativo, rotas, handle);
//! - expor comandos `#[tauri::command]` que a UI chama via `invoke`;
//! - emitir eventos (`proxy://status`, `proxy://traffic`) p/ a UI reagir;
//! - tray, single-instance e autostart.
//!
//! ATENCAO: este crate so compila numa maquina com `webkit2gtk-4.1` (Linux),
//! WebView2 (Windows) ou WKWebView (macOS). O nucleo (`devsplit-core`) compila
//! e e testado em qualquer lugar. Ver README.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::collections::VecDeque;

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{mpsc, Mutex};

use devsplit_core::config::{self, DevsplitConfig};
use devsplit_core::proxy::{self, ProxyHandle, SharedConfig};
use devsplit_core::types::{
    Decision, ProxyConfig, Route, RouteTable, TrafficEvent, Upstream,
};
use devsplit_core::{dns, hostsfile, tlsca};

const LEAF_MAX_DAYS: u32 = 825;

/// Estado runtime do app (a fonte de verdade da UI durante a sessao).
#[derive(Default)]
struct AppState {
    cfg: Mutex<Option<DevsplitConfig>>,
    profile: Mutex<String>,
    /// Rotas locais editaveis pela UI (catch-all/passthrough nao entra aqui).
    routes: Mutex<Vec<RouteDto>>,
    /// `ArcSwap` vivo enquanto o proxy roda (hot-reload).
    shared: Mutex<Option<SharedConfig>>,
    handle: Mutex<Option<ProxyHandle>>,
    running: Mutex<bool>,
    /// IP real do stage resolvido via DNS direto, cacheado enquanto ligado
    /// (evita re-resolver a cada toggle/reload).
    resolved_ip: Arc<Mutex<Option<IpAddr>>>,
    /// Sessao root persistente (1 prompt por sessao) p/ hosts/setcap/sysctl/revert.
    privileged: Arc<StdMutex<Option<PrivSession>>>,
    /// Task de re-resolucao periodica do IP do stage (abortada no stop).
    reresolve_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Ring buffer das ultimas requisicoes capturadas (detalhe do inspector).
    traffic: Arc<StdMutex<VecDeque<TrafficEvent>>>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StatusDto {
    running: bool,
    intercept_host: String,
    listen_addr: String,
    hosts: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RouteDto {
    prefix: String,
    target: String,
    /// "local" | "passthrough"
    kind: String,
    enabled: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DoctorCheck {
    id: String,
    label: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

#[derive(Serialize, Clone)]
struct Profiles {
    active: String,
    all: Vec<String>,
}

/// Resumo leve emitido por requisicao (evento `proxy://traffic`, lista ao vivo).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct TrafficSummary {
    id: u64,
    ts: u64,
    method: String,
    host: String,
    path: String,
    decision: &'static str,
    status: Option<u16>,
    latency_ms: Option<u64>,
    req_size: Option<u64>,
    resp_size: Option<u64>,
}

/// Detalhe completo de uma requisicao (`get_request_detail`).
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RequestDetail {
    id: u64,
    ts: u64,
    method: String,
    host: String,
    path: String,
    decision: &'static str,
    status: Option<u16>,
    latency_ms: Option<u64>,
    req_headers: Vec<(String, String)>,
    req_body: Option<String>,
    req_body_truncated: bool,
    req_size: Option<u64>,
    resp_headers: Vec<(String, String)>,
    resp_body: Option<String>,
    resp_body_truncated: bool,
    resp_size: Option<u64>,
    redacted: bool,
}

/// Servico local detectado (`detect_local_services`).
#[derive(Serialize, Clone)]
struct LocalService {
    port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

/// Aviso p/ a UI (toast) — evento `proxy://notice`.
#[derive(Serialize, Clone)]
struct Notice {
    level: &'static str,
    message: String,
}

fn decision_str(d: &Decision) -> &'static str {
    match d {
        Decision::Local(_) => "local",
        Decision::Passthrough => "passthrough",
    }
}

fn to_pairs(headers: &[devsplit_core::types::HeaderPair]) -> Vec<(String, String)> {
    headers.iter().map(|h| (h.name.clone(), h.value.clone())).collect()
}

fn any_redacted(headers: &[devsplit_core::types::HeaderPair]) -> bool {
    headers.iter().any(|h| h.value == "<redacted>")
}

fn traffic_summary(ev: &TrafficEvent) -> TrafficSummary {
    TrafficSummary {
        id: ev.id,
        ts: ev.ts,
        method: ev.method.clone(),
        host: ev.host.clone(),
        path: ev.path.clone(),
        decision: decision_str(&ev.decision),
        status: ev.status,
        latency_ms: ev.latency_ms,
        req_size: ev.req_size,
        resp_size: ev.resp_size,
    }
}

fn request_detail(ev: &TrafficEvent) -> RequestDetail {
    RequestDetail {
        id: ev.id,
        ts: ev.ts,
        method: ev.method.clone(),
        host: ev.host.clone(),
        path: ev.path.clone(),
        decision: decision_str(&ev.decision),
        status: ev.status,
        latency_ms: ev.latency_ms,
        req_headers: to_pairs(&ev.req_headers),
        req_body: ev.req_body.clone(),
        req_body_truncated: ev.req_body_truncated,
        req_size: ev.req_size,
        resp_headers: to_pairs(&ev.resp_headers),
        resp_body: ev.resp_body.clone(),
        resp_body_truncated: ev.resp_body_truncated,
        resp_size: ev.resp_size,
        redacted: any_redacted(&ev.req_headers) || any_redacted(&ev.resp_headers),
    }
}

fn e2s<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn loopback() -> IpAddr {
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

fn caroot(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(e2s)?.join("ca");
    std::fs::create_dir_all(&dir).map_err(e2s)?;
    Ok(dir)
}

/// Resolve o ProxyConfig do core p/ o perfil ativo (sem IP resolvido ainda).
async fn base_proxy_config(state: &AppState) -> Result<ProxyConfig, String> {
    let cfg = state.cfg.lock().await;
    let cfg = cfg.as_ref().ok_or("config nao carregada")?;
    let profile = state.profile.lock().await.clone();
    config::to_proxy_config(cfg, &profile, None).map_err(e2s)
}

// ---------------------------- comandos ----------------------------

#[tauri::command]
async fn get_status(state: State<'_, AppState>) -> Result<StatusDto, String> {
    let running = *state.running.lock().await;
    match base_proxy_config(&state).await.ok() {
        Some(pcfg) => Ok(status_dto(&pcfg, running)),
        None => Ok(StatusDto {
            running,
            intercept_host: String::new(),
            listen_addr: "0.0.0.0:443".into(),
            hosts: Vec::new(),
        }),
    }
}

#[tauri::command]
async fn list_routes(state: State<'_, AppState>) -> Result<Vec<RouteDto>, String> {
    let mut out = state.routes.lock().await.clone();
    // catch-all sempre por ultimo, nao editavel.
    out.push(RouteDto {
        prefix: "/*".into(),
        target: "stage real (passthrough)".into(),
        kind: "passthrough".into(),
        enabled: true,
    });
    Ok(out)
}

#[tauri::command]
async fn add_route(
    prefix: String,
    target: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let mut routes = state.routes.lock().await;
        routes.retain(|r| r.prefix != prefix);
        routes.push(RouteDto {
            prefix,
            target,
            kind: "local".into(),
            enabled: true,
        });
    }
    persist_routes(&app, &state).await;
    reload_if_running(&app, &state).await
}

#[tauri::command]
async fn remove_route(
    prefix: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.routes.lock().await.retain(|r| r.prefix != prefix);
    persist_routes(&app, &state).await;
    reload_if_running(&app, &state).await
}

#[tauri::command]
async fn toggle_route(
    prefix: String,
    enabled: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    {
        let mut routes = state.routes.lock().await;
        if let Some(r) = routes.iter_mut().find(|r| r.prefix == prefix) {
            r.enabled = enabled;
        }
    }
    persist_routes(&app, &state).await;
    reload_if_running(&app, &state).await
}

#[tauri::command]
async fn get_profiles(state: State<'_, AppState>) -> Result<Profiles, String> {
    let cfg = state.cfg.lock().await;
    let all = cfg
        .as_ref()
        .map(|c| config::profile_names(c))
        .unwrap_or_default();
    Ok(Profiles {
        active: state.profile.lock().await.clone(),
        all,
    })
}

#[tauri::command]
async fn set_profile(
    name: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    *state.profile.lock().await = name;
    // re-semeia as rotas do novo perfil
    let pcfg = base_proxy_config(&state).await?;
    let profile = state.profile.lock().await.clone();
    *state.routes.lock().await =
        load_saved_routes(&app, &profile).unwrap_or_else(|| routes_from(&pcfg));
    reload_if_running(&app, &state).await
}

/// Caminho do estado runtime das rotas (por perfil) — separado do devsplit.yaml.
fn routes_state_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("routes-state.json"))
}

/// Persiste as rotas runtime do perfil ativo (sobrevive a reinicios). NAO mexe
/// no devsplit.yaml — preserva os comentarios/`extends` que voce edita a mao.
async fn persist_routes(app: &AppHandle, state: &AppState) {
    let Some(path) = routes_state_path(app) else {
        return;
    };
    let profile = state.profile.lock().await.clone();
    let routes = state.routes.lock().await.clone();
    let mut map: serde_json::Map<String, serde_json::Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if let Ok(v) = serde_json::to_value(&routes) {
        map.insert(profile, v);
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(&map) {
        let _ = std::fs::write(&path, s);
    }
}

/// Carrega as rotas salvas do perfil, se houver.
fn load_saved_routes(app: &AppHandle, profile: &str) -> Option<Vec<RouteDto>> {
    let path = routes_state_path(app)?;
    let s = std::fs::read_to_string(&path).ok()?;
    let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&s).ok()?;
    serde_json::from_value(map.get(profile)?.clone()).ok()
}

#[tauri::command]
async fn run_doctor(app: AppHandle, state: State<'_, AppState>) -> Result<Vec<DoctorCheck>, String> {
    let mut checks = Vec::new();
    let running = *state.running.lock().await;

    // cert: confia de verdade? (CA gerada + presente no trust store NSS do navegador)
    let ca_ok = caroot(&app)
        .map(|d| d.join("rootCA.pem").exists())
        .unwrap_or(false);
    let cert_ok = ca_ok && nss_has_mkcert();
    checks.push(DoctorCheck {
        id: "cert".into(),
        label: "Certificado confiavel".into(),
        ok: cert_ok,
        hint: (!cert_ok).then(|| {
            if ca_ok {
                "CA gerada mas nao confiada no navegador — clique 'Instalar certificado'".into()
            } else {
                "CA ainda nao gerada — ligue a interceptacao".into()
            }
        }),
    });

    // hosts: estado-consciente. Com o proxy LIGADO o bloco DEVE existir; com ele
    // DESLIGADO o bloco NAO deve existir — se existir, e orfao de uma sessao
    // anterior que nao foi desligada (e quebra o acesso real ao stage).
    let hp = hostsfile::hosts_path();
    let has_block = std::fs::read_to_string(&hp)
        .map(|c| hostsfile::has_block(&c))
        .unwrap_or(false);
    let (hosts_ok, hosts_hint) = match (running, has_block) {
        (true, true) => (true, None),
        (true, false) => (false, Some("proxy ligado mas /etc/hosts sem a entrada".to_string())),
        (false, false) => (true, None),
        (false, true) => (
            false,
            Some("bloco orfao no /etc/hosts (sessao anterior nao desligada) — clique 'Limpar'".to_string()),
        ),
    };
    checks.push(DoctorCheck {
        id: "hosts".into(),
        label: "/etc/hosts".into(),
        ok: hosts_ok,
        hint: hosts_hint,
    });

    // upstream (DNS direto resolve?)
    if let Ok(pcfg) = base_proxy_config(&state).await {
        let resolved = dns::resolve_direct(&pcfg.passthrough.resolve_host).await.is_ok();
        checks.push(DoctorCheck {
            id: "upstream".into(),
            label: "Stage responde".into(),
            ok: resolved,
            hint: (!resolved).then(|| "Sem rota ate o stage (VPN ligada?)".into()),
        });
    }

    Ok(checks)
}

/// `true` se a CA do mkcert esta no trust store NSS do usuario (Chrome/Chromium/
/// Firefox usam NSS no Linux). Best-effort via certutil; ausente => assume nao.
fn nss_has_mkcert() -> bool {
    let home = match std::env::var_os("HOME") {
        Some(h) => h,
        None => return false,
    };
    let db = format!("sql:{}/.pki/nssdb", home.to_string_lossy());
    std::process::Command::new("certutil")
        .args(["-L", "-d", &db])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("mkcert"))
        .unwrap_or(false)
}

#[tauri::command]
async fn start_proxy(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if *state.running.lock().await {
        return Ok(());
    }
    let pcfg = build_runtime_config(&app, &state).await?;

    // Todos os FQDNs interceptados (primario + extras) -> cert SAN + /etc/hosts.
    let all_hosts: Vec<String> = std::iter::once(pcfg.intercept_host.clone())
        .chain(pcfg.extra_hosts.iter().map(|h| h.host.clone()))
        .collect();

    // CA + leaf locais + ServerConfig (sem privilegio).
    let caroot = caroot(&app)?;
    let ca = tlsca::ensure_ca(&caroot).map_err(e2s)?;
    let leaf = tlsca::issue_leaf(&ca, &all_hosts, LEAF_MAX_DAYS).map_err(e2s)?;
    let server_config = tlsca::build_server_config(&leaf).map_err(e2s)?;

    // Bootstrap privilegiado (UM prompt): /etc/hosts (todos os hosts) + libera :443.
    bootstrap_privileges(state.privileged.clone(), &all_hosts).await?;

    // serve — apos o bootstrap o processo consegue bindar a :443.
    let listen = format!("{}:{}", pcfg.listen_host, pcfg.listen_port)
        .parse()
        .map_err(|_| "endereco de bind invalido".to_string())?;
    let shared: SharedConfig = Arc::new(ArcSwap::from_pointee(pcfg.clone()));
    let (tx, mut rx) = mpsc::channel(1024);
    let handle = proxy::serve(listen, server_config, shared.clone(), Some(tx))
        .await
        .map_err(|e| format!("falha ao bindar {listen} (privilegio?): {e}"))?;

    // bombeia o trafego p/ a UI: emite o resumo, guarda o detalhe no ring, e
    // avisa (toast) quando um upstream falha (502).
    let app_traffic = app.clone();
    let ring = state.traffic.clone();
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            if ev.status == Some(502) {
                let (level, message) = match &ev.decision {
                    Decision::Local(target) => (
                        "warn",
                        format!("Servico local nao respondeu ({target}) em {}", ev.path),
                    ),
                    Decision::Passthrough => (
                        "error",
                        format!("Stage inalcancavel em {} (VPN ligada?)", ev.path),
                    ),
                };
                let _ = app_traffic.emit("proxy://notice", Notice { level, message });
            }
            let _ = app_traffic.emit("proxy://traffic", traffic_summary(&ev));
            if let Ok(mut buf) = ring.lock() {
                if buf.len() >= 500 {
                    buf.pop_front();
                }
                buf.push_back(ev);
            }
        }
    });

    // Task de re-resolucao: a cada 60s checa a saude do IP pinado e re-resolve
    // via DNS direto; se o IP do stage mudou (LB rotacionou) ou caiu, troca a
    // quente sem dropar conexao.
    let reresolve = {
        let shared = shared.clone();
        let resolved_ip = state.resolved_ip.clone();
        let app = app.clone();
        let resolve_host = pcfg.passthrough.resolve_host.clone();
        let port = pcfg.passthrough.port;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
            tick.tick().await; // consome o tick imediato
            loop {
                tick.tick().await;
                let current = shared.load_full();
                let cur_ip = current.passthrough.fixed_ip;
                let healthy = match cur_ip {
                    Some(ip) => tokio::time::timeout(
                        std::time::Duration::from_secs(3),
                        tokio::net::TcpStream::connect((ip, port)),
                    )
                    .await
                    .map(|r| r.is_ok())
                    .unwrap_or(false),
                    None => false,
                };
                if let Ok(new_ip) = dns::resolve_direct(&resolve_host).await {
                    if Some(new_ip) != cur_ip || !healthy {
                        let mut next = (*current).clone();
                        next.passthrough.fixed_ip = Some(new_ip);
                        shared.store(Arc::new(next));
                        *resolved_ip.lock().await = Some(new_ip);
                        let _ = app.emit("proxy://status", status_dto(&current, true));
                    }
                }
            }
        })
    };
    *state.reresolve_task.lock().await = Some(reresolve);

    *state.shared.lock().await = Some(shared);
    *state.handle.lock().await = Some(handle);
    *state.running.lock().await = true;
    let _ = app.emit("proxy://status", status_dto(&pcfg, true));
    Ok(())
}

#[tauri::command]
async fn stop_proxy(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    // Derruba o servidor e ESPERA liberar a :443 (essencial p/ religar limpo).
    if let Some(h) = state.handle.lock().await.take() {
        h.shutdown().await;
    }
    *state.shared.lock().await = None;
    *state.running.lock().await = false;
    if let Some(t) = state.reresolve_task.lock().await.take() {
        t.abort();
    }
    *state.resolved_ip.lock().await = None;

    // reverte o /etc/hosts: remove o bloco E as entradas soltas dos FQDNs, p/ o
    // stage voltar a ser alcancavel (senao "trava apontando local").
    if let Ok(pcfg) = base_proxy_config(&state).await {
        let hosts: Vec<String> = std::iter::once(pcfg.intercept_host.clone())
            .chain(pcfg.extra_hosts.iter().map(|h| h.host.clone()))
            .collect();
        let _ = revert_hosts(state.privileged.clone(), &hosts).await;
    }

    if let Ok(pcfg) = base_proxy_config(&state).await {
        let _ = app.emit("proxy://status", status_dto(&pcfg, false));
    }
    Ok(())
}

/// Remove um bloco ORFAO do /etc/hosts (sessao anterior fechada sem desligar),
/// sem precisar religar o proxy. Reusa a reversao elevada; e no-op (sem prompt)
/// se ja estiver limpo, porque `revert_hosts` so eleva quando ha o que remover.
#[tauri::command]
async fn cleanup_hosts(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let hosts: Vec<String> = match base_proxy_config(&state).await {
        Ok(pcfg) => std::iter::once(pcfg.intercept_host.clone())
            .chain(pcfg.extra_hosts.iter().map(|h| h.host.clone()))
            .collect(),
        Err(_) => Vec::new(),
    };
    revert_hosts(state.privileged.clone(), &hosts).await?;
    if let Ok(pcfg) = base_proxy_config(&state).await {
        let running = *state.running.lock().await;
        let _ = app.emit("proxy://status", status_dto(&pcfg, running));
    }
    Ok(())
}

/// Roda um script como root reusando a SESSAO privilegiada persistente: pede a
/// senha apenas UMA vez por sessao (pkexec /bin/sh mantido vivo). Reabre se caiu.
async fn run_elevated(
    privileged: Arc<StdMutex<Option<PrivSession>>>,
    script: String,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let mut guard = privileged
            .lock()
            .map_err(|_| "estado privilegiado corrompido".to_string())?;
        let alive = guard.as_mut().map(|s| s.is_alive()).unwrap_or(false);
        if !alive {
            *guard = Some(PrivSession::spawn()?);
        }
        guard
            .as_mut()
            .expect("sessao privilegiada presente")
            .run(&script)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Sessao root persistente: um `pkexec /bin/sh` mantido vivo, alimentado por
/// scripts via stdin. Pede a senha uma vez (no spawn) e reusa pelo resto da
/// sessao — acaba com o "duas senhas por ligar/desligar".
struct PrivSession {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: std::io::BufReader<std::process::ChildStdout>,
}

impl PrivSession {
    fn spawn() -> Result<Self, String> {
        use std::process::{Command, Stdio};
        let mut child = Command::new("pkexec")
            .arg("/bin/sh")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("falha ao iniciar pkexec: {e}"))?;
        let stdin = child.stdin.take().ok_or("pkexec sem stdin")?;
        let stdout = std::io::BufReader::new(child.stdout.take().ok_or("pkexec sem stdout")?);
        let mut session = Self { child, stdin, stdout };
        // Confirma auth: se o usuario cancelar, o pkexec sai e este ping falha.
        session
            .run("true")
            .map_err(|_| "elevacao nao autorizada (pkexec cancelado?)".to_string())?;
        Ok(session)
    }

    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Executa um script no shell root e espera o sentinela com o exit code.
    fn run(&mut self, script: &str) -> Result<(), String> {
        use std::io::{BufRead, Write};
        let line = format!("{{ {script}\n}} 2>&1\nprintf '__DSEND__%d\\n' \"$?\"\n");
        self.stdin
            .write_all(line.as_bytes())
            .map_err(|e| format!("falha escrevendo no helper root: {e}"))?;
        self.stdin.flush().map_err(|e| e.to_string())?;

        let mut out = String::new();
        let mut buf = String::new();
        loop {
            buf.clear();
            if self.stdout.read_line(&mut buf).map_err(|e| e.to_string())? == 0 {
                return Err("helper root encerrou inesperadamente".into());
            }
            if let Some(code) = buf.trim().strip_prefix("__DSEND__") {
                let code: i32 = code.trim().parse().unwrap_or(-1);
                let _ = std::fs::write(
                    "/tmp/devsplit-bootstrap.log",
                    format!("script:\n{script}\n---\nexit:{code}\nout:\n{out}"),
                );
                return if code == 0 {
                    Ok(())
                } else {
                    Err(format!("comando root falhou (cod {code}): {}", out.trim()))
                };
            }
            out.push_str(&buf);
        }
    }
}

/// Monta o script root: grava `content` no arquivo hosts via heredoc (sem temp
/// file -> sem TOCTOU). Com `exe` (no ativar), tambem libera o bind na :443.
fn build_priv_script(hosts_path: &Path, content: &str, exe: Option<&Path>) -> String {
    let hp = hosts_path.display();
    let body = content.trim_end_matches('\n');
    let mut s = format!("cat > '{hp}' <<'__DSHOSTS__'\n{body}\n__DSHOSTS__\n");
    if let Some(_exe) = exe {
        #[cfg(target_os = "linux")]
        {
            let exe = _exe.display();
            s.push_str(&format!(
                "setcap cap_net_bind_service=+ep '{exe}' 2>/dev/null || true\n\
                 sysctl -w net.ipv4.ip_unprivileged_port_start=443 >/dev/null 2>&1 || true\n"
            ));
        }
    }
    s
}
/// Resolve o caminho absoluto de um binario (pkexec zera o PATH, entao embutimos).
fn which(bin: &str) -> Option<String> {
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {bin}"))
        .output()
        .ok()?;
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !p.is_empty()).then_some(p)
}

/// Bootstrap privilegiado (no ativar): grava o bloco no arquivo hosts e libera
/// o bind na :443. Tudo via a sessao root persistente (1 prompt).
async fn bootstrap_privileges(
    privileged: Arc<StdMutex<Option<PrivSession>>>,
    hosts: &[String],
) -> Result<(), String> {
    let hp = hostsfile::hosts_path();
    let existing = std::fs::read_to_string(&hp).unwrap_or_default();
    let entries: Vec<(IpAddr, String)> = hosts.iter().map(|h| (loopback(), h.clone())).collect();
    let content = hostsfile::render(&existing, &entries);
    if cfg!(target_os = "linux") {
        let exe = std::env::current_exe().map_err(e2s)?;
        let script = build_priv_script(&hp, &content, Some(&exe));
        run_elevated(privileged, script).await
    } else {
        // macOS/Windows: sem sessao persistente — grava o hosts elevado em 1 passo.
        let _ = &privileged;
        apply_hosts_oneshot(&hp, &content).await
    }
}

/// Reverte o arquivo hosts (no desligar): remove o bloco do devsplit E as
/// entradas soltas do FQDN apontando p/ loopback. So eleva se houver o que tirar.
async fn revert_hosts(
    privileged: Arc<StdMutex<Option<PrivSession>>>,
    hosts: &[String],
) -> Result<(), String> {
    let hp = hostsfile::hosts_path();
    let existing = std::fs::read_to_string(&hp).map_err(e2s)?;
    let host_refs: Vec<&str> = hosts.iter().map(|s| s.as_str()).collect();
    let needs = hostsfile::has_block(&existing)
        || existing.lines().any(|l| {
            let t = l.split('#').next().unwrap_or("").trim();
            (t.starts_with("127.") || t.starts_with("::1"))
                && t.split_whitespace().any(|w| host_refs.contains(&w))
        });
    if !needs {
        return Ok(());
    }
    let content = hostsfile::render_revert(&existing, &host_refs);
    if cfg!(target_os = "linux") {
        let script = build_priv_script(&hp, &content, None);
        run_elevated(privileged, script).await
    } else {
        let _ = &privileged;
        apply_hosts_oneshot(&hp, &content).await
    }
}

/// Grava o conteudo COMPLETO do arquivo hosts COM elevacao, num unico passo (sem
/// sessao persistente). Usado em macOS/Windows; no Linux a sessao pkexec cuida disso.
/// Despacho por `cfg!` runtime: TODAS as variantes compilam em qualquer SO (so usam
/// `std::process`/`std::fs`), entao o build do Linux valida o codigo de macOS/Windows;
/// o RUNTIME (osascript/UAC) e validado pelo CI nas respectivas plataformas.
async fn apply_hosts_oneshot(hp: &Path, content: &str) -> Result<(), String> {
    if cfg!(target_os = "windows") {
        apply_hosts_windows(hp, content).await
    } else if cfg!(target_os = "macos") {
        apply_hosts_macos(hp, content).await
    } else {
        let _ = (hp, content);
        Err("elevacao one-shot nao implementada neste SO".into())
    }
}

/// macOS: grava num temp do usuario e copia p/ o hosts via UM prompt de admin
/// (`osascript … with administrator privileges`). bind :443 nao precisa de root (Mojave+).
async fn apply_hosts_macos(hp: &Path, content: &str) -> Result<(), String> {
    let hp = hp.to_path_buf();
    let content = content.to_string();
    tokio::task::spawn_blocking(move || {
        let tmp = std::env::temp_dir().join("devsplit-hosts.tmp");
        std::fs::write(&tmp, &content).map_err(|e| e.to_string())?;
        let osa = format!(
            "do shell script \"/bin/cp '{}' '{}'\" with administrator privileges",
            tmp.display(),
            hp.display()
        );
        let status = std::process::Command::new("osascript").arg("-e").arg(&osa).status();
        let _ = std::fs::remove_file(&tmp);
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(_) => Err("elevacao via osascript falhou ou foi cancelada".to_string()),
            Err(e) => Err(format!("osascript indisponivel: {e}")),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Windows: grava num temp do usuario e copia p/ o hosts via UAC
/// (`Start-Process … -Verb RunAs`). Sem porta privilegiada — bind :443 livre.
async fn apply_hosts_windows(hp: &Path, content: &str) -> Result<(), String> {
    let hp = hp.to_path_buf();
    let content = content.to_string();
    tokio::task::spawn_blocking(move || {
        let tmp = std::env::temp_dir().join("devsplit-hosts.tmp");
        std::fs::write(&tmp, &content).map_err(|e| e.to_string())?;
        let copy = format!("copy /Y \"{}\" \"{}\"", tmp.display(), hp.display());
        let ps = format!(
            "$p = Start-Process -FilePath cmd.exe -ArgumentList '/c {}' -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
            copy.replace('\'', "''")
        );
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
            .status();
        let _ = std::fs::remove_file(&tmp);
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(_) => Err("elevacao via UAC falhou ou foi cancelada".to_string()),
            Err(e) => Err(format!("powershell indisponivel: {e}")),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------- helpers ----------------------------

fn status_dto(pcfg: &ProxyConfig, running: bool) -> StatusDto {
    let mut hosts = vec![pcfg.intercept_host.clone()];
    hosts.extend(pcfg.extra_hosts.iter().map(|h| h.host.clone()));
    StatusDto {
        running,
        intercept_host: pcfg.intercept_host.clone(),
        listen_addr: format!("{}:{}", pcfg.listen_host, pcfg.listen_port),
        hosts,
    }
}

/// Constroi RouteDto's (locais) a partir de um ProxyConfig.
fn routes_from(pcfg: &ProxyConfig) -> Vec<RouteDto> {
    pcfg.routes
        .entries()
        .iter()
        .filter_map(|r| match &r.upstream {
            Upstream::Local { host, port } => Some(RouteDto {
                prefix: r.prefix.clone(),
                target: format!("{host}:{port}"),
                kind: "local".into(),
                enabled: true,
            }),
            Upstream::Passthrough => None,
        })
        .collect()
}

/// Monta o ProxyConfig final aplicando as rotas runtime (UI) + IP resolvido.
async fn build_runtime_config(
    app: &AppHandle,
    state: &AppState,
) -> Result<ProxyConfig, String> {
    let mut pcfg = base_proxy_config(state).await?;

    // rotas vem do estado runtime (UI), nao do arquivo: respeita add/rm/toggle.
    let routes = state.routes.lock().await;
    let entries: Vec<Route> = routes
        .iter()
        .filter(|r| r.enabled && r.kind == "local")
        .filter_map(|r| parse_local(&pcfg.intercept_host, &r.prefix, &r.target))
        .collect();
    pcfg.routes = RouteTable::new(entries);
    drop(routes);

    // IP real via DNS direto (anti-loop). Cacheado enquanto ligado p/ NAO
    // re-resolver a cada toggle/reload (evita reload lento/falho = "transporte nao ia").
    if pcfg.passthrough.fixed_ip.is_none() {
        let mut cache = state.resolved_ip.lock().await;
        let ip = match *cache {
            Some(ip) => ip,
            None => {
                let ip = dns::resolve_direct(&pcfg.passthrough.resolve_host)
                    .await
                    .map_err(|e| format!("falha resolvendo o IP do stage: {e}"))?;
                *cache = Some(ip);
                ip
            }
        };
        pcfg.passthrough.fixed_ip = Some(ip);
    }
    // Resolve o IP de cada host extra (passthrough-only) que ainda nao tem IP fixo.
    for hc in pcfg.extra_hosts.iter_mut() {
        if hc.passthrough.fixed_ip.is_none() {
            if let Ok(ip) = dns::resolve_direct(&hc.passthrough.resolve_host).await {
                hc.passthrough.fixed_ip = Some(ip);
            }
        }
    }
    let _ = app;
    Ok(pcfg)
}

/// "host:port" ou "http://host:port" -> Route local.
fn parse_local(host: &str, prefix: &str, target: &str) -> Option<Route> {
    let t = target
        .strip_prefix("http://")
        .or_else(|| target.strip_prefix("https://"))
        .unwrap_or(target);
    let (h, p) = t.rsplit_once(':')?;
    let port: u16 = p.split('/').next()?.parse().ok()?;
    Some(Route {
        host: host.to_string(),
        prefix: prefix.to_string(),
        upstream: Upstream::Local {
            host: h.to_string(),
            port,
        },
    })
}

/// Reaplica a config no proxy vivo (hot-reload) sem derrubar conexoes.
async fn reload_if_running(app: &AppHandle, state: &AppState) -> Result<(), String> {
    if !*state.running.lock().await {
        return Ok(());
    }
    let pcfg = build_runtime_config(app, state).await?;
    if let Some(shared) = state.shared.lock().await.as_ref() {
        shared.store(Arc::new(pcfg));
    }
    Ok(())
}

/// Detalhe completo de uma requisicao capturada (inspector).
#[tauri::command]
async fn get_request_detail(id: u64, state: State<'_, AppState>) -> Result<RequestDetail, String> {
    let buf = state
        .traffic
        .lock()
        .map_err(|_| "ring de trafego corrompido".to_string())?;
    buf.iter()
        .rev()
        .find(|e| e.id == id)
        .map(request_detail)
        .ok_or_else(|| "requisicao nao encontrada (saiu do buffer)".to_string())
}

/// Detecta servicos de dev escutando em portas comuns no 127.0.0.1.
#[tauri::command]
async fn detect_local_services() -> Result<Vec<LocalService>, String> {
    let ports: [u16; 13] = [
        3000, 3001, 3002, 3003, 3004, 3005, 4000, 5000, 5173, 8000, 8080, 8081, 9000,
    ];
    let mut found = Vec::new();
    for p in ports {
        let up = tokio::time::timeout(
            std::time::Duration::from_millis(120),
            tokio::net::TcpStream::connect(("127.0.0.1", p)),
        )
        .await
        .map(|r| r.is_ok())
        .unwrap_or(false);
        if up {
            found.push(LocalService {
                port: p,
                hint: hint_for_port(p),
            });
        }
    }
    Ok(found)
}

fn hint_for_port(port: u16) -> Option<String> {
    match port {
        5173 => Some("Vite (frontend)".into()),
        3000 => Some("Nest/Node".into()),
        _ => None,
    }
}

// ---------------------------- setup / run ----------------------------

/// Procura o `devsplit.yaml` subindo do cwd (app/src-tauri, app/, raiz do repo),
/// depois no app_config_dir.
fn find_config_path(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(mut dir) = std::env::current_dir() {
        for _ in 0..6 {
            let p = dir.join("devsplit.yaml");
            if p.exists() {
                return Some(p);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    if let Ok(dir) = app.path().app_config_dir() {
        let p = dir.join("devsplit.yaml");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Recarrega o devsplit.yaml do disco no estado (mantendo o perfil ativo se ainda
/// existir) e reaplica no proxy vivo. Usado pelo hot-reload.
async fn reload_from_disk(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let Some(path) = find_config_path(app) else {
        return false;
    };
    let Ok(cfg) = config::load(&path) else {
        return false;
    };
    let names = config::profile_names(&cfg);
    let mut profile = state.profile.lock().await.clone();
    if !names.contains(&profile) {
        profile = names.into_iter().next().unwrap_or_else(|| "default".into());
    }
    let seeded = config::to_proxy_config(&cfg, &profile, None)
        .map(|pc| routes_from(&pc))
        .unwrap_or_default();
    let routes = load_saved_routes(app, &profile).unwrap_or(seeded);
    *state.cfg.lock().await = Some(cfg);
    *state.profile.lock().await = profile;
    *state.routes.lock().await = routes;
    let _ = reload_if_running(app, &state).await;
    true
}

fn load_config_into_state(app: &AppHandle) {
    let state = app.state::<AppState>();
    // procura devsplit.yaml subindo a partir do cwd (cobre app/src-tauri, app/
    // e a raiz do repo, independente de onde o `cargo tauri dev` foi chamado),
    // depois <config>/devsplit/devsplit.yaml.
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(mut dir) = std::env::current_dir() {
        for _ in 0..6 {
            candidates.push(dir.join("devsplit.yaml"));
            if !dir.pop() {
                break;
            }
        }
    }
    if let Ok(dir) = app.path().app_config_dir() {
        candidates.push(dir.join("devsplit.yaml"));
    }
    for path in candidates {
        if let Ok(cfg) = config::load(&path) {
            let profile = config::profile_names(&cfg)
                .into_iter()
                .next()
                .unwrap_or_else(|| "default".into());
            // semeia as rotas do perfil (ou usa as salvas no estado, se houver)
            let seeded = config::to_proxy_config(&cfg, &profile, None)
                .map(|pc| routes_from(&pc))
                .unwrap_or_default();
            let routes = load_saved_routes(app, &profile).unwrap_or(seeded);
            tauri::async_runtime::block_on(async {
                *state.cfg.lock().await = Some(cfg);
                *state.profile.lock().await = profile;
                *state.routes.lock().await = routes;
            });
            break;
        }
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Ligar/Desligar split", true, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Abrir painel", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Sair", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &open, &quit])?;

    let mut builder = TrayIconBuilder::with_id("devsplit-tray")
        .tooltip("devsplit")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "toggle" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    let running = *state.running.lock().await;
                    let _ = if running {
                        stop_proxy(app.clone(), state).await
                    } else {
                        start_proxy(app.clone(), state).await
                    };
                });
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}
/// Instala a CA local nos trust stores dos NAVEGADORES (NSS: Firefox/Chrome) via
/// mkcert, rodando como o USUARIO (sem root). `TRUST_STORES=nss` evita o sudo
/// interno do mkcert e cobre exatamente o que o browser valida.
#[tauri::command]
async fn install_cert(app: AppHandle) -> Result<String, String> {
    let caroot = caroot(&app)?;
    tlsca::ensure_ca(&caroot).map_err(e2s)?;
    let mkcert = which("mkcert").ok_or("binario 'mkcert' nao encontrado — instale o mkcert")?;
    let out = tokio::task::spawn_blocking(move || {
        std::process::Command::new(&mkcert)
            .env("CAROOT", &caroot)
            .env("TRUST_STORES", "nss")
            .arg("-install")
            .output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Re-resolve o IP real do stage via DNS direto e aplica a quente (botao da UI).
#[tauri::command]
async fn reresolve_upstream(state: State<'_, AppState>) -> Result<StatusDto, String> {
    let pcfg = base_proxy_config(&state).await?;
    let ip = dns::resolve_direct(&pcfg.passthrough.resolve_host)
        .await
        .map_err(|e| format!("falha resolvendo o IP do stage: {e}"))?;
    *state.resolved_ip.lock().await = Some(ip);
    let running = *state.running.lock().await;
    if running {
        if let Some(shared) = state.shared.lock().await.as_ref() {
            let mut next = (*shared.load_full()).clone();
            next.passthrough.fixed_ip = Some(ip);
            shared.store(Arc::new(next));
        }
    }
    Ok(status_dto(&pcfg, running))
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState::default())
        .setup(|app| {
            let handle = app.handle().clone();
            load_config_into_state(&handle);
            build_tray(&handle)?;
            // Hot-reload: observa o devsplit.yaml e aplica ao salvar / git pull.
            let watch = handle.clone();
            tauri::async_runtime::spawn(async move {
                let mut last: Option<std::time::SystemTime> = None;
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(2));
                loop {
                    tick.tick().await;
                    let Some(path) = find_config_path(&watch) else {
                        continue;
                    };
                    let m = std::fs::metadata(&path).ok().and_then(|x| x.modified().ok());
                    match (last, m) {
                        (Some(prev), Some(now)) if prev != now => {
                            last = Some(now);
                            if reload_from_disk(&watch).await {
                                let _ = watch.emit(
                                    "proxy://notice",
                                    Notice {
                                        level: "info",
                                        message: "devsplit.yaml recarregado".into(),
                                    },
                                );
                            }
                        }
                        (None, Some(now)) => last = Some(now),
                        _ => {}
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            start_proxy,
            stop_proxy,
            list_routes,
            add_route,
            remove_route,
            toggle_route,
            run_doctor,
            get_profiles,
            set_profile,
            install_cert,
            reresolve_upstream,
            get_request_detail,
            detect_local_services,
            cleanup_hosts
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o devsplit");
}
