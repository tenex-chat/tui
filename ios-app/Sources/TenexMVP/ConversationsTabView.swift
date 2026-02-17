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
    @State private var showAudioQueue = false
    @State private var audioNotificationsEnabled = false
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
    // Conversation reference feature state - uses .sheet(item:) pattern for safe state management
    @State private var conversationToReference: ConversationFullInfo?
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    private var useSplitView: Bool {
        #if os(macOS)
        return true
        #else
        return horizontalSizeClass == .regular
        #endif
    }

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

    var body: some View {
        Group {
            if useSplitView {
                splitViewLayout
            } else {
                stackLayout
            }
        }
        .task {
            rebuildHierarchy()
            await updateRuntime()
            await coreManager.hierarchyCache.preloadForConversations(cachedHierarchy.sortedRootConversations)
            if let settings = try? await coreManager.safeCore.getAiAudioSettings() {
                audioNotificationsEnabled = settings.enabled
            }
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

    // MARK: - Split View Layout (iPad/Mac)

    private var splitViewLayout: some View {
        #if os(macOS)
        HSplitView {
            conversationListContent
                .navigationTitle("Conversations")
                .frame(minWidth: 340, idealWidth: 440, maxWidth: 520, maxHeight: .infinity)

            conversationDetailContent
                .frame(minWidth: 600, maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        #else
        NavigationSplitView {
            conversationListContent
                .navigationTitle("Conversations")
        } detail: {
            conversationDetailContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        #endif
    }

    @ViewBuilder
    private var conversationDetailContent: some View {
        if let conversation = selectedConversation {
            NavigationStack {
                ConversationDetailView(conversation: conversation)
                    .environmentObject(coreManager)
            }
            .id(conversation.id)
        } else {
            ContentUnavailableView(
                "Select a Conversation",
                systemImage: "bubble.left.and.bubble.right",
                description: Text("Choose a conversation from the list")
            )
        }
    }

    // MARK: - Stack Layout (iPhone)

    private var stackLayout: some View {
        NavigationStack {
            conversationListContent
                .navigationTitle("Conversations")
                .navigationBarTitleDisplayMode(.large)
                .sheet(item: $selectedConversation) { conversation in
                    NavigationStack {
                        ConversationDetailView(conversation: conversation)
                            .environmentObject(coreManager)
                            .toolbar {
                                ToolbarItem(placement: .topBarTrailing) {
                                    Button("Done") { selectedConversation = nil }
                                }
                            }
                    }
                    .presentationDetents([.large])
                    .presentationDragIndicator(.visible)
                }
        }
    }

    // MARK: - Conversation List Content (shared between layouts)

    private var conversationListContent: some View {
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
                                selectedConversation = selected
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
                        .swipeActions(edge: .leading, allowsFullSwipe: false) {
                            Button {
                                conversationToReference = conversation
                            } label: {
                                Label("Reference", systemImage: "link")
                            }
                            .tint(Color.agentBrand)
                        }
                    }
                }
                .listStyle(.plain)
                .refreshable {
                    await coreManager.manualRefresh()
                }
            }
        }
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                ControlGroup {
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

                        Toggle(isOn: $audioNotificationsEnabled) {
                            Label("Audio Notifications", systemImage: "speaker.wave.2")
                        }
                        .onChange(of: audioNotificationsEnabled) { _, enabled in
                            Task {
                                try? await coreManager.safeCore.setAudioNotificationsEnabled(enabled: enabled)
                            }
                        }

                        Divider()

                        Button(action: { showAudioQueue = true }) {
                            Label("Audio Queue", systemImage: "list.bullet")
                        }

                        Button(action: { showAISettings = true }) {
                            Label("AI Settings", systemImage: "waveform")
                        }

                        Button(action: { showDiagnostics = true }) {
                            Label("Diagnostics", systemImage: "gauge.with.needle")
                        }
                    } label: {
                        HStack(spacing: 4) {
                            Image(systemName: "person")
                            Image(systemName: "chevron.down")
                                .font(.caption2)
                        }
                    }

                    Button(action: { showStats = true }) {
                        Text(runtimeText)
                            .font(.subheadline)
                            .fontWeight(.medium)
                            .foregroundStyle(coreManager.hasActiveAgents ? Color.presenceOnline : .secondary)
                    }
                }
            }

            ToolbarItem(placement: .topBarTrailing) {
                ControlGroup {
                    Button(action: { showProjectPickerForNewConv = true }) {
                        Image(systemName: "plus")
                    }

                    Button(action: { showFilterSheet = true }) {
                        ZStack(alignment: .topTrailing) {
                            Image(systemName: "folder")
                            // Show badge when filtering is active
                            if !selectedProjectIds.isEmpty {
                                Circle()
                                    .fill(Color.unreadIndicator)
                                    .frame(width: 8, height: 8)
                                    .offset(x: 2, y: -2)
                            }
                        }
                    }
                    .accessibilityLabel(selectedProjectIds.isEmpty ? "Filter by project" : "Filtering \(selectedProjectIds.count) project\(selectedProjectIds.count == 1 ? "" : "s")")
                    .accessibilityHint("Opens project filter sheet")
                }
            }
        }
        .controlSize(.small)
        .sheet(isPresented: $showFilterSheet) {
            ProjectsSheet(selectedProjectIds: $selectedProjectIds)
                .environmentObject(coreManager)
        }
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
        .sheet(isPresented: $showAudioQueue) {
            AudioQueueSheet()
                .environmentObject(coreManager)
        }
        .sheet(isPresented: $showAISettings) {
            AISettingsView()
                #if os(macOS)
                .frame(minWidth: 500, idealWidth: 520, minHeight: 500, idealHeight: 600)
                #endif
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
                MessageComposerView(project: project)
                    .environmentObject(coreManager)
            }
        }
        .sheet(item: $conversationToReference) { conversation in
            // Extract project from conversation's projectATag
            let projectId = conversation.projectATag.split(separator: ":").dropFirst(2).joined(separator: ":")
            if let project = coreManager.projects.first(where: { $0.id == projectId }) {
                MessageComposerView(
                    project: project,
                    initialContent: ConversationFormatters.generateContextMessage(conversation: conversation),
                    referenceConversationId: conversation.id
                )
                .environmentObject(coreManager)
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

    #if os(macOS)
    @Environment(\.openWindow) private var openWindow
    @State private var isHovered = false
    #else
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    @State private var showDelegationTree = false
    #endif

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

    private var statusColor: Color {
        Color.conversationStatus(for: conversation.status, isActive: isHierarchicallyActive)
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
                            .foregroundStyle(Color.agentBrand)
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
                                .foregroundStyle(Color.skillBrand)
                            Text(activity)
                                .font(.subheadline)
                                .foregroundStyle(Color.skillBrand)
                                .lineLimit(1)
                        }
                    // Show "Delegation active" if hierarchically active but not directly active
                    } else if isHierarchicallyActive && !conversation.isActive {
                        HStack(spacing: 4) {
                            Image(systemName: "arrow.triangle.branch")
                                .font(.caption2)
                                .foregroundStyle(Color.presenceOnline)
                            Text("Delegation active")
                                .font(.subheadline)
                                .foregroundStyle(Color.presenceOnline)
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
                        .background(Color.projectBrandBackground)
                        .foregroundStyle(Color.projectBrand)
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
                            .background(Color.messageBubbleUserBackground)
                            .foregroundStyle(Color.agentBrand)
                            .clipShape(Capsule())
                    }

                    // Delegation tree button (Mac/iPad only)
                    #if os(macOS)
                    if conversation.hasChildren {
                        Button {
                            openWindow(id: "delegation-tree", value: conversation.id)
                        } label: {
                            Image(systemName: "arrow.triangle.branch")
                                .font(.system(size: 13))
                                .foregroundStyle(.secondary)
                        }
                        .buttonStyle(.plain)
                        .opacity(isHovered ? 1 : 0)
                        .help("View delegation tree")
                    }
                    #else
                    if conversation.hasChildren && horizontalSizeClass == .regular {
                        Button {
                            showDelegationTree = true
                        } label: {
                            Image(systemName: "arrow.triangle.branch")
                                .font(.system(size: 13))
                                .foregroundStyle(.secondary)
                        }
                        .buttonStyle(.plain)
                        .help("View delegation tree")
                    }
                    #endif
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
        #if os(macOS)
        .onHover { hovering in
            isHovered = hovering
        }
        #else
        .fullScreenCover(isPresented: $showDelegationTree) {
            NavigationStack {
                DelegationTreeView(rootConversationId: conversation.id)
                    .environmentObject(coreManager)
                    .toolbar {
                        ToolbarItem(placement: .topBarTrailing) {
                            Button("Done") { showDelegationTree = false }
                        }
                    }
            }
        }
        #endif
        // PERFORMANCE: Removed per-row .task that called loadDelegationAgentInfos()
        // Hierarchy data is now preloaded in batch by ConversationsTabView
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


#Preview {
    ConversationsTabView()
        .environmentObject(TenexCoreManager())
}
