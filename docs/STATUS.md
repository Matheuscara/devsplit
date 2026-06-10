# STATUS.md — Painel de entregas do devsplit

> **Para que serve:** visão de alto nível do que já está implementado e verificado vs. o
> que falta. O detalhe de design vive no `BLUEPRINT.md`; o contrato implementado, nos
> demais `docs/`.
>
> **Regra de manutenção:** toda mudança de comportamento deve atualizar este arquivo
> (mude o ✅/🟡/⛔/⬜ do item e a "Última atualização" no rodapé) junto do gate de
> verificação correspondente.

**Legenda:** ✅ entregue e verificado · 🟡 parcial / em aberto · ⛔ bloqueado (depende de
algo externo) · ⬜ não iniciado

---

## Resumo

| Camada | Estado |
|---|---|
| **Núcleo `devsplit-core`** (proxy, TLS, DNS, hosts, config) | ✅ **implementado e testado** — `cargo test -p devsplit-core` = **18 passam** (1 de rede `#[ignore]`), inclui e2e TLS e redação de headers |
| **Frontend React** (`app/src/`) | ✅ **implementado** — `npm run build` passa; explorável no navegador com IPC mock |
| **Casca Tauri** (`app/src-tauri/`) | ✅ **compila e RODA no Linux** (webkit2gtk-4.1 instalado) — app abre, lê o `devsplit.yaml`, doctor/rotas/perfis vêm do backend real (badge "TAURI", sem mock) |
| **Elevação cross-platform** | ✅ **Linux** (pkexec/setcap/sysctl, verificado) · 🟡 **macOS** (`osascript`) e **Windows** (UAC/PowerShell) **implementados** — compilam no Linux via despacho `cfg!`; runtime a validar no SO/CI |
| **Build macOS/Windows** | 🟡 **CI matriz escrito** (`.github/workflows/build.yml`, `tauri-action` ubuntu/macos/windows); pendente rodar no GitHub |
| **Distribuição** (CI matriz, assinatura, updater) | 🟡 **CI de build/release escrito** (release draft em tag `v*`); assinatura/notarização pendente (preencher secrets) |

> **Onde isso coloca o projeto hoje:** no **Linux**, o app está **rodando de ponta a
> ponta** — a parte difícil (proxy transparente com TLS local, split por path-prefix,
> passthrough validado por SNI, anti-loop, WebSocket, hot-reload, captura de tráfego) está
> **provada por testes** e a casca Tauri **compila, abre e fala com o backend real**. O que
> falta para "app rodando nos 3 SOs" é cola de plataforma (elevação macOS/Windows + buildar
> com o webview de cada SO + packaging/assinatura), **não risco de design**.

> **Como rodar (Linux, hoje):** `cd app/src-tauri && cargo run`. O app carrega o frontend
> **embutido no binário** (não precisa de servidor). **Não** use `cargo tauri dev` em
> máquina apertada de RAM — ele sobe o vite dev server (pesado, pode dar OOM). Ver
> `getting-started.md` e `04` §1.

---

## Núcleo (`devsplit-core`) ✅

**Verificação:** `cargo test -p devsplit-core` → **18 passam, 1 ignorado (rede), 0 falhas.**

| Item | Módulo | Status | Verificação |
|---|---|---|---|
| Tipos do contrato (`ProxyConfig`/`Route`/`RouteTable`/`PassthroughTarget`/`HostConfig`/`HeaderPair`/`TrafficEvent`/`Decision`) | `types.rs` | ✅ | usados em todos os testes |
| Roteamento por Host+PathPrefix (mais específico vence) | `proxy` | ✅ | `longest_prefix_wins`, `no_match_falls_through` |
| Terminação TLS local + forward local (e2e) | `proxy` | ✅ | `e2e_local_forward_over_tls` (cliente TLS → proxy → backend) |
| Passthrough IP pinado + SNI validado (sem `dangerous`) | `proxy` | ✅ | `build_client_config` (webpki-roots) |
| Anti-loop (`X-Devsplit-Hop` → 508, recusa loopback) | `proxy` | ✅ | implementado em `handle`/`forward_passthrough` |
| WebSocket transparente (upgrade + `copy_bidirectional`) | `proxy` | ✅ | `upgrade_detection` |
| Captura de tráfego (headers redatados + body ≤256KiB) | `proxy` | ✅ | **`redacts_sensitive_headers`** (cookie/x-api-key/*secret*/*token* → `<redacted>`; `authorization` visível; nada sensível vaza) + `preview` |
| CORS preflight + espelho de origin | `proxy` | 🟡 | inline; não aplica allowlist de `allow_origins` (`02` §2.3, `10` §7) |
| CA + leaf (rcgen, SAN wildcard/IP, ≤825d) | `tlsca` | ✅ | `ensure_ca_generates_and_reloads`, `issue_leaf_and_build_server_config` |
| mkcert install (system+NSS) | `tlsca` | ✅ | `mkcert_install` (shell-out) |
| DNS direto (hickory, ignora hosts) | `dns` | ✅ | `resolves_public_domain` (`#[ignore]`, rede) — **confirmado** com `--ignored` |
| Arquivo hosts idempotente (bloco + backup + atômico + revert) | `hostsfile` | ✅ | 7 testes (idempotência on-disk, roundtrip, dedup/revert) |
| Parser `devsplit.yaml` (perfis/`extends`/`environments`) | `config` | ✅ | `parse_and_convert_default_profile`, `extends_merges_routes`, `environment_overrides_upstream` |

---

## Frontend (`app/src/`) ✅

**Verificação:** `npm run build` (`tsc -b && vite build`) passa; UI explorável headless
(`RUNTIME === "mock"`). **Renderiza com dados reais no app Tauri** (verificado no Linux).

| Item | Status | Nota |
|---|---|---|
| Shell + navegação (6 telas) + botão on/off + doctor | ✅ | `App.tsx` |
| Telas Rotas / Tráfego / Sessão / Certificado / Hosts / Config | ✅ | `views/` |
| Inspector: lista ao vivo, detalhe, copy-as-curl, export HAR, busca | ✅ | `TrafficView` + `lib/export.ts` |
| Painel JWT/Sessão (decode do Bearer) | ✅ | `SessionView` + `lib/jwt.ts` |
| Onboarding animado | ✅ | `Onboarding.tsx` |
| Command palette (Cmd-K, fuzzy) | ✅ | `CommandPalette.tsx` |
| Toasts (consomem `proxy://notice`) | ✅ | `Toast.tsx` |
| Botão "Limpar" do bloco órfão de hosts (chama `cleanup_hosts`) | ✅ | `RoutesView` (painel Saúde) + `lib/ipc.ts` `cleanupHosts()` |
| Camada IPC (real Tauri + mock) | ✅ | `lib/ipc.ts` |
| Design tokens dark-only (Tailwind v4 `@theme`) | ✅ | `index.css` |
| shadcn/ui · Motion · gráfico de req/s em canvas | ⬜ | citados no desenho, **não** embutidos (`03` §1) |

---

## Casca Tauri (`app/src-tauri/`) ✅ (Linux)

**Status:** comandos/eventos/tray/elevação **escritos, compilados e em execução no Linux**
(`cargo check`/`cargo build`/`cargo run` ok; `webkit2gtk-4.1` 2.52.4 instalado). App abre e
fala com o backend real.

| Item | Status | Nota |
|---|---|---|
| Comandos IPC (**15**) | ✅ (Linux) | inclui `cleanup_hosts` (limpa bloco órfão sem religar); ver `02` §3.2 |
| Eventos `proxy://status`/`traffic`/`notice` | ✅ (Linux) | emitidos pela casca |
| Estado runtime + ring buffer (500) | ✅ (Linux) | `AppState` |
| `run_doctor` **state-aware** (cert/hosts/upstream) | ✅ (Linux) | hosts: ligado→bloco deve existir; **desligado+bloco = órfão (aviso)** |
| Sessão root persistente (1 prompt, pkexec) | ✅ (Linux) | `setcap`/`sysctl`/hosts via heredoc; **só Linux** |
| Tray + single-instance + autostart | ✅ (Linux) | plugins registrados; app sobe sem panic (smoke test) |
| Hot-reload (`devsplit.yaml`, polling 2s) | ✅ (Linux) | watcher no `setup` |
| Re-resolução de IP (auto 60s + manual) | ✅ (Linux) | task + `reresolve_upstream` |
| Detecção de serviços locais (13 portas) | ✅ (Linux) | `detect_local_services` |
| **Run via assets embutidos** (`devUrl` removido) | ✅ | app não depende do vite dev server; `cargo run` carrega o `dist/` embutido |
| Elevação macOS/Windows | 🟡 | **implementada** (`apply_hosts_oneshot`→`apply_hosts_macos`/`apply_hosts_windows`, via `std::process`, despacho `cfg!`); **compila no Linux**, runtime validado por CI; ver `04` §3 |
| Build/execução macOS/Windows | 🟡 | CI `tauri-action` matriz escrito; pendente execução no GitHub Actions |

---

## Caminho crítico até "app rodando nos 3 SOs"

1. ✅ **Linux:** compilado, app rodando, QA inicial feito (UI real, doctor, rotas, tray).
2. ✅ **Elevação implementada nos 3 SOs** (Linux pkexec persistente; macOS `osascript … with
   administrator privileges`; Windows UAC `Start-Process -Verb RunAs`), despacho por `cfg!`
   runtime — **compila no Linux**; o runtime de macOS/Windows é validado pelo CI.
3. 🟡 **Buildar** nos 3 SOs (Tauri não cross-compila): CI matriz `tauri-action`
   (`.github/workflows/build.yml`) escrito; falta **rodar no GitHub Actions**.
4. 🟡 **Packaging/assinatura**: o CI cria **release draft** em tag `v*` com os instaladores;
   assinatura/notarização pendente (Developer ID/notarização mac, EV/Azure win — preencher secrets).
5. 🟡 Reconciliar o bloco **`cors`** (aplicar allowlist no motor ou remover do schema).

---

> **Última atualização:** Linux **end-to-end rodando** (núcleo **18 testes** + redação de
> headers; casca Tauri compila/executa; `devUrl` removido → assets embutidos, `cargo run`;
> doctor state-aware + `cleanup_hosts`). **Elevação cross-platform implementada** (Linux
> pkexec; macOS osascript; Windows UAC — despacho `cfg!`, compila no Linux) e **CI matriz
> `tauri-action`** escrito (`.github/workflows/build.yml`). Pendente: rodar o CI (build/
> release macOS/Windows) e assinatura/notarização.
