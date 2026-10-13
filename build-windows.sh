#!/usr/bin/env bash
# Build the Windows x86_64 binaries with the isolated rustup toolchain + mingw-w64.
set -euo pipefail
export RUSTUP_HOME="$HOME/.rustup-cross" CARGO_HOME="$HOME/.cargo-cross"
CARGO="$CARGO_HOME/bin/cargo"
TARGET=x86_64-pc-windows-gnu
command -v x86_64-w64-mingw32-gcc >/dev/null || { echo "mingw-w64 missing: sudo pacman -S --needed mingw-w64-gcc"; exit 1; }
"$CARGO" build --release --target "$TARGET" "$@"
echo "artifacts:"; ls -la "target/$TARGET/release/"*.exe
