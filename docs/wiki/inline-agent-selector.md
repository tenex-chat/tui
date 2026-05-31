---
title: Inline Agent Selector
slug: inline-agent-selector
summary: When starting a new conversation, the composer displays an inline agent selector listing all available agents before an agent is explicitly chosen
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-04
updated: 2026-05-11
verified: 2026-05-04
compiled-from: conversation
sources:
  - session:6bd96cc8-b96e-4d84-b89a-d41c9c0c9df1
  - session:7e8bae19-1378-449a-a622-b9deba26edf2
  - session:512e5baf-ca7e-4fbf-8801-8eaada2e459e
---

# Inline Agent Selector

## Inline Agent Selector

The inline agent selector is removed from the new conversation composer view. The showsInlineAgentSelector computed property, inlineAgentSelectorSection view builder, selectAgentInline function, and hasPickedAgentInlineSelector state are all deleted.

<!-- citations: [^6bd96-1] [^7e8ba-2] [^512e5-1] -->
## Agent Selection in Conversation View

The toolbar in the conversation view shows the avatar of the current agent as a button in the top right. Tapping the agent avatar button opens the agent selector to choose a different agent. A ComposerAgentCoordinator observable class coordinates agent state between the toolbar and the composer; MessageComposerView syncs agentCoordinator.currentAgentPubkey whenever draft.agentPubkey changes, and responds to agentCoordinator.requestedAgentPubkey via onChange. When a new conversation starts, the agent last spoken to on that project is pre-selected, persisted per-device; the default agent for a new conversation is the project's PM. The last-used agent pubkey is saved to UserDefaults only after a successful message send in a new conversation, not on every agent selection change.

<!-- citations: [^7e8ba-3] [^512e5-2] -->
## See Also

