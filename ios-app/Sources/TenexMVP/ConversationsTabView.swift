import SwiftUI
import CryptoKit

// MARK: - Conversation Full Hierarchy

/// Precomputed hierarchy data for ConversationFullInfo with activity tracking.
/// Computes parent→children map and hierarchical activity status once per refresh,
/// avoiding O(n²) traversals during sorting and rendering.
final class ConversationFullHierarchy {
    /// Map from conversation ID to its direct children
    let childrenByParentId: [String: [ConversationFullInfo]]

    /// Map from conversation ID to conversation for O(1) lookups
    let conversationById: [String: ConversationFullInfo]

    /// Precomputed hierarchical activity status: true if conversation or any descendant is active
    let hierarchicallyActiveById: [String: Bool]

    /// Root conversations (no parent or parent doesn't exist in the set)
    let rootConversations: [ConversationFullInfo]

    /// Root conversations sorted by: hierarchically active first, then by effective last activity
    let sortedRootConversations: [ConversationFullInfo]

    /// Initialize hierarchy from a flat list of conversations
    /// - Parameter conversations: All conversations to process
    init(conversations: [ConversationFullInfo]) {
        // Step 1: Build O(1) lookup maps
        let byId = Dictionary(uniqueKeysWithValues: conversations.map { ($0.id, $0) })
        self.conversationById = byId

        // Step 2: Build parent→children map (O(n))
        var childrenMap: [String: [ConversationFullInfo]] = [:]
        for conversation in conversations {
            if let parentId = conversation.parentId {
                childrenMap[parentId, default: []].append(conversation)
            }
        }
        self.childrenByParentId = childrenMap

        // Step 3: Identify root conversations (no parent OR orphaned)
        let allIds = Set(conversations.map { $0.id })
        let roots = conversations.filter { conv in
            if let parentId = conv.parentId {
                return !allIds.contains(parentId) // Orphaned: parent doesn't exist
            }
            return true // No parent - true root
        }
        self.rootConversations = roots

        // Step 4: Compute hierarchical activity status using bottom-up BFS
        // We process in reverse topological order (leaves first)
        var activityMap: [String: Bool] = [:]
        Self.computeHierarchicalActivity(
            conversations: conversations,
            childrenMap: childrenMap,
            activityMap: &activityMap
        )
        self.hierarchicallyActiveById = activityMap

        // Step 5: Sort roots by hierarchical activity first, then by effective last activity
        self.sortedRootConversations = roots.sorted { a, b in
            let aActive = activityMap[a.id] ?? a.isActive
            let bActive = activityMap[b.id] ?? b.isActive

            // Active conversations come first
            if aActive && !bActive { return true }
            if !aActive && bActive { return false }

            // Within same activity status, sort by effective last activity (newest first)
            return a.effectiveLastActivity > b.effectiveLastActivity
        }
    }

    /// Compute hierarchical activity for all conversations in O(n) time.
    /// Uses DFS with memoization - each conversation is processed exactly once.
    private static func computeHierarchicalActivity(
        conversations: [ConversationFullInfo],
        childrenMap: [String: [ConversationFullInfo]],
        activityMap: inout [String: Bool]
    ) {
        let conversationsById = Dictionary(uniqueKeysWithValues: conversations.map { ($0.id, $0) })
        var visited = Set<String>()

        // Process all conversations using DFS with memoization
        for conversation in conversations {
            if activityMap[conversation.id] == nil {
                _ = computeActivityRecursive(
                    conversationId: conversation.id,
                    conversations: conversationsById,
                    childrenMap: childrenMap,
                    activityMap: &activityMap,
                    visited: &visited
                )
            }
        }
    }

    /// Recursively compute activity with memoization.
    /// Uses inout visited set to prevent cycles without copying.
    private static func computeActivityRecursive(
        conversationId: String,
        conversations: [String: ConversationFullInfo],
        childrenMap: [String: [ConversationFullInfo]],
        activityMap: inout [String: Bool],
        visited: inout Set<String>
    ) -> Bool {
        // Return cached result if available
        if let cached = activityMap[conversationId] {
            return cached
        }

        // Cycle detection
        if visited.contains(conversationId) {
            return false
        }
        visited.insert(conversationId)

        // Get the conversation
        guard let conversation = conversations[conversationId] else {
            activityMap[conversationId] = false
            visited.remove(conversationId)
            return false
        }

        // Check if directly active
        if conversation.isActive {
            activityMap[conversationId] = true
            visited.remove(conversationId)
            return true
        }

        // Check children recursively
        let children = childrenMap[conversationId] ?? []
        for child in children {
            if computeActivityRecursive(
                conversationId: child.id,
                conversations: conversations,
                childrenMap: childrenMap,
                activityMap: &activityMap,
                visited: &visited
            ) {
                activityMap[conversationId] = true
                visited.remove(conversationId)
                return true
            }
        }

        // Not active
        activityMap[conversationId] = false
        visited.remove(conversationId)
        return false
    }

    /// Check if a conversation is hierarchically active (O(1) lookup)
    func isHierarchicallyActive(_ conversationId: String) -> Bool {
        hierarchicallyActiveById[conversationId] ?? false
    }
}

/// Main tab view for Conversations - shows aggregated conversation tree from all/filtered projects
/// with a project filter button in the toolbar
struct ConversationsTabView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var selectedProjectIds: Set<String> = []  // Empty means show all
    @State private var showFilterSheet = false
    @State private var showDiagnostics = false
    @State private var showAISettings = false
    @State private var showStats = false
    @State private var showArchived = false
    /// Hide scheduled conversations (those with scheduled-task-id tag)
    @AppStorage("hideScheduled") private var hideScheduled = true
    @State private var selectedConversation: ConversationFullInfo?
    @State private var runtimeText: String = "0m"
    @State private var showProjectPickerForNewConv = false
    @State private var projectForNewConversation: ProjectInfo?
    @State private var showNewConversation = false
    @State private var cachedHierarchy = ConversationFullHierarchy(conversations: [])
    #if os(macOS)
    @Environment(\.openWindow) private var openWindow
    #endif

    /// Rebuild the cached hierarchy from current filtered conversations
    private func rebuildHierarchy() {
        cachedHierarchy = ConversationFullHierarchy(conversations: filteredConversations)
    }

    /// Updates the runtime text from SafeTenexCore
    private func updateRuntime() async {
        let totalMs = await coreManager.safeCore.getTodayRuntimeMs()
        let totalSeconds = totalMs / 1000

        if totalSeconds < 60 {
            // Less than 1 minute
            runtimeText = "\(totalSeconds)s"
        } else if totalSeconds < 3600 {
            // Less than 1 hour, show minutes only
            let minutes = totalSeconds / 60
            runtimeText = "\(minutes)m"
        } else {
            // 1 hour or more, show "2h 35m" format
            let hours = totalSeconds / 3600
            let minutes = (totalSeconds % 3600) / 60
            if minutes > 0 {
                runtimeText = "\(hours)h \(minutes)m"
            } else {
                runtimeText = "\(hours)h"
            }
        }
    }

    /// Filtered conversations based on selected projects, archived status, and scheduled status
    private var filteredConversations: [ConversationFullInfo] {
        var conversations = coreManager.conversations

        // Filter by archived status
        if !showArchived {
            conversations = conversations.filter { !$0.isArchived }
        }

        // Filter by scheduled status
        if hideScheduled {
            conversations = conversations.filter { !$0.isScheduled }
        }

        // Filter by selected projects
        if !selectedProjectIds.isEmpty {
            conversations = conversations.filter { conv in
                // projectATag is in a-tag format "kind:pubkey:d-tag", extract d-tag to match project.id
                let projectId = conv.projectATag.split(separator: ":").dropFirst(2).joined(separator: ":")
                return selectedProjectIds.contains(projectId)
            }
        }

        return conversations
    }

    /// Text for the filter button
    private var filterButtonLabel: String {
        if selectedProjectIds.isEmpty {
            return "All Projects"
        } else if selectedProjectIds.count == 1 {
            return coreManager.projects.first { $0.id == selectedProjectIds.first }?.title ?? "1 Project"
        } else {
            return "\(selectedProjectIds.count) Projects"
        }
    }

    var body: some View {
        NavigationStack {
            Group {
                if cachedHierarchy.sortedRootConversations.isEmpty {
                    ConversationsEmptyState(
                        hasFilter: !selectedProjectIds.isEmpty,
                        onClearFilter: { selectedProjectIds.removeAll() }
                    )
                } else {
                    List {
                        ForEach(cachedHierarchy.sortedRootConversations, id: \.id) { conversation in
                            ConversationRowFull(
                                conversation: conversation,
                                projectTitle: projectTitle(for: conversation),
                                isHierarchicallyActive: cachedHierarchy.isHierarchicallyActive(conversation.id),
                                onSelect: { selected in
                                    #if os(macOS)
                                    openWindow(id: "conversation-summary", value: selected.id)
                                    #else
                                    selectedConversation = selected
                                    #endif
                                }
                            )
                            .environmentObject(coreManager)
                            .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                                Button(role: .destructive) {
                                    // Archive action placeholder
                                } label: {
                                    Label("Archive", systemImage: "archivebox")
                                }
                            }
                        }
                    }
                    .listStyle(.plain)
                    .refreshable {
                        await coreManager.manualRefresh()
                    }
                }
            }
            .navigationTitle("Conversations")
            .navigationBarTitleDisplayMode(.large)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button(action: { showFilterSheet = true }) {
                        Label(filterButtonLabel, systemImage: selectedProjectIds.isEmpty ? "line.3.horizontal.decrease.circle" : "line.3.horizontal.decrease.circle.fill")
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    HStack(spacing: 12) {
                        Button(action: { showStats = true }) {
                            Text(runtimeText)
                                .font(.subheadline)
                                .fontWeight(.medium)
                                .foregroundStyle(coreManager.hasActiveAgents ? .green : .secondary)
                        }
                        Menu {
                            Toggle(isOn: $showArchived) {
                                Label("Show Archived", systemImage: "archivebox")
                            }

                            Toggle(isOn: Binding(
                                get: { !hideScheduled },
                                set: { hideScheduled = !$0 }
                            )) {
                                Label("Show Scheduled", systemImage: "calendar.badge.clock")
                            }

                            Divider()

                            Button(action: { showDiagnostics = true }) {
                                Label("Diagnostics", systemImage: "gauge.with.needle")
                            }

                            Button(action: { showAISettings = true }) {
                                Label("AI Settings", systemImage: "waveform")
                            }
                        } label: {
                            Image(systemName: "person.circle")
                                .font(.title3)
                        }
                    }
                }
            }
            .sheet(isPresented: $showFilterSheet) {
                ProjectFilterSheet(
                    projects: coreManager.projects,
                    projectOnlineStatus: coreManager.projectOnlineStatus,
                    selectedProjectIds: $selectedProjectIds
                )
            }
            #if os(iOS)
            .sheet(item: $selectedConversation) { conversation in
                NavigationStack {
                    ConversationDetailView(conversation: conversation)
                        .environmentObject(coreManager)
                        .toolbar {
                            ToolbarItem(placement: .topBarTrailing) {
                                Button("Done") {
                                    selectedConversation = nil
                                }
                            }
                        }
                }
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
            }
            #endif
            .sheet(isPresented: $showDiagnostics) {
                NavigationStack {
                    DiagnosticsView(coreManager: coreManager)
                        .toolbar {
                            ToolbarItem(placement: .topBarLeading) {
                                Button("Done") { showDiagnostics = false }
                            }
                        }
                }
            }
            .sheet(isPresented: $showAISettings) {
                NavigationStack {
                    AISettingsView()
                        .toolbar {
                            ToolbarItem(placement: .topBarLeading) {
                                Button("Done") { showAISettings = false }
                            }
                        }
                }
            }
            .sheet(isPresented: $showStats) {
                NavigationStack {
                    StatsView(coreManager: coreManager)
                        .toolbar {
                            ToolbarItem(placement: .topBarLeading) {
                                Button("Done") { showStats = false }
                            }
                        }
                }
            }
            .sheet(isPresented: $showProjectPickerForNewConv) {
                ProjectSelectorSheet(
                    projects: coreManager.projects,
                    projectOnlineStatus: coreManager.projectOnlineStatus,
                    selectedProject: $projectForNewConversation,
                    onDone: {
                        if projectForNewConversation != nil {
                            showProjectPickerForNewConv = false
                            showNewConversation = true
                        }
                    }
                )
            }
            .sheet(isPresented: $showNewConversation) {
                if let project = projectForNewConversation {
                    NavigationStack {
                        MessageComposerView(project: project)
                            .environmentObject(coreManager)
                    }
                }
            }
            .task {
                rebuildHierarchy()
                await updateRuntime()
                await coreManager.hierarchyCache.preloadForConversations(cachedHierarchy.sortedRootConversations)
            }
            .onChange(of: coreManager.conversations) { _, _ in
                rebuildHierarchy()
                Task {
                    await updateRuntime()
                    await coreManager.hierarchyCache.preloadForConversations(cachedHierarchy.sortedRootConversations)
                }
            }
            .onChange(of: showArchived) { _, _ in
                rebuildHierarchy()
            }
            .onChange(of: selectedProjectIds) { _, _ in
                rebuildHierarchy()
            }
            .onChange(of: hideScheduled) { _, _ in
                rebuildHierarchy()
                // Preload cache for newly visible conversations when showing scheduled
                Task {
                    await coreManager.hierarchyCache.preloadForConversations(cachedHierarchy.sortedRootConversations)
                }
            }
        }
    }

    private func projectTitle(for conversation: ConversationFullInfo) -> String? {
        // projectATag is in a-tag format "kind:pubkey:d-tag", extract d-tag to match project.id
        let projectId = conversation.projectATag.split(separator: ":").dropFirst(2).joined(separator: ":")
        return coreManager.projects.first { $0.id == projectId }?.title
    }
}

// MARK: - Conversation Row for ConversationFullInfo

/// Conversation row that uses ConversationFullInfo's rich data.
/// PERFORMANCE: Uses cached hierarchy data instead of per-row FFI calls.
/// The cache is preloaded in ConversationsTabView.task for all visible conversations.
private struct ConversationRowFull: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @ObservedObject var player = AudioNotificationPlayer.shared
    let conversation: ConversationFullInfo
    let projectTitle: String?
    /// Whether this conversation or any of its descendants has active work
    let isHierarchicallyActive: Bool
    let onSelect: (ConversationFullInfo) -> Void

    /// Whether this conversation is currently playing audio
    private var isPlayingAudio: Bool {
        player.playbackState != .idle && player.currentConversationId == conversation.id
    }

    /// Get cached hierarchy data (O(1) lookup, no FFI calls)
    private var cachedHierarchy: ConversationHierarchyCache.ConversationHierarchy? {
        coreManager.hierarchyCache.getHierarchy(for: conversation.id)
    }

    /// Delegation agent infos from cache (or empty if not yet loaded)
    private var delegationAgentInfos: [AgentAvatarInfo] {
        cachedHierarchy?.delegationAgentInfos ?? []
    }

    /// P-tagged recipient info from cache
    private var pTaggedRecipientInfo: AgentAvatarInfo? {
        cachedHierarchy?.pTaggedRecipientInfo
    }

    /// Status color based on hierarchical activity (accounts for descendants)
    private var statusColor: Color {
        // Show green if this conversation OR any descendant is active
        if isHierarchicallyActive { return .green }
        switch conversation.status?.lowercased() ?? "" {
        case "active", "in progress": return .green
        case "waiting", "blocked": return .orange
        case "completed", "done": return .gray
        default: return .blue
        }
    }

    var body: some View {
        HStack(spacing: 12) {
            // Status indicator with activity pulse (shows pulse if hierarchically active)
            ZStack {
                Circle()
                    .fill(statusColor)
                    .frame(width: 10, height: 10)

                if isHierarchicallyActive {
                    Circle()
                        .stroke(statusColor.opacity(0.5), lineWidth: 2)
                        .frame(width: 16, height: 16)
                }
            }

            VStack(alignment: .leading, spacing: 6) {
                // Row 1: Title and effective last active time
                HStack(alignment: .top) {
                    Text(conversation.title)
                        .font(.headline)
                        .lineLimit(2)

                    if isPlayingAudio {
                        Image(systemName: "speaker.wave.2.fill")
                            .font(.caption)
                            .foregroundStyle(.blue)
                            .symbolEffect(.variableColor.iterative, isActive: player.isPlaying)
                    }

                    Spacer()

                    Text(ConversationFormatters.formatRelativeTime(conversation.effectiveLastActivity))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                // Row 2: Summary or current activity
                HStack(alignment: .top) {
                    // Show current activity if directly active (not hierarchically via descendants)
                    if let activity = conversation.currentActivity, conversation.isActive {
                        HStack(spacing: 4) {
                            Image(systemName: "bolt.fill")
                                .font(.caption2)
                                .foregroundStyle(.orange)
                            Text(activity)
                                .font(.subheadline)
                                .foregroundStyle(.orange)
                                .lineLimit(1)
                        }
                    // Show "Delegation active" if hierarchically active but not directly active
                    } else if isHierarchicallyActive && !conversation.isActive {
                        HStack(spacing: 4) {
                            Image(systemName: "arrow.triangle.branch")
                                .font(.caption2)
                                .foregroundStyle(.green)
                            Text("Delegation active")
                                .font(.subheadline)
                                .foregroundStyle(.green)
                                .lineLimit(1)
                        }
                    } else if let summary = conversation.summary {
                        Text(summary)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                    } else {
                        Text("No summary")
                            .font(.subheadline)
                            .foregroundStyle(.tertiary)
                            .italic()
                    }

                    Spacer()

                    // Show message count
                    if conversation.messageCount > 0 {
                        HStack(spacing: 2) {
                            Image(systemName: "bubble.left")
                                .font(.caption2)
                            Text("\(conversation.messageCount)")
                        }
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                    }
                }

                // Row 3: Avatars (author + p-tagged overlapping, then delegation agents) + badges
                HStack(spacing: 0) {
                    ConversationAvatarGroup(
                        authorInfo: AgentAvatarInfo(name: conversation.author, pubkey: conversation.authorPubkey),
                        pTaggedRecipientInfo: pTaggedRecipientInfo,
                        otherParticipants: delegationAgentInfos,
                        maxVisibleAvatars: maxVisibleAvatars
                    )
                    .environmentObject(coreManager)

                    Spacer()

                    // Scheduled badge (shows when conversation has scheduled-task-id tag)
                    if conversation.isScheduled {
                        HStack(spacing: 2) {
                            Image(systemName: "clock")
                                .font(.caption2)
                            Text("Scheduled")
                        }
                        .font(.caption2)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Color.purple.opacity(0.15))
                        .foregroundStyle(.purple)
                        .clipShape(Capsule())
                    }

                    // Status badge
                    if let status = conversation.status {
                        Text(status)
                            .font(.caption2)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(statusColor.opacity(0.15))
                            .foregroundStyle(statusColor)
                            .clipShape(Capsule())
                    }

                    // Show project title badge if available
                    if let projectTitle = projectTitle {
                        Text(projectTitle)
                            .font(.caption2)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(Color.blue.opacity(0.15))
                            .foregroundStyle(.blue)
                            .clipShape(Capsule())
                    }
                }
            }

            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 10)
        .contentShape(Rectangle())
        .onTapGesture {
            onSelect(conversation)
        }
        // PERFORMANCE: Removed per-row .task that called loadDelegationAgentInfos()
        // Hierarchy data is now preloaded in batch by ConversationsTabView
    }
}

// MARK: - Project Filter Sheet

private struct ProjectFilterSheet: View {
    let projects: [ProjectInfo]
    let projectOnlineStatus: [String: Bool]
    @Binding var selectedProjectIds: Set<String>
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                // "All Projects" option
                Button(action: {
                    selectedProjectIds.removeAll()
                }) {
                    HStack {
                        Image(systemName: "square.grid.2x2")
                            .foregroundStyle(.blue)
                            .frame(width: 24)
                        Text("All Projects")
                            .foregroundStyle(.primary)
                        Spacer()
                        if selectedProjectIds.isEmpty {
                            Image(systemName: "checkmark")
                                .foregroundStyle(.blue)
                        }
                    }
                }

                Divider()

                // Individual projects
                ForEach(projects, id: \.id) { project in
                    Button(action: {
                        toggleProject(project.id)
                    }) {
                        HStack {
                            RoundedRectangle(cornerRadius: 6)
                                .fill(projectColor(for: project).gradient)
                                .frame(width: 24, height: 24)
                                .overlay {
                                    Image(systemName: "folder.fill")
                                        .foregroundStyle(.white)
                                        .font(.caption)
                                }

                            Text(project.title)
                                .foregroundStyle(.primary)
                                .lineLimit(1)

                            if projectOnlineStatus[project.id] == true {
                                Circle()
                                    .fill(.green)
                                    .frame(width: 8, height: 8)
                            }

                            Spacer()

                            if selectedProjectIds.contains(project.id) {
                                Image(systemName: "checkmark")
                                    .foregroundStyle(.blue)
                            }
                        }
                    }
                }
            }
            #if os(iOS)
                .listStyle(.insetGrouped)
                #else
                .listStyle(.inset)
                #endif
            .navigationTitle("Filter Projects")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        dismiss()
                    }
                    .fontWeight(.semibold)
                }
            }
        }
        .presentationDetents([.medium, .large])
    }

    private func toggleProject(_ id: String) {
        if selectedProjectIds.contains(id) {
            selectedProjectIds.remove(id)
        } else {
            selectedProjectIds.insert(id)
        }
    }

    /// Deterministic color using shared utility (stable across app launches)
    private func projectColor(for project: ProjectInfo) -> Color {
        deterministicColor(for: project.id)
    }
}

// MARK: - Empty State

private struct ConversationsEmptyState: View {
    let hasFilter: Bool
    let onClearFilter: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: hasFilter ? "line.3.horizontal.decrease.circle" : "bubble.left.and.bubble.right")
                .font(.system(size: 60))
                .foregroundStyle(.secondary)

            Text(hasFilter ? "No Matching Conversations" : "No Conversations")
                .font(.title2)
                .fontWeight(.semibold)

            Text(hasFilter ? "Try adjusting your project filter" : "Conversations will appear automatically")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            if hasFilter {
                Button(action: onClearFilter) {
                    Label("Clear Filter", systemImage: "xmark.circle")
                }
                .buttonStyle(.bordered)
                .padding(.top, 8)
            }
        }
        .padding()
    }
}

// MARK: - Messages Sheet View (for viewing conversation details)

private struct MessagesSheetView: View {
    let conversation: ConversationInfo
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var messages: [MessageInfo] = []
    @State private var isLoading = false
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 12) {
                    if isLoading {
                        ProgressView()
                            .frame(maxWidth: .infinity)
                            .padding()
                    } else if messages.isEmpty {
                        VStack(spacing: 12) {
                            Image(systemName: "bubble.left")
                                .font(.system(size: 40))
                                .foregroundStyle(.secondary)
                            Text("No messages yet")
                                .foregroundStyle(.secondary)
                        }
                        .frame(maxWidth: .infinity)
                        .padding(.top, 60)
                    } else {
                        ForEach(messages, id: \.id) { message in
                            MessageBubbleView(message: message)
                        }
                    }
                }
                .padding()
            }
            .navigationTitle(conversation.title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .task {
                await loadMessages()
            }
            .onReceive(coreManager.$messagesByConversation) { cache in
                if let updated = cache[conversation.id] {
                    messages = updated
                }
            }
        }
    }

    private func loadMessages() async {
        isLoading = true
        await coreManager.ensureMessagesLoaded(conversationId: conversation.id)
        messages = coreManager.messagesByConversation[conversation.id] ?? []
        isLoading = false
    }
}

// MARK: - Message Bubble View

private struct MessageBubbleView: View {
    let message: MessageInfo

    private var isUser: Bool {
        message.role == "user"
    }

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            if isUser { Spacer(minLength: 50) }

            VStack(alignment: isUser ? .trailing : .leading, spacing: 4) {
                // Author header
                HStack(spacing: 6) {
                    if !isUser {
                        Circle()
                            .fill(Color.blue.gradient)
                            .frame(width: 24, height: 24)
                            .overlay {
                                Image(systemName: "sparkle")
                                    .font(.caption2)
                                    .foregroundStyle(.white)
                            }
                    }

                    Text(message.author)
                        .font(.caption)
                        .fontWeight(.medium)
                        .foregroundStyle(.secondary)

                    Text(ConversationFormatters.formatRelativeTime(message.createdAt))
                        .font(.caption2)
                        .foregroundStyle(.tertiary)

                    if isUser {
                        Circle()
                            .fill(Color.green.gradient)
                            .frame(width: 24, height: 24)
                            .overlay {
                                Image(systemName: "person.fill")
                                    .font(.caption2)
                                    .foregroundStyle(.white)
                            }
                    }
                }

                // Message content
                Text(message.content)
                    .font(.body)
                    .padding(12)
                    .background(isUser ? Color.blue.opacity(0.15) : Color.systemGray6)
                    .clipShape(RoundedRectangle(cornerRadius: 16))
            }

            if !isUser { Spacer(minLength: 50) }
        }
    }
}

// Note: ConversationInfo Identifiable conformance is in ConversationsView.swift

#Preview {
    ConversationsTabView()
        .environmentObject(TenexCoreManager())
}
