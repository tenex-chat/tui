#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CORE_LIB_DEFAULT="$ROOT_DIR/target/aarch64-apple-ios-sim/release/libtenex_core.a"
UDL_PATH="$ROOT_DIR/crates/tenex-core/src/tenex_core.udl"
OUT_DIR="$ROOT_DIR/swift-bindings"

CORE_LIB_PATH="${TENEX_CORE_LIB_PATH:-$CORE_LIB_DEFAULT}"

if [ ! -f "$CORE_LIB_PATH" ]; then
  echo "Missing $CORE_LIB_PATH; building tenex-core for iOS simulator..." >&2
  cargo build --target aarch64-apple-ios-sim --release -p tenex-core
fi

mkdir -p "$OUT_DIR"

cargo run -p tenex-core --bin uniffi-bindgen -- generate \
  --library "$CORE_LIB_PATH" \
  --language swift \
  --out-dir "$OUT_DIR"

if [ ! -f "$OUT_DIR/tenex_core.swift" ]; then
  echo "Expected $OUT_DIR/tenex_core.swift to be generated." >&2
  exit 1
fi
