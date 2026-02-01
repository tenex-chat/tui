# iOS App Profiling Analysis

## Overview
This document captures the performance and memory analysis of the TENEX iOS Client.

**Date:** 2026-02-01
**Branch:** feature/ios-profiling
**Analyzed by:** claude-code

---

## Executive Summary

The iOS app has a **solid profiling foundation** already in place (`PerformanceProfiler.swift`, `ProfilingView.swift`). The architecture follows best practices with MVVM, actor-based SafeTenexCore for thread safety, and a centralized `TenexCoreManager`.

However, there are **key performance bottlenecks** and **potential memory issues** that need attention. This analysis identifies issues and proposes instrumentation to validate/quantify them.

---

## Existing Profiling Infrastructure

### Already Implemented ✅
1. **PerformanceProfiler.swift** - Comprehensive profiling utilities:
   - OSLog categories: Performance, FFI, SwiftUI, Memory
   - `measure()` / `measureAsync()` - Timing with signpost integration
   - `measureFFI()` - FFI-specific timing with metrics collection
   - `measureViewBody()` - SwiftUI view body profiling
   - `FFIMetrics` - Aggregate FFI call statistics
   - `MemoryMetrics` - Allocation/deallocation tracking
   - `ProfiledViewModel` - Base class with lifecycle logging
   - `profileBody()` view modifier

2. **ProfilingView.swift** - In-app debugging UI:
   - FFI Calls tab - shows call counts, total/avg/max duration
   - Memory tab - shows potential leaks
   - Performance Tips tab

### Missing/Needs Enhancement ⚠️
1. **FFI instrumentation not applied** - SafeTenexCore methods aren't wrapped with `measureFFI()`
2. **No real-time memory monitoring** - Only allocation counting, not actual memory footprint
3. **No frame rate monitoring** - Can't detect UI jank
4. **ViewModels not using ProfiledViewModel** - Missing lifecycle tracking

---

## Architecture Summary

```
┌─────────────────────────────────────────────────────────────────┐
│                        App.swift (Entry)                         │
├─────────────────────────────────────────────────────────────────┤
│                      TenexCoreManager                            │
│  - Projects, InboxItems (centralized state)                      │
│  - ConversationHierarchyCache (N+1 optimization)                │
│  - SafeTenexCore (actor for FFI thread safety)                   │
├─────────────────────────────────────────────────────────────────┤
│                        TenexCore (FFI)                           │
│  - Rust library via UniFFI                                       │
│  - Event callbacks → TenexEventHandler                          │
└─────────────────────────────────────────────────────────────────┘
```

---

## Identified Performance Issues

### 🔴 CRITICAL - Must Fix

#### 1. **FFI Calls Not Profiled**
**Location:** `SafeTenexCore.swift`
**Issue:** All FFI methods call through to `core.*` directly without timing/profiling
**Impact:** Cannot measure actual FFI overhead; blind to slow operations
**Solution:** Wrap all FFI calls with `PerformanceProfiler.shared.measureFFI()`

```swift
// BEFORE:
func getProjects() -> [ProjectInfo] {
    core.getProjects()
}

// AFTER:
func getProjects() -> [ProjectInfo] {
    PerformanceProfiler.shared.measureFFI("getProjects") {
        core.getProjects()
    }
}
```

#### 2. **ConversationDetailViewModel - Unbounded Descendant Fetching**
**Location:** `ConversationDetailViewModel.swift` lines 248-266
**Issue:** For conversations with many descendants, fetches ALL descendant messages concurrently
```swift
let descendantMsgs = await withTaskGroup(...) { group in
    for descendant in allDescendants {
        group.addTask {
            let msgs = await coreManager.safeCore.getMessages(conversationId: descendant.id)
            return (descendant.id, msgs)
        }
    }
    ...
}
```
**Impact:** For large conversation trees (100+ descendants), this spawns 100+ concurrent FFI calls
**Recommendation:**
- Add batching (process in groups of 10-20)
- Implement lazy loading (only load on-screen descendants)
- Add memory limit for stored messages

#### 3. **MarkdownView - Static Cache Without Eviction Policy**
**Location:** `MarkdownView.swift` lines 16-17, 47-53
**Issue:** Static parse cache with simple FIFO eviction
```swift
private static var parseCache: [Int: [MarkdownElement]] = [:]
private static let maxCacheSize = 100

// Eviction removes half the cache randomly
let keysToRemove = Array(Self.parseCache.keys.prefix(Self.maxCacheSize / 2))
```
**Impact:**
- Cache eviction removes random entries, not LRU
- Hash collision could cause incorrect rendering
- Large markdown content stored in memory indefinitely
**Recommendation:**
- Implement proper LRU eviction
- Use content length limits
- Consider NSCache for automatic memory pressure handling

---

### 🟠 HIGH - Should Fix

#### 4. **NotificationCenter Overuse in ConversationDetailViewModel**
**Location:** `ConversationDetailViewModel.swift` lines 124-148
**Issue:** Subscribes to `.tenexDataChanged` which fires on ANY data change
```swift
dataChangedObserver = NotificationCenter.default.addObserver(
    forName: .tenexDataChanged,
    ...
) { [weak self] _ in
    Task { await self?.loadData() }
}
```
**Impact:** Every general data change triggers full `loadData()` for all open detail views
**Recommendation:** Filter notifications more specifically or debounce

#### 5. **ConversationHierarchyCache - Unbounded Cache Growth**
**Location:** `ConversationHierarchyCache.swift`
**Issue:** Cache grows without limit; only cleared on logout or memory warning
```swift
private var cache: [String: ConversationHierarchy] = [:]
private var loadedForConversationIds: Set<String> = []
```
**Impact:** Memory grows as user browses conversations over time
**Recommendation:**
- Implement max cache size with LRU eviction
- Subscribe to `UIApplication.didReceiveMemoryWarningNotification`

#### 6. **SlackMessageRow - GeometryReader for Height Measurement**
**Location:** `SlackMessageRow.swift` lines 123-133
**Issue:** Uses GeometryReader to measure content height on every render
```swift
.background(
    GeometryReader { geometry in
        Color.clear
            .onAppear { contentHeight = geometry.size.height }
            .onChange(of: message.content) { ... }
    }
)
```
**Impact:** Extra layout pass for every message; compounds in long lists
**Recommendation:** Pre-compute height estimate based on content length, or use LazyVStack virtualization properly

---

### 🟡 MEDIUM - Nice to Fix

#### 7. **Regex Parsing on Every Inline Markdown**
**Location:** `MarkdownView.swift` lines 183-217
**Issue:** `parseInlineMarkdown()` uses regex matching in a while loop
```swift
while !current.isEmpty {
    if let boldRange = current.range(of: "\\*\\*(.+?)\\*\\*", options: .regularExpression) {
        ...
    } else if let codeRange = current.range(of: "`(.+?)`", options: .regularExpression) {
        ...
    }
}
```
**Impact:** Regex operations are expensive; called for every line of markdown
**Recommendation:** Pre-compile regex patterns; cache per-line results

#### 8. **Profile Picture Loading Not Cached**
**Location:** Various uses of `getProfilePicture(pubkey:)`
**Issue:** Each call goes to FFI without client-side caching
**Recommendation:** Cache profile pictures at Swift layer

#### 9. **TodoParser Called Multiple Times for Same Messages**
**Location:** `ConversationDetailViewModel.swift` lines 334-338
**Issue:** Parses todos from messages, stores result, but parses current conversation again on line 338
```swift
for (convId, msgs) in descendantMessages {
    parsedTodoStates[convId] = TodoParser.parse(messages: msgs)
}
todoState = TodoParser.parse(messages: messages)  // <-- Also parsed above if current is a descendant?
```
**Impact:** Minor - redundant parsing
**Recommendation:** Unify todo parsing logic

---

## Memory Analysis

### Known Good Patterns ✅
1. **Weak references** used correctly in view models (`weak var coreManager`)
2. **Task cancellation** handled properly (`loadTask?.cancel()` in deinit)
3. **NotificationCenter observers** removed in deinit
4. **Actor isolation** prevents data races

### Potential Memory Issues ⚠️

| Issue | Location | Risk | Notes |
|-------|----------|------|-------|
| Static parse cache | MarkdownView | Medium | Grows unbounded, no memory pressure response |
| Hierarchy cache | ConversationHierarchyCache | Medium | No max size, only cleared on logout |
| Descendant messages dict | ConversationDetailViewModel | High | Stores ALL descendant messages in memory |
| Child conversations array | ConversationDetailViewModel | Low | Usually small, but could grow |

### Memory Profiling Checklist
- [ ] Run Instruments Allocations over 30-min session
- [ ] Monitor memory footprint during deep conversation navigation
- [ ] Check for monotonic memory growth pattern
- [ ] Verify cleanup on logout

---

## Performance Profiling Checklist

### Instruments Templates
1. **Time Profiler** - Find slow functions
2. **Allocations** - Track memory growth
3. **Leaks** - Detect retain cycles
4. **SwiftUI** - View body computation
5. **System Trace** - I/O and threading

### Key Scenarios to Profile
- [ ] App launch to first content
- [ ] Project list loading
- [ ] Conversation list scrolling
- [ ] Deep conversation tree navigation
- [ ] Large markdown message rendering
- [ ] Rapid navigation between views
- [ ] Memory under sustained use (30+ min)

---

## Recommended Fixes (Priority Order)

### Phase 1: Enable Visibility (This PR) ✅ COMPLETE
1. ✅ Add FFI profiling to SafeTenexCore - All 40+ FFI methods instrumented
2. ✅ Add real-time memory monitoring overlay - `MemoryMonitor` + `PerformanceOverlayView`
3. ✅ Add frame rate monitoring - `FrameRateMonitor` with dropped frame detection

### Phase 2: Fix Critical Issues
1. [ ] Batch/limit descendant message fetching
2. [ ] Implement proper LRU caching for markdown
3. [ ] Add memory pressure response handlers

### Phase 3: Optimize
1. [ ] Debounce NotificationCenter handlers
2. [ ] Pre-compute markdown heights
3. [ ] Cache profile pictures at Swift layer
4. [ ] Pre-compile regex patterns

---

## Files Modified in This Branch

| File | Changes | Status |
|------|---------|--------|
| `SafeTenexCore.swift` | Wrapped all FFI calls with `measureFFI()` profiling | ✅ Complete |
| `PerformanceProfiler.swift` | Added MemoryMonitor, FrameRateMonitor, PerformanceOverlayView | ✅ Complete |
| `PROFILING_ANALYSIS.md` | This comprehensive analysis document | ✅ Complete |

---

## How to Use Profiling

### In-App Profiling View
1. Navigate to Diagnostics tab
2. Open "Performance Profiling" section
3. Use app normally to generate data
4. Review FFI calls and memory metrics

### Instruments Profiling
1. Open Xcode → Product → Profile (Cmd+I)
2. Select "Time Profiler" template
3. Run scenarios from checklist above
4. Analyze hotspots and call trees

### Console Logging
All profiling output uses OSLog with category filtering:
```bash
# View performance logs
log stream --predicate 'subsystem == "com.tenex.app" AND category == "Performance"'

# View FFI timing
log stream --predicate 'subsystem == "com.tenex.app" AND category == "FFI"'

# View memory events
log stream --predicate 'subsystem == "com.tenex.app" AND category == "Memory"'
```

---

## Next Steps

1. Review and merge FFI profiling changes
2. Run profiling session with real user data
3. Quantify actual impact of identified issues
4. Prioritize fixes based on measured data
5. Implement Phase 2 fixes

