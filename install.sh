#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

BIN_DIR="$HOME/.local/bin"
APP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/scalable/apps"

if [[ "${1:-}" == "--uninstall" ]]; then
    rm -f "$BIN_DIR/codevisualizer" \
          "$APP_DIR/codevisualizer.desktop" \
          "$ICON_DIR/codevisualizer.svg"
    update-desktop-database "$APP_DIR" 2>/dev/null || true
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true
    echo "CodeVisualizer удалён."
    exit 0
fi

if ! command -v python3 >/dev/null 2>&1; then
    echo "Ошибка: не найден python3. Установите Python 3.10 или новее." >&2
    exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "Ошибка: не найден cargo. Установите Rust: https://rustup.rs" >&2
    exit 1
fi

cargo build --release --locked

TARGET_DIR="${CARGO_TARGET_DIR:-target}"

mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR"

install -m 755 "$TARGET_DIR/release/codevisualizer" "$BIN_DIR/codevisualizer"
install -m 644 assets/codevisualizer.svg "$ICON_DIR/codevisualizer.svg"

cat > "$APP_DIR/codevisualizer.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=CodeVisualizer
Comment=Пошаговая визуализация выполнения Python-кода
Exec="$BIN_DIR/codevisualizer" %f
TryExec=$BIN_DIR/codevisualizer
Icon=codevisualizer
Terminal=false
Categories=Development;Education;
Keywords=python;debug;visualization;trace;
MimeType=text/x-python;
StartupWMClass=codevisualizer
EOF

update-desktop-database "$APP_DIR" 2>/dev/null || true
gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" 2>/dev/null || true

echo "Установлено: $BIN_DIR/codevisualizer"
echo "Ярлык:      $APP_DIR/codevisualizer.desktop"
echo "Запуск:     CodeVisualizer (в меню приложений)"
echo "Удаление:   ./install.sh --uninstall"
