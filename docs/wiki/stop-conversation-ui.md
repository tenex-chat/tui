---
title: Stop Conversation UI
slug: stop-conversation-ui
summary: The conversation composer shows a stop button that replaces the send button (or mic button on iOS) when the input is empty and agents are actively working.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-12
updated: 2026-05-12
verified: 2026-05-12
compiled-from: conversation
sources:
  - session:946e795a-ad90-454d-99d5-0241b2fdacb3
---

# Stop Conversation UI

## Stop Button Visibility

The conversation composer shows a stop button that replaces the send button (or mic button on iOS) when the input is empty and agents are actively working. [^946e7-2]


MessageComposerView accepts `isConversationActive: Bool` and `onStop: (() -> Void)?` parameters. ConversationWorkspaceView wires `isConversationActive` from `viewModel.currentIsActive` and an `onStop` closure that calls `core.stopConversation` to MessageComposerView. [^946e7-3]

On iOS, the composer trailing action button uses a single stable Button with conditional branches rather than swapping Button types, because swapping Button types breaks keyboard dictation. When `isConversationActive && !showSend && !isRecording`, the button shows a red circle with the `stop.fill` icon. [^946e7-4]

On macOS, the composer shows a separate red stop Button when `isConversationActive` and `canSend` is false. [^946e7-5]
## See Also

