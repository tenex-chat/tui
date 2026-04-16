# Reports Tab Implementation

## Context

The TENEX iOS/Mac app currently has multiple main tabs: Chats, Projects, Inbox, and Settings (in sidebar views). Users need a way to browse, organize, and interact with NIP-23 Long-form Articles (kind 30023) stored as reports. Reports contain metadata (author, title, summary, markdown content) and hashtags (t-tags) that can be used for grouping. The app already has the data structures, FFI bindings, and markdown rendering capabilities to support this feature.

**Existing infrastructure:**
- `Report` struct in `tenex_core.swift` (lines 6828-6917) with fields: `id`, `slug`, `projectATag`, `author`, `title`, `summary`, `content`, `hashtags`, `createdAt`, `readingTimeMins`
- `AppSection` enum in `MainTabView.swift` (lines 3-60) controls tab routing
- `TenexCoreManager.swift` (line 81 area) manages reactive state for app entities
- `TenexEventHandler.swift` (line 94-95) handles Nostr events, including reportUpsert
- `MarkdownView.swift` already implemented and ready to render markdown content
- `MessageComposerView.swift` (line 41) has `referenceConversationId` but needs `referenceReportATag` parameter
- `SafeTenexCore.swift` wraps FFI calls for type safety
- `getDocumentThreads(reportATag:)` at `tenex_core.swift:883` expects a-tag format: `30023:pubkey:slug`

**Tab routing infrastructure:**
- `compactTabView` (lines 168-200) uses hardcoded integer values: Chats=0, Projects=1, Inbox=3, Search=10
- `sidebarAdaptableTabView` (lines 204-250) uses: Chats=0, Projects=1, Inbox=3, LLM Runtime=2, Teams=5, Agent Definitions=6, Nudges=7, Settings=8
- Reports will use value 4 in both views (first available unused value in each)

**Why this change:** Reports provide a structured way for users to organize long-form content (articles, plans, analysis) with tagging and search. By placing the Reports tab between Projects and Inbox, we make it a discoverable top-level feature alongside primary workflows.

## Approach

The implementation follows a **dependency-order workflow** moving from navigation infrastructure → data layer integration → UI presentation → chat integration:

1. **Navigation Layer:** Add Reports to `AppSection` enum and wire it into both compact and sidebar tab views with correct tab value (4)
2. **Data Layer:** Add reactive `reports` storage and `reportsVersion` counter to `TenexCoreManager`, wire `reportUpsert` event handler, add FFI wrapper, and integrate initial load into `fetchData()`
3. **UI Layer:** Create `ReportsTabView.swift` with grouped list presentation, empty state handling, and markdown detail view
4. **Chat Integration:** Thread `referenceReportATag` parameter (in full a-tag format) through `MessageComposerView` and FFI sendThread call

This approach ensures the tab is accessible before the data layer wires reports, allowing for parallel work on UI and event handling. The version counter pattern matches existing reactive state patterns in `TenexCoreManager` for other data types.

**Alternatives considered:**
- **Nested within Projects:** Reports could have been a detail view under each project, but t-tag grouping suggests they are independently queryable across projects, justifying a top-level tab.
- **Searchable modal instead of tab:** A modal search interface for reports would save tab space but reduces discoverability and makes browsing less efficient than a dedicated list view.
- **Single-view report rendering without grouping:** Could skip t-tag grouping to reduce complexity, but grouping aligns with NIP-23 conventions and improves UX for users with many reports.

The chosen approach leverages existing architectural patterns (tab-based navigation, reactive state, markdown rendering) and integrates smoothly with the event-driven data flow.

**Data scope decision:** Reports are globally loaded for all projects via `getReports(projectId)` called during `fetchData()`. This aligns with the read-only nature of reports and allows users to browse cross-project reports. (See discussion in Execution Order, step 5b.)

## File Changes

### `ios-app/Sources/TenexMVP/Navigation/MainTabView.swift`

- **Action:** Modify
- **What:** 
  - Add `case reports` to `AppSection` enum at line 6, between `.chats` and `.projects`, so the enum reads: `case chats`, `case reports`, `case projects`, `case agents`, ...
  - In `AppSection.title` switch (lines 19-34), add: `case .reports: return "Reports"`
  - In `AppSection.systemImage` switch (lines 36-51), add: `case .reports: return "doc.richtext"`
  - In `compactTabView` (lines 168-200), insert a Reports tab between the Projects tab (line 176) and Inbox tab (line 182) with **value 4**:
    ```swift
    Tab("Reports", systemImage: "doc.richtext", value: 4) {
      ReportsTabView()
        .environment(coreManager)
        .nowPlayingInset(coreManager: coreManager)
    }
    ```
  - In `sidebarAdaptableTabView` (lines 204-250), insert the Reports tab between Projects (line 212) and Inbox (line 218) with **value 4**:
    ```swift
    Tab("Reports", systemImage: "doc.richtext", value: 4) {
      ReportsTabView()
        .environment(coreManager)
        .nowPlayingInset(coreManager: coreManager)
    }
    ```
- **Why:** Makes the Reports section selectable and routable from both compact (iPhone) and sidebar (iPad/Mac) navigation modes. Using value 4 avoids collision: compactTabView has 0, 1, 3, 10; sidebarAdaptableTabView has 0, 1, 3, 2, 5, 6, 7, 8. Both leave 4 unused. See note on tab ordering and hardcoded values below.

**Tab ordering clarification:** The `AppSection` enum lists cases in logical order (chats, reports, projects, agents, etc.), but **tab visibility differs between compact and sidebar modes:**
- **Compact (iPhone):** Tabs shown in tab bar; includes Chats (0), Reports (4), Projects (1), Inbox (3), Search (10). Only high-priority workflows visible at once.
- **Sidebar (iPad/Mac):** Full menu list; includes all enum cases.

Confirm that `.reports` appears in both modes (visible in compact tab bar, and listed in sidebar). If there are platform-specific visibility rules (e.g., Reports only in sidebar, not in iPhone tab bar), clarify this during implementation and adjust accordingly.

**Note on hardcoded values (known issue):** Both `compactTabView` and `sidebarAdaptableTabView` use raw integer tab values (0, 1, 3, 10, etc.) rather than an `AppSection`-driven enum. This is a maintenance risk and should be refactored in a future improvement to use `AppSection` cases as tag values directly (e.g., `value: AppSection.reports`). For now, verify that value 4 is not used elsewhere and document the correct values above.

### `ios-app/Sources/TenexMVP/Navigation/MainShellView.swift`

- **Action:** Modify
- **What:** 
  - In `sectionContentHost` method (lines 268-358), add a new `case` branch after `.projects:` and before `.inbox:`:
    ```swift
    case .reports:
      ReportsTabView()
        .environmentObject(coreManager)
        .accessibilityIdentifier(AppSection.reports.accessibilityContentID)
    ```
  - This renders the Reports tab content when `selectedSection == .reports`
- **Why:** Routes the selected navigation state to the appropriate view and includes accessibility identifier for testing

### `ios-app/Sources/TenexMVP/CoreManager/TenexCoreManager.swift`

- **Action:** Modify
- **What:**
  - Add `@Published var reports: [Report] = []` property to the class around line 81 (near existing `projects` property)
  - Add `private(set) var reportsVersion: UInt64 = 0` property immediately after the `reports` property to track version changes
  - Add method `func applyReportUpsert(report: Report)` that updates the `reports` array and bumps the version:
    ```swift
    func applyReportUpsert(report: Report) {
        if let index = reports.firstIndex(where: { $0.id == report.id }) {
            reports[index] = report
        } else {
            reports.append(report)
        }
        reportsVersion &+= 1  // Increment with overflow semantics matching other version counters
    }
    ```
  - In the existing `fetchData()` method (find the method that calls `getProjects()`, typically line ~150-200), add a call after `getProjects()`:
    ```swift
    do {
        let fetchedReports = try safeCore.getReports(projectId: currentProjectId)
        reports = fetchedReports
        reportsVersion &+= 1
    } catch {
        os_log("Failed to fetch reports in fetchData: %{public}@", 
               log: .tenexCore, type: .error, error.localizedDescription)
        // Do not crash; reports remain empty until next refresh
    }
    ```
    **Decision:** Use eager loading in `fetchData()` rather than lazy loading in `ReportsTabView.task()`. This ensures reports are available at app startup like other data types (projects, conversations) and simplifies lifecycle management. Include error handling to prevent app startup failure if reports fetch fails.
- **Why:** Maintains reactive state for reports so SwiftUI views automatically update when reports change. The version counter pattern matches `contentCatalogVersion` and other existing counters, triggering SwiftUI re-renders when data changes. Initial load in `fetchData()` ensures consistency with other data types.

### `ios-app/Sources/TenexMVP/TenexCore/TenexEventHandler.swift`

- **Action:** Modify
- **What:**
  - At line 94-95, verify the `reportUpsert` case signature in `DataChangeType` enum. The handler must match the FFI definition — confirm whether `reportUpsert` carries an associated `Report` value or is signal-only.
  - If `reportUpsert` has an associated `Report` payload, replace the no-op `case .reportUpsert: break` with:
    ```swift
    case .reportUpsert(let report):
        coreManager.applyReportUpsert(report: report)
    ```
  - If `reportUpsert` is signal-only (no payload), update the handler to trigger a full refresh instead:
    ```swift
    case .reportUpsert:
        // Signal to refetch reports; implementation depends on whether SafeTenexCore provides 
        // a dedicated method or requires calling fetchData() on the main app context
        Task {
            let reports = try? await SafeTenexCore.shared.getReports(projectId: coreManager.appFilterProjectIds.first ?? "")
            for report in reports ?? [] {
                coreManager.applyReportUpsert(report: report)
            }
        }
    ```
- **Why:** Wires the Nostr event handler to the reactive data layer so incoming reports trigger UI updates and version bumps. The FFI signature must be verified first.

### `ios-app/Sources/TenexMVP/TenexCore/SafeTenexCore.swift`

- **Action:** Modify
- **What:**
  - Add wrapper method after existing `getProjects()` or similar (check current structure around line 200):
    ```swift
    func getReports(projectId: String) throws -> [Report] {
        let profiler = Profiler.shared
        let measureID = profiler.startMeasure(operation: "SafeTenexCore.getReports")
        defer { profiler.endMeasure(id: measureID) }
        
        do {
            let reports = try tenex_core_getReports(projectId)
            return reports
        } catch {
            os_log("Failed to fetch reports for project %{public}@: %{public}@", 
                   log: .tenexCore, type: .error, projectId, error.localizedDescription)
            throw TenexCoreError.reportsFetchFailed(projectId: projectId, underlying: error)
        }
    }
    ```
  - Verify FFI signature at `tenex_core.swift:965` — confirm whether the underlying `tenex_core_getReports()` is sync or async, and whether it throws
  - If the FFI call is already async (`async throws`), adjust the wrapper accordingly to maintain the pattern
  - This wraps the FFI call and implements error handling consistent with other FFI wrappers like `getProjects()`
- **Why:** Provides type-safe Swift interface with profiling (performance tracking), proper error propagation, and logging. Matches the pattern of other FFI wrappers in SafeTenexCore for consistency.

### `ios-app/Sources/TenexMVP/Views/MessageComposerView.swift`

- **Action:** Modify
- **What:**
  - At line 41 where `referenceConversationId: String?` parameter is defined, add:
    ```swift
    var referenceReportATag: String? = nil  // NIP-23 a-tag format: "30023:authorPubkey:dTag"
    ```
  - In the `sendMessage()` method (find where it calls `SafeTenexCore.sendThread()`), update the call to pass the parameter:
    ```swift
    SafeTenexCore.sendThread(
        content: ...,
        referenceConversationId: referenceConversationId,
        referenceReportATag: referenceReportATag
    )
    ```
    instead of the current hardcoded `nil`
  - **Critical:** The `referenceReportATag` must be in full NIP-23 a-tag format: `"\(30023):\(report.author):\(report.slug)"`. This is required by `getDocumentThreads(reportATag:)` in the backend and for NIP-27 reference tags in Nostr events.
- **Why:** Allows ReportsTabView to pass a report's a-tag when opening a chat composer, creating a linked NIP-27 reference to the report. The full a-tag format ensures the backend can correctly query threads related to that report.

### `ios-app/Sources/TenexMVP/Views/ReportsTabView.swift`

- **Action:** Create
- **What:** New file implementing the Reports tab interface
  - Import SwiftUI and TenexCore; add `import os.log` for accessibility logging
  - `@EnvironmentObject var coreManager: TenexCoreManager`
  - `@State private var isLoading = false` to track initial data fetch
  - `@State private var selectedReportId: String?` to track selected report using MainShellView binding pattern
  - Add explicit `@MainActor` annotation to the view struct
  - **Initial data loading:** Add `.task { ... }` block that runs on first appear:
    ```swift
    .task {
        isLoading = true
        defer { isLoading = false }
        do {
            let reports = try await SafeTenexCore.shared.getReports(
                projectId: coreManager.appFilterProjectIds.first ?? ""
            )
            for report in reports {
                coreManager.applyReportUpsert(report: report)
            }
        } catch {
            os_log("Failed to load reports: %{public}@", 
                   log: .tenexUI, type: .error, error.localizedDescription)
        }
    }
    ```
    This ensures reports are loaded when the tab is first opened, in addition to the eager load in `fetchData()`.
  - **Implement grouping logic:**
    - Compute grouped reports by iterating `coreManager.reports` and building a `Dictionary<String, [Report]>` keyed by the "plan" t-tag value
    - Extract "plan" tag from each report's `hashtags` array (iterate and find the tag with key "plan")
    - Group reports with missing plan tag under an "Ungrouped" section (not "Other")
    - **Grouping edge cases:** If a report has multiple "plan" tags (malformed), use the first one. If none match, place in "Ungrouped". Sort sections alphabetically with "Ungrouped" always last.
    - **Important:** The "plan" t-tag is a TENEX-specific convention. NIP-23 defines generic t-tags; this app uses "plan" as a grouping key by convention.
  - **Empty state:** When `coreManager.reports.isEmpty`, show `ContentUnavailableView`:
    ```swift
    ContentUnavailableView(
        label: { Label("No Reports", systemImage: "doc.richtext") },
        description: { Text("Check back later or create one in the editor.") }
    )
    ```
  - **Pull-to-refresh:** Add `.refreshable` modifier to the List:
    ```swift
    .refreshable {
        do {
            let reports = try await SafeTenexCore.shared.getReports(
                projectId: coreManager.appFilterProjectIds.first ?? ""
            )
            for report in reports {
                coreManager.applyReportUpsert(report: report)
            }
        } catch {
            os_log("Pull-to-refresh failed: %{public}@", 
                   log: .tenexUI, type: .error, error.localizedDescription)
        }
    }
    ```
  - **Navigation pattern:** Use `MainShellView` state binding pattern (don't use local NavigationStack). Implement `selectedReportId` binding that syncs with parent state. When a report is tapped, navigate to detail view using the binding.
  - **Detail view (when report selected):**
    - Report title (accessibility: `accessibilityLabel("Report title: \(report.title)")`), author, createdAt formatted as date, readingTimeMins
    - MarkdownView(content: report.content) for body with proper accessibility labels
    - "Open Discussion" button to open MessageComposerView with `referenceReportATag` set to full a-tag format: `"\(30023):\(report.author):\(report.slug)"`
      - **Critical format:** The parameter must be the complete a-tag `30023:author:slug`. This is required by `getDocumentThreads(reportATag:)` and NIP-27 reference tags.
      - Add comment: `// NIP-23 a-tag format: kind:authorPubkey:dTag`
    - Add dismiss button (e.g., `.toolbar { ToolbarItem(placement: .cancellationAction) { Button("Done") { selectedReportId = nil } } }`)
  - **Accessibility:** 
    - Add `.accessibilityIdentifier(AppSection.reports.accessibilityContentID)` to the top-level ReportsTabView
    - Label report rows with `accessibilityLabel("Report: \(report.title) by \(report.author)")`
    - Add `.accessibilityElement(children: .combine)` to group list row content
    - Support Dynamic Type: use relative font sizes (e.g., `.font(.body)` not fixed sizes)
    - Provide VoiceOver labels for interactive buttons ("Open Discussion", group headers)
  - Use `coreManager.reportsVersion` to ensure SwiftUI re-renders when data changes (observe via `.onChange(of: coreManager.reportsVersion)` if needed)
  - **User scenarios handled:**
    - Report received while tab is active (reactive update via event handler + version bump)
    - Very long report list (100+ reports) — performance: avoid loading all content into memory; use lazy evaluation and list row optimization
    - Report deleted via event — NOTE: only `reportUpsert` is wired; if reports can be deleted, add `reportDelete` event handler in TenexEventHandler
    - Deep linking to specific report — TODO: requires MainShellView to accept a report ID on app launch; out of scope for this plan
    - Report loading during network offline — graceful error handling via try/catch with user-facing logging
- **Why:** Provides the UI for browsing and interacting with grouped reports, with explicit initial data loading, proper empty-state and pull-to-refresh handling, correct a-tag format for chat linking, full accessibility support, and navigation pattern consistency with MainShellView. Handles reactive updates, performance at scale, and error scenarios.

## Execution Order

1. **Add Reports to AppSection enum and properties** — `MainTabView.swift` lines 3-60
   - Verify: `AppSection` compiles; enum contains `.reports` case with title "Reports" and systemImage "doc.richtext"

2. **Update compact tab view with value 4** — `MainTabView.swift` lines 168-200
   - Verify: Simulator shows Reports tab in tab bar between Projects and Inbox; tapping it changes selection; tab value is 4 (check selectedTab state)

3. **Update sidebar tab view with value 4** — `MainTabView.swift` lines 204-250
   - Verify: iPad simulator shows Reports in sidebar navigation with value 4; selection routing works

4. **Add sectionContentHost case** — `MainShellView.swift` lines 268-358
   - Verify: App compiles; Reports tab appears but shows empty view (ReportsTabView doesn't exist yet, so expect build error that will be resolved in step 9)

5. **Add reports property, reportsVersion, and applyReportUpsert** — `TenexCoreManager.swift` line 81 area
   - Verify: Property is @Published and accessible; `reportsVersion` is defined; method compiles; version increments on upsert

6. **Integrate getReports into fetchData** — `TenexCoreManager.swift` in the `fetchData()` method
   - Verify: `fetchData()` calls `safeCore.getReports()` and populates `coreManager.reports`; no compiler errors

7. **Verify FFI signature and wire reportUpsert event handler** — `TenexEventHandler.swift` line 94-95
   - **BLOCKING:** Confirm whether `DataChangeType.reportUpsert` carries an associated `Report` value or is signal-only
   - Based on the signature, implement the appropriate handler (see File Changes section for both cases)
   - Verify: Event handler calls `applyReportUpsert` or triggers refresh; `reportsVersion` increments; build succeeds

8. **Add SafeTenexCore wrapper with error handling and profiling** — `SafeTenexCore.swift`
   - **BLOCKING:** Verify FFI signature at `tenex_core.swift:965` — confirm whether `tenex_core_getReports()` is sync or async, and whether it throws
   - Add wrapper with `throws` annotation, `profiler.measureFFI()`, error logging, and custom error type
   - Verify: Wrapper method exists; matches FFI signature; includes profiling; handles errors consistently with other FFI wrappers (e.g., `getProjects()`)

9. **Add referenceReportATag parameter** — `MessageComposerView.swift` line 41 and sendThread call
   - Verify: Parameter added with type `String?` and comment documenting NIP-23 a-tag format; passed through sendThread call; no compiler errors

10. **Create ReportsTabView.swift** — New file `ios-app/Sources/TenexMVP/Views/ReportsTabView.swift`
    - Add `.task { ... }` to load reports on first appear (in addition to fetchData eager load)
    - Implement grouping by "plan" t-tag with "Ungrouped" section for reports without plan tag
    - Implement `ContentUnavailableView` for empty state
    - Add `.refreshable` modifier for pull-to-refresh
    - Use MainShellView binding pattern for `selectedReportId` navigation (not local NavigationStack)
    - Add full accessibility support: identifiers, labels, Dynamic Type, VoiceOver
    - Construct a-tag format correctly: `"\(30023):\(report.author):\(report.slug)"`
    - Verify: File compiles; tab displays grouped list with proper sorting; tapping report opens detail with markdown; empty state appears when empty; pull-to-refresh works; "Open Discussion" button passes correct a-tag format; accessibility identifiers set; no accessibility warnings

11. **Run full build and test**
    - Build: `cargo build --target aarch64-apple-ios-sim --release -p tenex-core && cd ios-app && tuist generate && xcodebuild -scheme TenexMVP -destination 'platform=iOS Simulator,name=iPhone 15'`
    - Verify: No compilation errors; Reports tab appears with value 4 in both compact and sidebar modes; reports display if test data exists; markdown renders without errors; chat reference passes full a-tag format; pull-to-refresh populates reports; empty state displays correctly

## Verification

### FFI and Data Layer
- **Build verification:** All targets compile without errors; no warnings related to unused parameters or type mismatches
- **FFI signature verification (BLOCKING):**
  - Confirm `DataChangeType.reportUpsert` signature — does it carry a `Report` payload or is it signal-only?
  - Confirm `tenex_core_getReports()` at line 965 — is it sync or async? Does it throw?
  - Verify that TenexEventHandler implementation matches the actual FFI signature
  - Verify that SafeTenexCore.getReports() signature matches the underlying FFI call (async throws vs sync)
- **Data flow verification:**
  - Inject test report via Nostr event → verify `TenexEventHandler` calls `applyReportUpsert` or triggers refresh
  - Verify `coreManager.reports` updates reactively; `reportsVersion` increments after upsert
  - Verify `fetchData()` populates `reports` on app startup
  - Verify grouped list renders correctly with multiple groups (test with varied plan tags)
  - Verify empty state displays when `reports.isEmpty`
  - Report received while ReportsTabView is active → verify reactive update without manual refresh
  - Test report deletion event (if supported) — verify either add delete handler or document as limitation

### Navigation and Tab Routing
- **Tab value verification:** 
  - Inspect `selectedTab` state in both compactTabView and sidebarAdaptableTabView; Reports uses value 4 in both
  - No collision with other tabs (verify 4 is not used elsewhere)
  - Tab bar updates correctly when Reports is selected
  - Verify tab order in compact view: Chats, Reports, Projects, Inbox (with values 0, 4, 1, 3 respectively)
- **Navigation verification:** 
  - iPhone compact mode: Reports tab visible in tab bar between Projects and Inbox; tapping switches view
  - iPad/Mac sidebar: Reports option visible in sidebar list; selecting it renders ReportsTabView
  - ReportsTabView uses MainShellView binding pattern (selectedReportId) — not local NavigationStack
  - Tapping a report opens detail view with correct report data; back/dismiss navigates back to list
  - Deep linking: out of scope for this plan; document as future work

### UI and Accessibility
- **UI verification:**
  - Report detail renders markdown without parsing errors (test with code blocks, links, images, tables)
  - "Open Discussion" button is clickable and opens MessageComposerView with correct a-tag
  - MessageComposerView receives `referenceReportATag` parameter set to full a-tag format `30023:author:slug`
  - Verify a-tag format by logging or breakpoint: must be exactly `"30023:<author>:<slug>"`, not just slug
  - Pull-to-refresh works: swipe down updates report list
  - Empty state displays with `ContentUnavailableView` icon and text
- **Accessibility verification:**
  - VoiceOver labels present: Reports title, "Open Discussion" button, group headers
  - Accessibility identifiers set on ReportsTabView and interactive elements
  - Dynamic Type support: UI scales correctly with system font size (test at accessibility sizes)
  - Keyboard navigation works: Tab through list rows and buttons
  - Run Accessibility Inspector or XCTest accessibility audits

### Grouping Logic and Edge Cases
- **Grouping verification:**
  - Reports with "plan" tag appear in named sections (e.g., "plan-A", "plan-B")
  - Reports with no "plan" tag appear in "Ungrouped" section
  - Multiple "plan" tags (malformed) — first tag used; not duplicated across sections
  - Sections sorted alphabetically; "Ungrouped" always last
  - Verify with test data containing 5–10 reports with varied plan tags
- **Edge cases:**
  - Empty reports list shows helpful message; no stale UI from previous state
  - Long report titles (100+ chars) truncate gracefully in list view; display fully in detail
  - Markdown with code blocks, nested lists, and links renders without breaking layout
  - Very long markdown content (10,000+ words) — performance: list scrolling remains smooth; use lazy content rendering if needed
  - Chat composer correctly formats a-tag parameter — verify with console logging or breakpoint
  - Report author pubkey with non-ASCII characters (if supported) — verify a-tag formatting doesn't break

### User Scenarios and Network Conditions
- **Reactive updates:**
  - Report received via Nostr while ReportsTabView is active → verify list updates without manual refresh
  - Report upserted (title/content changed) → list item updates with new data
  - Multiple reports received in rapid succession → batch updates efficiently
- **Performance at scale:**
  - Test with 100+ reports in list → scrolling remains responsive (no frame drops)
  - Test with 20+ groups (varied plan tags) → section headers render smoothly
  - Markdown rendering: 5,000-word report loads and scrolls smoothly in detail view
- **Network and offline:**
  - Reports load on app startup via `fetchData()` — test with network initially unavailable
  - Pull-to-refresh while offline — graceful error message (not crash)
  - App recovers when network returns — reports can be refreshed
- **Search and filtering (noted as out of scope):**
  - Plan mentions search/filter pattern exists in ProjectsTabView
  - For future work: add `searchText` and `filteredReports` computed property if needed
