---
title: TUI Inbox
slug: tui-inbox
summary: How the TUI Inbox populates and what determines whether an ask event appears there
tags:
  - tui
  - inbox
  - ask
  - messages
volatility: hot
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:06b93166-2310-4980-92b4-d9bd5df410fd
---

# TUI Inbox

> How the TUI Inbox populates and what determines whether an ask event appears there

## Overview

The Inbox view aggregates conversations visible to a user. Ask events must appear in the Inbox to be discoverable outside the conversation thread view. [^06b93-17]

## Inbox Population via p-tags

The inbox logic checks `message.p_tags` to determine whether a message should appear in a user's inbox. Events directed to a user must carry a `["p", "<pubkey>"]` tag to be routed to that user's inbox. [^06b93-18]

## Ask Inbox Visibility

An ask event that has the correct p-tag but still does not appear in the inbox suggests there is additional filtering upstream — either a filter rejecting the event before message storage, or a real-time inbox handler that applies additional constraints beyond just p-tag matching. The full inbox pipeline needs to be traced to diagnose missing inbox entries. [^06b93-19]

## See Also
- [[ask-delivery-patterns|Ask Delivery Patterns]] — related guide
- [[tui-ask-rendering|TUI Ask Rendering]] — related guide

