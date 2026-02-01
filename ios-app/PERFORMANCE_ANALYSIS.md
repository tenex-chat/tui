# iOS App Performance & Memory Analysis

## Executive Summary

This document identifies performance bottlenecks and memory issues in the TENEX iOS app, prioritized by severity and impact.

---

## ✅ Fixes Implemented

### 1. MarkdownView - Caching Parsed Elements
**File:** `Views/MarkdownView.swift`
**Status:** ✅ FIXED
**Issue:** Parsing markdown on every SwiftUI body evaluation

**Changes Made:**
- Added static cache with content hash key (LRU-style, max 100 entries)
- Added Equatable conformance to prevent unnecessary re-renders
- Created Identifiable `MarkdownElement` wrapper for stable ForEach identity
- Added `clearCache()` method for memory warnings

**Performance Impact:** Eliminates redundant parsing during scroll - significant reduction in CPU usage for message lists.

---

### 2. ConversationDetailViewModel - Concurrent Descendant Message Fetching
**File:** `Views/ConversationDetailViewModel.swift:231-280`
**Status:** ✅ FIXED
**Issue:** Sequential O(n) FFI calls for descendant messages

**Before:**
```swift
for descendant in allDescendants {
    let msgs = await coreManager.safeCore.getMessages(conversationId: descendant.id)
    descendantMsgs[descendant.id] = msgs
}
```

**After:**
```swift
await withTaskGroup(of: (String, [MessageInfo]).self) { group in
    for descendant in allDescendants {
        group.addTask {
            let msgs = await coreManager.safeCore.getMessages(conversationId: descendant.id)
            return (descendant.id, msgs)
        }
    }
    for await (id, msgs) in group {
        results[id] = msgs
    }
}
```

**Performance Impact:** Reduces load time from O(n * latency) to O(latency) for conversations with many descendants.

---

### 3. ConversationsTabView - Batch Hierarchy Preloading
**Files:** `ConversationsTabView.swift`, `Services/ConversationHierarchyCache.swift`
**Status:** ✅ FIXED
**Issue:** N+1 query problem - each row made 3 FFI calls on appear

**Solution:**
Created `ConversationHierarchyCache` service that:
- Preloads hierarchy for all visible conversations in a single batch operation
- Provides O(1) lookup for each row (instead of per-row FFI calls)
- Uses concurrent TaskGroup for parallel loading
- Invalidates cache on data changes

**Performance Impact:** Eliminates N+1 queries. For 20 conversations, reduces from 60+ FFI calls to ~20 (all concurrent).

---

### 4. Profiling Instrumentation
**Files:** `Profiling/PerformanceProfiler.swift`, `Profiling/ProfilingView.swift`
**Status:** ✅ IMPLEMENTED

Created comprehensive profiling utilities:
- **OSLog signposts** for Instruments integration
- **FFI metrics tracking** (call count, total/avg/max duration)
- **Memory tracking** (allocation/deallocation counts for leak detection)
- **SwiftUI view profiling** (body evaluation timing)
- **In-app ProfilingView** for real-time debugging

Usage:
```swift
// Measure FFI calls
PerformanceProfiler.shared.measureFFI("getMessages") {
    core.getMessages(conversationId: id)
}

// Measure async operations
await PerformanceProfiler.shared.measureAsync("loadData") {
    await viewModel.loadData()
}
```

---

## 🔶 Remaining Issues (Not Yet Fixed)

### HIGH Priority

#### 1. ProfilePictureCache - Synchronous FFI on Cache Miss
**File:** `App.swift:183-201`
**Severity:** HIGH
**Impact:** Main thread blocks on first access to each profile picture

**Recommendation:** Convert to async/await pattern:
```swift
func getProfilePicture(pubkey: String) async -> String? {
    // ... async implementation
}
```

#### 2. SlackMessageRow - GeometryReader Performance
**File:** `Views/SlackMessageRow.swift:123-133`
**Severity:** MEDIUM
**Impact:** Layout thrashing during scroll

**Recommendation:** Consider using `ViewThatFits` or `alignmentGuide` instead.

### MEDIUM Priority

#### 3. Task Retain Cycles in Notification Observers
**File:** `Views/ConversationDetailViewModel.swift:126-147`
**Severity:** MEDIUM (mitigated by cancellation in deinit)

The Task inside notification closure creates strong reference:
```swift
Task { await self.loadData() }  // Should be Task { [weak self] in ... }
```

#### 4. TenexCoreManager.fetchData - Sequential Operations
**File:** `App.swift:137-175`
**Severity:** LOW-MEDIUM

Could parallelize some operations, but current flow has dependencies.

### LOW Priority

- deterministicColor caching for repeated pubkeys
- DateFormatter static caching (already done in InboxView)

---

## Files Modified

| File | Changes |
|------|---------|
| `Views/MarkdownView.swift` | Added caching, Equatable, Identifiable wrapper |
| `Views/ConversationDetailViewModel.swift` | Concurrent TaskGroup for descendant messages |
| `ConversationsTabView.swift` | Batch hierarchy preloading, removed per-row FFI |
| `Services/ConversationHierarchyCache.swift` | **NEW** - Hierarchy caching service |
| `App.swift` | Added hierarchyCache to TenexCoreManager |
| `Profiling/PerformanceProfiler.swift` | **NEW** - Profiling utilities |
| `Profiling/ProfilingView.swift` | **NEW** - Debug UI for profiling |

---

## Testing & Validation

### To verify fixes:

1. **Instruments Time Profiler**
   - Launch with Product → Profile
   - Scroll through conversation list
   - Verify no repeated `parseMarkdown` calls
   - Check FFI call concurrency in Time Profiler

2. **Instruments Allocations**
   - Monitor memory during navigation
   - Verify MarkdownView cache doesn't grow unbounded
   - Check for retain cycles when dismissing views

3. **In-App Profiling**
   - Access via Diagnostics → Profiling (needs UI integration)
   - Check FFI call statistics
   - Monitor memory leak indicators

4. **Console Logging**
   - Enable DEBUG builds
   - Watch for "⚠️ Slow operation" warnings
   - Check FFI call timing logs

---

## Recommended Next Steps

1. **Merge these fixes** and test on device with large conversation lists
2. **Profile before/after** using Instruments Time Profiler
3. **Address remaining HIGH priority issues** (profile picture async)
4. **Consider lazy loading** for conversation detail view content
5. **Add memory warning handler** to clear caches:
   ```swift
   NotificationCenter.default.addObserver(
       forName: UIApplication.didReceiveMemoryWarningNotification,
       object: nil,
       queue: .main
   ) { _ in
       MarkdownView.clearCache()
       coreManager.hierarchyCache.clearCache()
   }
   ```
