#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IOS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_PARENT="$(cd "$IOS_DIR/../.." && pwd)"
VOICE_CAPTURE_KIT_LINK="$REPO_PARENT/VoiceCaptureKit"
VOICE_CAPTURE_KIT_SOURCE="${VOICE_CAPTURE_KIT_SOURCE:-/Users/pablofernandez/Work/VoiceCaptureKit}"

if [[ ! -e "$VOICE_CAPTURE_KIT_LINK" ]]; then
  if [[ -d "$VOICE_CAPTURE_KIT_SOURCE" ]]; then
    ln -s "$VOICE_CAPTURE_KIT_SOURCE" "$VOICE_CAPTURE_KIT_LINK"
  else
    echo "VoiceCaptureKit is missing at $VOICE_CAPTURE_KIT_LINK and $VOICE_CAPTURE_KIT_SOURCE." >&2
    exit 1
  fi
fi

if ! command -v tuist >/dev/null 2>&1; then
  brew update
  brew tap tuist/tuist
  brew install --formula tuist
fi

cd "$IOS_DIR"
tuist generate
