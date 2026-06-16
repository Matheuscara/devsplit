# 04 — Build, distribuição e elevação de privilégios

> Como o app sai do repo e chega na máquina do dev, e como ganha os privilégios pontuais
> que precisa (bindar `:443`, editar o arquivo hosts, instalar a CA). É o documento de
> **plataforma/alcance** — o equivalente cross-platform do que, num SaaS, seria o doc de
> **mobile**. Estado: app **roda no Linux** (verificado); elevação **Linux verificada** e
> **macOS/Windows implementadas** (compilam no Linux via despacho `cfg!`), runtime via CI.

---

## 1. Build

O `tauri-action` numa **matriz de CI** (cross-compile do Tauri não é viável — build por
SO). Artefatos por plataforma:

| SO | Artefatos | Webview |
|---|---|---|
| Linux | `.AppImage` (embute WebKitGTK 4.1, previsível), `.deb`, `.rpm` | `webkit2gtk-4.1` |
| macOS | `.dmg` / `.app` | WKWebView (sistema) |
| Windows | `.msi` / `.exe` (NSIS) | WebView2 |

`cargo tauri build` gera tudo localmente quando as libs nativas estão presentes. O
workspace separa o núcleo da casca: o `Cargo.toml` da raiz inclui **só** o
`devsplit-core` (compila/testa em qualquer lugar); a casca Tauri (`app/src-tauri`) fica
fora do workspace para não exigir webkit em CI de núcleo. Ver `desenvolvimento.md` §1.

**CI.** O workflow `.github/workflows/build.yml` (matriz `ubuntu-22.04` / `macos-latest` /
`windows-latest`) roda os testes do núcleo, **builda o app nos 3 SOs** e, em tag `v*`, cria
um **release draft** com os instaladores; em push/PR, anexa os instaladores ao run.

### 1.0 Estado no Linux — compila e roda

Com `webkit2gtk-4.1` (2.52.4) instalado, a casca Tauri **compila** (`cargo check`/`build`)
e o app **roda** no Linux: a janela renderiza a UI real (lê o `devsplit.yaml`;
doctor/rotas/perfis vêm do backend de verdade) e sobe sem panic (o tray monta).

**Modelo de execução (importante).** O `devUrl` foi **removido** do
`app/src-tauri/tauri.conf.json`. Sem ele, o app carrega o frontend **embutido no binário**
(`frontendDist=../dist`) — sem dev server. Rodar no Linux:

```sh
cd app/src-tauri && cargo run
```

Depois de mexer no frontend: `npm run build` (em `app/`) e então `cargo run`. **Não** use
`cargo tauri dev` em máquina apertada: ele sobe o Vite (`beforeDevCommand`) — pesado. Com
`devUrl` presente, o Tauri tentava conectar no Vite (`localhost:1420`) e abria **janela
branca** ("Could not connect to localhost") quando o dev server não estava de pé; remover
o `devUrl` é o que fecha esse buraco.

> **OOM, não código.** Em máquina sem RAM (swap cheio), `npm run build`/`vite build` pode
> ser morto pelo OOM-killer (exit **137**) — é do **ambiente**, não erro de código (o
> `tsc -b` passa). Libere RAM antes de buildar o frontend.

### 1.1 Tamanho

App Tauri puro ~15 MB (ordem de grandeza do `BLUEPRINT.md` §9 — **não medido aqui**,
tratar como `[ESTIMATIVA]`) + o binário **mkcert** (~4 MB, Go) quando bundlado. Muito
menor que Electron. Atenção ao bug conhecido de `externalBin` + notarização macOS
([tauri#11992](https://github.com/tauri-apps/tauri/issues/11992)) — assinar o mkcert à
mão, ou não bundlar e detectar/baixar no primeiro uso, ou migrar p/ trust 100% Rust
(`00` §8, v0.3).

### 1.2 Assinatura e auto-update

macOS Developer ID + **notarização**; Windows EV/Azure Trusted Signing; auto-update via
`tauri-plugin-updater` com chave de assinatura. **Planejado, não exercido neste repo.**

### 1.3 Distribuição na máquina do dev

O CI **publica** o release em tag `v*` (`releaseDraft: false`); o `tauri.conf.json` gera
`appimage`/`deb`/`rpm` no Linux (mais `dmg`/`nsis`). Três caminhos de instalação, todos
lendo o **último release**:

- **Instalador 1-linha** (`packaging/install.sh`) — `curl -fsSL …/packaging/install.sh | bash`.
  Detecta a distro via `/etc/os-release` e instala o artefato nativo: `.deb` (apt), `.rpm`
  (dnf/zypper) ou `.AppImage` (registrado por-usuário, sem root). Garante o **mkcert**
  (gerenciador da distro → fallback p/ binário oficial em `~/.local/bin` quando não há sudo),
  checa **polkit/pkexec** e semeia `~/.config/dev.devsplit.app/devsplit.yaml`. `--uninstall` reverte.
- **NixOS** (`flake.nix`) — `nix profile install github:Matheuscara/devsplit`. `appimageTools.wrapType2`
  embrulha o `.AppImage` num FHS-env e injeta **mkcert + nssTools** no PATH (`extraPkgs`).
  Atenção: o `setcap` **não pega** no `/nix/store` (read-only) — o app usa o fallback
  `sysctl net.ipv4.ip_unprivileged_port_start=443`; garanta `security.polkit.enable` e o sysctl
  no `configuration.nix`. A cada release, suba `version` e refaça o `sha256` (deixe `lib.fakeHash`,
  rode `nix build`, cole o hash que o Nix imprime).
- **Manual/AUR** — baixar o artefato da página de Releases, ou Arch via `packaging/PKGBUILD`
  (`makepkg -si`, extrai o `.deb` do release).

> O `mkcert` **não** é declarável como dep do `.deb`/`.rpm` (não é uma lib; ausente nos repos
> padrão) — por isso o instalador e o flake o provêm fora do pacote.

---

## 2. Plugins nativos (em uso)

Registrados em `run()` (`app/src-tauri/src/lib.rs`):

- `tauri-plugin-single-instance` — garante **um único dono de `:443`** (foca a janela
  existente ao reabrir).
- `tauri-plugin-shell` — usado só para rodar o `mkcert`.
- `tauri-plugin-autostart` — `MacosLauncher::LaunchAgent`.
- **Tray** (`build_tray`) — fonte de verdade do on/off; menu Ligar/Desligar, Abrir
  painel, Sair.

---

## 3. Elevação de privilégios

O app **roda sem root**. Precisa de privilégio pontual em três pontos: **bindar `:443`**,
**editar o arquivo hosts** e **instalar a CA** no trust store. O objetivo de produto é
**1 prompt por sessão**.

### 3.1 Linux (implementado)

`PrivSession` mantém um `pkexec /bin/sh` **vivo**: a senha é pedida uma vez (no spawn) e
todos os scripts root seguem por stdin pelo resto da sessão. No **ligar**
(`bootstrap_privileges`), um único script root:

1. grava o arquivo hosts via heredoc (`cat > '/etc/hosts' <<'__DSHOSTS__' … __DSHOSTS__`)
   — sem temp file, **sem TOCTOU**;
2. `setcap cap_net_bind_service=+ep <exe>` — libera o bind na `:443` para o binário;
3. `sysctl -w net.ipv4.ip_unprivileged_port_start=443` — fallback se o setcap não pegar.

No **desligar** (`revert_hosts`), remove o bloco do devsplit e as entradas soltas do FQDN
apontando p/ loopback (só eleva se houver o que tirar). A **instalação da CA** nos
navegadores (`install_cert`) roda como **usuário** (`mkcert -install` com
`TRUST_STORES=nss`) — **sem** root. Detalhe de segurança em `11` §3.

### 3.2 macOS e Windows (implementados; runtime validado por CI)

A elevação é abstraída por **despacho `cfg!` runtime**: `bootstrap_privileges`/`revert_hosts`
usam a sessão pkexec no Linux e, fora dele, chamam `apply_hosts_oneshot` →
`apply_hosts_macos` / `apply_hosts_windows`. Essas variantes usam só `std::process`/`std::fs`,
então **compilam no Linux** (validadas no `cargo check`/`build` daqui); o **runtime** de cada
SO é exercido pelo CI.

- **macOS:** grava o conteúdo do hosts num temp do usuário e copia p/ o hosts com **um**
  prompt de admin: `osascript -e 'do shell script "cp …" with administrator privileges'`.
  bind `0.0.0.0:443` não precisa de root (Mojave+).
- **Windows:** grava num temp e copia p/ o hosts com **UAC**:
  `Start-Process cmd '/c copy …' -Verb RunAs -Wait`. Sem porta privilegiada (bind livre).

| SO | bind `:443` | editar hosts | instalar CA |
|---|---|---|---|
| macOS | nenhuma (bind `0.0.0.0:443` sem root, Mojave+) | `osascript` admin (1 prompt) | Keychain (prompt GUI 1x; APIs exigem app assinado) — CA do navegador via `mkcert`/NSS |
| Windows | nenhuma (sem porta privilegiada) | UAC (`Start-Process -Verb RunAs`) | `CurrentUser\Root` (sem admin) → `LocalMachine\Root` (UAC) |

> **Pendente de validação real:** o RUNTIME do `osascript`/UAC só é confirmado buildando e
> rodando em cada SO (CI ou máquina) — o build do Linux garante compilação/tipos, não a
> sintaxe AppleScript/PowerShell em execução. A instalação da CA no **system store**
> (Keychain / `LocalMachine\Root`) ainda não foi codada por-SO; hoje a CA de **navegador**
> vai via `mkcert`/NSS.

---

## 4. Honestidade sobre "zero-config"

"Zero-config" vale no **caminho feliz** (dev na máquina, 1 prompt). Headless/CI **não** é
zero-config (macOS abre prompt GUI mesmo como root para o Keychain; trust de CA exige
interação). O leaf tem validade ≤ **825 dias** (`LEAF_MAX_DAYS`, teto do macOS); re-trust
só quando a **root** muda.
