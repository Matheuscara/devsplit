# Contribuindo com o devsplit

Obrigado pelo interesse em contribuir! Este guia cobre o fluxo de desenvolvimento,
como rodar os testes e o que esperamos de um Pull Request.

## Pré-requisitos

- **Rust** estável (via [rustup](https://rustup.rs)) — o núcleo (`crates/devsplit-core`)
  compila e testa sem GUI.
- **Node 20+** e **npm** — para o frontend em `app/src`.
- Para buildar o app Tauri (Linux): `webkit2gtk-4.1` + `libsoup3`
  (Debian/Ubuntu: `libwebkit2gtk-4.1-dev librsvg2-dev`).
- **mkcert** no PATH — TLS local confiável em runtime.

## Layout

O workspace Rust da raiz contém **apenas** o `devsplit-core` (lógica pura: proxy, TLS,
DNS, hosts), de propósito — ele compila e testa em qualquer máquina, sem webkit nem root.
A casca Tauri vive em `app/src-tauri/` com seu próprio `Cargo.toml` e depende do core por
path. Detalhes em [`docs/02-arquitetura.md`](./docs/02-arquitetura.md).

## Rodando

```bash
# Testes do núcleo (só precisa de Rust):
cargo test -p devsplit-core              # suíte completa (inclui e2e cliente-TLS → proxy → backend)
cargo test -p devsplit-core -- --ignored # + teste de rede (DNS direto)

# Frontend (lint de tipos):
cd app && npm install && npm run build   # tsc + vite build (frontend embutido no binário)

# Explorar a UI no navegador, sem backend (modo demo):
cd app && npm run dev                    # http://localhost:1420
```

Antes de abrir um PR, garanta que **passa local**:

```bash
cargo test -p devsplit-core
cargo fmt --all -- --check
cargo clippy -p devsplit-core -- -D warnings
cd app && npm run build
```

## Estilo

- Rust: `cargo fmt` + `clippy` sem warnings.
- TypeScript: siga o estilo existente em `app/src` (sem novas dependências sem necessidade).
- Comentários e docs em **pt-BR** (a base de código é toda em pt-BR).
- Não introduza uma segunda convenção ao lado de uma já existente — reuse os padrões do repo.

## Pull Requests

1. Faça um fork e crie um branch a partir de `main`.
2. Mantenha o PR focado: uma mudança lógica por PR.
3. Inclua testes para comportamento novo ou corrigido (o núcleo tem cobertura de proxy/TLS/DNS).
4. Descreva **o porquê** da mudança, não só o quê. Use o template de PR.
5. O CI roda build dos 3 SOs e o `security-scan` (CodeQL, Trivy, gitleaks, cargo-audit,
   npm audit). PRs precisam passar nessas verificações.

## Segurança

devsplit termina TLS e edita `/etc/hosts` com elevação de privilégio — é um software
sensível. **Não** reporte vulnerabilidades em issues públicas; veja
[`SECURITY.md`](./SECURITY.md).

## Licença

Ao contribuir, você concorda em licenciar sua contribuição sob os mesmos termos do
projeto: **MIT OR Apache-2.0**.
