#!/usr/bin/env bash
set -euo pipefail

require_env() {
  local name="$1"
  : "${!name:?${name} is required}"
}

require_env APP_STORE_CONNECT_KEY_ID
require_env APP_STORE_CONNECT_ISSUER_ID
require_env APP_STORE_CONNECT_API_KEY_P8
require_env KEYCHAIN_PATH
require_env CI_APP_PROFILE_SPECIFIER

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
IOS_DIR="$REPO_ROOT/ios-app"

APP_SCHEME="${APP_SCHEME:-TenexMVP}"
WORKSPACE_PATH="${WORKSPACE_PATH:-$IOS_DIR/TenexMVP.xcworkspace}"
APPLE_TEAM_ID="${APPLE_TEAM_ID:-456SHKPP26}"
BUILD_ROOT="${BUILD_ROOT:-$IOS_DIR/build}"
ARCHIVE_PATH="${ARCHIVE_PATH:-$BUILD_ROOT/TenexMVP.xcarchive}"
EXPORT_PATH="${EXPORT_PATH:-$BUILD_ROOT/testflight-$(date -u +%Y%m%d%H%M%S)}"
EXPORT_OPTIONS_PLIST="${EXPORT_OPTIONS_PLIST:-$BUILD_ROOT/ExportOptions.plist}"
DERIVED_DATA_PATH="${DERIVED_DATA_PATH:-$BUILD_ROOT/DerivedData}"
AUTH_KEY_DIR="$HOME/.appstoreconnect/private_keys"
AUTH_KEY_PATH="$AUTH_KEY_DIR/AuthKey_${APP_STORE_CONNECT_KEY_ID}.p8"

MARKETING_VERSION="${MARKETING_VERSION:-}"
if [[ -z "$MARKETING_VERSION" ]]; then
  MARKETING_VERSION="1.0"
fi

BUILD_NUMBER="${BUILD_NUMBER:-$(date -u +%Y%m%d%H%M)}"

mkdir -p "$BUILD_ROOT" "$EXPORT_PATH" "$DERIVED_DATA_PATH"
rm -rf "$ARCHIVE_PATH"

mkdir -p "$AUTH_KEY_DIR"
printf '%s' "$APP_STORE_CONNECT_API_KEY_P8" > "$AUTH_KEY_PATH"
chmod 600 "$AUTH_KEY_PATH"

if [[ ! -f "$KEYCHAIN_PATH" ]]; then
  echo "Signing keychain does not exist at $KEYCHAIN_PATH." >&2
  exit 1
fi

CODE_SIGN_ARGS=(
  CODE_SIGN_STYLE=Manual
  "CODE_SIGN_IDENTITY=Apple Distribution"
  "PROVISIONING_PROFILE_SPECIFIER=${CI_APP_PROFILE_SPECIFIER}"
  "CI_APP_PROFILE_SPECIFIER=${CI_APP_PROFILE_SPECIFIER}"
)

PROVISIONING_PROFILES_XML="
  <key>provisioningProfiles</key>
  <dict>
    <key>com.tenex.mvp</key>
    <string>${CI_APP_PROFILE_SPECIFIER}</string>
  </dict>"

cat > "$EXPORT_OPTIONS_PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>destination</key>
  <string>export</string>
  <key>method</key>
  <string>app-store-connect</string>
  <key>signingStyle</key>
  <string>manual</string>
  <key>signingCertificate</key>
  <string>Apple Distribution</string>
  <key>stripSwiftSymbols</key>
  <true/>
  <key>teamID</key>
  <string>${APPLE_TEAM_ID}</string>
  <key>uploadSymbols</key>
  <true/>${PROVISIONING_PROFILES_XML}
</dict>
</plist>
EOF

echo "Archiving ${APP_SCHEME} ${MARKETING_VERSION} (${BUILD_NUMBER}) for TestFlight."

xcodebuild \
  -workspace "$WORKSPACE_PATH" \
  -scheme "$APP_SCHEME" \
  -configuration Release \
  -destination "generic/platform=iOS" \
  -derivedDataPath "$DERIVED_DATA_PATH" \
  -archivePath "$ARCHIVE_PATH" \
  -skipPackagePluginValidation \
  "DEVELOPMENT_TEAM=${APPLE_TEAM_ID}" \
  "MARKETING_VERSION=${MARKETING_VERSION}" \
  "CURRENT_PROJECT_VERSION=${BUILD_NUMBER}" \
  archive \
  "${CODE_SIGN_ARGS[@]}"

xcodebuild \
  -exportArchive \
  -archivePath "$ARCHIVE_PATH" \
  -exportPath "$EXPORT_PATH" \
  -exportOptionsPlist "$EXPORT_OPTIONS_PLIST"

IPA_PATH="$(find "$EXPORT_PATH" -maxdepth 1 -name '*.ipa' -print -quit)"
if [[ -z "$IPA_PATH" ]]; then
  echo "No IPA was exported to $EXPORT_PATH." >&2
  exit 1
fi

upload_cmd=(
  xcrun
  altool
  --upload-app
  --type
  ios
  --file
  "$IPA_PATH"
  --apiKey
  "$APP_STORE_CONNECT_KEY_ID"
  --apiIssuer
  "$APP_STORE_CONNECT_ISSUER_ID"
  --output-format
  xml
)

if [[ -n "${APP_STORE_CONNECT_PROVIDER:-}" ]]; then
  upload_cmd+=(--asc-provider "$APP_STORE_CONNECT_PROVIDER")
fi

"${upload_cmd[@]}"

if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  {
    echo "build_number=${BUILD_NUMBER}"
    echo "marketing_version=${MARKETING_VERSION}"
    echo "ipa_path=${IPA_PATH}"
  } >> "$GITHUB_OUTPUT"
fi
