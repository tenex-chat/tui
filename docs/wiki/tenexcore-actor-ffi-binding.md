---
title: TenexCore Actor & FFI Binding
slug: tenexcore-actor-ffi-binding
summary: The actor wrapping the UniFFI-generated TenexCore FFI binding is named TenexCoreActor
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-04
updated: 2026-05-11
verified: 2026-05-04
compiled-from: conversation
sources:
  - session:ec746991-adb4-41c6-8c08-19718097c01d
  - session:bd85fbf3-e572-4155-95c9-12abf646b79e
  - session:188c28d3-b111-449c-8129-ef89b3647bf9
  - session:563f410b-5566-4d50-ac89-6886347df22b
  - session:9ba9c406-93ec-4ec5-a080-9531e27b5048
  - session:4b2873af-9a6c-4bcb-b29c-cce458e2aa4b
---

# TenexCore Actor & FFI Binding

## TenexCoreActor

The actor wrapping the UniFFI-generated TenexCore FFI binding is named TenexCoreActor (formerly SafeTenexCore). The file containing the TenexCoreActor is named TenexCoreActor.swift (formerly SafeTenexCore.swift). The protocol abstraction for the TenexCoreActor is named TenexCoreActorProtocol (formerly SafeTenexCoreProtocol), defined in TenexCoreActorProtocol.swift (formerly SafeTenexCoreProtocol.swift). TenexCoreActor and TenexCoreActorProtocol include setEventCallback and clearEventCallback methods.

<!-- citations: [^ec746-1] [^bd85f-1] -->
## TenexCoreManager Properties

TenexCoreManager exposes the actor via a property named core (formerly safeCore). It exposes the raw FFI binding via a property named rawCore (formerly core). The raw FFI binding (rawCore) is only used for intentional detached-task bypasses of actor isolation. registerEventCallback and unregisterEventCallback are async functions with await at their call sites. isLoggedIn(), refresh(), setEventCallback, and clearEventCallback use await where needed for actor isolation. The FFI callback path must process DataChange::NoteKeys into deltas for Swift consumers rather than dropping them. attemptAutoLogin() is an async function that awaits core.login(nsec:). attemptAutoLogin() calls loadNsecAsync() instead of loadNsec() to avoid a main-thread precondition failure. getProfilePicture and prefetchProfilePictures use async/await instead of synchronous DispatchQueue calls. displayName(for:) uses a lazy async cache population pattern with a profileNamesVersion counter to trigger SwiftUI re-renders. The NWPathMonitor stored property in TenexCoreManager uses AnyObject? type to avoid importing Network framework in the main manager file, with runtime casting in the extension.

<!-- citations: [^ec746-2] [^bd85f-2] [^9ba9c-11] [^4b287-2] -->
## Swift Bindings Generation

Swift bindings are auto-generated from the Rust core via UniFFI and must be regenerated when the AgentConfig struct changes. The Swift bindings for the Rust FFI use `skills`, `mcp_servers`, and `tags` parameters instead of the old `tools` parameter on `updateAgentConfig` and `updateGlobalAgentConfig`.

<!-- citations: [^188c2-4] [^563f4-2] -->
## See Also

