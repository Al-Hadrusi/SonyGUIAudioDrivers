#!/bin/sh
# Build and install Headphones Control for the current user (or system-wide
# with PREFIX=/usr/local and sudo). Run from the repository root:
#
#   ./packaging/linux/install.sh            # installs into ~/.local
#   ./packaging/linux/install.sh uninstall  # removes it again
set -eu

PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin/headphones-control"
DESKTOP="$PREFIX/share/applications/headphones-control.desktop"
ICON_SVG="$PREFIX/share/icons/hicolor/scalable/apps/headphones-control.svg"
ICON_PNG="$PREFIX/share/icons/hicolor/256x256/apps/headphones-control.png"

if [ "${1:-}" = "uninstall" ]; then
    rm -f "$BIN" "$DESKTOP" "$ICON_SVG" "$ICON_PNG"
    echo "Removed Headphones Control from $PREFIX"
    exit 0
fi

cargo build --release --locked
install -Dm755 target/release/headphones-control "$BIN"
install -Dm644 packaging/linux/headphones-control.desktop "$DESKTOP"
install -Dm644 assets/headphones-control.svg "$ICON_SVG"
install -Dm644 assets/headphones-control-256.png "$ICON_PNG"

# Refresh menus and icon caches where the tools exist.
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$PREFIX/share/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" || true

echo "Installed Headphones Control to $PREFIX"
case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) echo "Note: $PREFIX/bin is not on your PATH; the menu entry still works." ;;
esac
