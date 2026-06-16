#!/usr/bin/env bash
# devsplit — instalador 1-linha p/ qualquer Linux (detecta a distro sozinho).
#
#   curl -fsSL https://raw.githubusercontent.com/Matheuscara/devsplit/main/packaging/install.sh | bash
#
# O que faz:
#   1) baixa o artefato certo do ULTIMO release publicado no GitHub:
#        Debian/Ubuntu/Mint .......... .deb  (apt, system-wide -> aparece no launcher)
#        Fedora/RHEL/Rocky/Alma ...... .rpm  (dnf,  system-wide -> aparece no launcher)
#        openSUSE .................... .rpm  (zypper)
#        qualquer outra .............. .AppImage (registra no launcher do usuario, sem root)
#   2) garante o mkcert (gerenciador da distro; sem sudo -> baixa o binario oficial p/ ~/.local/bin);
#   3) checa polkit/pkexec (necessario p/ editar /etc/hosts + liberar a :443) e avisa se faltar;
#   4) semeia ~/.config/dev.devsplit.app/devsplit.yaml (so se nao existir) p/ o app achar o stage.
#
# Variaveis:
#   DEVSPLIT_TAG=v0.1.0   forca uma versao (default: ultimo release)
#   DEVSPLIT_FORCE_APPIMAGE=1   ignora .deb/.rpm e usa AppImage
#
# Desinstalar:  curl -fsSL .../install.sh | bash -s -- --uninstall
set -euo pipefail

REPO="Matheuscara/devsplit"
API="https://api.github.com/repos/$REPO"
RAW="https://raw.githubusercontent.com/$REPO/main"
IDENT="dev.devsplit.app"

PREFIX="${PREFIX:-$HOME/.local}"
APPS_DIR="$PREFIX/share/applications"
ICONS_ROOT="$PREFIX/share/icons/hicolor"
BIN_APPIMAGE="$PREFIX/bin/devsplit.AppImage"

say()  { printf '==> %s\n' "$*"; }
warn() { printf 'AVISO: %s\n' "$*" >&2; }
die()  { printf 'ERRO: %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# sudo so quando preciso e disponivel
SUDO=""
if [ "$(id -u)" -ne 0 ]; then have sudo && SUDO="sudo"; fi

refresh_caches() {
  have update-desktop-database && update-desktop-database "$APPS_DIR" 2>/dev/null || true
  have gtk-update-icon-cache   && gtk-update-icon-cache -f "$ICONS_ROOT" 2>/dev/null || true
}

# ---------------------------------------------------------------- desinstalar
if [ "${1:-}" = "--uninstall" ]; then
  say "removendo devsplit"
  # pacote nativo, se instalado por aqui
  if have dpkg && dpkg -s devsplit >/dev/null 2>&1; then $SUDO apt-get remove -y devsplit || true
  elif have rpm && rpm -q devsplit >/dev/null 2>&1; then
    if   have dnf;    then $SUDO dnf remove -y devsplit || true
    elif have zypper; then $SUDO zypper --non-interactive remove devsplit || true
    fi
  fi
  # AppImage por-usuario
  rm -f "$BIN_APPIMAGE" "$APPS_DIR/devsplit.desktop" "$ICONS_ROOT/256x256/apps/devsplit.png"
  refresh_caches
  echo "(CA do mkcert e /etc/hosts sao limpos pelo proprio app ao fechar; config em ~/.config/$IDENT preservada.)"
  say "feito."
  exit 0
fi

# ------------------------------------------------------------- detectar distro
distro_family() {
  [ -r /etc/os-release ] || { echo "unknown"; return; }
  . /etc/os-release
  local ids="${ID:-} ${ID_LIKE:-}"
  case " $ids " in
    *debian*|*ubuntu*|*mint*)            echo "debian" ;;
    *fedora*|*rhel*|*centos*|*rocky*|*almalinux*) echo "fedora" ;;
    *suse*|*opensuse*)                   echo "suse" ;;
    *)                                   echo "unknown" ;;
  esac
}

cpu_arch() {
  case "$(uname -m)" in
    x86_64|amd64)  echo "amd64" ;;
    aarch64|arm64) echo "arm64" ;;
    *)             echo "unsupported" ;;
  esac
}

# url do asset do release por extensao (.deb/.rpm/.AppImage); sem jq
RELEASE_JSON=""
load_release() {
  local ref="${DEVSPLIT_TAG:+tags/$DEVSPLIT_TAG}"; ref="${ref:-latest}"
  RELEASE_JSON="$(curl -fsSL "$API/releases/$ref" 2>/dev/null)" \
    || die "nao consegui ler o release ($ref). Ja existe release publicado em github.com/$REPO/releases ?"
}
asset_url() { # $1 = regex de extensao, ex '\.deb'
  printf '%s\n' "$RELEASE_JSON" \
    | grep -o '"browser_download_url": *"[^"]*"' \
    | sed 's/.*"browser_download_url": *"\([^"]*\)".*/\1/' \
    | grep -iE "$1\$" | head -1
}

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
fetch() { say "baixando $(basename "$2")"; curl -fL --progress-bar -o "$1" "$2" || die "download falhou: $2"; }

# ------------------------------------------------------------- garantir mkcert
ensure_mkcert() {
  if have mkcert; then say "mkcert ok ($(command -v mkcert))"; return; fi
  say "mkcert ausente — instalando"
  case "$(distro_family)" in
    debian) $SUDO apt-get update -y && $SUDO apt-get install -y mkcert libnss3-tools && return || true ;;
    fedora) $SUDO dnf install -y mkcert nss-tools && return || true ;;
    suse)   $SUDO zypper --non-interactive install mkcert mozilla-nss-tools && return || true ;;
  esac
  have mkcert && return
  # fallback sem sudo: binario oficial -> ~/.local/bin
  local arch; arch="$(cpu_arch)"
  [ "$arch" = unsupported ] && { warn "arch $(uname -m) sem binario mkcert oficial; instale manualmente"; return; }
  local url
  url="$(curl -fsSL "https://api.github.com/repos/FiloSottile/mkcert/releases/latest" \
        | grep -o '"browser_download_url": *"[^"]*"' \
        | sed 's/.*"browser_download_url": *"\([^"]*\)".*/\1/' \
        | grep -iE "linux-${arch}\$" | head -1)"
  [ -n "$url" ] || { warn "nao achei binario mkcert p/ linux-$arch; instale manualmente"; return; }
  mkdir -p "$PREFIX/bin"
  fetch "$PREFIX/bin/mkcert" "$url"; chmod +x "$PREFIX/bin/mkcert"
  say "mkcert -> $PREFIX/bin/mkcert"
  case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) warn "$PREFIX/bin nao esta no PATH — adicione p/ o app achar o mkcert";; esac
}

# --------------------------------------------------------------- checar polkit
check_polkit() {
  if have pkexec; then say "polkit/pkexec ok"; else
    warn "pkexec (polkit) ausente — sem ele o app nao edita /etc/hosts nem libera a :443."
    case "$(distro_family)" in
      debian) warn "  instale:  $SUDO apt-get install -y policykit-1" ;;
      fedora) warn "  instale:  $SUDO dnf install -y polkit" ;;
      suse)   warn "  instale:  $SUDO zypper install polkit" ;;
      *)      warn "  instale o polkit pela sua distro" ;;
    esac
    warn "  Em WM enxutos (niri/sway/hyprland) tenha um AGENTE polkit ativo, senao o prompt de senha nao aparece."
  fi
}

# ---------------------------------------------------------- semear devsplit.yaml
seed_config() {
  local dir="${XDG_CONFIG_HOME:-$HOME/.config}/$IDENT"
  if [ -f "$dir/devsplit.yaml" ]; then say "config ja existe -> $dir/devsplit.yaml (mantida)"; return; fi
  mkdir -p "$dir"
  if curl -fsSL "$RAW/examples/devsplit.yaml" -o "$dir/devsplit.yaml" 2>/dev/null; then
    say "config semeada -> $dir/devsplit.yaml"
    warn "EDITE upstream.host (FQDN do stage) e os profiles.*.routes antes de ligar."
  else
    warn "nao consegui semear devsplit.yaml; crie $dir/devsplit.yaml a partir de examples/ do repo."
  fi
}

# ----------------------------------------------------- instalar AppImage (user)
install_appimage() {
  local url; url="$(asset_url '\.AppImage')"
  [ -n "$url" ] || die "release sem .AppImage."
  local f="$WORK/devsplit.AppImage"; fetch "$f" "$url"
  install -Dm755 "$f" "$BIN_APPIMAGE"; say "binario -> $BIN_APPIMAGE"
  # icone: extrai de dentro do AppImage (best-effort)
  if ( cd "$WORK" && APPIMAGE_EXTRACT_AND_RUN=1 "$BIN_APPIMAGE" --appimage-extract >/dev/null 2>&1 ); then
    local ic; ic="$(find "$WORK/squashfs-root" -maxdepth 3 -name 'devsplit.png' 2>/dev/null | head -1)"
    [ -z "$ic" ] && ic="$(find "$WORK/squashfs-root" -maxdepth 3 -name '*.png' 2>/dev/null | head -1)"
    [ -n "$ic" ] && install -Dm644 "$ic" "$ICONS_ROOT/256x256/apps/devsplit.png"
  fi
  install -d "$APPS_DIR"
  cat > "$APPS_DIR/devsplit.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=devsplit
GenericName=Dev traffic splitter
Comment=Split de trafego local de dev por path-prefix
Exec=$BIN_APPIMAGE
Icon=devsplit
Terminal=false
Categories=Development;
Keywords=proxy;dev;split;tls;mkcert;stage;
StartupWMClass=devsplit
EOF
  say "desktop -> $APPS_DIR/devsplit.desktop"
  refresh_caches
}

# -------------------------------------------------------- instalar .deb / .rpm
install_deb() {
  local url; url="$(asset_url '\.deb')"
  [ -n "$url" ] || { warn "release sem .deb; usando AppImage"; install_appimage; return; }
  local f="$WORK/devsplit.deb"; fetch "$f" "$url"
  say "instalando .deb (apt resolve deps; pode pedir senha do sudo)"
  $SUDO apt-get update -y || true
  $SUDO apt-get install -y "$f" || { $SUDO dpkg -i "$f" || true; $SUDO apt-get -f install -y; }
}
install_rpm() {
  local url; url="$(asset_url '\.rpm')"
  [ -n "$url" ] || { warn "release sem .rpm; usando AppImage"; install_appimage; return; }
  local f="$WORK/devsplit.rpm"; fetch "$f" "$url"
  say "instalando .rpm (pode pedir senha do sudo)"
  if   have dnf;    then $SUDO dnf install -y "$f"
  elif have zypper; then $SUDO zypper --non-interactive install --allow-unsigned-rpm "$f"
  else die "nem dnf nem zypper encontrados"; fi
}

# ----------------------------------------------------------------------- main
[ "$(cpu_arch)" = unsupported ] && warn "arch $(uname -m) nao testada (releases sao x86_64); seguindo mesmo assim."
have curl || die "curl e necessario."

load_release
fam="$(distro_family)"
[ "${DEVSPLIT_FORCE_APPIMAGE:-0}" = 1 ] && fam="unknown"

say "distro detectada: $fam"
case "$fam" in
  debian) install_deb ;;
  fedora) install_rpm ;;
  suse)   install_rpm ;;
  *)      install_appimage ;;
esac

ensure_mkcert
check_polkit
seed_config

echo
say "pronto. Abra o launcher (Super+Espaco) e procure 'devsplit'."
echo "    primeiro uso: tela Certificado -> Instalar certificado (mkcert), depois ligue a interceptacao."
