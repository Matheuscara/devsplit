#!/usr/bin/env bash
# devsplit — registra um AppImage baixado no launcher de QUALQUER distro, SEM root.
#
# O AppImage roda em qualquer Linux, mas sozinho nao aparece no menu de apps.
# Este script copia o AppImage p/ ~/.local/bin e cria o .desktop + icone nos
# diretorios XDG do usuario -> abre por Super+Espaco igual app nativo.
#
# Uso:  ./install-appimage.sh ~/Downloads/devsplit_0.1.0_amd64.AppImage
# Remove: ./install-appimage.sh --uninstall
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"
BIN_DST="$PREFIX/bin/devsplit.AppImage"
APPS_DIR="$PREFIX/share/applications"
ICON_DST="$PREFIX/share/icons/hicolor/256x256/apps/devsplit.png"

refresh() {
  command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS_DIR" 2>/dev/null || true
  command -v gtk-update-icon-cache   >/dev/null 2>&1 && gtk-update-icon-cache -f "$PREFIX/share/icons/hicolor" 2>/dev/null || true
}

if [[ "${1:-}" == "--uninstall" ]]; then
  rm -f "$BIN_DST" "$APPS_DIR/devsplit.desktop" "$ICON_DST"
  refresh
  echo "AppImage do devsplit removido de $PREFIX."
  exit 0
fi

APPIMAGE="${1:?uso: install-appimage.sh <devsplit*.AppImage>}"
[[ -f "$APPIMAGE" ]] || { echo "ERRO: '$APPIMAGE' nao existe" >&2; exit 1; }

install -Dm755 "$APPIMAGE" "$BIN_DST"
echo "==> appimage -> $BIN_DST"

# extrai o icone de dentro do AppImage (best-effort) p/ o launcher exibir
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
if ( cd "$WORK" && APPIMAGE_EXTRACT_AND_RUN=1 "$BIN_DST" --appimage-extract >/dev/null 2>&1 ); then
  ICON_SRC="$(find "$WORK/squashfs-root" -maxdepth 3 -name 'devsplit.png' 2>/dev/null | head -1)"
  [[ -z "${ICON_SRC:-}" ]] && ICON_SRC="$(find "$WORK/squashfs-root" -maxdepth 3 -name '*.png' 2>/dev/null | head -1)"
  [[ -n "${ICON_SRC:-}" ]] && install -Dm644 "$ICON_SRC" "$ICON_DST" && echo "==> icone    -> $ICON_DST"
fi

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

refresh
echo
echo "Pronto. Procure 'devsplit' no launcher (Super+Espaco)."
echo "Runtime: precisa de mkcert + polkit (pkexec) instalados."
