#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

if ! command -v python3 >/dev/null 2>&1; then
    echo "Ошибка: не найден python3. Установите Python 3.10 или новее." >&2
    exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
    cat >&2 <<'EOF'
Ошибка: не найден cargo (Rust).

Установите Rust:
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

Затем перезапустите терминал или выполните:
  source "$HOME/.cargo/env"
EOF
    exit 1
fi

exec cargo run --release --locked -- "$@"
