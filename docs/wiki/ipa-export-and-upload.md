---
title: IPA Export and Upload
slug: ipa-export-and-upload
summary: IPA exports use destination=upload in ExportOptions so xcodebuild uploads via Xcode's stored Apple ID session, bypassing the need for an App Store Connect issue
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-13
updated: 2026-05-13
verified: 2026-05-13
compiled-from: conversation
sources:
  - session:fbd9a382-9ebb-450e-a699-37ca3f63241c
---

# IPA Export and Upload

## Export Configuration

IPA exports use destination=upload in ExportOptions so xcodebuild uploads via Xcode's stored Apple ID session, bypassing the need for an App Store Connect issuer ID. [^fbd9a-2]

## See Also

