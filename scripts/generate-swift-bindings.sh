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
IOS_DEVICE_LIB="$ROOT_DIR/target/aarch64-apple-ios/release/libtenex_core.a"
MACOS_LIB="$ROOT_DIR/target/release/libtenex_core.a"
UNIVERSAL_SIM_DIR="$ROOT_DIR/target/universal-ios-sim/release"
UNIVERSAL_SIM_LIB="$UNIVERSAL_SIM_DIR/libtenex_core.a"

platform_name="${PLATFORM_NAME:-}"
default_bindgen_lib=""

build_ios_sim_libs() {
  echo "Building iOS simulator libraries..." >&2
  cargo build --target aarch64-apple-ios-sim --release -p tenex-core
  cargo build --target x86_64-apple-ios --release -p tenex-core

  echo "Creating universal simulator library..." >&2
  mkdir -p "$UNIVERSAL_SIM_DIR"
  lipo -create "$ARM64_SIM_LIB" "$X86_64_SIM_LIB" -output "$UNIVERSAL_SIM_LIB"
}

case "$platform_name" in
  macosx)
    echo "Building macOS Rust library for bindings..." >&2
    cargo build --release -p tenex-core
    default_bindgen_lib="$MACOS_LIB"
    ;;
  iphoneos)
    echo "Building iOS device Rust library for bindings..." >&2
    cargo build --target aarch64-apple-ios --release -p tenex-core
    default_bindgen_lib="$IOS_DEVICE_LIB"
    ;;
  iphonesimulator|"")
    # Empty PLATFORM_NAME is treated as simulator for direct script runs.
    build_ios_sim_libs
    # Universal archive is used for linking, but bindgen must read a thin archive.
    default_bindgen_lib="$ARM64_SIM_LIB"
    ;;
  *)
    echo "Unknown PLATFORM_NAME '$platform_name'; defaulting to macOS bindings." >&2
    cargo build --release -p tenex-core
    default_bindgen_lib="$MACOS_LIB"
    ;;
esac

mkdir -p "$SWIFT_OUT_DIR"
mkdir -p "$FFI_OUT_DIR"

# Optional override for advanced local workflows.
BINDGEN_LIB="${TENEX_CORE_LIB_PATH:-$default_bindgen_lib}"

if [ ! -f "$BINDGEN_LIB" ]; then
  echo "Expected Rust library at $BINDGEN_LIB" >&2
  exit 1
fi

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
