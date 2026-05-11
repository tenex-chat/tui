# GitHub Actions TestFlight Deployment

This repository includes a GitHub Actions workflow at `.github/workflows/testflight.yml` that archives the TENEX iOS app on a self-hosted macOS runner, exports an App Store Connect IPA, and uploads it to TestFlight.

## What The Workflow Does

1. Checks out the repository on pushes to `master` or on manual dispatch.
2. Installs the Rust iOS target and builds `tenex-core` for device.
3. Regenerates the Tuist Xcode project under `ios-app/`.
4. Installs an Apple Distribution certificate and App Store provisioning profile from GitHub Secrets.
5. Archives the `TenexMVP` scheme with a UTC timestamp build number.
6. Exports with manual App Store signing for `com.tenex.mvp` and uploads the IPA using an App Store Connect API key.

## Required Apple-Side Setup

- The app record must already exist in App Store Connect for bundle ID `com.tenex.mvp`.
- The App Store provisioning profile for `com.tenex.mvp` must be created in the Apple Developer portal.
- The Apple Distribution certificate must match team `456SHKPP26`.
- Create an App Store Connect API key with a role that can upload builds.

## Required GitHub Secrets

- `APP_STORE_CONNECT_KEY_ID`
  The App Store Connect API key ID.
- `APP_STORE_CONNECT_ISSUER_ID`
  The App Store Connect issuer ID.
- `APP_STORE_CONNECT_API_KEY_P8`
  The full contents of the downloaded `.p8` private key.
- `APPLE_DISTRIBUTION_CERTIFICATE_BASE64`
  Base64 of an exported Apple Distribution `.p12`.
- `APPLE_DISTRIBUTION_CERTIFICATE_PASSWORD`
  Password used when exporting the `.p12`.
- `KEYCHAIN_PASSWORD`
  Temporary keychain password for the GitHub runner.
- `APP_PROVISION_PROFILE_BASE64`
  Base64 of the App Store provisioning profile for `com.tenex.mvp`.
- `APP_STORE_CONNECT_PROVIDER`
  Optional provider short name if `altool` needs it for upload.

## Creating Base64 Secrets

Use these commands on a Mac before adding values to GitHub Secrets:

```sh
base64 -i AppleDistribution.p12 | pbcopy
base64 -i TenexMVP_AppStore.mobileprovision | pbcopy
```

## Local Secret Setup Helper

This repository includes `ios-app/ci_scripts/set_github_secrets.sh` to push the required GitHub Actions secrets from a local machine with `gh` installed and authenticated.

```sh
ios-app/ci_scripts/set_github_secrets.sh \
  --issuer-id YOUR_ISSUER_UUID \
  --p12 ~/Downloads/Certificates.p12 \
  --app-profile ~/Downloads/TenexMVP_AppStore.mobileprovision
```

The helper defaults to the newest `AuthKey_*.p8` file in `~/Downloads` and infers the key ID from the filename.

## Manual Releases

Trigger the workflow from the GitHub Actions UI with an optional `marketing_version` input. If omitted, the workflow uses `1.0`.

The workflow always stamps a fresh UTC build number, so repeated TestFlight uploads for the same marketing version stay monotonic.
