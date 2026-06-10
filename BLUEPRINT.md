# devsplit — Blueprint do projeto (app desktop Tauri + Rust)

> App **desktop** que intercepta o gateway HTTP de um ambiente remoto (stage/HML)
> e faz **split por path-prefix**: alguns prefixos vão para serviços rodando na sua
> máquina, o resto faz **passthrough** para o ambiente remoto real. O front não muda.
> Interface bonita, cross-platform, config commitável e reutilizável por times.
>
> **Stack: Tauri v2** — núcleo em **Rust** (proxy/TLS/hosts/cert), UI em **React +
> Tailwind + shadcn/ui** (TypeScript). App nativo único (~15 MB), sem Node embutido.
> Working name: **devsplit** (alternativas na §13). Documento em pt-BR, denso, técnico.
> Fundamentado em pesquisa de prior art + camadas técnicas (fontes na §14).

---

## 1. Veredito: faz sentido? **Sim.**

Faz sentido e existe um **gap real** de mercado. Você já provou o conceito com a
implementação hand-rolled em `agenciamento-pedagio/docker/local-proxy` (Traefik +
mkcert + `/etc/hosts` + scripts bash). O que falta é **produtizar**: tirar do
"3 passos com sudo + script bash que só roda no seu Arch" para um **app com interface**
cross-platform, gerenciável, com config de time.

**Por que faz sentido (e não é reinventar a roda):**
- O problema é concreto e comum: microsserviços demais para caber na RAM do dev.
  Rodar 1-2 serviços local + apontar o resto para stage é a economia certa.
- **Nenhuma ferramenta existente cobre o caso exato** (ver §3). A mais próxima em
  filosofia, [Localias](https://github.com/peterldowns/localias), tem uma issue
  **aberta** pedindo exatamente isto — proxy de paths diferentes do mesmo domínio
  para portas diferentes ([#39](https://github.com/peterldowns/localias/issues/39)) —
  e ninguém entregou.
- A parte difícil (TLS confiável, validação de cert remoto por SNI, edição de hosts)
  é reusável (mkcert, rustls, rcgen). O valor é a **costura** + a **interface**.

**Risco honesto:** WebKitGTK no Linux (o webview do Tauri) é instável e exige UI
conservadora e QA real nos 3 SOs (§7.7). E há armadilhas de TLS/DNS/elevação
cross-platform (§7). Tudo factível; o diferencial é claro.

---

## 2. O insight central

A implementação de referência já contém a ideia certa, invariante do produto:

```
front (browser/app) ──▶ https://api.stage.acme.com
                              │  (no SEU PC o domínio resolve p/ 127.0.0.1)
                              ▼
                     devsplit (núcleo Rust, :443, TLS confiável)
                       ├─ /transporte, /auth  ─▶ 127.0.0.1:3000 / :3001  (LOCAL)
                       └─ /*  (resto)          ─▶ IP_REAL_DO_STAGE:443    (PASSTHROUGH)
                                                  ServerName (SNI) = api.stage.acme.com
                                                  (cert remoto VALIDADO de verdade)
```

Quatro propriedades **não-negociáveis** (vêm do que você acertou):
1. **Transparência:** o front continua apontando para a URL de stage. Sem PAC, sem
   proxy de sistema, sem mudar `.env`. Reverse-proxy por domínio, não forward-proxy de
   debug (separa de Charles/Proxyman/Whistle).
2. **Split por path-prefix com passthrough real:** prefixo mais específico ganha; o
   catch-all vai para o **remoto verdadeiro** (separa de Localias/Valet/DDEV).
3. **Cert remoto validado no passthrough** — conecta no IP real + `ServerName`=FQDN,
   **sem** desabilitar verificação. Em Rust (rustls) isso é direto e seguro (§5/§7).
4. **Mesmo ambiente:** interceptar **stage**, nunca prod — token e banco do dev no mesmo
   ambiente, serviço local valida contra o mesmo issuer. Regra de produto (§7.3).

---

## 3. Diferenciação (prior art) — onde está o gap

Legenda: ✅ sim · ⚠️ parcial · ❌ não.

| Classe | Exemplos | Transparente (domínio) | Split por path-prefix p/ remoto | TLS+DNS local auto-geridos | Fora de k8s | Tem UI / UX de time |
|---|---|---|---|---|---|---|
| **k8s inner-loop** | Telepresence, mirrord, Tilt, DevSpace | ⚠️ | ⚠️ (por header/serviço) | n/a | ❌ exige cluster | ⚠️ |
| **Tunnels** | ngrok, localtunnel, cloudflared | ✅ (público) | ⚠️ (problema inverso) | ✅ (edge) | ✅ | ⚠️ |
| **Debug/MITM** | Charles, Proxyman, Whistle, mitmproxy | ❌ (forward proxy) | ✅ (map remote/local) | ⚠️ (CA manual) | ✅ | ✅ (mas debug individual) |
| **Reverse proxies** | Caddy, Traefik, nginx | ✅ | ✅ (nativo) | ⚠️ | ✅ | ❌ (engine cru) |
| **Dev-env DNS+TLS** | Valet, DDEV, Lando, Localias | ✅ | ❌ (domínio→porta local) | ✅ | ✅ | ⚠️ (CLI) |
| **devsplit** | — | ✅ | ✅ | ✅ | ✅ | ✅ (app desktop) |

**A lacuna:** `reverse-proxy transparente por domínio + split por path-prefix com
passthrough p/ remoto real (cert validado) + TLS+DNS local auto-geridos + app de
gerência por time, fora de k8s` **não é atendida por nenhuma ferramenta única**. E
ninguém com **interface visual** — quem chega perto (Charles/Proxyman) é forward-proxy
de debug individual, não infraestrutura de dev transparente por time.

---

## 4. Arquitetura — um app Tauri, três partes

Um **único processo** Tauri. A UI bonita é o rosto; o núcleo Rust é o cérebro; os
plugins nativos dão a casca de SO.

```
┌─ APP devsplit (1 processo Tauri) ──────────────────────────────────────────┐
│                                                                             │
│  WEBVIEW UI  (React + Vite + Tailwind v4 + shadcn/ui, TypeScript)           │
│      ▲  a tela bonita: botão on/off, lista de rotas, status, log ao vivo    │
│      │  invoke() → comandos        listen() ← eventos (status, tráfego)     │
│      ▼                                                                      │
│  NÚCLEO RUST  (roda no runtime tokio do Tauri)                              │
│   ├ Motor de proxy   axum + hyper + rustls  ── segura :443                  │
│   │    ├ /transporte, /auth   →  127.0.0.1:3000 / :3001    (LOCAL)          │
│   │    └ /*  (catch-all)       →  IP_REAL:443  SNI=FQDN     (PASSTHROUGH)    │
│   ├ TLS/CA           rcgen (gera CA+leaf) + mkcert (instala na trust store) │
│   ├ hosts            módulo próprio: bloco demarcado + backup + atômico     │
│   ├ DNS direto       hickory-resolver — pega o IP real, fura o /etc/hosts   │
│   └ elevação         elevated-command — 1 prompt p/ hosts + CA + setcap     │
│                                                                             │
│  PLUGINS NATIVOS:  tray (on/off)  ·  autostart  ·  single-instance  ·       │
│                    window-state  ·  notification  ·  updater  ·  opener     │
└─────────────────────────────────────────────────────────────────────────────┘

  Bootstrap (1x, elevado):  setcap (Linux) · /etc/hosts · mkcert -install
```

**Por que esta forma é a ideal:** como a lógica é Rust, ela roda **dentro** do Tauri
(no mesmo tokio) — não precisa de servidor HTTP intermediário nem de processo separado.
A UI conversa com o núcleo por **comandos** (`invoke`, ações como "ligar/desligar",
"adicionar rota") e **eventos** (`app.emit`/`listen`, para status e log de tráfego ao
vivo). Sem porta de controle extra, sem CORS, sem daemon órfão.

### As partes
- **L1 — UI (webview):** React + Tailwind + shadcn/ui. É o produto que o usuário vê (§6).
- **L2 — Motor de proxy (Rust):** axum (servidor + roteamento + upgrade de WebSocket)
  sobre hyper; rustls termina o TLS com o cert local; o catch-all usa hyper de baixo
  nível sobre um `TlsStream` aberto no IP real com `ServerName`=FQDN (valida o cert sem
  `dangerous`); `copy_bidirectional` faz o passthrough de WebSocket; tabela de rotas em
  `ArcSwap` para reload sem dropar conexão; `tower-http` responde o preflight CORS.
- **L3 — TLS/CA (Rust):** `rcgen` gera a CA local + o leaf (SAN wildcard + IP, validade
  ≤825 dias). O **binário mkcert** (com `CAROOT` apontando para a CA do rcgen) faz o
  `-install` que cobre o trust store do SO **e** o NSS (Firefox/Chrome) — a parte chata
  que ninguém quer reimplementar. (Evolução: trust 100% Rust depois, §12.)
- **L4 — DNS direto (Rust):** `hickory-resolver` com nameserver explícito (8.8.8.8/DoH)
  pega o IP real do FQDN **sem** passar pelo `/etc/hosts` (senão = loop, §7.1).
- **L5 — hosts (Rust):** módulo próprio — bloco demarcado idempotente + backup + escrita
  atômica (`tempfile`+persist) + 1 nome por linha (limite do Windows).
- **L6 — elevação (Rust):** `elevated-command` agrupa [editar hosts + `mkcert -install` +
  `setcap`] num **único** prompt nativo (UAC/pkexec/osascript). O app roda sem root.
- **L7 — casca nativa (plugins Tauri):** tray como fonte de verdade do on/off, autostart,
  single-instance (só 1 dono de :443), window-state, notification, updater, opener.

---

## 5. Stack recomendada + crates

### Backend (Rust, dentro do tokio do Tauri)
| Papel | Crate | Por quê / nota |
|---|---|---|
| Servidor HTTP + roteamento + WS upgrade | **axum 0.8** (sobre hyper 1.x) | Roteamento ergonômico + handshake de WebSocket pronto; embarca no `tauri::async_runtime::spawn`. |
| Cliente upstream (catch-all) | **hyper** baixo nível + hyper-util | Único caminho que faz pin de IP **e** SNI validado **e** passthrough de WS. (reqwest `.resolve()` serviria só p/ HTTP sem WS.) |
| TLS (servidor + cliente) | **rustls 0.23 + tokio-rustls** | `ResolvesServerCertUsingSni` no servidor; `ClientConfig` + `ServerName`=FQDN no upstream (sem `dangerous`). |
| Roots p/ validar o cert remoto | **webpki-roots** (ou rustls-native-certs) | Cert do stage é público; valida normal. |
| DNS direto (fura /etc/hosts) | **hickory-resolver 0.26** | `getaddrinfo`/std NÃO serve (lê o hosts → loop). |
| Hot-reload de rotas | **arc-swap 1.7** | `ArcSwap<RouteTable>` lock-free; conexões em voo seguem. |
| CORS preflight | **tower-http 0.6** `CorsLayer` | Responde OPTIONS automático. |
| WS (se inspecionar frames) | tokio-tungstenite | Opcional; passthrough puro é byte-copy. |
| Gerar CA + leaf | **rcgen 0.14** | SAN wildcard + IP, validade ≤825 dias. |
| Instalar trust (system + NSS/Firefox) | **mkcert** (binário, `CAROOT`→CA do rcgen) | Aposta segura do MVP; NSS/Firefox é um pântano p/ reimplementar. |
| Editar /etc/hosts | **módulo próprio** (+ `tempfile`) | bloco demarcado + backup + atômico + 1 nome/linha. |
| Elevação pontual | **elevated-command 1.1** | UAC/pkexec/osascript; feito p/ Tauri. |
| bind :443 | `tokio::net::TcpListener` | Linux: `setcap` 1x; macOS: `0.0.0.0:443` sem root (Mojave+); Windows: livre. |

> **rejeitado:** `pingora` (gerencia o próprio tokio runtime → conflita com o do Tauri;
> pré-1.0; Linux tier-1). `fastcert` (mkcert-em-Rust, dez/2025, ~224 downloads, shell-a
> `sudo` sem GUI — imaturo; reavaliar em ~1 ano).

### Frontend (TypeScript, no webview do Tauri)
| Papel | Escolha | Nota |
|---|---|---|
| Framework | **React 19 + Vite** | Maior ecossistema "bonito-rápido"; templates Tauri+shadcn prontos. (Alternativa leve: SvelteKit SPA.) |
| Estilo | **Tailwind CSS v4** (OKLCH) | — |
| Componentes | **shadcn/ui** (Radix, código no seu repo) | Visual "Linear/Vercel" pronto; dark mode = classe `.dark`. |
| Animação | **Motion** + CSS transitions | Regras de microinteração (Emil Kowalski); **só `transform`/`opacity`** (WebKitGTK). |
| Chart de tráfego ao vivo | **TradingView Lightweight Charts** (canvas) | Leve, 60 FPS; alimentar via ref, fora do render do React. |
| Updates ao vivo UI←núcleo | **eventos Tauri** (`app.emit`/`listen`), com throttle/batch | Substitui SSE/WebSocket de controle; canal nativo, sem porta extra. |

### Casca nativa (plugins Tauri v2)
`tray-icon` (core) · `tauri-plugin-autostart` · `tauri-plugin-single-instance` ·
`tauri-plugin-window-state` · `tauri-plugin-notification` · `tauri-plugin-updater` ·
`tauri-plugin-opener` · `tauri-plugin-shell` (só p/ rodar o `mkcert` no bootstrap).

---

## 6. A interface (a "UI bonita")

Linguagem visual de **dev-tool premium** (referências: Linear, Raycast, Tailscale,
OrbStack). Regras concretas: **um único accent colorido** (o botão on/off é o único
elemento "vivo"; resto em escala de cinzas frios), tipografia Inter com escala de 8px,
densidade de dados para power-user, dark mode, **animação só no ocasional** (abrir
painel, toast de "cert instalado") — nunca no toggle repetido.

**Telas / componentes:**
- **Cabeçalho com botão grande ligar/desligar** a interceptação + status colorido
  (verde = ativo) e o domínio de stage sendo interceptado.
- **Seletor de perfil** (ex.: "só transporte", "tudo local") e de ambiente (stage/qa).
- **Lista de rotas:** cada linha `/transporte → localhost:3000` com **interruptor** por
  rota + badge `local`/`passthrough`; botão "+ adicionar rota". Tabela densa.
- **Painel de saúde (o "doctor" visual):** ✅ cert confiável · ✅ hosts ok · ✅ porta 443
  livre · ✅ upstream responde — cada um com botão "corrigir" quando ✗.
- **Log de tráfego ao vivo:** lista virtualizada (método, host, path, decisão local/
  passthrough, status, latência) + **gráfico de req/s** em canvas.
- **Ícone na bandeja** (fonte de verdade do on/off) com menu: Ligar/Desligar split,
  Abrir painel, Sair. Fecha a janela → continua na bandeja; sai → para de interceptar.

**Pegadinha de design (Linux):** WebKitGTK borra o app durante animação CSS e é atrás do
Chromium em features modernas (`backdrop-filter`, etc.). Regra: CSS conservador, animar
só `transform`/`opacity`, e **QA real no Linux**. Distribuir no Linux via **AppImage**
(embute o WebKitGTK 4.1, previsível).

---

## 7. Pontos perigosos — red-team e blindagem

### 7.1 Loop no passthrough
O `/etc/hosts` faz o FQDN resolver p/ 127.0.0.1 para **todo** processo, incluindo o
próprio app. Mitigação: resolver o **IP real** via **DNS direto** (`hickory-resolver` com
nameserver explícito — NÃO `getaddrinfo`, que lê o hosts) e conectar nesse IP com
`ServerName`=FQDN. IP rotaciona (cloud LB)? Re-resolver por TTL + botão "re-resolver" na
UI. Cinto e suspensório: header `X-Devsplit-Hop` (recusa 502 se repetido) e recusar
conectar se o destino == IP de bind próprio. Stage só via VPN? Validar rota ao IP no
`up`/doctor.

### 7.2 WebSocket/socket.io
Rotear **tanto** o prefixo do app **quanto** `/socket.io` para o mesmo serviço local. O
passthrough de WS é byte-copy (`copy_bidirectional`) após o `101`; long-polling do
socket.io cai nas mesmas rotas como HTTP normal. gRPC fica fora do MVP (raro neste stack:
microsserviços falam RabbitMQ, não gRPC pelo gateway).

### 7.3 HSTS, cookies, CORS
- O cookie de auth do gateway real (`Domain=api.stage.acme.com`, `Secure`, `SameSite`)
  é enviado ao **mesmo host** → chega tanto no passthrough quanto no serviço local. **É o
  truque que faz funcionar:** o local recebe o mesmo token, do mesmo ambiente. Por isso
  **stage, nunca prod** (mesmo issuer/JWKS/banco).
- **HSTS:** se o stage manda HSTS, o browser força HTTPS e **não deixa** clicar em cert
  não confiável → o cert local **tem** que ser confiável (mkcert). doctor detecta.
- **CORS:** `tower-http CorsLayer` responde o preflight p/ os paths locais (origens do
  front: stage + localhost).

### 7.4 Trust de CA headless / sem admin
macOS Big Sur+ abre **prompt GUI mesmo como root** p/ o Keychain admin → quebra CI. As
APIs de Keychain exigem **app assinado** (sem assinatura = `MissingEntitlement`), por isso
no MVP o `mkcert` (CLI com prompt) é mais confiável que a crate. Windows
`CurrentUser\Root` funciona **sem** admin; `LocalMachine\Root` pede UAC. Honestidade:
"zero-config" vale no caminho feliz (dev na máquina, 1 prompt); headless/CI **não** é
zero-config. Leaf ≤825 dias; re-trust só quando a **root** muda.

### 7.5 Estado de time via git + reversão no crash
`/etc/hosts` é por-máquina → bloco sujo quebra o stage **só na máquina do dev**, não no
time. Reversão confiável: bloco demarcado + backup + escrita atômica; o app mantém a
entrada só enquanto a interceptação está ligada. Crash → detecta bloco órfão (PID morto
no state) e oferece limpeza; doctor sempre reporta. single-instance garante 1 dono de :443.

### 7.6 Segurança (blindar dia 1)
Um proxy local em :443 com **CA confiável na máquina** é vetor sério. Chave da CA root
(`rootCA-key.pem`): `0600`, por-usuário, nunca versionada/logada. Passthrough: **sempre**
validar o cert remoto (`ServerName`, sem `dangerous`); pin de IP evita redirecionamento.
`/etc/hosts`: escrever só no bloco demarcado, validar FQDN (allowlist do config). Regra
anti-prod: recusar domínios de produção por padrão. Releases assinados/notarizados.

### 7.7 WebKitGTK (Linux) — o maior risco de UI
Instável, borra o app durante animação CSS, atrás em CSS/JS moderno, `contenteditable`
quebrado. Mitigação: UI conservadora (só `transform`/`opacity`), evitar `backdrop-filter`/
contenteditable, AppImage p/ embutir o runtime, e QA real nos 3 SOs.

---

## 8. Config de time (`devsplit.yaml`)

O app gerencia tudo pela UI, **mas** lê/escreve um `devsplit.yaml` commitável na raiz do
repo, para o time inteiro compartilhar com um `git pull` (modelo Localias/DDEV).

```yaml
version: 1
upstream:
  host: api.stage.acme.com          # FQDN de stage → /etc/hosts (127.0.0.1)
  passthrough:
    resolve: api.stage.acme.com     # re-resolver via DNS direto (ignora /etc/hosts), TTL
    sni: api.stage.acme.com         # ServerName no upstream → cert remoto VALIDADO
    verify: true                    # NUNCA false em stage real
  kind: stage                       # stage|qa  (prod recusado por padrão)
tls:
  provider: mkcert                  # mkcert | self (rcgen em-processo)
  leaf_max_days: 825
profiles:
  default:
    routes:
      - { prefix: /transporte, target: "http://127.0.0.1:3000", also: [/socket.io] }
      - { prefix: /auth,       target: "http://127.0.0.1:3001" }
  full-local:
    extends: default
    routes:
      - { prefix: /financeiro, target: "http://127.0.0.1:3002" }
environments:
  qa:
    upstream: { host: api.qa.acme.com, passthrough: { resolve: api.qa.acme.com, sni: api.qa.acme.com, verify: true } }
cors:
  enabled: true
  allow_origins: ["https://app.stage.acme.com"]
  allow_origins_regex: ['^https?://(localhost|127\.0\.0\.1)(:\d+)?$']
  allow_credentials: true
```

Estado runtime (rotas ativas, PID, IP pinado, hash do cert) fica **separado** em
`$XDG_STATE_HOME`/AppData — nunca no YAML commitado.

---

## 9. Build, distribuição & elevação

**Build:** `tauri-action` numa **matriz de CI** (cross-compile do Tauri não é viável —
buildar por-OS). Gera `.dmg`/`.app` (mac), `.msi`/`.exe` (Win), `.deb`/`.AppImage`/`.rpm`
(Linux). Assinatura: macOS Developer ID + **notarização**; Windows EV/Azure Trusted
Signing. Auto-update via `tauri-plugin-updater` (com chave de assinatura).

**Tamanho:** ~15 MB (Tauri puro) + o binário **mkcert** bundlado (~4 MB, Go) → ainda
muito menor que Electron (80-150 MB). *Atenção:* `externalBin` (o mkcert) tem um bug
conhecido com a **notarização macOS** ([#11992](https://github.com/tauri-apps/tauri/issues/11992))
— assinar o binário do mkcert à mão. (Alternativa: não bundlar, detectar/baixar mkcert no
primeiro uso, ou migrar p/ trust 100% Rust — §12.)

**Elevação pontual** (`elevated-command`, app roda sem root):

| SO | bind :443 | editar hosts | instalar CA |
|---|---|---|---|
| Linux | `setcap` no binário (1x; reaplicar no update) | pkexec só na escrita | NSS user (sem root) + system (sudo) |
| macOS | nenhuma (bind `0.0.0.0:443`, Mojave+) | sudo pontual | Keychain (prompt GUI 1x) |
| Windows | nenhuma (sem porta privilegiada) | Admin (UAC) | `CurrentUser\Root` (sem admin) → `LocalMachine\Root` (UAC) |

Idealmente **1 prompt** no setup agrupando hosts + CA + setcap.

---

## 10. Estrutura de repositório

Layout padrão Tauri v2 (`src/` = UI React, `src-tauri/` = núcleo Rust):

```
devsplit/
├── package.json · vite.config.ts · tailwind.config.ts   # frontend
├── src/                          # L1 — UI React (TypeScript)
│   ├── App.tsx · main.tsx
│   ├── components/               # shadcn/ui + componentes do painel
│   ├── views/                    # Rotas · Tráfego · Certificado · Hosts · Config
│   ├── lib/ipc.ts                # invoke() comandos + listen() eventos do núcleo
│   └── lib/charts.ts             # Lightweight Charts (req/s)
├── src-tauri/
│   ├── Cargo.toml · tauri.conf.json · capabilities/      # casca + ACL
│   ├── binaries/mkcert-<triple>  # bootstrap de trust (externalBin)
│   └── src/
│       ├── main.rs               # setup: spawn do proxy, tray, plugins
│       ├── commands.rs           # #[tauri::command] start/stop/route/cert/doctor
│       ├── proxy/                # L2 — axum + hyper + rustls + ArcSwap
│       │   ├── server.rs · router.rs · upstream.rs (pin IP + SNI) · ws.rs
│       ├── tlsca.rs              # L3 — rcgen + orquestra mkcert
│       ├── dns.rs                # L4 — hickory (DNS direto)
│       ├── hostsfile.rs          # L5 — bloco demarcado + backup + atômico
│       ├── elevate.rs            # L6 — elevated-command
│       ├── config.rs             # devsplit.yaml (serde_yaml) + validação
│       └── state.rs              # ProxyConfig (ArcSwap) + estado runtime
├── packaging/                    # tauri-action CI, signing, install scripts
└── README.md
```

Fronteira-chave: a UI nunca fala com a rede do proxy direto — só com o núcleo via
comandos/eventos Tauri. O motor fica atrás de um módulo `proxy/` trocável.

---

## 11. MVP — fatia vertical (3–4 semanas)

Objetivo: **um app que substitui os scripts bash** da impl. de referência, em
**Linux + macOS** (Windows fast-follow), já com a interface.

**Dentro do MVP:**
- App Tauri com a UI essencial: botão on/off, lista de rotas (add/rm + toggle por rota),
  painel de saúde (doctor visual), e log de tráfego ao vivo simples.
- Núcleo Rust: motor axum+hyper+rustls (TLS local, roteamento Host+PathPrefix, passthrough
  IP+SNI validado, WebSocket, reload via ArcSwap, CORS via tower-http).
- TLS: rcgen gera a CA+leaf; orquestra `mkcert -install` (detecta/baixa mkcert) p/ o trust.
- hosts: módulo próprio idempotente + reversão no off/crash.
- DNS direto (hickory) + anti-loop (header `X-Devsplit-Hop` + recusa self-connect).
- Elevação agrupada (elevated-command). bind :443 (setcap Linux / 0.0.0.0 macOS).
- Lê/escreve `devsplit.yaml` mínimo (`upstream` + `profiles[].routes[]` + `cors`).
- Tray + single-instance.
- Distribuição: `.AppImage` (Linux) + `.dmg` (macOS) via CI.

**Fora do MVP (corta sem dó):**
- Windows (fast-follow: UAC, CurrentUser\Root, conflito :443/http.sys).
- `environments` + `extends` + `devsplit.local.yaml`.
- gRPC, mDNS `.local`, dnsmasq/split-DNS, ACME.
- Updater/autostart polidos, chart de req/s avançado (começa com lista de log).
- Trust 100% Rust (usa mkcert no MVP).

**Critério de pronto:** num repo NestJS com 2+ serviços, abrir o app, ligar o split no
perfil X → o front de stage cai no serviço local nos prefixos certos e no remoto no
resto, cert confiável, socket.io funcionando; desligar restaura o `/etc/hosts`. Tudo sem
Docker e sem editar YAML do Traefik à mão.

---

## 12. Roadmap

- **v0.1 (MVP):** §11. Linux+macOS, app Tauri+Rust, UI essencial, mkcert p/ trust.
- **v0.2:** Windows completo; updater + autostart; `environments`+`extends`; chart de
  req/s; polish de UI (animações, dark/light, tray rico).
- **v0.3:** trust **100% Rust** (security-framework + schannel + certutil) p/ remover o
  binário mkcert e o bug de notarização; `devsplit.local.yaml`; re-resolução de IP com
  healthcheck; perfis avançados.
- **v0.4:** mDNS `.local` p/ QA mobile; inspeção/replay de requests (estilo Proxyman);
  exportar/importar config de time.
- **v0.5+:** gRPC (HTTP/2 passthrough), fallback dnsmasq/split-DNS p/ wildcard,
  plugin/hook (`on_up`/`on_route_add`), registry de perfis de time.

---

## 13. Nome

Working name **devsplit** é claro e descritivo. Alternativas brandáveis ("desviar alguns
caminhos p/ local, resto p/ remoto"):
- **Detour** — desvio de rotas; memorável, verbo natural.
- **Splice** — emendar local no fluxo remoto; curto.
- **Sidedoor** — porta lateral p/ o ambiente remoto.
- **Patchbay** — painel de roteamento (metáfora de áudio/telecom).

Recomendo **Detour** ou **Splice** se quiser marca; **devsplit** se quiser descritivo.
Checar disponibilidade no GitHub e (pro Homebrew/Scoop) nos gerenciadores antes de cravar.

---

## 14. Fontes (pesquisa)

**Prior art:** Telepresence https://telepresence.io/docs/reference/architecture · Localias
https://github.com/peterldowns/localias + #39 https://github.com/peterldowns/localias/issues/39
· Caddy https://caddyserver.com/docs/caddyfile/directives/reverse_proxy · DDEV
https://ddev.com/blog/ddev-local-trusted-https-certificates/ · Proxyman https://proxyman.com/

**Tauri v2:** sidecar/externalBin https://v2.tauri.app/develop/sidecar/ · tray
https://v2.tauri.app/learn/system-tray/ · autostart https://v2.tauri.app/plugin/autostart/
· single-instance https://v2.tauri.app/plugin/single-instance/ · updater
https://v2.tauri.app/plugin/updater/ · async runtime https://docs.rs/tauri/latest/tauri/async_runtime/
· eventos p/ UI https://v2.tauri.app/develop/calling-frontend/ · build/sign
https://v2.tauri.app/distribute/ · bug externalBin+notarize
https://github.com/tauri-apps/tauri/issues/11992 · tamanho https://v2.tauri.app/concept/size/

**Motor Rust:** axum https://github.com/tokio-rs/axum · axum WS
https://docs.rs/axum/latest/axum/extract/ws/struct.WebSocketUpgrade.html · hyper
https://docs.rs/hyper · rustls https://docs.rs/rustls · ResolvesServerCertUsingSni
https://docs.rs/rustls/latest/rustls/server/struct.ResolvesServerCertUsingSni.html ·
tokio-rustls https://github.com/rustls/tokio-rustls · hyper upgrades (WS)
https://seanmonstar.com/blog/http-upgrades-with-hyper/ · copy_bidirectional
https://docs.rs/tokio/latest/tokio/io/fn.copy_bidirectional.html · arc-swap
https://docs.rs/arc-swap · tower-http CORS https://docs.rs/tower-http/latest/tower_http/cors/
· hickory-resolver https://docs.rs/hickory-resolver · pingora (rejeitada)
https://github.com/cloudflare/pingora

**TLS/CA/hosts/elevação Rust:** rcgen https://github.com/rustls/rcgen · mkcert
https://github.com/FiloSottile/mkcert · fastcert (imatura) https://crates.io/crates/fastcert
· security-framework https://github.com/kornelski/rust-security-framework · schannel
https://docs.rs/schannel · hostsfile https://crates.io/crates/hostsfile · elevated-command
https://crates.io/crates/elevated-command · macOS :443 sem root
https://zameermanji.com/blog/2024/1/5/binding-to-privileged-ports-without-root-on-macos/

**UI:** Tauri frontend https://v2.tauri.app/start/frontend/ · WebKitGTK instável
https://github.com/orgs/tauri-apps/discussions/8524 · animação borra (Linux)
https://github.com/tauri-apps/tauri/discussions/9088 · shadcn/ui
https://ui.shadcn.com/docs/tailwind-v4 · Lightweight Charts
https://github.com/tradingview/lightweight-charts · Linear design
https://linear.app/now/how-we-redesigned-the-linear-ui · Raycast menubar
https://developers.raycast.com/api-reference/menu-bar-commands · template Tauri+React+shadcn
https://github.com/kitlib/tauri-app-template · emil-design-eng / animations.dev https://animations.dev/
```
