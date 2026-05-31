---
title: iOS Physical Device Build
slug: ios-physical-device-build
summary: The iOS app must build and run on the user's physical iPhone device, not just the simulator.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-04
updated: 2026-05-13
verified: 2026-05-04
compiled-from: conversation
sources:
  - session:307f5061-9802-4a47-bd42-797aa71dd277
  - session:188c28d3-b111-449c-8129-ef89b3647bf9
  - session:563f410b-5566-4d50-ac89-6886347df22b
  - session:173aa11a-834f-405c-8af8-c8a45f409020
  - session:d70dee43-71f9-4e8a-9ccd-15fb7dd9dd2e
  - session:d4fe90da-ffc7-427f-8b05-a53d310a7a43
  - session:9a06fd27-53a2-4274-a321-65b5dcbc43ad
  - session:fbd9a382-9ebb-450e-a699-37ca3f63241c
---

# iOS Physical Device Build

## Physical Device Requirement

The iOS app must build and run on the user's physical iPhone or iPad device, not just the simulator. The iOS app can be installed directly onto a connected iPhone by building the Rust core for aarch64-apple-ios, generating the Xcode project with Tuist, and then building and installing via xcodebuild. The iOS/macOS app is built and launched on both the connected iPhone and Mac desktop for verification after changes land. When deploying to a physical iPad, XcodeBuildMCP device workflow tools may be unavailable, requiring command-line build and deployment via xcodebuild. The connected physical iPad is identified as 'iPad (6)' — iPad Pro 12.9" (5th gen). The iOS app bundle identifier is com.tenex.mvp. The Release build configuration uses Manual code signing style in Project.swift, while Debug uses Automatic, ensuring provisioning profiles apply only to the main app target and not to Swift package dependencies. iOS app signing uses automatic signing with team `456SHKPP26`. The `generate-swift-bindings.sh` script is run with `PLATFORM_NAME=iphoneos` to regenerate Swift bindings for iOS device builds.

<!-- citations: [^173aa-1] [^307f5-4] [^188c2-2] [^563f4-1] [^d70de-1] [^d4fe9-2] [^9a06f-1] [^fbd9a-1] -->
## TestFlight Deployment Secrets

The App Store Connect API key `AuthKey_9HUH4HRW25.p8` is used for the TestFlight deployment secrets. The App Store Connect Issuer ID for the TENEX project is `0acdb473-8d3f-4eba-85bc-d2de82234bea`. The distribution certificate P12 is exported with no password. [^173aa-2]
## See Also

