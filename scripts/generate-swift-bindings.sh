#!/usr/bin/env bash
set -euo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# Generate bindings to a temp location, then copy into iOS source tree.
TEMP_OUT_DIR="$(mktemp -d "${TMPDIR:-/tmp}/tenex-swift-bindings.XXXXXX")"
SWIFT_OUT_DIR="$ROOT_DIR/ios-app/Sources/TenexMVP/TenexCore"
FFI_OUT_DIR="$ROOT_DIR/ios-app/Sources/TenexMVP/TenexCoreFFI"
trap 'rm -rf "$TEMP_OUT_DIR"' EXIT

# Paths for architecture-specific and universal builds
ARM64_SIM_LIB="$ROOT_DIR/target/aarch64-apple-ios-sim/release/libtenex_core.a"
X86_64_SIM_LIB="$ROOT_DIR/target/x86_64-apple-ios/release/libtenex_core.a"
UNIVERSAL_SIM_DIR="$ROOT_DIR/target/universal-ios-sim/release"
UNIVERSAL_SIM_LIB="$UNIVERSAL_SIM_DIR/libtenex_core.a"

# Allow override via environment variable
CORE_LIB_PATH="${TENEX_CORE_LIB_PATH:-$UNIVERSAL_SIM_LIB}"

# Build universal iOS simulator library if needed
if [ ! -f "$UNIVERSAL_SIM_LIB" ] || [ "${FORCE_REBUILD:-}" = "1" ]; then
  echo "Building universal iOS simulator library..." >&2

  # Build for arm64 simulator (Apple Silicon)
  if [ ! -f "$ARM64_SIM_LIB" ] || [ "${FORCE_REBUILD:-}" = "1" ]; then
    echo "  Building for aarch64-apple-ios-sim..." >&2
    cargo build --target aarch64-apple-ios-sim --release -p tenex-core
  fi

  # Build for x86_64 simulator (Intel Mac)
  if [ ! -f "$X86_64_SIM_LIB" ] || [ "${FORCE_REBUILD:-}" = "1" ]; then
    echo "  Building for x86_64-apple-ios..." >&2
    cargo build --target x86_64-apple-ios --release -p tenex-core
  fi

  # Create universal binary using lipo
  echo "  Creating universal binary with lipo..." >&2
  mkdir -p "$UNIVERSAL_SIM_DIR"
  lipo -create "$ARM64_SIM_LIB" "$X86_64_SIM_LIB" -output "$UNIVERSAL_SIM_LIB"
  echo "  Universal library created at $UNIVERSAL_SIM_LIB" >&2
fi

mkdir -p "$SWIFT_OUT_DIR"
mkdir -p "$FFI_OUT_DIR"

# Use arm64 library for uniffi-bindgen (bindings are architecture-independent)
# The universal library cannot be used directly with uniffi-bindgen
BINDGEN_LIB="${TENEX_CORE_LIB_PATH:-$ARM64_SIM_LIB}"
cargo run -p tenex-core --bin uniffi-bindgen -- generate \
  --library "$BINDGEN_LIB" \
  --language swift \
  --out-dir "$TEMP_OUT_DIR"

if [ ! -f "$TEMP_OUT_DIR/tenex_core.swift" ]; then
  echo "Expected $TEMP_OUT_DIR/tenex_core.swift to be generated." >&2
  exit 1
fi

# Copy Swift bindings to iOS source tree.
cp "$TEMP_OUT_DIR/tenex_core.swift" "$SWIFT_OUT_DIR/tenex_core.swift"

# Copy FFI files to iOS source tree.
cp "$TEMP_OUT_DIR/tenex_coreFFI.h" "$FFI_OUT_DIR/tenex_coreFFI.h"
cp "$TEMP_OUT_DIR/tenex_coreFFI.modulemap" "$FFI_OUT_DIR/tenex_coreFFI.modulemap"

echo "Swift bindings generated in ios-app sources" >&2
