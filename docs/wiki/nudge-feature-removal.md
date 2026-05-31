---
title: Nudge Feature Removal
slug: nudge-feature-removal
summary: The nudge feature must be entirely removed from the codebase (views, form state, modal handlers, CRUD, tool permissions, and platform glue), leaving only empty
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-04
updated: 2026-05-04
verified: 2026-05-04
compiled-from: conversation
sources:
  - session:307f5061-9802-4a47-bd42-797aa71dd277
  - session:ae21e123-bea0-4ea5-a083-dc39981d4802
  - session:e9579051-925c-4cbc-b360-c6669eba56bb
---

# Nudge Feature Removal

## Nudge Feature Removal

The nudge feature must be entirely removed from the codebase (views, form state, modal handlers, CRUD, tool permissions, and platform glue), leaving only empty pass-through stubs where FFI/messaging signatures require them. Unrelated in-progress changes (such as the NudgeSkill to Skill rename) must not be stashed, destroyed, or committed by mistake. Kind 14202 BookmarkList is completely removed from both the Swift and Rust codebases. The SkillSelectorSheet operates without bookmark state, star button, or filter toggle.

<!-- citations: [^307f5-5] [^ae21e-2] [^e9579-2] -->
## See Also

