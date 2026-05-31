---
title: iPhone Conversation Navigation
slug: iphone-conversation-navigation
summary: Conversations push onto the NavigationStack instead of presenting as a sheet on iPhone
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-03
updated: 2026-05-11
verified: 2026-05-03
compiled-from: conversation
sources:
  - session:83822ddb-cc93-4ac6-945d-9cd6f6fd6b88
  - session:88148493-78da-44b5-bf35-d4bfc1a10523
  - session:e332b76b-010a-447c-a0ee-461ff5743977
  - session:7e8bae19-1378-449a-a622-b9deba26edf2
---

# iPhone Conversation Navigation

## Navigation

Conversations push onto the NavigationStack instead of presenting as a sheet on iPhone. Tapping a conversation opens the full conversation workspace (transcript + composer) directly, bypassing the conversation overview sheet. The ConversationDetailView (the overview sheet with 'View full conversation') is entirely deleted, and all its former callsites route to ConversationWorkspaceView instead. Tapping new conversation navigates (pushes) into the conversation view instead of presenting a modal sheet; after sending the first message the user remains in that conversation. The old sheet(item: $projectForNewConversation) is replaced by navigationDestination(item: $projectForNewConversation) inside the iPhone stackLayout. The SelectedProjectForComposer struct includes a UUID id field so that re-navigating to the same project triggers a new navigation, and an agentPubkey field to carry the pre-selected agent. The chat, project, reports, and inbox toolbar (tab bar) is hidden when viewing a conversation on iPhone.

<!-- citations: [^83822-1] [^88148-3] [^e332b-1] [^7e8ba-4] -->
## Input Bar Layout

The conversation input bar matches a Telegram-style UI: a compact ~52pt pill with a single-line autoresizing text field. The input bar hides the agent selector, skills button, and pin button. The microphone icon resides inside the text-field pill on the trailing edge as a small secondary-tinted button, while the send button slides in outside the pill when the user starts typing. [^83822-2]

## Input Bar Styling

The conversation input bar uses iOS 26 Liquid Glass styling with a GlassEffectContainer wrapping a capsule-glass TextField and a .buttonStyle(.glass) trailing button that morphs mic↔send via shared glassEffectID and .bouncy animation, replacing the old material/divider chrome. [^83822-3]

## Mode Switching

ConversationWorkspaceView uses internal mode-switching via a createdConversation state; when a thread is created, handleThreadCreatedInternally resolves the conversation and sets it, flipping isNewThreadMode to false and reinitializing the viewModel with the live conversation. MessageComposerView in ConversationWorkspaceView uses .id() keyed off mode (new vs existing) to force SwiftUI recreation when transitioning from new-thread to existing-conversation mode. [^7e8ba-5]
## See Also

