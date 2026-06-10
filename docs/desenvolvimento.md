# Desenvolvimento e contribuição

> Layout do repo, como rodar os gates, a fronteira UI↔núcleo e convenções. Para entender
> o sistema antes de mexer, leia `02-arquitetura.md`.

---

## 1. Layout do repositório

```
devsplit/
├── BLUEPRINT.md                 # desenho/pesquisa do produto (intenção)
├── docs/                        # esta documentação (estado implementado)
├── Cargo.toml                   # workspace — inclui SÓ o devsplit-core
├── crates/devsplit-core/        # L2 — núcleo Rust puro (sem GUI)
│   └── src/{types,config,proxy/mod,tlsca,dns,hostsfile,lib}.rs
├── app/                         # frontend + casca Tauri
│   ├── src/                     # L1 — UI React (views/, components/, lib/)
│   └── src-tauri/src/{lib,main}.rs   # L3 — casca Tauri (precisa de webkit)
├── design/{mockup.html,icon.svg,icon.png}
└── examples/devsplit.yaml       # config de exemplo (commitável por time)
```

**Por que o workspace só tem o núcleo:** o `Cargo.toml` da raiz inclui apenas
`crates/devsplit-core` para que `cargo test`/`cargo build` na raiz **não** exijam
`webkit2gtk`. A casca Tauri (`app/src-tauri`) é um crate à parte, construído via
`cargo tauri`. Assim CI e contribuidores sem webview ainda exercem a parte difícil.

---

## 2. Gates (rodar antes de abrir PR)

```bash
# Núcleo (a parte difícil, testada):
cargo test -p devsplit-core              # 18 testes; + `-- --ignored` p/ o teste de rede

# Frontend (tipos + bundle):
cd app && npx tsc -b && npm run build    # `npm run build` já faz `tsc -b && vite build`

# Casca Tauri (precisa de webkit — ver getting-started §1.1):
cd app && cargo tauri build              # só onde o webview nativo está instalado
```

Não há um gate único de repo; rode o do que você tocou. O núcleo é o que mais importa
manter verde.

### 2.1 Rodar o app ao iterar no frontend

O app embute o `dist/` (`tauri.conf.json` → `frontendDist=../dist`, **sem `devUrl`**).
Então, depois de mexer no **frontend**, rebuilde o bundle e rode a casca:

```bash
cd app && npm run build          # tsc -b && vite build → gera app/dist/
cd app/src-tauri && cargo run    # casca carrega o dist/ embutido, sem dev server
```

Como o `devUrl` foi **removido**, hot-reload via `cargo tauri dev` exigiria re-adicioná-lo
(aponta a webview para o vite na `:1420`) **e** RAM livre para o vite — por isso o loop
padrão aqui é `npm run build` + `cargo run`.

> **OOM:** em máquina com pouca RAM/swap cheio, `npm run build` (`vite build`) pode ser
> morto pelo OOM-killer (exit 137). É pressão de **ambiente**, não bug — o `tsc -b` passa.
> Libere RAM (feche apps) antes de buildar o frontend.

---

## 3. Fronteira UI ↔ núcleo (regra de ouro)

- A **UI nunca** fala com a rede do proxy direto — só com a casca via `invoke`/`listen`
  (`02` §3). Toda capacidade nova é um **comando** ou um **evento** novo.
- O **núcleo não** depende de Tauri nem de webkit. Lógica de proxy/TLS/DNS/hosts/config
  vive em `devsplit-core` e ganha **teste** lá (funções puras quando possível — ver
  `hostsfile`).
- Tipos que cruzam a fronteira ficam em `types.rs` (Rust) e espelhados em `lib/ipc.ts`
  (TS). Mexeu num, sincronize o outro.

---

## 4. Convenções

- **pt-BR** nos comentários e docs (como o resto do repo).
- Adicionou um comando? Registre em `invoke_handler![…]` (`lib.rs`), adicione à interface
  `DevsplitIpc` **e** ao mock (`createMockIpc`) para a UI seguir explorável headless.
- Tocou no schema do `devsplit.yaml`? Atualize `config.rs`, `examples/devsplit.yaml` **e**
  `docs/10-referencia-devsplit-yaml.md`.
- Mudou comportamento? Atualize o `docs/STATUS.md`.
- Animações: só `transform`/`opacity` (WebKitGTK — `05` §3).

---

## 5. Iterar sem o webview

Sem `webkit2gtk` instalado você ainda pode:

- evoluir e testar **todo o núcleo** (`cargo test -p devsplit-core`);
- desenvolver **toda a UI** no navegador com o IPC mock (`npm run dev`, `RUNTIME ===
  "mock"`).

A compilação/QA da casca completa nos 3 SOs é o gate que depende de ambiente nativo
(`STATUS.md`).
