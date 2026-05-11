#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ios-app/ci_scripts/set_github_secrets.sh --issuer-id <UUID> [options]

Options:
  --repo <owner/name>               GitHub repository. Defaults from origin remote.
  --issuer-id <UUID>                Required App Store Connect issuer ID.
  --auth-key <path>                 Path to AuthKey_*.p8. Defaults to newest in ~/Downloads.
  --key-id <id>                     App Store Connect key ID. Defaults from AuthKey filename.
  --provider <name>                 Optional App Store Connect provider short name.
  --p12 <path>                      Apple Distribution .p12 export for manual signing.
  --p12-password <password>         .p12 export password. Prompted if omitted.
  --keychain-password <password>    Temp CI keychain password. Random if omitted.
  --app-profile <path>              App Store provisioning profile for com.tenex.mvp.
  --help                            Show this help.

Examples:
  ios-app/ci_scripts/set_github_secrets.sh \
    --issuer-id 00000000-0000-0000-0000-000000000000

  ios-app/ci_scripts/set_github_secrets.sh \
    --issuer-id 00000000-0000-0000-0000-000000000000 \
    --p12 ~/Downloads/Certificates.p12 \
    --app-profile ~/Downloads/TenexMVP_AppStore.mobileprovision
EOF
}

die() {
  echo "Error: $*" >&2
  exit 1
}

repo_from_origin() {
  local remote
  remote="$(git remote get-url origin 2>/dev/null || true)"
  if [[ "$remote" =~ github\.com[:/]([^/]+)/([^.]+)(\.git)?$ ]]; then
    printf '%s/%s\n' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}"
  fi
}

newest_auth_key() {
  ls -t "$HOME"/Downloads/AuthKey_*.p8 2>/dev/null | head -n 1
}

base64_single_line() {
  base64 -i "$1" | tr -d '\n'
}

set_secret_body() {
  local name="$1"
  local value="$2"

  gh secret set "$name" -R "$REPO" --body "$value"
  echo "Set $name"
}

set_secret_file() {
  local name="$1"
  local path="$2"

  gh secret set "$name" -R "$REPO" < "$path"
  echo "Set $name from $path"
}

REPO="${REPO:-$(repo_from_origin)}"
ISSUER_ID="${APP_STORE_CONNECT_ISSUER_ID:-}"
AUTH_KEY_PATH="${AUTH_KEY_PATH:-}"
KEY_ID="${APP_STORE_CONNECT_KEY_ID:-}"
PROVIDER="${APP_STORE_CONNECT_PROVIDER:-}"
P12_PATH="${P12_PATH:-}"
P12_PASSWORD="${APPLE_DISTRIBUTION_CERTIFICATE_PASSWORD:-}"
KEYCHAIN_PASSWORD="${KEYCHAIN_PASSWORD:-}"
APP_PROFILE_PATH="${APP_PROFILE_PATH:-}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)          REPO="${2:-}"; shift 2 ;;
    --issuer-id)     ISSUER_ID="${2:-}"; shift 2 ;;
    --auth-key)      AUTH_KEY_PATH="${2:-}"; shift 2 ;;
    --key-id)        KEY_ID="${2:-}"; shift 2 ;;
    --provider)      PROVIDER="${2:-}"; shift 2 ;;
    --p12)           P12_PATH="${2:-}"; shift 2 ;;
    --p12-password)  P12_PASSWORD="${2:-}"; shift 2 ;;
    --keychain-password) KEYCHAIN_PASSWORD="${2:-}"; shift 2 ;;
    --app-profile)   APP_PROFILE_PATH="${2:-}"; shift 2 ;;
    --help|-h)       usage; exit 0 ;;
    *) die "Unknown argument: $1" ;;
  esac
done

command -v gh >/dev/null 2>&1 || die "gh is not installed"
gh auth status >/dev/null

[[ -n "$REPO" ]] || die "Could not infer GitHub repo from origin remote; pass --repo"
[[ -n "$ISSUER_ID" ]] || die "Missing --issuer-id"

if [[ -z "$AUTH_KEY_PATH" ]]; then
  AUTH_KEY_PATH="$(newest_auth_key)"
fi
[[ -n "$AUTH_KEY_PATH" ]] || die "No AuthKey_*.p8 found; pass --auth-key"
[[ -f "$AUTH_KEY_PATH" ]] || die "Auth key not found: $AUTH_KEY_PATH"

if [[ -z "$KEY_ID" ]]; then
  if [[ "$(basename "$AUTH_KEY_PATH")" =~ ^AuthKey_([A-Z0-9]+)\.p8$ ]]; then
    KEY_ID="${BASH_REMATCH[1]}"
  else
    die "Could not infer key ID from $(basename "$AUTH_KEY_PATH"); pass --key-id"
  fi
fi

echo "Repository: $REPO"
echo "Auth key:   $AUTH_KEY_PATH"
echo "Key ID:     $KEY_ID"

set_secret_body APP_STORE_CONNECT_KEY_ID "$KEY_ID"
set_secret_body APP_STORE_CONNECT_ISSUER_ID "$ISSUER_ID"
set_secret_file APP_STORE_CONNECT_API_KEY_P8 "$AUTH_KEY_PATH"

if [[ -n "$PROVIDER" ]]; then
  set_secret_body APP_STORE_CONNECT_PROVIDER "$PROVIDER"
fi

if [[ -n "$P12_PATH" ]]; then
  [[ -f "$P12_PATH" ]] || die "p12 file not found: $P12_PATH"

  if [[ -z "$P12_PASSWORD" ]]; then
    if [[ -t 0 ]]; then
      read -rsp "P12 password: " P12_PASSWORD
      echo
    else
      die "Missing --p12-password for $P12_PATH"
    fi
  fi

  if [[ -z "$KEYCHAIN_PASSWORD" ]]; then
    KEYCHAIN_PASSWORD="$(openssl rand -base64 24 | tr -d '\n')"
  fi

  set_secret_body APPLE_DISTRIBUTION_CERTIFICATE_BASE64 "$(base64_single_line "$P12_PATH")"
  set_secret_body APPLE_DISTRIBUTION_CERTIFICATE_PASSWORD "$P12_PASSWORD"
  set_secret_body KEYCHAIN_PASSWORD "$KEYCHAIN_PASSWORD"
fi

if [[ -n "$APP_PROFILE_PATH" ]]; then
  [[ -f "$APP_PROFILE_PATH" ]] || die "App provisioning profile not found: $APP_PROFILE_PATH"
  set_secret_body APP_PROVISION_PROFILE_BASE64 "$(base64_single_line "$APP_PROFILE_PATH")"
fi

echo "Finished setting GitHub Actions secrets."
