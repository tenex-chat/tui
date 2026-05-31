---
title: iOS Agent Configuration Sheet
slug: ios-agent-config-sheet
summary: The agent configuration sheet uses a native iOS Form layout with standard Sections instead of custom card/gradient styling
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-11
updated: 2026-05-11
verified: 2026-05-11
compiled-from: conversation
sources:
  - session:de0328ae-0556-4434-bb8f-cebb2025a34b
  - session:80ffeadd-e452-45f2-8b33-45b77424a223
---

# iOS Agent Configuration Sheet

## Layout & Styling

The agent configuration sheet uses a native iOS Form layout with standard Sections instead of custom card/gradient styling. Custom GlassPanel cards, gradient backgrounds, manual ScrollView/VStack layouts, statPill capsule badges, custom inner rounded-rectangle skill containers, and the accessibilityReduceTransparency environment read are removed from the agent configuration sheet. [^de032-1]


## Sections

The Form contains an Agent section using LabeledContent to display the agent name. The Form contains a Model section using a Picker with .navigationLink style that pushes to a list of model choices. The Form contains a Skills section with standard Toggle rows, 'Select All' and 'Clear' buttons in the section header, and a footer showing selection count. [^de032-2]

A gear button appears next to the agent avatar in the toolbar, which opens the agent's settings sheet for changing the agent's model, skills, and other configuration. [^80ffe-1]
## See Also

