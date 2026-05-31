---
title: Reconnect and Refresh
slug: reconnect-and-refresh
summary: When the iOS app returns from background to foreground and the user is logged in, reconnectAndRefresh() is called immediately to avoid waiting for stale connect
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-11
updated: 2026-05-11
verified: 2026-05-11
compiled-from: conversation
sources:
  - session:27aca61b-6a75-41d2-be84-f72ffb614f39
  - session:4b2873af-9a6c-4bcb-b29c-cce458e2aa4b
---

# Reconnect and Refresh

## Reconnect and Refresh

When the iOS app transitions from background to active and the user is logged in, reconnectAndRefresh() is called immediately to proactively force-reconnect rather than waiting for heartbeat timeouts. An NWPathMonitor watches for the network path transitioning from unsatisfied to satisfied and calls reconnectAndRefresh() when connectivity is restored. reconnectAndRefresh() calls forceReconnect() on the Rust core to drop stale WebSocket connections and reconnect all relays with restarted subscriptions, then calls fetchData() to refresh local UI state from nostrdb. reconnectAndRefresh() is debounced to 5 seconds to prevent double-reconnects when both foreground-return and network-restoration triggers fire simultaneously. The network monitor starts after login and stops on logout.

<!-- citations: [^27aca-1] [^4b287-1] -->
## See Also

