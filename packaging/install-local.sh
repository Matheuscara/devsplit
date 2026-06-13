#!/usr/bin/env bash
# devsplit — instalacao local (usuario), SEM root.
#
# Coloca o binario release, os icones e um .desktop nos diretorios XDG do usuario
# (~/.local). Resultado: o devsplit aparece no launcher (fuzzel/rofi/anyrun/wofi/
# krunner...) e pode ser aberto por Super+Espaco, igual qualquer app.
#
# Uso:   ./packaging/install-local.sh
#        PREFIX=/usr/local sudo ./packaging/install-local.sh   # system-wide
#
# Desinstala:  ./packaging/install-local.sh --uninstall
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_DIR="$REPO_ROOT/app"
SRC_TAURI="$APP_DIR/src-tauri"
BIN_SRC="$SRC_TAURI/target/release/devsplit"
ICON_DIR="$SRC_TAURI/icons"

PREFIX="${PREFIX:-$HOME/.local}"
BIN_DST="$PREFIX/bin/devsplit"
APPS_DIR="$PREFIX/share/applications"
ICONS_ROOT="$PREFIX/share/icons/hicolor"

# Tamanho XDG  ->  arquivo de origem em src-tauri/icons.
ICON_SIZES=(32x32 64x64 128x128 256x256 512x512)
icon_src_for() {
  case "$1" in
    32x32)   echo "$ICON_DIR/32x32.png" ;;
    64x64)   echo "$ICON_DIR/64x64.png" ;;
    128x128) echo "$ICON_DIR/128x128.png" ;;
    256x256) echo "$ICON_DIR/128x128@2x.png" ;;
    512x512) echo "$ICON_DIR/icon.png" ;;
  esac
}

refresh_caches() {
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS_DIR" 2>/dev/null || true
  command -v gtk-update-icon-cache   >/dev/null 2>&1 && gtk-update-icon-cache -f "$ICONS_ROOT" 2>/dev/null || true
}

if [[ "${1:-}" == "--uninstall" ]]; then
  rm -f "$BIN_DST" "$APPS_DIR/devsplit.desktop"
  for size in "${ICON_SIZES[@]}"; do rm -f "$ICONS_ROOT/$size/apps/devsplit.png"; done
  refresh_caches
  echo "devsplit removido de $PREFIX."
  echo "(CA do mkcert e /etc/hosts ja sao limpos pelo proprio app ao fechar; nada a fazer aqui.)"
  exit 0
fi

# 1) binario release (compila se faltar)
if [[ ! -x "$BIN_SRC" ]]; then
  echo "==> binario release ausente; compilando (cargo tauri build --no-bundle)..."
  # --no-bundle: so o binario + frontend; pula o AppImage/deb (que exige FUSE/linuxdeploy).
  ( cd "$APP_DIR" && env -u CI cargo tauri build --no-bundle )
fi
[[ -x "$BIN_SRC" ]] || { echo "ERRO: $BIN_SRC nao encontrado apos o build" >&2; exit 1; }
install -Dm755 "$BIN_SRC" "$BIN_DST"
echo "==> binario  -> $BIN_DST"

# 2) icones (todas as resolucoes disponiveis)
for size in "${ICON_SIZES[@]}"; do
  src="$(icon_src_for "$size")"
  [[ -f "$src" ]] && install -Dm644 "$src" "$ICONS_ROOT/$size/apps/devsplit.png"
done
echo "==> icones   -> $ICONS_ROOT/<size>/apps/devsplit.png"

# 2b) semeia o devsplit.yaml no diretorio de config do app. CRUCIAL: aberto pelo
#     launcher (Super+Espaco) o cwd e o $HOME, nao o repo — sem isto o app nao
#     ACHA a config e o botao "ligar" nao faz nada. Nao sobrescreve config sua.
CONFIG_SRC="$REPO_ROOT/devsplit.yaml"
APP_CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/dev.devsplit.app"
if [[ -f "$APP_CONFIG_DIR/devsplit.yaml" ]]; then
  echo "==> config   -> $APP_CONFIG_DIR/devsplit.yaml (ja existe, preservada)"
elif [[ -f "$CONFIG_SRC" ]]; then
  install -Dm644 "$CONFIG_SRC" "$APP_CONFIG_DIR/devsplit.yaml"
  echo "==> config   -> $APP_CONFIG_DIR/devsplit.yaml"
else
  echo "==> config   AVISO: $CONFIG_SRC nao existe; ajuste $APP_CONFIG_DIR/devsplit.yaml a mao"
fi

# 3) .desktop — Exec com caminho ABSOLUTO (funciona mesmo se ~/.local/bin nao
#    estiver no PATH do launcher). StartupWMClass agrupa a janela ao lancador.
install -d "$APPS_DIR"
cat > "$APPS_DIR/devsplit.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=devsplit
GenericName=Dev traffic splitter
Comment=Split de trafego local de dev por path-prefix
Exec=$BIN_DST
Icon=devsplit
Terminal=false
Categories=Development;
Keywords=proxy;dev;split;tls;mkcert;stage;
StartupWMClass=devsplit
EOF
echo "==> desktop  -> $APPS_DIR/devsplit.desktop"

refresh_caches

# 4) dependencias de runtime
echo
echo "==> dependencias de runtime:"
miss=0
for dep in mkcert pkexec; do
  if command -v "$dep" >/dev/null 2>&1; then echo "    ok    $dep"; else echo "    FALTA $dep"; miss=1; fi
done
[[ "$miss" == 1 ]] && echo "    (mkcert: confiar a CA no navegador; pkexec/polkit: editar /etc/hosts e liberar a :443)"
case ":$PATH:" in
  *":$PREFIX/bin:"*) : ;;
  *) echo "    nota: $PREFIX/bin fora do PATH — ok, o .desktop usa caminho absoluto" ;;
esac

echo
echo "Pronto. Abra o launcher (Super+Espaco) e digite 'devsplit'."
