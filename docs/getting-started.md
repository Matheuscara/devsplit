# Getting started — instalação e primeiro uso

> Do zero ao "ligar a interceptação". Pré-requisitos, como rodar cada camada e o primeiro
> uso. Conceitual em `01`; arquitetura em `02`.

---

## 1. Pré-requisitos

| Para | Precisa de | Como obter |
|---|---|---|
| Testar o **núcleo** | Rust (stable) | `rustup` — nada além. |
| Rodar o **frontend** no navegador | Node + npm | `app/` com IPC mock; sem privilégio. |
| Rodar o **app Tauri** completo | webview nativo + Tauri CLI | abaixo. |
| **TLS confiável** (qualquer SO) | binário `mkcert` no PATH | [`mkcert`](https://github.com/FiloSottile/mkcert) — `mkcert -version` para checar. |

### 1.1 Webview nativo (para o app Tauri)

```bash
# Linux (Arch/CachyOS):
sudo pacman -S webkit2gtk-4.1 libsoup3
# Debian/Ubuntu:
sudo apt install libwebkit2gtk-4.1-dev libssl-dev
```

macOS usa WKWebView (sistema); Windows usa WebView2. **Sem o webview o app Tauri não
compila** — mas o núcleo e o frontend-no-navegador rodam mesmo assim.

---

## 2. Rodar cada camada

### 2.1 Testes do núcleo (só Rust)

```bash
cargo test -p devsplit-core            # 18 testes (inclui e2e TLS local→proxy→backend + redação de headers)
cargo test -p devsplit-core -- --ignored   # + teste de rede (DNS direto)
```

### 2.2 Frontend no navegador (dados mock)

```bash
cd app && npm install && npm run dev   # http://localhost:5173 — RUNTIME = "mock"
```

Toda a UI é explorável sem Tauri nem privilégio (`03` §4.1).

### 2.3 App completo (Tauri)

```bash
cargo install tauri-cli --version "^2"            # CLI (1x; só p/ icon/build)
cd app && cargo tauri icon caminho/para/logo.png  # ícones (1x, precisa de uma imagem fonte)

# RODAR (recomendado): frontend EMBUTIDO no binário (frontendDist=../dist), sem dev server
cd app/src-tauri && cargo run

# EMPACOTAR: cargo tauri build  → .AppImage/.deb/.rpm (Linux), .dmg (macOS), .msi/.exe (Windows)
```

`cargo run` carrega o frontend já embutido — não sobe o vite. **Evite `cargo tauri dev`
em máquina apertada de RAM**: ele dispara `beforeDevCommand` (`npm run dev`, vite dev
server na `:1420`), que pesa. Pré-requisito Linux do webview: `webkit2gtk-4.1` (+
`libsoup3`), já instalável (§1.1).

Ao abrir, o app lê o `devsplit.yaml` da raiz (§3) e mostra doctor/rotas/perfis do backend
real. O toggle **Ativar split** pede a senha **uma vez** (pkexec, Linux) no bootstrap.

---

## 3. Configurar o `devsplit.yaml`

Copie `examples/devsplit.yaml` para a raiz do seu repo como `devsplit.yaml` e ajuste:

- `upstream.host` / `passthrough.resolve` / `passthrough.sni` → o FQDN do seu stage;
- `profiles.*.routes` → os prefixos que você quer **local** e suas portas.

O app procura o arquivo subindo do cwd até a raiz do repo, depois no diretório de config
(`02` §3.6). Referência completa do schema em `10`.

---

## 4. Primeiro uso

1. Abra o app. O **onboarding** aparece na primeira vez; a **detecção de serviços** (`12`
   §4) mostra o que já está de pé em `127.0.0.1`.
2. Escolha o **perfil** (ex.: `default`) e confira as rotas na tela **Rotas**.
3. Clique no **botão grande ligar**. Aqui acontece o **bootstrap (1 prompt de senha)**:
   - gera a CA + leaf local;
   - escreve o bloco no `/etc/hosts` (FQDN → 127.0.0.1);
   - no Linux, `setcap`/`sysctl` para bindar `:443`.
4. Se o navegador ainda não confia no cert, vá em **Certificado** → **Instalar
   certificado** (`mkcert -install` NSS, sem root). O **doctor** deve ficar ✅ cert / ✅
   hosts / ✅ upstream.
5. Abra a URL de stage no navegador. As requisições aparecem ao vivo em **Tráfego**; os
   prefixos roteados caem no serviço local, o resto no stage real.
6. Ao **desligar**, o `/etc/hosts` é revertido — o stage volta a ser alcançável direto.

> A senha é pedida **uma vez por sessão** (sessão root persistente, `11` §3) — ligar e
> desligar de novo não pede outra. Hoje a elevação é Linux/pkexec (`04` §3).

Problemas comuns em `troubleshooting.md`.
