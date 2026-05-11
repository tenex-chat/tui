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

find_installed_app_profile() {
  local destination_dir="$HOME/Library/MobileDevice/Provisioning Profiles"
  local profile

  for profile in "$destination_dir"/*.mobileprovision; do
    [[ -f "$profile" ]] || continue

    local plist_path="${RUNNER_TEMP:-/tmp}/installed-profile.$$.plist"
    security cms -D -i "$profile" > "$plist_path" 2>/dev/null || continue

    local app_id
    app_id="$(/usr/libexec/PlistBuddy -c 'Print :Entitlements:application-identifier' "$plist_path" 2>/dev/null || true)"
    if [[ "$app_id" == "${APPLE_TEAM_ID}.com.tenex.mvp" ]]; then
      /usr/libexec/PlistBuddy -c 'Print :Name' "$plist_path"
      rm -f "$plist_path"
      return 0
    fi

    rm -f "$plist_path"
  done

  return 1
}

APPLE_TEAM_ID="${APPLE_TEAM_ID:-456SHKPP26}"
APPLE_DISTRIBUTION_CERTIFICATE_PASSWORD="${APPLE_DISTRIBUTION_CERTIFICATE_PASSWORD:-}"

if [[ -n "${APP_PROVISION_PROFILE_BASE64:-}" ]]; then
  install_profile "$APP_PROVISION_PROFILE_BASE64" "CI_APP_PROFILE_SPECIFIER"
elif profile_name="$(find_installed_app_profile)"; then
  echo "Using installed provisioning profile '$profile_name'."
  if [[ -n "${GITHUB_ENV:-}" ]]; then
    echo "CI_APP_PROFILE_SPECIFIER=$profile_name" >> "$GITHUB_ENV"
  fi
else
  echo "APP_PROVISION_PROFILE_BASE64 is required because no installed com.tenex.mvp App Store profile was found." >&2
  exit 1
fi

if security find-identity -v -p codesigning | grep -q "Apple Distribution: .*(${APPLE_TEAM_ID})"; then
  echo "Using existing Apple Distribution identity from the runner keychain."
  exit 0
fi

: "${APPLE_DISTRIBUTION_CERTIFICATE_BASE64:?APPLE_DISTRIBUTION_CERTIFICATE_BASE64 is required because no local Apple Distribution identity was found}"
: "${KEYCHAIN_PASSWORD:?KEYCHAIN_PASSWORD is required when importing TestFlight signing assets}"

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
