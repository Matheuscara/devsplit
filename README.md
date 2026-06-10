<p align="center">
  <img src="design/banner.png" alt="devsplit" width="840">
</p>

<p align="center">
  <b>devsplit</b> — proxy de desenvolvimento que <b>divide o tráfego do seu gateway de stage</b>:
  os caminhos que você escolher rodam no seu <code>localhost</code>, o resto continua indo
  pro ambiente real. <b>Sem Docker, sem mudar o front.</b>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/Linux-rodando-34d399?style=flat-square" alt="Linux">
  <img src="https://img.shields.io/badge/macOS-implementado%20(via%20CI)-9aa0a6?style=flat-square" alt="macOS">
  <img src="https://img.shields.io/badge/Windows-implementado%20(via%20CI)-9aa0a6?style=flat-square" alt="Windows">
  <img src="https://img.shields.io/badge/core-18%20testes%20%E2%9C%93-34d399?style=flat-square" alt="testes">
  <img src="https://img.shields.io/badge/Tauri%20v2-Rust%20+%20React-2d3137?style=flat-square" alt="stack">
</p>

---

## O que é

Um **app de desktop** que fica entre o seu navegador e o gateway HTTP de um ambiente de
**stage**. Ele intercepta o domínio do stage na sua máquina e faz **split por path-prefix**:

- os prefixos que você marca (`/transporte`, `/auth`, …) vão para **serviços rodando no
  seu `localhost`**;
- **todo o resto** faz **passthrough** para o stage real — com o certificado validado de
  verdade (sem `insecureSkipVerify`).

O front **não muda**: continua apontando para a URL de stage. Você liga/desliga rotas numa
interface, e o devsplit cuida do TLS local confiável, do `/etc/hosts` e do roteamento.

## O problema que resolve

> "Pra rodar 1 task eu preciso subir 10 microsserviços + RabbitMQ + observabilidade. Meu
> PC não aguenta."

Com o devsplit você roda **só o serviço que está mexendo**. O resto da malha (auth, banco,
filas, os outros microsserviços) continua sendo o **stage**, que já está de pé. Acabou o
"sobe tudo localmente pra mexer em um pedaço".

```
front (browser) ─▶ https://api.stage.acme.com     (no seu PC, resolve p/ 127.0.0.1)
                        │
                        ▼
                 devsplit  (:443, TLS local confiável)
                   ├─ /transporte, /auth ─▶ 127.0.0.1:3000 / :3001   (LOCAL)
                   └─ /*                  ─▶ IP_REAL_DO_STAGE:443      (PASSTHROUGH, cert validado)
```

## Como funciona (invariantes)

- **Transparência:** o front aponta p/ a URL de stage; o `/etc/hosts` manda o FQDN p/
  `127.0.0.1`; o devsplit termina o TLS com um cert local **confiável** (via `mkcert`).
- **Anti-loop:** o IP real do stage é descoberto por **DNS direto** (`hickory`, nameserver
  explícito) que **ignora** o `/etc/hosts` — senão o proxy conectaria em si mesmo.
- **Passthrough seguro:** conecta no IP real e **valida** o cert remoto contra o SNI (FQDN).
- **Hot-reload:** a tabela de rotas vive num `ArcSwap`; ligar/desligar rota não derruba
  conexões em voo (inclusive WebSocket).
- **1 prompt de senha por sessão** (Linux): elevação só para editar o `/etc/hosts` e
  liberar a `:443`. O app roda sem root.

## Stack

**Tauri v2** — núcleo em **Rust** (`crates/devsplit-core`: proxy `hyper`+`rustls`, TLS
`rcgen`+`mkcert`, DNS `hickory`, hosts, config) e UI em **React + Tailwind + TypeScript**
(`app/src`). App nativo único, leve. Desenho completo em
[`BLUEPRINT.md`](./BLUEPRINT.md) e em [`docs/`](./docs/).

## Rodar

### Testes do núcleo (só precisa de Rust)

```bash
cargo test -p devsplit-core              # 18 testes (inclui e2e cliente-TLS → proxy → backend)
cargo test -p devsplit-core -- --ignored # + teste de rede (DNS direto)
```

### O app (Linux — verificado)

```bash
# 1. webview nativo (Arch/CachyOS):
sudo pacman -S webkit2gtk-4.1 libsoup3        # Debian/Ubuntu: libwebkit2gtk-4.1-dev librsvg2-dev
# 2. mkcert no PATH (TLS confiável):           https://github.com/FiloSottile/mkcert
# 3. rodar (carrega o frontend embutido no binário — sem dev server):
cd app && npm install && npm run build
cd src-tauri && cargo run
```

> **Não** use `cargo tauri dev` em máquina apertada de RAM: ele sobe o Vite (dev server),
> que pode dar OOM. O `cargo run` carrega o `dist/` embutido. Detalhes em
> [`docs/getting-started.md`](./docs/getting-started.md) e
> [`docs/troubleshooting.md`](./docs/troubleshooting.md).

### Configurar

Copie [`examples/devsplit.yaml`](./examples/devsplit.yaml) para a raiz do seu repo como
`devsplit.yaml` e ajuste o `upstream.host` (FQDN do stage) e os `profiles.*.routes`
(prefixos → portas locais). Referência: [`docs/10-referencia-devsplit-yaml.md`](./docs/10-referencia-devsplit-yaml.md).

## Status por plataforma

| Plataforma | Estado |
|---|---|
| **Linux** | ✅ **rodando de ponta a ponta** — app compila, abre, intercepta; núcleo 18 testes |
| **macOS** | 🟡 **implementado** (elevação via `osascript`); compila no Linux via `cfg!`, **build/runtime via CI** |
| **Windows** | 🟡 **implementado** (elevação via UAC/PowerShell); **build/runtime via CI** |

Build dos 3 SOs + release com instaladores: CI em
[`.github/workflows/build.yml`](./.github/workflows/build.yml) (`tauri-action`, matriz
`ubuntu`/`macos`/`windows`; tag `v*` → release draft). Painel completo de entregas em
[`docs/STATUS.md`](./docs/STATUS.md).

## Documentação

Guia completo em [`docs/`](./docs/). Atalhos:
[`getting-started`](./docs/getting-started.md) ·
[`00-blueprint`](./docs/00-blueprint.md) ·
[`02-arquitetura`](./docs/02-arquitetura.md) ·
[`04-build-distribuicao`](./docs/04-build-distribuicao.md) ·
[`11-tls-privilegios-seguranca`](./docs/11-tls-privilegios-seguranca.md) ·
[`troubleshooting`](./docs/troubleshooting.md) ·
[`STATUS`](./docs/STATUS.md).

## Logo & design

A marca (um fluxo que se divide em **local**, preenchido, e **passthrough**, vazado) está em
[`design/`](./design/): `icon.svg` (fonte), `logo.png`, `banner.png`. Os ícones do app são
gerados dela (`cargo tauri icon design/logo.png`). Mockup da UI em
[`design/mockup.html`](./design/mockup.html).

## Layout

```
devsplit/
├── README.md · BLUEPRINT.md           # front-door + desenho completo
├── docs/                              # documentação (pt-BR)
├── crates/devsplit-core/              # NÚCLEO Rust (sem GUI) — compila/testa em qualquer lugar
├── app/
│   ├── src/                           # UI React (TypeScript)
│   └── src-tauri/                     # casca Tauri (Rust): comandos, tray, elevação por-SO
├── design/                            # logo, banner, ícones-fonte, mockup
├── examples/devsplit.yaml             # config de time (commitável)
└── .github/workflows/build.yml        # CI: testa + builda os 3 SOs + release
```

## Licença

MIT OR Apache-2.0.
