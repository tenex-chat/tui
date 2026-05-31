---
title: Project Detail View
slug: project-detail-view
summary: The projects tab shows a ProjectDetailView as the primary view for a selected project, instead of showing settings directly
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-11
updated: 2026-05-11
verified: 2026-05-11
compiled-from: conversation
sources:
  - session:6c76b359-4216-4937-b2c7-3c7bc1ffed50
  - session:dce1122a-e221-4748-a714-0ebdd9a0078c
---

# Project Detail View

## Project Detail View

The projects tab shows a ProjectDetailView as the primary view for a selected project, instead of showing settings directly. Both the iPhone stack layout and the iPad/Mac split-view detail pane navigate to ProjectDetailView instead of ProjectSettingsView. The split-view detail pane for projects is wrapped in a NavigationStack to support sub-navigation. [^6c76b-1]


## Unified Feed

ProjectDetailView displays chats and reports belonging to that project in a single unified feed sorted by most recent activity. Rows used to display chats and reports in ProjectDetailView must reuse the same row components (ConversationRowFull and report row components) that are used on the chat tab and reports tab, rather than implementing separate renderers.

<!-- citations: [^6c76b-2] [^dce11-1] -->
## Project Settings Access

Project settings are accessible from a top-right tab within the project detail view. A gear icon in the top-right corner of ProjectDetailView navigates to ProjectSettingsView. [^6c76b-3]

## Sub-Navigation

Tapping a conversation in ProjectDetailView pushes ConversationAdaptiveDetailView. Tapping a report in ProjectDetailView pushes the appropriate report detail view. [^6c76b-4]
## See Also

