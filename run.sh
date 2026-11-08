#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "$0")"
if ! command -v cargo >/dev/null 2>&1 && [[ -x "$PWD/.cargo/bin/cargo" ]]; then
  export CARGO_HOME="$PWD/.cargo"
  export RUSTUP_HOME="$PWD/.rustup"
  export PATH="$CARGO_HOME/bin:$PATH"
fi
exec cargo run --release -- "$@"
