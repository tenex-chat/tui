import SwiftUI

struct ConversationsTabView: View {
    @Environment(TenexCoreManager.self) var coreManager
    @ObservedObject private var audioPlayer = AudioNotificationPlayer.shared
    let layoutMode: ConversationsLayoutMode
    private let selectedConversationBindingOverride: Binding<ConversationFullInfo?>?
    private let newConversationProjectIdBindingOverride: Binding<String?>?
    private let onShowDiagnosticsInApp: (() -> Void)?

    @State private var showDiagnostics = false
    @State private var showAISettings = false
    @State private var showAudioQueue = false
    @State private var audioNotificationsEnabled = false
    @State private var showStats = false
    @State private var showArchived = false
    @State private var selectedConversationState: ConversationFullInfo?
    @State private var newConversationProjectIdState: String?
    @State private var projectForNewConversation: SelectedProjectForComposer?
    @State private var pendingCreatedConversationId: String?
    @State private var cachedHierarchy = ConversationFullHierarchy(conversations: [])
    @State private var hierarchyRebuildTask: Task<Void, Never>?
    @State private var projectMenuState = ProjectMenuState()
    @State private var projectTitleById: [String: String] = [:]
    // Conversation reference feature state - uses .sheet(item:) pattern for safe state management
    @State private var conversationToReference: ConversationFullInfo?
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    init(
        layoutMode: ConversationsLayoutMode = .adaptive,
        selectedConversation: Binding<ConversationFullInfo?>? = nil,
        newConversationProjectId: Binding<String?>? = nil,
        onShowDiagnosticsInApp: (() -> Void)? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedConversationBindingOverride = selectedConversation
        self.newConversationProjectIdBindingOverride = newConversationProjectId
        self.onShowDiagnosticsInApp = onShowDiagnosticsInApp
    }

    private var selectedConversationBinding: Binding<ConversationFullInfo?> {
        selectedConversationBindingOverride ?? $selectedConversationState
    }

    private var newConversationProjectIdBinding: Binding<String?> {
        newConversationProjectIdBindingOverride ?? $newConversationProjectIdState
    }

    private var useSplitView: Bool {
        if layoutMode == .shellList || layoutMode == .shellDetail || layoutMode == .shellComposite {
            return true
        }
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

    /// Debounced hierarchy rebuild — coalesces rapid conversation upserts
    /// into a single rebuild to prevent render starvation on the main thread.
    private func scheduleHierarchyRebuild() {
        hierarchyRebuildTask?.cancel()
        hierarchyRebuildTask = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(300))
            guard !Task.isCancelled else { return }
            rebuildHierarchy()
            await coreManager.hierarchyCache.preloadForConversations(cachedHierarchy.sortedRootConversations)
        }
    }

    private func rebuildProjectCaches() {
        let projects = coreManager.projects
        let onlineStatus = coreManager.projectOnlineStatus

        projectTitleById = Dictionary(uniqueKeysWithValues: projects.map { ($0.id, $0.title) })

        let sorted = projects.sorted { a, b in
            let aOnline = onlineStatus[a.id] ?? false
            let bOnline = onlineStatus[b.id] ?? false
            if aOnline != bOnline { return aOnline }
            return a.title.localizedCaseInsensitiveCompare(b.title) == .orderedAscending
        }

        var booted: [Project] = []
        var unbooted: [Project] = []
        booted.reserveCapacity(sorted.count)
        unbooted.reserveCapacity(sorted.count)

        for project in sorted {
            if onlineStatus[project.id] ?? false {
                booted.append(project)
            } else {
                unbooted.append(project)
            }
        }

        projectMenuState = ProjectMenuState(booted: booted, unbooted: unbooted)
    }

    /// Filtered conversations based on archived status.
    /// Global app filtering (project/time/scheduled/status/hashtags) is applied centrally in TenexCoreManager.
    private var filteredConversations: [ConversationFullInfo] {
        var conversations = coreManager.conversations

        if !showArchived {
            conversations = conversations.filter { !$0.isArchived }
        }

        return conversations
    }

    var body: some View {
        Group {
            switch layoutMode {
            case .shellList:
                shellListLayout
            case .shellDetail:
                shellDetailLayout
            case .shellComposite:
                shellCompositeLayout
            case .adaptive:
                if useSplitView {
                    splitViewLayout
                } else {
                    stackLayout
                }
            }
        }
        .task {
            rebuildHierarchy()
            rebuildProjectCaches()
            await coreManager.hierarchyCache.preloadForConversations(cachedHierarchy.sortedRootConversations)
            if let settings = try? await coreManager.safeCore.getAiAudioSettings() {
                audioNotificationsEnabled = settings.enabled
            }
        }
        .onChange(of: coreManager.conversations) { _, _ in
            if let pendingId = pendingCreatedConversationId,
               let conversation = coreManager.conversationById[pendingId] {
                selectCreatedConversation(conversation)
            }
            scheduleHierarchyRebuild()
            if let selectedId = selectedConversationBinding.wrappedValue?.thread.id,
               !filteredConversations.contains(where: { $0.thread.id == selectedId }) {
                selectedConversationBinding.wrappedValue = nil
                newConversationProjectIdBinding.wrappedValue = nil
                pendingCreatedConversationId = nil
            }
        }
        .onChange(of: showArchived) { _, _ in
            rebuildHierarchy()
        }
        .onChange(of: coreManager.projects) { _, _ in
            rebuildProjectCaches()
        }
        .onChange(of: coreManager.projectOnlineStatus) { _, _ in
            rebuildProjectCaches()
        }
        .onChange(of: selectedConversationBinding.wrappedValue?.thread.id) { _, newId in
            guard newId != nil else { return }
            newConversationProjectIdBinding.wrappedValue = nil
            pendingCreatedConversationId = nil
        }
        .sheet(isPresented: $showDiagnostics) {
            #if os(macOS)
            DiagnosticsView(coreManager: coreManager)
                .toolbar {
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Done") { showDiagnostics = false }
                    }
                }
                .frame(minWidth: 500, idealWidth: 520, minHeight: 500, idealHeight: 600)
            #else
            NavigationStack {
                DiagnosticsView(coreManager: coreManager)
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) {
                            Button("Done") { showDiagnostics = false }
                        }
                    }
            }
            .tenexModalPresentation(detents: [.large])
            #endif
        }
        .sheet(isPresented: $showAudioQueue) {
            AudioQueueSheet()
                .environment(coreManager)
        }
        .sheet(isPresented: $showAISettings) {
            AppSettingsView(defaultSection: .audio)
                .tenexModalPresentation(detents: [.large])
                #if os(macOS)
                .frame(minWidth: 500, idealWidth: 520, minHeight: 500, idealHeight: 600)
                #endif
        }
        .sheet(isPresented: $showStats) {
            NavigationStack {
                StatsView()
                    .environment(coreManager)
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) {
                            Button("Done") { showStats = false }
                        }
                    }
            }
            .tenexModalPresentation(detents: [.large])
        }
        .sheet(item: $projectForNewConversation) { selectedProject in
            MessageComposerView(project: selectedProject.project)
                .environment(coreManager)
                .tenexModalPresentation(detents: [.large])
        }
        .sheet(item: $conversationToReference) { conversation in
            let projectId = TenexCoreManager.projectId(fromATag: conversation.projectATag)
            if let project = coreManager.projects.first(where: { $0.id == projectId }) {
                MessageComposerView(
                    project: project,
                    initialContent: ConversationFormatters.generateContextMessage(conversation: conversation),
                    referenceConversationId: conversation.thread.id
                )
                .environment(coreManager)
                .tenexModalPresentation(detents: [.large])
            }
        }
    }

    // MARK: - Split View Layout (iPad/Mac)

    private var splitViewLayout: some View {
        NavigationSplitView {
            splitSidebarContent
                .navigationTitle("Chats")
                #if os(macOS)
                .navigationSplitViewColumnWidth(min: 340, ideal: 440, max: 520)
                #endif
        } detail: {
            conversationDetailContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
    }

    private var splitSidebarContent: some View {
        List(selection: selectedConversationBinding) {
            conversationRows(isSplitInteraction: true)
        }
        .toolbar {
            #if os(macOS)
            ToolbarItem(placement: .navigation) {
                AppGlobalFilterToolbarButton()
            }
            ToolbarItem(placement: .navigation) {
                newConversationMenuButton
            }
            #else
            ToolbarItem(placement: .automatic) {
                AppGlobalFilterToolbarButton()
            }
            ToolbarItem(placement: .automatic) {
                newConversationMenuButton
            }
            #endif
        }
        .modifier(
            ShellConversationListStyle(
                isShellColumn: layoutMode == .shellList || layoutMode == .shellComposite
            )
        )
        .refreshable {
            await coreManager.manualRefresh()
        }
    }

    @ViewBuilder
    private var conversationDetailContent: some View {
        if let conversation = selectedConversationBinding.wrappedValue {
            ConversationAdaptiveDetailView(conversation: conversation)
                .environment(coreManager)
            .id(conversation.thread.id)
        } else if let newProjectId = newConversationProjectIdBinding.wrappedValue,
                  let project = coreManager.projects.first(where: { $0.id == newProjectId }) {
            ConversationWorkspaceView(
                source: .newThread(project: project),
                onThreadCreated: handleThreadCreated
            )
            .environment(coreManager)
            .id("new-thread-\(project.id)")
        } else {
            ContentUnavailableView(
                "Select a Conversation",
                systemImage: "bubble.left.and.bubble.right",
                description: Text("Choose a conversation from the list")
            )
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
        }
    }

    private var shellListLayout: some View {
        splitSidebarContent
            .navigationTitle("Chats")
            .accessibilityIdentifier("section_list_column")
    }

    private var shellDetailLayout: some View {
        NavigationStack {
            conversationDetailContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .accessibilityIdentifier("detail_column")
    }

    private var shellCompositeLayout: some View {
        #if os(macOS)
        HSplitView {
            shellListLayout
                .frame(minWidth: 340, idealWidth: 430, maxWidth: 520)

            shellDetailLayout
                .frame(minWidth: 520)
        }
        #else
        HStack(spacing: 0) {
            shellListLayout
                .frame(minWidth: 340, idealWidth: 430, maxWidth: 520)

            Divider()

            shellDetailLayout
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        #endif
    }

    // MARK: - Stack Layout (iPhone)

    private var stackLayout: some View {
        NavigationStack {
            conversationListContent
                .navigationTitle("Chats")
                #if os(iOS)
                .navigationBarTitleDisplayMode(.large)
                #else
                .toolbarTitleDisplayMode(.inline)
                #endif
                .toolbar {
                    ToolbarItem(placement: .automatic) {
                        ControlGroup {
                            AppGlobalFilterToolbarButton()
                            settingsMenu(compact: true)
                            runtimeButton
                        }
                    }

                    ToolbarItem(placement: .automatic) {
                        newConversationMenuButton
                    }
                }
                .sheet(item: selectedConversationBinding) { conversation in
                    NavigationStack {
                        ConversationAdaptiveDetailView(conversation: conversation)
                            .environment(coreManager)
                            .toolbar {
                                ToolbarItem(placement: .confirmationAction) {
                                    Button("Done") { selectedConversationBinding.wrappedValue = nil }
                                }
                            }
                    }
                    .tenexModalPresentation(detents: [.large])
                }
        }
    }

    // MARK: - Conversation List Content

    private var conversationListContent: some View {
        List {
            conversationRows(isSplitInteraction: false)
        }
        .listStyle(.plain)
        .refreshable {
            await coreManager.manualRefresh()
        }
    }

    @ViewBuilder
    private func conversationRows(isSplitInteraction: Bool) -> some View {
        if cachedHierarchy.sortedRootConversations.isEmpty {
            ConversationsEmptyState(
                hasFilter: !coreManager.isAppFilterDefault,
                onClearFilter: { coreManager.resetAppFilterToDefaults() }
            )
            .listRowBackground(Color.clear)
            .listRowSeparator(.hidden)
        } else {
            ForEach(cachedHierarchy.sortedRootConversations, id: \.thread.id) { conversation in
                let hierarchy = coreManager.hierarchyCache.getHierarchy(for: conversation.thread.id)
                let pTaggedRecipientInfo = hierarchy?.pTaggedRecipientInfo
                let delegationAgentInfos = hierarchy?.delegationAgentInfos ?? []
                let isPlayingAudio = audioPlayer.playbackState != .idle && audioPlayer.currentConversationId == conversation.thread.id
                #if os(iOS)
                if isSplitInteraction {
                    ConversationRowFull(
                        conversation: conversation,
                        projectTitle: projectTitle(for: conversation),
                        isHierarchicallyActive: cachedHierarchy.isHierarchicallyActive(conversation.id),
                        pTaggedRecipientInfo: pTaggedRecipientInfo,
                        delegationAgentInfos: delegationAgentInfos,
                        isPlayingAudio: isPlayingAudio,
                        isAudioPlaying: audioPlayer.isPlaying,
                        showsChevron: false,
                        onSelect: nil
                    )
                    .equatable()
                    .tag(conversation)
                } else {
                    ConversationRowFull(
                        conversation: conversation,
                        projectTitle: projectTitle(for: conversation),
                        isHierarchicallyActive: cachedHierarchy.isHierarchicallyActive(conversation.id),
                        pTaggedRecipientInfo: pTaggedRecipientInfo,
                        delegationAgentInfos: delegationAgentInfos,
                        isPlayingAudio: isPlayingAudio,
                        isAudioPlaying: audioPlayer.isPlaying,
                        showsChevron: true,
                        onSelect: { selected in
                            selectedConversationBinding.wrappedValue = selected
                        }
                    )
                    .equatable()
                    .tag(conversation)
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
                #else
                ConversationRowFull(
                    conversation: conversation,
                    projectTitle: projectTitle(for: conversation),
                    isHierarchicallyActive: cachedHierarchy.isHierarchicallyActive(conversation.id),
                    pTaggedRecipientInfo: pTaggedRecipientInfo,
                    delegationAgentInfos: delegationAgentInfos,
                    isPlayingAudio: isPlayingAudio,
                    isAudioPlaying: audioPlayer.isPlaying,
                    showsChevron: false,
                    onSelect: nil
                )
                .equatable()
                .tag(conversation)
                #endif
            }
        }
    }

    private var newConversationMenuButton: some View {
        Menu {
            if projectMenuState.booted.isEmpty && projectMenuState.unbooted.isEmpty {
                Text("No projects available")
            } else {
                if !projectMenuState.booted.isEmpty {
                    Section("Booted Projects") {
                        ForEach(projectMenuState.booted, id: \.id) { project in
                            Button {
                                startNewConversation(in: project)
                            } label: {
                                Label(project.title, systemImage: "bolt.fill")
                            }
                        }
                    }
                }

                if !projectMenuState.unbooted.isEmpty {
                    Menu("Unbooted Projects") {
                        ForEach(projectMenuState.unbooted, id: \.id) { project in
                            Button {
                                startNewConversation(in: project)
                            } label: {
                                Label(project.title, systemImage: "moon.zzz")
                            }
                        }
                    }
                }
            }
        } label: {
            Image(systemName: "plus")
        }
        .accessibilityLabel("Create conversation")
    }

    private var runtimeButton: some View {
        Button(action: { showStats = true }) {
            Text(coreManager.runtimeText)
                .font(.subheadline)
                .fontWeight(.medium)
                .foregroundStyle(coreManager.hasActiveAgents ? Color.presenceOnline : .secondary)
        }
    }

    private func settingsMenu(compact: Bool) -> some View {
        Menu {
            Toggle(isOn: $showArchived) {
                Label("Show Archived", systemImage: "archivebox")
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
                Label("Settings", systemImage: "gearshape")
            }

            Button {
                #if os(macOS)
                if let onShowDiagnosticsInApp {
                    onShowDiagnosticsInApp()
                } else {
                    showDiagnostics = true
                }
                #else
                showDiagnostics = true
                #endif
            } label: {
                Label("Diagnostics", systemImage: "gauge.with.needle")
            }
        } label: {
            if compact {
                HStack(spacing: 4) {
                    Image(systemName: "person")
                    Image(systemName: "chevron.down")
                        .font(.caption2)
                }
            } else {
                Label("You & Settings", systemImage: "person.crop.circle")
                    .labelStyle(.titleAndIcon)
            }
        }
    }

    private func startNewConversation(in project: Project) {
        // Defer presentation one turn so the menu can fully dismiss first.
        DispatchQueue.main.async {
            #if os(macOS)
            selectedConversationBinding.wrappedValue = nil
            newConversationProjectIdBinding.wrappedValue = project.id
            #else
            projectForNewConversation = SelectedProjectForComposer(project: project)
            #endif
        }
    }

    private func handleThreadCreated(_ eventId: String) {
        pendingCreatedConversationId = eventId

        if let conversation = coreManager.conversationById[eventId] {
            selectCreatedConversation(conversation)
            return
        }

        Task {
            let fetched = await coreManager.safeCore.getConversationsByIds(conversationIds: [eventId])
            await MainActor.run {
                if let conversation = fetched.first {
                    selectCreatedConversation(conversation)
                }
            }
        }
    }

    private func selectCreatedConversation(_ conversation: ConversationFullInfo) {
        let canonical = coreManager.conversationById[conversation.thread.id] ?? conversation
        selectedConversationBinding.wrappedValue = canonical
        newConversationProjectIdBinding.wrappedValue = nil
        pendingCreatedConversationId = nil
    }

    private func projectTitle(for conversation: ConversationFullInfo) -> String? {
        let projectId = TenexCoreManager.projectId(fromATag: conversation.projectATag)
        return projectTitleById[projectId]
    }
}

private struct SelectedProjectForComposer: Identifiable {
    let project: Project
    var id: String { project.id }
}

private struct ProjectMenuState {
    var booted: [Project] = []
    var unbooted: [Project] = []
}

private struct ShellConversationListStyle: ViewModifier {
    let isShellColumn: Bool

    @ViewBuilder
    func body(content: Content) -> some View {
        if isShellColumn {
            #if os(macOS)
            content.listStyle(.inset)
            #else
            content.listStyle(.plain)
            #endif
        } else {
            content.listStyle(.sidebar)
        }
    }
}
// MARK: - Conversation Row for ConversationFullInfo

/// Conversation row that uses ConversationFullInfo's rich data.
/// PERFORMANCE: Uses cached hierarchy data instead of per-row FFI calls.
/// The cache is preloaded in ConversationsTabView.task for all visible conversations.
private struct ConversationRowFull: View, Equatable {
    static func == (lhs: ConversationRowFull, rhs: ConversationRowFull) -> Bool {
        lhs.conversation == rhs.conversation &&
            lhs.projectTitle == rhs.projectTitle &&
            lhs.isHierarchicallyActive == rhs.isHierarchicallyActive &&
            lhs.pTaggedRecipientInfo == rhs.pTaggedRecipientInfo &&
            lhs.delegationAgentInfos == rhs.delegationAgentInfos &&
            lhs.isPlayingAudio == rhs.isPlayingAudio &&
            lhs.isAudioPlaying == rhs.isAudioPlaying &&
            lhs.showsChevron == rhs.showsChevron
    }

    let conversation: ConversationFullInfo
    let projectTitle: String?
    /// Whether this conversation or any of its descendants has active work
    let isHierarchicallyActive: Bool
    let pTaggedRecipientInfo: AgentAvatarInfo?
    let delegationAgentInfos: [AgentAvatarInfo]
    let isPlayingAudio: Bool
    let isAudioPlaying: Bool
    let showsChevron: Bool
    let onSelect: ((ConversationFullInfo) -> Void)?

    #if os(macOS)
    @Environment(\.openWindow) private var openWindow
    @State private var isHovered = false
    #else
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    @Environment(TenexCoreManager.self) private var coreManager
    @State private var showDelegationTree = false
    #endif

    private var statusColor: Color {
        Color.conversationStatus(for: conversation.thread.statusLabel, isActive: isHierarchicallyActive)
    }

    private var rowContent: some View {
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
                    Text(conversation.thread.title)
                        .font(.headline)
                        .lineLimit(2)

                    if isPlayingAudio {
                        Image(systemName: "speaker.wave.2.fill")
                            .font(.caption)
                            .foregroundStyle(Color.agentBrand)
                            .symbolEffect(.variableColor.iterative, isActive: isAudioPlaying)
                    }

                    Spacer()

                    Text(ConversationFormatters.formatRelativeTime(conversation.thread.effectiveLastActivity))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                // Row 2: Summary or current activity
                HStack(alignment: .top) {
                    // Show current activity if directly active (not hierarchically via descendants)
                    if let activity = conversation.thread.statusCurrentActivity, conversation.isActive {
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
                    } else if let summary = conversation.thread.summary {
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
                        authorInfo: AgentAvatarInfo(name: conversation.author, pubkey: conversation.thread.pubkey),
                        pTaggedRecipientInfo: pTaggedRecipientInfo,
                        otherParticipants: delegationAgentInfos,
                        maxVisibleAvatars: maxVisibleAvatars
                    )

                    Spacer()

                    // Scheduled badge (shows when conversation has scheduled-task-id tag)
                    if conversation.thread.isScheduled {
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

                    if conversation.isActive {
                        HStack(spacing: 3) {
                            ZStack {
                                Circle()
                                    .fill(Color.presenceOnline)
                                    .frame(width: 6, height: 6)
                                Circle()
                                    .stroke(Color.presenceOnline.opacity(0.45), lineWidth: 1.5)
                                    .frame(width: 10, height: 10)
                            }
                            Text("Working")
                        }
                        .font(.caption2.weight(.medium))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Color.presenceOnline.opacity(0.16))
                        .foregroundStyle(Color.presenceOnline)
                        .clipShape(Capsule())
                    }

                    // Status badge
                    if let status = conversation.thread.statusLabel {
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
                            openWindow(id: "delegation-tree", value: conversation.thread.id)
                        } label: {
                            Image(systemName: "arrow.triangle.branch")
                                .font(.system(size: 13))
                                .foregroundStyle(.secondary)
                        }
                        .buttonStyle(.borderless)
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
                        .buttonStyle(.borderless)
                        .help("View delegation tree")
                    }
                    #endif
                }
            }

            if showsChevron {
                Image(systemName: "chevron.right")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
        }
    }

    var body: some View {
        Group {
            if let onSelect {
                rowContent
                    .contentShape(Rectangle())
                    .onTapGesture {
                        onSelect(conversation)
                    }
            } else {
                rowContent
            }
        }
        .padding(.vertical, 10)
        #if os(macOS)
        .onHover { hovering in
            isHovered = hovering
        }
        #else
        .fullScreenCover(isPresented: $showDelegationTree) {
            NavigationStack {
                DelegationTreeView(rootConversationId: conversation.thread.id)
                    .environment(coreManager)
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

            Text(hasFilter ? "Try adjusting your project/time filter" : "Conversations from the last 24h will appear automatically")
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
        .environment(TenexCoreManager())
}
