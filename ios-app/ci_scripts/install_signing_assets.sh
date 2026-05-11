#!/usr/bin/env bash
set -euo pipefail

decode_base64_to_file() {
  local encoded="$1"
  local output_path="$2"

  printf '%s' "$encoded" | tr -d '\n' | openssl base64 -d -A -out "$output_path"
}

install_profile() {
  local encoded="$1"
  local env_var="$2"

  if [[ -z "$encoded" ]]; then
    return 0
  fi

  local label
  label="$(echo "$env_var" | tr '[:upper:]' '[:lower:]')"
  local raw_profile_path="${RUNNER_TEMP:-/tmp}/${label}.mobileprovision"
  local plist_path="${RUNNER_TEMP:-/tmp}/${label}.plist"
  local destination_dir="$HOME/Library/MobileDevice/Provisioning Profiles"

  decode_base64_to_file "$encoded" "$raw_profile_path"
  security cms -D -i "$raw_profile_path" > "$plist_path"

  local uuid
  local name
  uuid="$(/usr/libexec/PlistBuddy -c 'Print :UUID' "$plist_path")"
  name="$(/usr/libexec/PlistBuddy -c 'Print :Name' "$plist_path")"

  mkdir -p "$destination_dir"
  cp "$raw_profile_path" "$destination_dir/$uuid.mobileprovision"

  echo "Installed provisioning profile '$name' ($uuid)."

  if [[ -n "${GITHUB_ENV:-}" ]]; then
    echo "${env_var}=${name}" >> "$GITHUB_ENV"
  fi
}

: "${APPLE_DISTRIBUTION_CERTIFICATE_BASE64:?APPLE_DISTRIBUTION_CERTIFICATE_BASE64 is required for TestFlight signing}"
: "${APPLE_DISTRIBUTION_CERTIFICATE_PASSWORD:?APPLE_DISTRIBUTION_CERTIFICATE_PASSWORD is required for TestFlight signing}"
: "${KEYCHAIN_PASSWORD:?KEYCHAIN_PASSWORD is required for TestFlight signing}"
: "${APP_PROVISION_PROFILE_BASE64:?APP_PROVISION_PROFILE_BASE64 is required for TestFlight signing}"

CERTIFICATE_PATH="${RUNNER_TEMP:-/tmp}/apple_distribution.p12"
KEYCHAIN_PATH="${RUNNER_TEMP:-/tmp}/app-signing.keychain-db"

decode_base64_to_file "$APPLE_DISTRIBUTION_CERTIFICATE_BASE64" "$CERTIFICATE_PATH"

security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
security set-keychain-settings -lut 21600 "$KEYCHAIN_PATH"
security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
security import "$CERTIFICATE_PATH" -P "$APPLE_DISTRIBUTION_CERTIFICATE_PASSWORD" -A -t cert -f pkcs12 -k "$KEYCHAIN_PATH"
security set-key-partition-list -S apple-tool:,apple: -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN_PATH"
security list-keychains -d user -s "$KEYCHAIN_PATH" "$HOME/Library/Keychains/login.keychain-db" /Library/Keychains/System.keychain

if [[ -n "${GITHUB_ENV:-}" ]]; then
  echo "KEYCHAIN_PATH=$KEYCHAIN_PATH" >> "$GITHUB_ENV"
fi

echo "Installed Apple distribution certificate into a temporary keychain."

install_profile "$APP_PROVISION_PROFILE_BASE64" "CI_APP_PROFILE_SPECIFIER"
