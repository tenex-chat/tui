---
title: SwiftUI Sheet Management
slug: swiftui-sheet-management
summary: "Chaining multiple `.sheet(isPresented:)` modifiers on the same SwiftUI view causes only the last sheet to be active, silently swallowing earlier ones"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-11
updated: 2026-05-11
verified: 2026-05-11
compiled-from: conversation
sources:
  - session:6e4a880c-8e83-4ef2-a336-646e335aa6f1
---

# SwiftUI Sheet Management

## Sheet Presentation

Chaining multiple `.sheet(isPresented:)` modifiers on the same SwiftUI view causes only the last sheet to be active, silently swallowing earlier ones. To avoid this, ReportsTabView uses a unified `ReportSelectionItem` enum with a single `.sheet(item:)` modifier instead of two separate `.sheet(isPresented:)` modifiers. [^6e4a8-2]

## See Also

