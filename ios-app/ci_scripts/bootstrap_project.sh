#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IOS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

if ! command -v tuist >/dev/null 2>&1; then
  brew update
  brew tap tuist/tuist
  brew install --formula tuist
fi

cd "$IOS_DIR"
tuist generate
