# devsplit

App desktop que intercepta o gateway HTTP de um ambiente remoto de **stage** e faz
**split por path-prefix**: alguns caminhos vão para serviços rodando na sua máquina, o
resto faz **passthrough** para o stage real (com o certificado validado de verdade). O
front não muda — continua apontando para a URL de stage.

Resolve a dor de "preciso subir 10 microsserviços + RabbitMQ + observabilidade pra rodar
1 task". Você roda só o serviço que está mexendo; o devsplit costura o resto.

> Stack: **Tauri v2** — núcleo em **Rust** (proxy/TLS/DNS/hosts), UI em **React +
> Tailwind + shadcn** (TypeScript). App nativo, leve. Veja o desenho completo em
> [`BLUEPRINT.md`](./BLUEPRINT.md).

```
front (browser) ─▶ https://api.stage.acme.com   (no seu PC resolve p/ 127.0.0.1)
                        ▼
                 devsplit (:443, TLS local confiável)
                   ├─ /transporte, /auth ─▶ 127.0.0.1:3000 / :3001   (LOCAL)
                   └─ /*                  ─▶ IP_REAL_DO_STAGE:443      (PASSTHROUGH, cert validado)
```

## Documentação

Guia completo em [`docs/`](./docs/) — comece por
[`docs/00-blueprint.md`](./docs/00-blueprint.md) (síntese + índice). Atalhos:
[`getting-started`](./docs/getting-started.md) ·
[`02-arquitetura`](./docs/02-arquitetura.md) ·
[`10-referencia-devsplit-yaml`](./docs/10-referencia-devsplit-yaml.md) ·
[`troubleshooting`](./docs/troubleshooting.md) ·
[`STATUS`](./docs/STATUS.md).

## Estado atual

| Parte | Status | Verificação |
|---|---|---|
| **Núcleo `devsplit-core`** (proxy, TLS, DNS, hosts, config) | ✅ implementado | **17 testes passam** (`cargo test -p devsplit-core`), incluindo um e2e cliente-TLS→proxy→backend |
| **Frontend React** (`app/`) | ✅ implementado | `npm run build` passa; roda no navegador com IPC mock |
| **Mockup de UI** (`design/mockup.html`) | ✅ pronto | abre no navegador |
| **Casca Tauri** (`app/src-tauri/`) | ✅ escrita | ⚠️ **não compilada aqui** — precisa de `webkit2gtk-4.1` (ver abaixo) |

O núcleo é a parte difícil e está **provado por testes**, inclusive o caminho ponta a
ponta (terminação TLS com cert local + roteamento Host+PathPrefix + forward local). A
casca Tauri é cola; só não foi compilada porque o webview do Linux (`webkit2gtk-4.1`) não
está instalado nesta máquina (precisa de `sudo`).

## Layout

```
devsplit/
├── BLUEPRINT.md                 # desenho completo do produto (pt-BR)
├── Cargo.toml                   # workspace (só o core entra; o Tauri fica fora p/ não exigir webkit)
├── crates/devsplit-core/        # NÚCLEO (Rust puro, sem GUI) — compila e testa em qualquer lugar
│   └── src/{types,proxy/,tlsca,dns,hostsfile,config}.rs
├── app/                         # frontend React + casca Tauri
│   ├── src/                     # UI React (views, components, lib/ipc.ts com mock)
│   └── src-tauri/               # casca Tauri (commands + tray + plugins) — precisa de webkit
├── design/mockup.html           # mockup visual da UI
└── examples/devsplit.yaml       # config de exemplo (commitável por time)
```

## Rodar

### Testes do núcleo (sem nada extra além do Rust)
```bash
cargo test -p devsplit-core            # 16 testes
cargo test -p devsplit-core -- --ignored   # + teste de rede (DNS direto)
```

### Frontend no navegador (com dados mock)
```bash
cd app && npm install && npm run dev   # http://localhost:5173
```

### App completo (Tauri) — precisa das libs nativas
```bash
# 1. lib do webview (Linux/Arch/CachyOS):
sudo pacman -S webkit2gtk-4.1 libsoup3   # Debian/Ubuntu: libwebkit2gtk-4.1-dev libssl-dev
# 2. CLI do Tauri:
cargo install tauri-cli --version "^2"
# 3. ícones (uma vez; precisa de uma imagem fonte):
cd app && cargo tauri icon caminho/para/logo.png
# 4. rodar / empacotar:
cargo tauri dev        # ou: cargo tauri build  (gera .AppImage/.deb/.dmg/.nsis)
```

### Pré-requisito de TLS confiável (qualquer SO)
O devsplit usa o [`mkcert`](https://github.com/FiloSottile/mkcert) para instalar a CA
local na trust store do SO e dos navegadores. Tenha-o no PATH (`mkcert -version`).

## Como funciona (invariantes)

- **Transparência:** o front aponta p/ a URL de stage; `/etc/hosts` manda o FQDN p/
  127.0.0.1; o devsplit termina o TLS com um cert local confiável.
- **Anti-loop:** o IP real do stage é descoberto via **DNS direto** (`hickory`,
  nameserver explícito) que ignora o `/etc/hosts` — senão o proxy conectaria em si mesmo.
- **Passthrough seguro:** conecta no IP real e valida o cert remoto contra o SNI (FQDN),
  **sem** desabilitar verificação.
- **Hot-reload:** a tabela de rotas vive num `ArcSwap`; ligar/desligar rotas não derruba
  conexões em voo.

Detalhes, riscos (HSTS/cookies, WebKitGTK, segurança) e roadmap em
[`BLUEPRINT.md`](./BLUEPRINT.md).
# devsplit
