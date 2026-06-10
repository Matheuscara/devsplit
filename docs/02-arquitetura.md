# 02 — Arquitetura e contrato canônico

> **Contrato canônico** da costura entre UI, núcleo e SO. Define as 3 partes, o motor de
> proxy Rust (módulo a módulo), a casca Tauri e o **contrato IPC** (comandos + eventos)
> consumido pelo frontend (`03`) e detalhado em `12`. Espelha `crates/devsplit-core/src/`
> e `app/src-tauri/src/lib.rs`; em qualquer divergência, vale o código.

---

## 1. Um processo, três partes

```
┌─ APP devsplit (1 processo Tauri) ───────────────────────────────────────────┐
│                                                                             │
│  L1  WEBVIEW UI  (React 19 + Vite + Tailwind v4 + lucide-react)             │
│      ▲  telas: Rotas · Tráfego · Sessão · Certificado · Hosts · Config      │
│      │  invoke() → comandos         listen() ← eventos proxy://*            │
│      ▼                                                                      │
│  L3  CASCA TAURI  (app/src-tauri/src/lib.rs)                                │
│      estado runtime · #[tauri::command] · eventos · tray · single-instance  │
│      · autostart · sessão root persistente (1 prompt) · hot-reload          │
│      │  usa o núcleo (resolve IP, gera cert, edita hosts, sobe o proxy)     │
│      ▼                                                                      │
│  L2  NÚCLEO  devsplit-core  (Rust puro, sem GUI — compila/testa em qq lugar)│
│      proxy (hyper+rustls) · tlsca (rcgen+mkcert) · dns (hickory)            │
│      · hostsfile · config (serde_yaml) · types (o contrato)                 │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Fronteira-chave:** a UI **nunca** fala com a rede do proxy direto — só com a casca via
comandos/eventos Tauri. O motor fica atrás do módulo `proxy/`, trocável.

---

## 2. L2 — O núcleo (`crates/devsplit-core`)

Crate independente de Tauri e de webkit; é onde mora a parte difícil e **testada**.

| Módulo | Responsabilidade |
|---|---|
| `types.rs` | **O contrato.** `Upstream`, `Route`, `RouteTable`, `PassthroughTarget`, `ProxyConfig`, `HostConfig`, `Decision`, `HeaderPair`, `TrafficEvent`. |
| `config.rs` | Parse/validação do `devsplit.yaml` (DTOs serde) + `to_proxy_config` (resolve perfis/`extends`/`environments` → `ProxyConfig`). |
| `proxy/mod.rs` | Motor de proxy: termina TLS, roteia, local vs passthrough, WS, captura de tráfego, CORS, hot-reload. |
| `tlsca.rs` | CA/leaf local (rcgen) + `mkcert -install`. |
| `dns.rs` | DNS direto (hickory) — anti-loop. |
| `hostsfile.rs` | Edição idempotente do arquivo hosts (bloco demarcado + backup + atômico). |

### 2.1 Tipos centrais (`types.rs`)

```rust
enum Upstream { Local { host, port }, Passthrough }

struct Route { host: String, prefix: String, upstream: Upstream }

struct RouteTable { /* Vec<Route> pré-ordenado por Reverse(prefix.len()) */ }
// match_route(host, path): primeiro r onde r.host == host && path.starts_with(r.prefix)

struct PassthroughTarget {
    sni: String,             // SNI E alvo da validação do cert remoto
    resolve_host: String,    // FQDN p/ DNS direto
    fixed_ip: Option<IpAddr>,// IP pinado (a casca preenche antes de servir)
    port: u16,               // tipicamente 443
    verify: bool,            // DEVE ser true em stage real
}

struct ProxyConfig {
    listen_host, listen_port,        // bind (0.0.0.0:443)
    intercept_host: String,          // FQDN primário
    routes: RouteTable,
    passthrough: PassthroughTarget,
    extra_hosts: Vec<HostConfig>,    // multi-host (passthrough-only)
}
```

`Decision` e `TrafficEvent` são `Serialize` (cruzam para a UI). `TrafficEvent` carrega
método/host/path/decisão/status/latência + headers redatados + preview de body (quando
pequeno) — ver `12` §1.

### 2.2 Motor de proxy (`proxy/mod.rs`)

`serve(listen, server_config, config: SharedConfig, traffic_tx)` binda TLS na `listen` e
serve até `ProxyHandle::abort()`/`shutdown()` liberar a `:443`. `SharedConfig =
Arc<ArcSwap<ProxyConfig>>` permite troca a quente.

Fluxo de `handle` (por requisição):

1. Carrega o snapshot (`config.load_full()`), lê Host/path/método/Origin.
2. **Anti-loop:** se há `X-Devsplit-Hop` → `508 Loop Detected` (early return).
3. `OPTIONS` → preflight CORS (early return).
4. **Multi-host:** escolhe `routes`/`passthrough` pelo Host (primário ou `extra_hosts`).
5. `routes.match_route(host, path)` → `Some(Local)` ou `None` (→ passthrough).
6. **Captura da request:** headers sempre (redatados); body só se **não-upgrade** e
   `Content-Length ≤ 256 KiB` (`CAP`); preview truncado em 64 KiB (`PREVIEW_CAP`).
7. Encaminha: `forward_local` (TCP claro) ou `forward_passthrough` (TLS no IP + SNI).
8. **Captura da response** (mesma regra de `CAP`; pula corpos em `101`).
9. Adiciona headers CORS, emite o `TrafficEvent` no canal (`try_send`, não-bloqueante).

| Helper | O que faz |
|---|---|
| `build_client_config` | `ClientConfig` rustls com webpki-roots, sem `dangerous()`, SNI ligado. |
| `forward_local` | TCP → handshake HTTP/1.1 → `send_maybe_upgrade` (suporta WS). |
| `forward_passthrough` | exige `fixed_ip`; recusa loopback; TLS no IP com `ServerName`=SNI; reescreve `Host`=FQDN; injeta `X-Devsplit-Hop: 1`. |
| `send_maybe_upgrade` | se a resposta for `101`, faz `copy_bidirectional` (túnel WS). |
| `redact_headers` | redige `cookie`/`set-cookie`/`x-api-key`/`proxy-authorization` + qualquer header contendo `secret`/`token`; **mantém `authorization` visível** (é o que o painel de Sessão inspeciona). |

### 2.3 CORS (divergência anotada)

O CORS é **inline** no motor (não `tower-http`): `cors_preflight` responde `204` ao
`OPTIONS` espelhando `Access-Control-Request-Headers`; `add_cors` espelha a `Origin` (ou
`*` quando ausente), com métodos fixos, `Allow-Credentials: true` e `Vary: Origin`.

> `[VERIFICAR]` O `devsplit.yaml` tem um bloco `cors` com `allow_origins`/
> `allow_origins_regex`, mas o `CorsSpec` do núcleo só lê `enabled`/`allow_origins`/
> `allow_credentials` (o `allow_origins_regex` do `examples/` é **ignorado** pelo serde) e
> o motor **espelha a origin** em vez de aplicar uma allowlist. Reconciliar em `10` §6.

### 2.4 TLS/CA (`tlsca.rs`) — resumo

`ensure_ca(caroot)` gera (rcgen) ou recarrega a CA (`rootCA.pem` + `rootCA-key.pem`) com
DN determinístico. `issue_leaf(ca, fqdns, max_days)` emite o leaf (SAN = cada FQDN + o
wildcard `*.dominio` + `127.0.0.1` + `::1`, validade ≤ `max_days`). `build_server_config`
monta o `ServerConfig` rustls. `mkcert_install(caroot)` shell-a o `mkcert -install` com
`CAROOT` apontando para a CA do rcgen. Detalhe em `11` §1.

### 2.5 DNS e hosts — resumo

`dns::resolve_direct(fqdn)` → IP real (anti-loop, `01` §4). `hostsfile` edita o arquivo
hosts via bloco demarcado `# >>> devsplit BEGIN >>>` / `# <<< devsplit END <<<`, com
backup `<hosts>.devsplit.bak` e escrita atômica (tempfile + rename); funções puras
(`render`/`render_revert`) testáveis sem disco. Detalhe em `12` §6.

---

## 3. L3 — A casca Tauri (`app/src-tauri/src/lib.rs`)

Cola mínima: mantém o estado runtime, expõe comandos, emite eventos, monta o tray e
gerencia a elevação. O trabalho pesado fica no núcleo.

### 3.1 Estado runtime (`AppState`)

`cfg` (config carregada), `profile` (perfil ativo), `routes` (rotas locais editáveis pela
UI), `shared` (`SharedConfig` vivo enquanto roda), `handle` (`ProxyHandle`), `running`,
`resolved_ip` (IP do stage cacheado), `privileged` (sessão root persistente),
`reresolve_task` (task de re-resolução), `traffic` (ring buffer das últimas 500
requisições, para o detalhe do inspector).

### 3.2 Contrato IPC — comandos

Os **15 comandos** são registrados em `invoke_handler![…]`; a UI os chama via `invoke`
(tipados em `app/src/lib/ipc.ts`). Erros voltam como `Result<_, String>`.

| Comando | Assinatura (TS) | O que faz |
|---|---|---|
| `get_status` | `getStatus(): Status` | `running`, `interceptHost`, `listenAddr`, `hosts[]`. |
| `start_proxy` | `startProxy(): void` | Gera CA+leaf, **bootstrap privilegiado (1 prompt)**: hosts + libera `:443`; sobe `proxy::serve`; inicia a bomba de tráfego e a task de re-resolução. |
| `stop_proxy` | `stopProxy(): void` | Derruba o servidor (espera liberar `:443`), aborta re-resolução, **reverte o `/etc/hosts`**. |
| `list_routes` | `listRoutes(): Route[]` | Rotas locais do perfil ativo (catch-all não entra). |
| `add_route` | `addRoute(prefix, target): void` | Adiciona rota local + hot-reload + persiste. |
| `remove_route` | `removeRoute(prefix): void` | Remove rota + hot-reload. |
| `toggle_route` | `toggleRoute(prefix, enabled): void` | Liga/desliga rota + hot-reload. |
| `get_profiles` | `getProfiles(): Profiles` | `active` + `all[]`. |
| `set_profile` | `setProfile(name): void` | Troca o perfil ativo (carrega rotas salvas) + hot-reload. |
| `run_doctor` | `runDoctor(): DoctorCheck[]` | Saúde **estado-consciente**: `cert` (CA + NSS), `hosts` (proxy **ligado**: o bloco deve existir; **parado + bloco = órfão/aviso** "clique Limpar"), `upstream` (DNS resolve). |
| `install_cert` | `installCert(): string` | `mkcert -install` com `TRUST_STORES=nss` (navegador, sem root). |
| `reresolve_upstream` | `reresolveUpstream(): Status` | Re-resolve o IP do stage via DNS direto e aplica a quente. |
| `get_request_detail` | `getRequestDetail(id): RequestDetail` | Detalhe completo (headers + body) do ring buffer. |
| `detect_local_services` | `detectServices(): LocalService[]` | Sonda portas comuns em `127.0.0.1` (3000–3005, 4000, 5000, 5173, 8000, 8080–8081, 9000). |
| `cleanup_hosts` | `cleanupHosts(): void` | Remove um **bloco órfão** do `/etc/hosts` (sessão anterior fechada sem desligar) via a reversão elevada; **no-op sem prompt** se já estiver limpo. |

### 3.3 Contrato IPC — eventos

A casca emite; a UI escuta via `listen`.

| Evento | Payload | Quando |
|---|---|---|
| `proxy://status` | `Status` | Liga/desliga, re-resolução do IP. |
| `proxy://traffic` | `TrafficEntry` (resumo leve) | A cada requisição capturada. |
| `proxy://notice` | `Notice` (`level`: info/warn/error + `message`) | `502` (serviço local fora / stage inalcançável), `devsplit.yaml` recarregado, etc. |

### 3.4 Elevação (sessão root persistente)

Hoje **Linux/pkexec**. `PrivSession` mantém um `pkexec /bin/sh` **vivo**: a senha é pedida
**uma vez** (no spawn) e os scripts root são alimentados por stdin pelo resto da sessão —
acaba com o "duas senhas por ligar/desligar". `build_priv_script` grava o arquivo hosts
via heredoc (sem temp file → sem TOCTOU) e, no ligar, roda
`setcap cap_net_bind_service=+ep <exe>` + `sysctl -w net.ipv4.ip_unprivileged_port_start=443`
para bindar a `:443` sem root daí em diante. `which()` resolve caminhos absolutos (pkexec
zera o `PATH`). Detalhe e o plano macOS/Windows em `11` §3 e `04` §3.

### 3.5 Plugins, tray e hot-reload

`tauri_plugin_single_instance` (só 1 dono de `:443`), `tauri_plugin_shell` (rodar
`mkcert`), `tauri_plugin_autostart` (LaunchAgent no macOS). O `setup` carrega a config,
monta o **tray** e dispara um watcher que faz **polling de 2s** do `devsplit.yaml`:
mudou → recarrega do disco e emite `proxy://notice`.

### 3.6 Descoberta da config

`find_config_path` procura o `devsplit.yaml` subindo do cwd (`app/src-tauri`, `app/`, raiz
do repo) e, por fim, no `app_config_dir`. As **rotas runtime** editadas pela UI são
persistidas **separadas** do YAML (por perfil), para não destruir comentários/`extends`
que você edita à mão.
