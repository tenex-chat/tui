---
title: iOS Composer Telegram-Style Input
slug: ios-composer-telegram-style
summary: "The chat input UI must be styled like Telegram's input: compact height, a mic button on the right for voice recording, with no agent selector, skills button, or"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-04
updated: 2026-05-11
verified: 2026-05-04
compiled-from: conversation
sources:
  - session:307f5061-9802-4a47-bd42-797aa71dd277
  - session:ae21e123-bea0-4ea5-a083-dc39981d4802
  - session:1aa5547a-aedd-4c11-b43f-1aa41c8652fb
  - session:55c5fc7d-0ffd-488f-aacd-f048101286d3
  - session:8c920ed8-b5c1-45df-8b23-c89cd3990646
---

# iOS Composer Telegram-Style Input

## iOS Composer — Telegram Style

The chat input UI must be styled like Telegram's input: compact height, a mic button on the right for voice recording, with no agent selector, skills button, or pin button shown. The composer must use the telegram-style input row with mic↔send swap, reduced height, and no agent/project/skills chip headers cluttering the view. The mic button must swap to a send indicator when text is present in the input, and revert to the mic icon when the input is empty. When voice recording finishes, the transcribed text is sent immediately without requiring the user to tap Send. The composer input placeholder text must be 'Message...' rather than 'Type your message...'. The iOS message composer uses the telegram-style inline layout regardless of inlineLayoutStyle, not the workspace layout. The iOS composer is not wrapped in an opaque RoundedRectangle.fill(.systemBackground) shell, allowing the composer's own ToolbarGlassBackground to show through edge-to-edge. The iOS composer's trailing action button uses a single stable Button view on iOS that changes only its label content between send-arrow and mic icon, rather than an if/else ViewBuilder that swaps between two different Button types, preventing structural HStack sibling changes that cause SwiftUI to re-apply .focused() and break native iOS dictation. The macOS composer trailing action button retains the original if/else structure with keyboard shortcut support for the send action. Tapping the send button during dictation recording calls stopRecording() rather than sendMessage(), letting the existing onChange(.idle) handler send the transcribed text to avoid double-send. The send button displays a send arrow (filled circle) whenever dictation recording is active, indicating 'tap to stop & send' even before any text is transcribed. The send button is never disabled while dictation recording is active, ensuring the user can always tap to stop recording.

<!-- citations: [^307f5-3] [^ae21e-1] [^1aa55-1] [^55c5f-1] [^8c920-1] -->
## See Also

