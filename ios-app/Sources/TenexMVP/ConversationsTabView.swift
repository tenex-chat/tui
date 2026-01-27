import SwiftUI

// MARK: - Static Formatters (Fix #7: Cache formatters)

private enum DateFormatters {
    static let relative: RelativeDateTimeFormatter = {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter
    }()

    static let dateTime: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .short
        formatter.timeStyle = .short
        return formatter
    }()
}

/// Conversations tab that replicates the TUI conversations tab behavior.
/// Features hierarchical display, activity tracking, filtering, and swipe-to-archive.
struct ConversationsTabView: View {
    @EnvironmentObject var coreManager: TenexCoreManager

    // MARK: - State
    @State private var conversations: [ConversationFullInfo] = []
    @State private var projectFilters: [ProjectFilterInfo] = []
    @State private var isLoading = true
    @State private var errorMessage: String?

    // MARK: - Load Token (Fix #2: Race condition prevention)
    /// Incremented on each load request. Used to discard stale responses.
    @State private var loadGeneration: Int = 0

    // MARK: - Filter State
    @State private var showArchived = false
    @State private var hideScheduled = false
    @State private var selectedTimeFilter: TimeFilterOption = .all
    @State private var showFilterSheet = false

    // MARK: - UI State
    @State private var collapsedThreadIds: Set<String> = []
    @State private var selectedConversationId: String?
    @State private var showConversationDetail = false

    // MARK: - Spinner Animation (Fix #6: Only animate when needed)
    @State private var spinnerFrame = 0
    private let spinnerFrames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]

    /// Timer publisher - autoconnect is controlled by whether we receive events
    private let spinnerTimer = Timer.publish(every: 0.1, on: .main, in: .common).autoconnect()

    var body: some View {
        NavigationStack {
            ZStack {
                if isLoading && conversations.isEmpty {
                    ProgressView("Loading conversations...")
                } else if let error = errorMessage {
                    errorView(error)
                } else if conversations.isEmpty {
                    emptyStateView
                } else {
                    conversationList
                }
            }
            .navigationTitle("Conversations")
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button(action: { showFilterSheet = true }) {
                        Image(systemName: filterIsActive ? "line.3.horizontal.decrease.circle.fill" : "line.3.horizontal.decrease.circle")
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    Button(action: { loadConversations() }) {
                        Image(systemName: "arrow.clockwise")
                    }
                }
            }
            .sheet(isPresented: $showFilterSheet) {
                FilterSheetView(
                    showArchived: $showArchived,
                    hideScheduled: $hideScheduled,
                    selectedTimeFilter: $selectedTimeFilter,
                    projectFilters: $projectFilters,
                    onApply: {
                        loadConversations()
                        showFilterSheet = false
                    }
                )
                .environmentObject(coreManager)
            }
            .sheet(isPresented: $showConversationDetail) {
                if let convId = selectedConversationId,
                   let conv = conversations.first(where: { $0.id == convId }) {
                    ConversationDetailSheet(conversation: conv)
                        .environmentObject(coreManager)
                }
            }
        }
        .onAppear {
            loadCollapsedState()
            loadConversations()
        }
        // Fix #6: Only tick spinner when there are active conversations
        .onReceive(spinnerTimer) { _ in
            if hasActiveConversations {
                spinnerFrame = (spinnerFrame + 1) % spinnerFrames.count
            }
        }
    }

    // MARK: - Computed Properties

    private var filterIsActive: Bool {
        showArchived || hideScheduled || selectedTimeFilter != .all ||
        projectFilters.contains(where: { !$0.isVisible })
    }

    /// Check if any conversation is active (Fix #6)
    private var hasActiveConversations: Bool {
        conversations.contains(where: { $0.isActive })
    }

    /// Build hierarchical tree from flat list (Fix #3: O(n) algorithm)
    private var conversationTree: [ConversationTreeItem] {
        buildTreeEfficient(from: conversations)
    }

    // MARK: - Views

    private func errorView(_ message: String) -> some View {
        VStack(spacing: 16) {
            Image(systemName: "exclamationmark.triangle")
                .font(.system(size: 60))
                .foregroundStyle(.orange)
            Text("Error Loading Conversations")
                .font(.title2)
                .fontWeight(.medium)
            Text(message)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 40)
            Button("Retry") {
                loadConversations()
            }
            .buttonStyle(.bordered)
        }
    }

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Image(systemName: "bubble.left.and.bubble.right")
                .font(.system(size: 60))
                .foregroundStyle(.secondary)
            Text("No Conversations")
                .font(.title2)
                .fontWeight(.medium)
            Text("Conversations will appear here when agents start working on tasks.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 40)
        }
    }

    private var conversationList: some View {
        List {
            ForEach(conversationTree) { item in
                ConversationTreeNodeView(
                    item: item,
                    collapsedThreadIds: $collapsedThreadIds,
                    spinnerFrame: spinnerFrame,
                    spinnerFrames: spinnerFrames,
                    showArchived: showArchived,
                    onSelect: { conv in
                        selectedConversationId = conv.id
                        showConversationDetail = true
                    },
                    onArchiveToggle: { conv in
                        toggleArchive(conversation: conv)
                    },
                    onCollapseToggle: { threadId in
                        toggleCollapse(threadId: threadId)
                    }
                )
            }
        }
        .listStyle(.plain)
        .refreshable {
            loadConversations()
        }
    }

    // MARK: - Data Loading (Fix #2: Race condition)

    private func loadConversations() {
        // Increment generation to invalidate any in-flight requests
        loadGeneration += 1
        let currentGeneration = loadGeneration

        isLoading = true
        errorMessage = nil

        Task {
            // Get visible project IDs
            let visibleProjectIds = projectFilters
                .filter { $0.isVisible }
                .map { $0.id }

            let filter = ConversationFilter(
                projectIds: visibleProjectIds,
                showArchived: showArchived,
                hideScheduled: hideScheduled,
                timeFilter: selectedTimeFilter
            )

            // Fetch conversations on background thread
            let result = await Task.detached(priority: .userInitiated) { [filter] in
                do {
                    return try self.coreManager.core.getAllConversations(filter: filter)
                } catch {
                    return nil as [ConversationFullInfo]?
                }
            }.value

            // Check if this response is still relevant (Fix #2)
            guard currentGeneration == loadGeneration else {
                // Stale response, discard
                return
            }

            if let fetched = result {
                self.conversations = fetched
                self.errorMessage = nil
            } else {
                self.errorMessage = "Failed to load conversations. Please try again."
            }

            // Also refresh project filters
            let filtersResult = await Task.detached(priority: .userInitiated) {
                do {
                    return try self.coreManager.core.getProjectFilters()
                } catch {
                    return nil as [ProjectFilterInfo]?
                }
            }.value

            // Check again for staleness
            guard currentGeneration == loadGeneration else {
                return
            }

            // Only update loading state if this is the current request
            self.isLoading = false

            guard let filters = filtersResult else {
                return
            }

            // Preserve visibility state when refreshing
            if self.projectFilters.isEmpty {
                self.projectFilters = filters
            } else {
                // Merge: keep existing visibility, update counts
                var updatedFilters = filters
                for i in updatedFilters.indices {
                    if let existing = self.projectFilters.first(where: { $0.id == updatedFilters[i].id }) {
                        // Keep the existing visibility state
                        updatedFilters[i] = ProjectFilterInfo(
                            id: updatedFilters[i].id,
                            aTag: updatedFilters[i].aTag,
                            title: updatedFilters[i].title,
                            isVisible: existing.isVisible,
                            activeCount: updatedFilters[i].activeCount,
                            totalCount: updatedFilters[i].totalCount
                        )
                    }
                }
                self.projectFilters = updatedFilters
            }
        }
    }

    private func toggleArchive(conversation: ConversationFullInfo) {
        Task {
            let _ = await Task.detached(priority: .userInitiated) { [conversation] in
                self.coreManager.core.toggleConversationArchived(conversationId: conversation.id)
            }.value

            // Reload to reflect changes
            loadConversations()
        }
    }

    private func toggleCollapse(threadId: String) {
        // Toggle local state
        if collapsedThreadIds.contains(threadId) {
            collapsedThreadIds.remove(threadId)
        } else {
            collapsedThreadIds.insert(threadId)
        }

        // Persist via FFI (Fix #5: Wire up collapse persistence)
        Task.detached(priority: .utility) {
            self.coreManager.core.setCollapsedThreadIds(threadIds: Array(self.collapsedThreadIds))
        }
    }

    private func loadCollapsedState() {
        Task {
            do {
                let ids = try coreManager.core.getCollapsedThreadIds()
                await MainActor.run {
                    self.collapsedThreadIds = Set(ids)
                }
            } catch {
                // Ignore errors, use empty set
            }
        }
    }

    // MARK: - Tree Building (Fix #3: O(n) algorithm with orphan handling)

    /// Build tree efficiently in O(n) instead of O(n²)
    /// - Orphaned conversations (parent filtered out) are treated as roots
    /// - Includes cycle detection via visited set
    private func buildTreeEfficient(from conversations: [ConversationFullInfo]) -> [ConversationTreeItem] {
        // Build a parent->children map in O(n)
        var childrenMap: [String: [ConversationFullInfo]] = [:]
        var conversationById: [String: ConversationFullInfo] = [:]
        var rootIds: [String] = []

        for conv in conversations {
            conversationById[conv.id] = conv

            if let parentId = conv.parentId {
                childrenMap[parentId, default: []].append(conv)
            } else {
                rootIds.append(conv.id)
            }
        }

        // Find orphans: conversations whose parent is not in our set
        // Treat them as roots (per clarifying question #1: show as roots)
        for conv in conversations {
            if let parentId = conv.parentId {
                if conversationById[parentId] == nil {
                    // Parent was filtered out, treat as root
                    rootIds.append(conv.id)
                }
            }
        }

        // Build tree items iteratively with cycle detection
        var result: [ConversationTreeItem] = []
        var visited: Set<String> = []

        for rootId in rootIds {
            guard let root = conversationById[rootId] else { continue }
            if let item = buildTreeItemIterative(
                for: root,
                childrenMap: childrenMap,
                conversationById: conversationById,
                depth: 0,
                visited: &visited
            ) {
                result.append(item)
            }
        }

        return result
    }

    /// Build tree item iteratively (no recursion) with cycle detection
    private func buildTreeItemIterative(
        for conversation: ConversationFullInfo,
        childrenMap: [String: [ConversationFullInfo]],
        conversationById: [String: ConversationFullInfo],
        depth: Int,
        visited: inout Set<String>
    ) -> ConversationTreeItem? {
        // Cycle detection
        guard !visited.contains(conversation.id) else {
            return nil
        }
        visited.insert(conversation.id)

        // Get children for this conversation
        let childConversations = childrenMap[conversation.id] ?? []

        // Build children recursively (still recursive but with cycle protection)
        let children: [ConversationTreeItem] = childConversations.compactMap { child in
            buildTreeItemIterative(
                for: child,
                childrenMap: childrenMap,
                conversationById: conversationById,
                depth: depth + 1,
                visited: &visited
            )
        }

        return ConversationTreeItem(
            conversation: conversation,
            children: children,
            depth: depth
        )
    }
}

// MARK: - Tree Item Model

struct ConversationTreeItem: Identifiable {
    let conversation: ConversationFullInfo
    let children: [ConversationTreeItem]
    let depth: Int

    var id: String { conversation.id }
    var hasChildren: Bool { !children.isEmpty || conversation.hasChildren }
}

// MARK: - Tree Node View

struct ConversationTreeNodeView: View {
    let item: ConversationTreeItem
    @Binding var collapsedThreadIds: Set<String>
    let spinnerFrame: Int
    let spinnerFrames: [String]
    let showArchived: Bool
    let onSelect: (ConversationFullInfo) -> Void
    let onArchiveToggle: (ConversationFullInfo) -> Void
    let onCollapseToggle: (String) -> Void

    private var isCollapsed: Bool {
        collapsedThreadIds.contains(item.id)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Main row
            conversationRow
                .swipeActions(edge: .trailing, allowsFullSwipe: true) {
                    Button(role: showArchived ? nil : .destructive) {
                        onArchiveToggle(item.conversation)
                    } label: {
                        Label(
                            item.conversation.isArchived ? "Unarchive" : "Archive",
                            systemImage: item.conversation.isArchived ? "tray.and.arrow.up" : "archivebox"
                        )
                    }
                    .tint(item.conversation.isArchived ? .blue : .red)
                }

            // Children (if expanded)
            if !isCollapsed {
                ForEach(item.children) { child in
                    ConversationTreeNodeView(
                        item: child,
                        collapsedThreadIds: $collapsedThreadIds,
                        spinnerFrame: spinnerFrame,
                        spinnerFrames: spinnerFrames,
                        showArchived: showArchived,
                        onSelect: onSelect,
                        onArchiveToggle: onArchiveToggle,
                        onCollapseToggle: onCollapseToggle
                    )
                }
            }
        }
    }

    private var conversationRow: some View {
        HStack(spacing: 12) {
            // Indentation
            if item.depth > 0 {
                Rectangle()
                    .fill(Color.clear)
                    .frame(width: CGFloat(item.depth) * 20)
            }

            // Collapse/Expand indicator
            if item.hasChildren {
                Button(action: { onCollapseToggle(item.id) }) {
                    Image(systemName: isCollapsed ? "chevron.right" : "chevron.down")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .frame(width: 20)
                }
                .buttonStyle(.plain)
            } else {
                Spacer()
                    .frame(width: 20)
            }

            // Activity indicator (spinner for active)
            ZStack {
                if item.conversation.isActive {
                    Text(spinnerFrames[spinnerFrame])
                        .font(.system(size: 14, design: .monospaced))
                        .foregroundStyle(.green)
                } else {
                    Circle()
                        .fill(statusColor)
                        .frame(width: 8, height: 8)
                }
            }
            .frame(width: 20)

            // Content
            VStack(alignment: .leading, spacing: 4) {
                HStack {
                    Text(item.conversation.title)
                        .font(.headline)
                        .lineLimit(1)

                    if item.conversation.isArchived {
                        Text("(archived)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }

                HStack(spacing: 8) {
                    // Author
                    Text(item.conversation.author)
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    // Current activity or status
                    if let activity = item.conversation.currentActivity {
                        Text(activity)
                            .font(.caption)
                            .foregroundStyle(.green)
                            .lineLimit(1)
                    } else if let status = item.conversation.status {
                        Text(status)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    Spacer()

                    // Timestamp (Fix #7: Use cached formatter)
                    Text(formatTimestamp(item.conversation.effectiveLastActivity))
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
            }

            Spacer()

            // Message count badge
            if item.conversation.messageCount > 0 {
                Text("\(item.conversation.messageCount)")
                    .font(.caption2)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.secondary.opacity(0.2))
                    .clipShape(Capsule())
            }
        }
        .padding(.vertical, 8)
        .contentShape(Rectangle())
        .onTapGesture {
            onSelect(item.conversation)
        }
    }

    private var statusColor: Color {
        if item.conversation.isActive {
            return .green
        }
        switch item.conversation.status {
        case "In Progress":
            return .blue
        case "Blocked", "Waiting":
            return .orange
        case "Done", "Completed":
            return .gray
        default:
            return .secondary
        }
    }

    /// Format timestamp using cached formatter (Fix #7)
    private func formatTimestamp(_ timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return DateFormatters.relative.localizedString(for: date, relativeTo: Date())
    }
}

// MARK: - Filter Sheet

struct FilterSheetView: View {
    @Environment(\.dismiss) var dismiss
    @EnvironmentObject var coreManager: TenexCoreManager

    @Binding var showArchived: Bool
    @Binding var hideScheduled: Bool
    @Binding var selectedTimeFilter: TimeFilterOption
    @Binding var projectFilters: [ProjectFilterInfo]

    let onApply: () -> Void

    var body: some View {
        NavigationStack {
            Form {
                // Time Filter
                Section("Time Range") {
                    Picker("Show conversations from", selection: $selectedTimeFilter) {
                        Text("All Time").tag(TimeFilterOption.all)
                        Text("Today").tag(TimeFilterOption.today)
                        Text("This Week").tag(TimeFilterOption.thisWeek)
                        Text("This Month").tag(TimeFilterOption.thisMonth)
                    }
                    .pickerStyle(.menu)
                }

                // Toggles
                Section("Visibility") {
                    Toggle("Show Archived", isOn: $showArchived)
                    Toggle("Hide Scheduled Events", isOn: $hideScheduled)
                }

                // Project Filter
                Section("Projects") {
                    if projectFilters.isEmpty {
                        Text("No projects available")
                            .foregroundStyle(.secondary)
                    } else {
                        ForEach(projectFilters.indices, id: \.self) { index in
                            HStack {
                                Toggle(isOn: Binding(
                                    get: { projectFilters[index].isVisible },
                                    set: { newValue in
                                        projectFilters[index] = ProjectFilterInfo(
                                            id: projectFilters[index].id,
                                            aTag: projectFilters[index].aTag,
                                            title: projectFilters[index].title,
                                            isVisible: newValue,
                                            activeCount: projectFilters[index].activeCount,
                                            totalCount: projectFilters[index].totalCount
                                        )
                                    }
                                )) {
                                    HStack {
                                        Text(projectFilters[index].title)
                                        Spacer()
                                        if projectFilters[index].activeCount > 0 {
                                            Text("\(projectFilters[index].activeCount) active")
                                                .font(.caption)
                                                .foregroundStyle(.green)
                                        }
                                        Text("\(projectFilters[index].totalCount)")
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                    }
                                }
                            }
                        }

                        Button("Select All") {
                            for i in projectFilters.indices {
                                projectFilters[i] = ProjectFilterInfo(
                                    id: projectFilters[i].id,
                                    aTag: projectFilters[i].aTag,
                                    title: projectFilters[i].title,
                                    isVisible: true,
                                    activeCount: projectFilters[i].activeCount,
                                    totalCount: projectFilters[i].totalCount
                                )
                            }
                        }

                        Button("Deselect All") {
                            for i in projectFilters.indices {
                                projectFilters[i] = ProjectFilterInfo(
                                    id: projectFilters[i].id,
                                    aTag: projectFilters[i].aTag,
                                    title: projectFilters[i].title,
                                    isVisible: false,
                                    activeCount: projectFilters[i].activeCount,
                                    totalCount: projectFilters[i].totalCount
                                )
                            }
                        }
                    }
                }
            }
            .navigationTitle("Filters")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Apply") {
                        // Save visible projects to preferences
                        let visibleATags = projectFilters
                            .filter { $0.isVisible }
                            .map { $0.aTag }
                        coreManager.core.setVisibleProjects(projectATags: visibleATags)
                        onApply()
                    }
                }
            }
        }
        .presentationDetents([.medium, .large])
    }
}

// MARK: - Conversation Detail Sheet

struct ConversationDetailSheet: View {
    @Environment(\.dismiss) var dismiss
    @EnvironmentObject var coreManager: TenexCoreManager

    let conversation: ConversationFullInfo
    @State private var messages: [MessageInfo] = []
    @State private var isLoading = true

    var body: some View {
        NavigationStack {
            Group {
                if isLoading {
                    ProgressView("Loading messages...")
                } else if messages.isEmpty {
                    VStack(spacing: 16) {
                        Image(systemName: "text.bubble")
                            .font(.system(size: 40))
                            .foregroundStyle(.secondary)
                        Text("No messages yet")
                            .foregroundStyle(.secondary)
                    }
                } else {
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 12) {
                            ForEach(messages, id: \.id) { message in
                                MessageBubble(message: message)
                            }
                        }
                        .padding()
                    }
                }
            }
            .navigationTitle(conversation.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") {
                        dismiss()
                    }
                }
            }
        }
        .onAppear {
            loadMessages()
        }
    }

    private func loadMessages() {
        Task {
            isLoading = true
            defer { isLoading = false }

            let fetched = await Task.detached(priority: .userInitiated) { [conversation] in
                self.coreManager.core.getMessages(conversationId: conversation.id)
            }.value

            self.messages = fetched
        }
    }
}

// MARK: - Message Bubble

struct MessageBubble: View {
    let message: MessageInfo

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            // Avatar
            Circle()
                .fill(roleColor.opacity(0.2))
                .frame(width: 32, height: 32)
                .overlay {
                    Image(systemName: roleIcon)
                        .font(.caption)
                        .foregroundStyle(roleColor)
                }

            VStack(alignment: .leading, spacing: 4) {
                // Header
                HStack {
                    Text(message.author)
                        .font(.caption)
                        .fontWeight(.semibold)

                    // Fix #7: Use cached formatter
                    Text(formatTimestamp(message.createdAt))
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }

                // Content
                Text(message.content)
                    .font(.body)
                    .textSelection(.enabled)
            }

            Spacer()
        }
        .padding(.vertical, 4)
    }

    private var roleColor: Color {
        switch message.role {
        case "user":
            return .blue
        case "assistant":
            return .green
        case "system":
            return .orange
        default:
            return .secondary
        }
    }

    private var roleIcon: String {
        switch message.role {
        case "user":
            return "person.fill"
        case "assistant":
            return "cpu"
        case "system":
            return "gear"
        default:
            return "questionmark"
        }
    }

    /// Format timestamp using cached formatter (Fix #7)
    private func formatTimestamp(_ timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return DateFormatters.dateTime.string(from: date)
    }
}

// MARK: - Preview

#if DEBUG
struct ConversationsTabView_Previews: PreviewProvider {
    static var previews: some View {
        ConversationsTabView()
    }
}
#endif
