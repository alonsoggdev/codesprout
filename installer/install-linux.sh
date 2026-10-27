#!/usr/bin/env bash
# CodeSprout installer for Linux and WSL. Like the Windows one, it installs per user, without
# root, so the app's updater can swap its own executable:
#   ~/.local/share/codesprout/codesprout            executable (and codesprout.png, the icon)
#   ~/.local/bin/codesprout                         link to it, to start it from a terminal
#   ~/.local/share/applications/codesprout.desktop  menu entry (WSLg also adds it to the Windows Start menu)
#
# Run it from an extracted CodeSprout-linux-x64.tar.gz (`./install.sh`) or straight from the web:
#   curl -fsSL https://github.com/alonsoggdev/codesprout/releases/latest/download/install-linux.sh | bash
# `--uninstall` removes CodeSprout and leaves your settings in ~/.config.
set -euo pipefail

REPO="alonsoggdev/codesprout"
SHARE="${XDG_DATA_HOME:-$HOME/.local/share}"
APP_DIR="$SHARE/codesprout"
BIN_DIR="$HOME/.local/bin"
DESKTOP="$SHARE/applications/codesprout.desktop"

say() { printf '\033[1;32m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!!\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31mError:\033[0m %s\n' "$*" >&2; exit 1; }

if [ "${1:-}" = "--uninstall" ]; then
    rm -rf "$APP_DIR"
    rm -f "$BIN_DIR/codesprout" "$DESKTOP"
    say "CodeSprout desinstalado."
    exit 0
fi

[ "$(uname -s)" = "Linux" ] || die "Este instalador es para Linux. En Windows usa CodeSprout-Setup-windows-x64.exe."
[ "$(uname -m)" = "x86_64" ] || die "Solo hay versión para x86_64 (este equipo es $(uname -m))."

# Files from the tarball next to this script; when piped from curl, download the tarball first.
src=""
if [ -n "${BASH_SOURCE[0]:-}" ] && [ -f "$(dirname "${BASH_SOURCE[0]}")/codesprout" ]; then
    src="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
else
    command -v curl >/dev/null || die "Hace falta curl para descargar CodeSprout."
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    say "Descargando la última versión…"
    curl -fL --progress-bar -o "$tmp/codesprout.tar.gz" \
        "https://github.com/$REPO/releases/latest/download/CodeSprout-linux-x64.tar.gz"
    tar -xzf "$tmp/codesprout.tar.gz" -C "$tmp"
    src="$tmp/CodeSprout"
fi

say "Instalando en $APP_DIR"
mkdir -p "$APP_DIR" "$BIN_DIR" "$(dirname "$DESKTOP")"
# Through a temporary name, so a running CodeSprout keeps working until it restarts
install -m 755 "$src/codesprout" "$APP_DIR/codesprout.new"
mv -f "$APP_DIR/codesprout.new" "$APP_DIR/codesprout"
rm -f "$APP_DIR/codesprout.old"
install -m 644 "$src/codesprout.png" "$APP_DIR/codesprout.png"
ln -sf "$APP_DIR/codesprout" "$BIN_DIR/codesprout"

cat > "$DESKTOP" <<EOF
[Desktop Entry]
Type=Application
Name=CodeSprout
Comment=Cliente Git de escritorio
Exec="$APP_DIR/codesprout"
Icon=$APP_DIR/codesprout.png
Terminal=false
Categories=Development;RevisionControl;
EOF
command -v update-desktop-database >/dev/null && update-desktop-database -q "$(dirname "$DESKTOP")" 2>/dev/null || true

# Git, plus the system libraries the window and the file dialogs load. Each line: library or
# command, then its Debian/Ubuntu package.
missing=()
libs="$(ldconfig -p 2>/dev/null || /sbin/ldconfig -p 2>/dev/null || true)"
has_lib() { grep -qF "$1" <<<"$libs"; }
command -v git >/dev/null || missing+=(git)
while read -r lib pkg; do
    has_lib "$lib" || missing+=("$pkg")
done <<'EOF'
libgtk-3.so.0 libgtk-3-0
libxkbcommon-x11.so.0 libxkbcommon-x11-0
libEGL.so.1 libegl1
libGL.so.1 libgl1
EOF

if [ ${#missing[@]} -gt 0 ]; then
    if command -v apt-get >/dev/null; then
        say "Instalando dependencias del sistema (pide tu contraseña de sudo): ${missing[*]}"
        if ! { sudo apt-get update -qq && sudo apt-get install -y -qq "${missing[@]}"; }; then
            warn "No se pudieron instalar. Hazlo a mano: sudo apt-get install ${missing[*]}"
        fi
    else
        warn "Faltan estas dependencias; instálalas con el gestor de paquetes de tu distribución: ${missing[*]}"
    fi
fi

version="$("$APP_DIR/codesprout" --version 2>/dev/null || true)"
say "CodeSprout ${version:+$version }instalado. Ábrelo desde el menú de aplicaciones o con: codesprout"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) warn "$BIN_DIR no está en tu PATH. Abre una terminal nueva o ejecuta: export PATH=\"\$HOME/.local/bin:\$PATH\"" ;;
esac
