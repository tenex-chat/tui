#!/usr/bin/env bash
set -euo pipefail

KEYCHAIN_PATH="${RUNNER_TEMP:-/tmp}/app-signing.keychain-db"

if [[ -f "$KEYCHAIN_PATH" ]]; then
  security delete-keychain "$KEYCHAIN_PATH" || true
  echo "Deleted temporary signing keychain."
fi

security list-keychains -d user -s "$HOME/Library/Keychains/login.keychain-db" /Library/Keychains/System.keychain
security default-keychain -s "$HOME/Library/Keychains/login.keychain-db"
echo "Restored login keychain as default."
