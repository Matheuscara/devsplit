# Resultados de varredura de segurança — devsplit

> Relatório **real** gerado a partir de execuções locais dos scanners no repositório.
> Reflete o estado **após as correções** aplicadas no working tree (ver "Correções aplicadas").

| | |
|---|---|
| **Repositório** | `Matheuscara/devsplit` (Rust workspace + app Tauri/React) |
| **Base** | commit `d3ba4e1` + correções de segurança (hickory 0.26) |
| **Data da varredura** | 2026-06-11 |
| **Escopo** | deps Rust (`Cargo.lock` raiz + `app/src-tauri/Cargo.lock`), deps npm (`app/package-lock.json`), código-fonte TS (`app/src`), engine Rust (`crates/devsplit-core`, `app/src-tauri/src`), segredos (histórico git + working tree) |
| **CI** | `.github/workflows/security-scan.yml` — roda CodeQL + Trivy + cargo-audit + npm audit + gitleaks a cada push/PR e semanalmente |

---

## Resumo executivo

Os números abaixo são os que valem como "badge" no front.

| Scanner | O que cobre | Resultado | Status |
|---|---|---|---|
| **gitleaks** 8.30.1 | Segredos vazados (histórico git + árvore) | **0 segredos** | ✓ limpo |
| **CodeQL** 2.25.6 | SAST do código (TS + Rust) | **0 alertas** | ✓ limpo |
| **cargo-audit** 0.22.2 | Vulnerabilidades RustSec | **0 vulnerabilidades** | ✓ limpo |
| **npm audit** | Advisories npm | **0 vulnerabilidades** (154 deps) | ✓ limpo |
| **Trivy** 0.71.0 | CVEs de deps + misconfig + secret | **1 MEDIUM** (glib, transitiva via Tauri), 0 misconfig, 0 secret | ⚠ 1 média sem patch upstream |

**Tradução honesta:** nenhum segredo, nenhum alerta de SAST, nenhuma vulnerabilidade no front (npm) e **zero vulnerabilidade RustSec** após a correção do `hickory-proto`. Sobra **1 única** pendência: uma issue MEDIUM *unsound* em `glib`, puxada transitivamente pela stack GTK do Tauri — **sem correção aplicável** enquanto o Tauri não subir as bindings GTK (detalhe abaixo).

---

## Correções aplicadas

### ✅ `hickory-proto` DoS — **RESOLVIDO**

- **Antes:** `hickory-proto 0.24.4` (via `hickory-resolver 0.24`) — `RUSTSEC-2026-0119` / `GHSA-q2qq-hmj6-3wpp` (DoS, CPU exhaustion O(n²)).
- **Correção:** bump `hickory-resolver "0.24" → "0.26"` em `crates/devsplit-core/Cargo.toml`, que puxa `hickory-proto 0.26.1` (≥ 0.26.1 exigido pelo advisory).
- **Migração de API** em `crates/devsplit-core/src/dns.rs` (0.24 → 0.26):
  - `NameServerConfigGroup::cloudflare()/google()/merge()` → constantes `CLOUDFLARE`/`GOOGLE` (`ServerGroup`) + `.udp_and_tcp()` encadeados em `ResolverConfig::from_parts`.
  - `opts.use_hosts_file = false` → `opts.use_hosts_file = ResolveHosts::Never` (campo virou enum).
  - `TokioAsyncResolver::tokio(config, opts)` → `Resolver::builder_with_config(config, TokioRuntimeProvider::default()).with_options(opts).build()?`.
- **Lockfiles atualizados:** `Cargo.lock` (raiz) **e** `app/src-tauri/Cargo.lock` agora pinam `hickory-proto 0.26.1`.
- **Validação:**
  - `cargo build -p devsplit-core` ✓
  - `cargo test -p devsplit-core` → **18/18** ✓
  - teste de DNS real (`dns::tests::resolves_public_domain`, `--ignored`) → **passou** (resolveu domínio público com a API nova) ✓
  - `cargo check` no app Tauri (com o lock novo) ✓
- **Confirmação pós-fix:** `cargo audit` = **0 vulnerabilidades** nos dois lockfiles; Trivy não reporta mais o hickory.

### ⚠ `glib` unsound — **ABERTO (sem correção possível hoje)**

| Campo | Valor |
|---|---|
| **IDs** | `RUSTSEC-2024-0429` · `GHSA-wrw7-89jp-8q8g` |
| **Severidade** | MEDIUM (Trivy) / *unsound* (cargo-audit) |
| **Título** | Unsoundness nas impls de `Iterator`/`DoubleEndedIterator` de `glib::VariantStrIter` |
| **Onde** | `app/src-tauri/Cargo.lock` |
| **Caminho de dep** | `tauri 2.11.2` → `tray-icon` → `libappindicator` → `gtk 0.18` → `atk` → `glib 0.18.5` |
| **Correção** | `glib >= 0.20.0` |

**Por que não dá pra corrigir:** tentamos elevar e o cargo recusa —
`gtk v0.18.2` exige `glib = "^0.18"`, e esse `gtk 0.18` está fixado pelo `tauri 2.11.2` (**já é a versão mais recente do Tauri**). Não existe release `0.18.x` do glib com o fix; ele só veio na linha `0.20`. Ou seja: só se resolve quando o Tauri migrar as bindings GTK (depende do `webkit2gtk-rs`).

**Risco prático:** baixo. É *unsound* (não é exploit direto) e o devsplit **não usa** `glib::VariantStrIter` — o código está enterrado no caminho do tray-icon usado pelo Tauri. Por isso o CI **não quebra** nisso (gate em HIGH/CRITICAL; ver workflow).

```text
$ cargo update -p glib --precise 0.20.0
error: failed to select a version for the requirement `glib = "^0.18"`
  candidate versions found which didn't match: 0.20.0
  required by package `gtk v0.18.2` ... `tauri v2.11.2`
```

---

## Avisos informativos (não são vulnerabilidades)

cargo-audit também sinaliza crates **não mantidos** / *unsound*. São avisos, **não** vulnerabilidades (cargo-audit não quebra o build por eles), todos transitivos:

- **`rustls-pemfile`** (`RUSTSEC-2025-0134`, não mantido) — dep direta de devsplit-core; substituível pelo parsing de PEM já presente em `rustls-pki-types`.
- **Stack GTK3** (`atk`, `atk-sys`, `gdk`, `gdk-sys`, `gdkwayland-sys`, `gdkx11`, `gdkx11-sys`, `gtk`, `gtk-sys`, `gtk3-macros`) — bindings GTK3 em fim de vida, puxadas pelo Tauri v2 no Linux. Migração depende do Tauri.
- **`unic-*`** (`unic-char-property`, `unic-char-range`, `unic-common`, `unic-ucd-ident`, `unic-ucd-version`) — transitivas, não mantidas.
- **`proc-macro-error`** — não mantido, transitiva (build-time).

Contagem pós-fix: **1** aviso no lockfile raiz (rustls-pemfile) · **18** no lockfile do Tauri (17 não mantidos + 1 unsound glib).

---

## SAST — CodeQL 2.25.6

Suítes oficiais de *code scanning* do GitHub, executadas localmente.

| Alvo | Suíte | Arquivos | Qualidade da extração | Alertas |
|---|---|---|---|---|
| **Frontend TS** (`app/src`) | `javascript-code-scanning.qls` (88 queries) | 22/22 extraídos sem erro | completa | **0** |
| **Engine** (`crates/devsplit-core`, build real) | `rust-code-scanning.qls` (25 queries) | 7 first-party OK · tipos resolvidos **94%** | confiável | **0** |
| **App Tauri** (`app/src-tauri`, build-free) | `rust-code-scanning.qls` (25 queries) | extração parcial (sem compilação) | parcial | **0** |

**Notas de honestidade:**
- O resultado do **frontend TS** e do **engine Rust** (compilado, 94% das expressões com tipo resolvido) é confiável.
- O suporte a Rust no CodeQL é recente; a métrica "calls with call target" ficou em 29% (abaixo do limiar de 50%) por causa de resolução de traits/genéricos — possível origem de falsos-negativos. O `app/src-tauri` foi extraído em modo *build-free* (extração parcial), então o "0" dele é indicativo, não auditoria completa.
- No CI (`security-scan.yml`), CodeQL roda em `build-mode: none` p/ TS e Rust (sem precisar de webkit/GTK no runner) e publica na aba Security.

---

## Segredos — gitleaks 8.30.1

| Modo | Escopo | Resultado |
|---|---|---|
| `gitleaks git` | histórico completo (4 commits, ~673 KB) | **0 leaks** |
| `gitleaks dir` | working tree | 0 leaks reais (5 **falsos-positivos**, todos em build artifacts) |

Os 5 achados do scan de diretório estão **inteiramente** em `app/src-tauri/target/` (arquivos compilados `.rmeta` da dep `muda` + 1 `*.gschema.xml` da stack GNOME). `target/` é ignorado pelo git, não é código-fonte e não vai pro repositório. O scan canônico de CI (histórico) = **0 segredos**.

---

## Como reproduzir

Pré-requisitos baixados localmente em `.sectools/` (ignorado pelo git):
gitleaks 8.30.1, Trivy 0.71.0, cargo-audit 0.22.2, CodeQL bundle 2.25.6.

```bash
# Segredos (histórico + árvore)
gitleaks git . --redact
gitleaks dir . --redact

# CVEs de deps + misconfig + secret (pula build/vendor)
trivy fs . --scanners vuln,secret,misconfig \
  --skip-dirs app/src-tauri/target --skip-dirs app/node_modules

# Advisories Rust (RustSec) — os dois lockfiles
cargo audit -f Cargo.lock
cargo audit -f app/src-tauri/Cargo.lock

# Advisories npm
( cd app && npm audit )

# SAST CodeQL — frontend TS
codeql database create db-js --language=javascript-typescript --source-root=app/src
codeql database analyze db-js \
  codeql/javascript-queries:codeql-suites/javascript-code-scanning.qls \
  --format=sarif-latest --output=codeql-js.sarif

# SAST CodeQL — engine Rust (build real p/ extração completa)
codeql database create db-rust --language=rust --source-root=. \
  --command="cargo build -p devsplit-core"
codeql database analyze db-rust \
  codeql/rust-queries:codeql-suites/rust-code-scanning.qls \
  --format=sarif-latest --output=codeql-rust.sarif
```

No CI tudo isso roda automático via `.github/workflows/security-scan.yml`.

---

## Para o front (valores de badge)

Card honesto, baseado nos números reais após a correção:

- **gitleaks** — ✓ `0 segredos`
- **CodeQL** — ✓ `0 alertas`
- **npm audit** — ✓ `0 vulnerabilidades`
- **cargo-audit (Rust)** — ✓ `0 vulnerabilidades`
- **Trivy** — ⚠ `1 média (glib, transitiva do Tauri, sem patch upstream)`

> O `hickory-proto` (DoS) foi **corrigido**. A única pendência (`glib`, *unsound*) vem da stack GTK do Tauri e some quando o Tauri atualizar — não é código do devsplit nem há patch aplicável hoje. Mostrar isso explicitamente é mais honesto do que esconder.

**Badge "ao vivo" no front:** aponte para o run do workflow —
`https://github.com/Matheuscara/devsplit/actions/workflows/security-scan.yml/badge.svg`
(já adicionado ao README). Ele fica verde quando os gates passam (gitleaks/CodeQL/cargo-audit/npm + Trivy HIGH/CRITICAL).
