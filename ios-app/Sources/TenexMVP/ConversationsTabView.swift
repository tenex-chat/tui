import SwiftUI

struct ConversationsTabView: View {
    @Environment(TenexCoreManager.self) var coreManager
    @ObservedObject private var audioPlayer = AudioNotificationPlayer.shared
    let layoutMode: ConversationsLayoutMode
    private let selectedConversationBindingOverride: Binding<ConversationFullInfo?>?
    private let newConversationProjectIdBindingOverride: Binding<String?>?
    private let newConversationAgentPubkeyBindingOverride: Binding<String?>?
    private let onShowDiagnosticsInApp: (() -> Void)?
    private let onLogout: (() async -> Void)?

    @State private var showDiagnostics = false
    @State private var showAISettings = false
    @State private var showAudioQueue = false
    @State private var audioNotificationsEnabled = false
    @State private var showStats = false
    @State private var selectedConversationState: ConversationFullInfo?
    @State private var newConversationProjectIdState: String?
    @State private var newConversationAgentPubkeyState: String?
    @State private var newConversationSeedState: NewThreadComposerSeed?
    @State private var detailNavigationPath: [String] = []
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
        newConversationAgentPubkey: Binding<String?>? = nil,
        onShowDiagnosticsInApp: (() -> Void)? = nil,
        onLogout: (() async -> Void)? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedConversationBindingOverride = selectedConversation
        self.newConversationProjectIdBindingOverride = newConversationProjectId
        self.newConversationAgentPubkeyBindingOverride = newConversationAgentPubkey
        self.onShowDiagnosticsInApp = onShowDiagnosticsInApp
        self.onLogout = onLogout
    }

    private var selectedConversationBinding: Binding<ConversationFullInfo?> {
        selectedConversationBindingOverride ?? $selectedConversationState
    }

    private var newConversationProjectIdBinding: Binding<String?> {
        newConversationProjectIdBindingOverride ?? $newConversationProjectIdState
    }

    private var newConversationAgentPubkeyBinding: Binding<String?> {
        newConversationAgentPubkeyBindingOverride ?? $newConversationAgentPubkeyState
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

        if !coreManager.appFilterShowArchived {
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
                newConversationSeedState = nil
            }
        }
        .onChange(of: coreManager.appFilterShowArchived) { _, _ in
            rebuildHierarchy()
        }
        .onChange(of: coreManager.projects) { _, _ in
            rebuildProjectCaches()
        }
        .onChange(of: coreManager.projectOnlineStatus) { _, _ in
            rebuildProjectCaches()
        }
        .onChange(of: selectedConversationBinding.wrappedValue?.thread.id) { oldId, newId in
            if oldId != newId {
                detailNavigationPath.removeAll()
            }
            guard newId != nil else { return }
            newConversationProjectIdBinding.wrappedValue = nil
            newConversationAgentPubkeyBinding.wrappedValue = nil
            pendingCreatedConversationId = nil
            newConversationSeedState = nil
        }
        .onChange(of: newConversationProjectIdBinding.wrappedValue) { _, newProjectId in
            guard let seed = newConversationSeedState else { return }
            if newProjectId != seed.projectId || newConversationAgentPubkeyBinding.wrappedValue != seed.agentPubkey {
                newConversationSeedState = nil
            }
        }
        .onChange(of: newConversationAgentPubkeyBinding.wrappedValue) { _, newAgentPubkey in
            guard let seed = newConversationSeedState else { return }
            if newAgentPubkey != seed.agentPubkey {
                newConversationSeedState = nil
            }
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
            DiagnosticsView(coreManager: coreManager)
                .tenexModalPresentation(detents: [.large])
            #endif
        }
        .sheet(isPresented: $showAudioQueue) {
            AudioQueueSheet()
                .environment(coreManager)
        }
        .sheet(isPresented: $showAISettings) {
            AppSettingsView(defaultSection: .audio, onLogout: onLogout)
                .tenexModalPresentation(detents: [.large])
                #if os(macOS)
                .frame(minWidth: 500, idealWidth: 520, minHeight: 500, idealHeight: 600)
                #endif
        }
        .sheet(isPresented: $showStats) {
            StatsView()
                .environment(coreManager)
                .tenexModalPresentation(detents: [.large])
        }
        .sheet(item: $projectForNewConversation) { selectedProject in
            // TODO(#modal-composer-deprecation): migrate this modal composer entry point to inline flow.
            MessageComposerView(project: selectedProject.project)
                .environment(coreManager)
                .tenexModalPresentation(detents: [.large])
        }
        .sheet(item: $conversationToReference) { conversation in
            let projectId = TenexCoreManager.projectId(fromATag: conversation.projectATag)
            if let project = coreManager.projects.first(where: { $0.id == projectId }) {
                // TODO(#modal-composer-deprecation): migrate this modal composer entry point to inline flow.
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
        .tenexListSurfaceBackground()
        .refreshable {
            await coreManager.manualRefresh()
        }
    }

    @ViewBuilder
    private var conversationDetailContent: some View {
        if let conversation = selectedConversationBinding.wrappedValue {
            ConversationAdaptiveDetailView(
                conversation: conversation,
                onOpenConversationId: pushConversationInDetailStack,
                onReferenceConversationRequested: handleReferenceConversationLaunch
            )
                .environment(coreManager)
            .id(conversation.thread.id)
        } else if let newProjectId = newConversationProjectIdBinding.wrappedValue,
                  let project = coreManager.projects.first(where: { $0.id == newProjectId }) {
            let composerSeed =
                (newConversationSeedState?.projectId == newProjectId
                && newConversationSeedState?.agentPubkey == newConversationAgentPubkeyBinding.wrappedValue)
                ? newConversationSeedState
                : nil
            ConversationWorkspaceView(
                source: .newThread(
                    project: project,
                    agentPubkey: newConversationAgentPubkeyBinding.wrappedValue,
                    composerSeed: composerSeed
                ),
                onThreadCreated: handleThreadCreated
            )
            .environment(coreManager)
            .id("new-thread-\(project.id)-\(newConversationAgentPubkeyBinding.wrappedValue ?? "none")-\(composerSeed?.identity ?? "none")")
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
        NavigationStack(path: $detailNavigationPath) {
            conversationDetailContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .navigationDestination(for: String.self) { conversationId in
            ConversationByIdAdaptiveDetailView(
                conversationId: conversationId,
                onOpenConversationId: pushConversationInDetailStack
            )
            .environment(coreManager)
            .id("delegated-\(conversationId)")
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
        .tenexListSurfaceBackground()
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
                        onSelect: nil,
                        onToggleArchive: { target in
                            toggleArchive(target)
                        }
                    )
                    .equatable()
                    .tag(conversation)
                    .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                        Button(role: .destructive) {
                            toggleArchive(conversation)
                        } label: {
                            Label(
                                conversation.isArchived ? "Unarchive" : "Archive",
                                systemImage: conversation.isArchived ? "tray.and.arrow.up" : "archivebox"
                            )
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
                        },
                        onToggleArchive: { target in
                            toggleArchive(target)
                        }
                    )
                    .equatable()
                    .tag(conversation)
                    .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                        Button(role: .destructive) {
                            toggleArchive(conversation)
                        } label: {
                            Label(
                                conversation.isArchived ? "Unarchive" : "Archive",
                                systemImage: conversation.isArchived ? "tray.and.arrow.up" : "archivebox"
                            )
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
                    onSelect: nil,
                    onToggleArchive: { target in
                        toggleArchive(target)
                    }
                )
                .equatable()
                .tag(conversation)
                .contextMenu {
                    Button {
                        toggleArchive(conversation)
                    } label: {
                        Label(
                            conversation.isArchived ? "Unarchive" : "Archive",
                            systemImage: conversation.isArchived ? "tray.and.arrow.up" : "archivebox"
                        )
                    }

                    Button {
                        conversationToReference = conversation
                    } label: {
                        Label("Reference", systemImage: "link")
                    }
                }
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

            if let onLogout {
                Divider()

                Button(role: .destructive) {
                    Task {
                        await onLogout()
                    }
                } label: {
                    Label("Log Out", systemImage: "rectangle.portrait.and.arrow.right")
                }
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
            newConversationAgentPubkeyBinding.wrappedValue = nil
            newConversationSeedState = nil
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
        newConversationAgentPubkeyBinding.wrappedValue = nil
        newConversationSeedState = nil
        pendingCreatedConversationId = nil
    }

    private func handleReferenceConversationLaunch(_ payload: ReferenceConversationLaunchPayload) {
        selectedConversationBinding.wrappedValue = nil
        newConversationProjectIdBinding.wrappedValue = payload.seed.projectId
        newConversationAgentPubkeyBinding.wrappedValue = payload.seed.agentPubkey
        newConversationSeedState = payload.seed
        pendingCreatedConversationId = nil
        detailNavigationPath.removeAll()
    }

    private func pushConversationInDetailStack(_ conversationId: String) {
        guard !conversationId.isEmpty else { return }
        guard selectedConversationBinding.wrappedValue != nil else { return }
        if detailNavigationPath.last == conversationId { return }
        detailNavigationPath.append(conversationId)
    }

    private func toggleArchive(_ conversation: ConversationFullInfo) {
        _ = coreManager.safeCore.toggleConversationArchived(conversationId: conversation.thread.id)
        scheduleHierarchyRebuild()
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
                .adaptiveGlassButtonStyle()
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
