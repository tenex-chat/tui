import SwiftUI

/// Adaptive conversation destination:
/// - macOS: always workspace layout
/// - iPad (regular width): workspace layout
/// - iPhone (compact): existing overview-first detail layout
struct ConversationAdaptiveDetailView: View {
    let conversation: ConversationFullInfo
    @EnvironmentObject private var coreManager: TenexCoreManager
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    var body: some View {
        #if os(macOS)
        ConversationWorkspaceView(source: .existing(conversation: conversation))
            .environmentObject(coreManager)
        #else
        if horizontalSizeClass == .regular {
            ConversationWorkspaceView(source: .existing(conversation: conversation))
                .environmentObject(coreManager)
        } else {
            ConversationDetailView(conversation: conversation)
                .environmentObject(coreManager)
        }
        #endif
    }
}

/// Resolves a conversation by ID and presents the adaptive conversation destination.
/// Useful for entry points that only carry conversation IDs (e.g. inbox items).
struct ConversationByIdAdaptiveDetailView: View {
    let conversationId: String
    @EnvironmentObject private var coreManager: TenexCoreManager

    @State private var conversation: ConversationFullInfo?
    @State private var isLoading = true

    var body: some View {
        Group {
            if let conversation {
                ConversationAdaptiveDetailView(conversation: conversation)
                    .environmentObject(coreManager)
            } else if isLoading {
                ProgressView("Loading conversation...")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ContentUnavailableView(
                    "Conversation Not Found",
                    systemImage: "doc.questionmark",
                    description: Text("Unable to load this conversation.")
                )
            }
        }
        .task(id: conversationId) {
            await resolveConversation()
        }
        .onReceive(coreManager.$conversations) { conversations in
            if let updated = conversations.first(where: { $0.id == conversationId }) {
                conversation = updated
            }
        }
    }

    private func resolveConversation() async {
        if let cached = coreManager.conversations.first(where: { $0.id == conversationId }) {
            conversation = cached
            isLoading = false
            return
        }

        let fetched = await coreManager.safeCore.getConversationsByIds(conversationIds: [conversationId])
        conversation = fetched.first
        isLoading = false
    }
}

private struct SelectedReportDestination: Identifiable, Hashable {
    let report: ReportInfo
    var id: String { "\(report.projectId):\(report.id)" }
}

enum ConversationWorkspaceSource {
    case existing(conversation: ConversationFullInfo)
    case newThread(project: ProjectInfo)

    var identity: String {
        switch self {
        case .existing(let conversation):
            return "existing-\(conversation.id)"
        case .newThread(let project):
            return "new-thread-\(project.id)"
        }
    }

    fileprivate var seedConversation: ConversationFullInfo {
        switch self {
        case .existing(let conversation):
            return conversation
        case .newThread(let project):
            let now = UInt64(Date().timeIntervalSince1970)
            return ConversationFullInfo(
                id: "new-thread-\(project.id)",
                title: "New Conversation",
                author: "You",
                authorPubkey: "",
                summary: nil,
                messageCount: 0,
                lastActivity: now,
                effectiveLastActivity: now,
                parentId: nil,
                status: "draft",
                currentActivity: nil,
                isActive: false,
                isArchived: false,
                hasChildren: false,
                projectATag: "31922:local:\(project.id)",
                isScheduled: false,
                pTags: []
            )
        }
    }
}

/// Native split workspace for a conversation:
/// - Left: full Slack-style transcript + inline composer
/// - Right: metadata inspector (status/todos/delegations/reports)
struct ConversationWorkspaceView: View {
    let source: ConversationWorkspaceSource
    let onThreadCreated: ((String) -> Void)?

    @EnvironmentObject private var coreManager: TenexCoreManager

    private let seedConversation: ConversationFullInfo
    @StateObject private var viewModel: ConversationDetailViewModel
    @State private var inspectorVisible = true
    @State private var selectedDelegationConversation: ConversationFullInfo?
    @State private var selectedReportDestination: SelectedReportDestination?
    @State private var availableAgents: [OnlineAgentInfo] = []
    @State private var isAtBottom = true
    @State private var scrollViewHeight: CGFloat = 0
    @State private var navigationErrorMessage: String?

    private let bottomAnchorId = "workspace-bottom-anchor"
    private let bottomThreshold: CGFloat = 60

    init(source: ConversationWorkspaceSource, onThreadCreated: ((String) -> Void)? = nil) {
        self.source = source
        self.onThreadCreated = onThreadCreated
        self.seedConversation = source.seedConversation
        _viewModel = StateObject(wrappedValue: ConversationDetailViewModel(conversation: source.seedConversation))
    }

    init(conversation: ConversationFullInfo) {
        self.init(source: .existing(conversation: conversation))
    }

    private var isNewThreadMode: Bool {
        if case .newThread = source {
            return true
        }
        return false
    }

    private var currentConversation: ConversationFullInfo {
        switch source {
        case .existing(let conversation):
            return coreManager.conversations.first(where: { $0.id == conversation.id }) ?? conversation
        case .newThread:
            return seedConversation
        }
    }

    private var project: ProjectInfo? {
        switch source {
        case .existing:
            return coreManager.projects.first { $0.id == currentConversation.extractedProjectId }
        case .newThread(let project):
            return coreManager.projects.first { $0.id == project.id } ?? project
        }
    }

    private var conversationTitle: String {
        isNewThreadMode ? "New Conversation" : currentConversation.title
    }

    private var transcriptMessages: [MessageInfo] {
        isNewThreadMode ? [] : viewModel.messages
    }

    private var messagesWithConsecutive: [(message: MessageInfo, isConsecutive: Bool)] {
        transcriptMessages.enumerated().map { index, msg in
            let isConsecutive = index > 0 && transcriptMessages[index - 1].authorNpub == msg.authorNpub
            return (msg, isConsecutive)
        }
    }

    private var streamingBuffer: StreamingBuffer? {
        guard !isNewThreadMode else { return nil }
        return coreManager.streamingBuffers[currentConversation.id]
    }

    private var streamingTextCount: Int? {
        streamingBuffer?.text.count
    }

    private var lastAgentPubkey: String? {
        guard !isNewThreadMode else { return nil }
        return LastAgentFinder.findLastAgentPubkey(
            messages: transcriptMessages,
            availableAgents: availableAgents,
            npubToHex: { Bech32.npubToHex($0) }
        )
    }

    private var statusText: String {
        isNewThreadMode ? "draft" : viewModel.currentStatus
    }

    private var isActiveState: Bool {
        isNewThreadMode ? false : viewModel.currentIsActive
    }

    private var currentActivityText: String? {
        isNewThreadMode ? nil : viewModel.currentActivity
    }

    private var runtimeText: String {
        guard !isNewThreadMode else { return "0s" }
        return viewModel.formattedRuntime.isEmpty ? "0s" : viewModel.formattedRuntime
    }

    var body: some View {
        workspaceLayout
        .navigationTitle(conversationTitle)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                inspectorToggleToolbarButton
            }
        }
        .navigationDestination(item: $selectedDelegationConversation) { delegatedConversation in
            ConversationAdaptiveDetailView(conversation: delegatedConversation)
                .environmentObject(coreManager)
        }
        .navigationDestination(item: $selectedReportDestination) { destination in
            ReportsTabDetailView(
                report: destination.report,
                project: coreManager.projects.first { $0.id == destination.report.projectId }
            )
            .environmentObject(coreManager)
        }
        .task(id: source.identity) {
            await initializeWorkspace()
        }
        .onReceive(coreManager.$onlineAgents) { _ in
            refreshAvailableAgents()
        }
        .onReceive(coreManager.$conversations) { _ in
            refreshAvailableAgents()
        }
        .alert("Navigation Error", isPresented: Binding(
            get: { navigationErrorMessage != nil },
            set: { newValue in
                if !newValue { navigationErrorMessage = nil }
            }
        )) {
            Button("OK", role: .cancel) {
                navigationErrorMessage = nil
            }
        } message: {
            Text(navigationErrorMessage ?? "")
        }
    }

    @ViewBuilder
    private var workspaceLayout: some View {
        #if os(macOS)
        HSplitView {
            transcriptColumn
                .frame(minWidth: 560, maxWidth: .infinity, maxHeight: .infinity)

            if inspectorVisible {
                inspectorColumn
                    .frame(minWidth: 320, idealWidth: 360, maxWidth: 440, maxHeight: .infinity)
            }
        }
        #else
        HStack(spacing: 0) {
            transcriptColumn
                .frame(maxWidth: .infinity, maxHeight: .infinity)

            if inspectorVisible {
                Divider()
                inspectorColumn
                    .frame(width: 360)
                    .frame(maxHeight: .infinity)
                    .transition(.move(edge: .trailing).combined(with: .opacity))
            }
        }
        #endif
    }

    private var inspectorToggleToolbarButton: some View {
        Button {
            withAnimation(.easeInOut(duration: 0.2)) {
                inspectorVisible.toggle()
            }
        } label: {
            Image(systemName: "sidebar.right")
        }
        .accessibilityLabel(inspectorVisible ? "Hide Inspector" : "Show Inspector")
    }

    private var transcriptColumn: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(messagesWithConsecutive, id: \.message.id) { item in
                        SlackMessageRow(
                            message: item.message,
                            isConsecutive: item.isConsecutive,
                            conversationId: currentConversation.id,
                            projectId: currentConversation.extractedProjectId,
                            onDelegationTap: { delegationId in
                                openDelegation(byId: delegationId)
                            }
                        )
                        .environmentObject(coreManager)
                        .id(item.message.id)
                    }

                    if let buffer = streamingBuffer {
                        StreamingMessageRow(
                            buffer: buffer,
                            isConsecutive: transcriptMessages.last?.authorNpub == buffer.agentPubkey
                        )
                        .environmentObject(coreManager)
                        .id("streaming-row")
                    }

                    Color.clear
                        .frame(height: 1)
                        .id(bottomAnchorId)
                        .background(
                            GeometryReader { geo in
                                Color.clear.preference(
                                    key: WorkspaceBottomAnchorOffsetKey.self,
                                    value: geo.frame(in: .named("workspaceScroll")).maxY
                                )
                            }
                        )
                }
                .padding()
                .padding(.bottom, 12)
            }
            .coordinateSpace(name: "workspaceScroll")
            .background(
                GeometryReader { geo in
                    Color.clear.preference(
                        key: WorkspaceScrollViewHeightKey.self,
                        value: geo.size.height
                    )
                }
            )
            .onAppear {
                if let lastMessage = transcriptMessages.last {
                    proxy.scrollTo(lastMessage.id, anchor: .bottom)
                }
            }
            .onChange(of: transcriptMessages.last?.id) { _, _ in
                guard let lastMessage = transcriptMessages.last else { return }
                if isAtBottom {
                    DispatchQueue.main.async {
                        withAnimation(.easeOut(duration: 0.2)) {
                            proxy.scrollTo(lastMessage.id, anchor: .bottom)
                        }
                    }
                }
            }
            .onChange(of: streamingTextCount) { _, _ in
                if isAtBottom {
                    DispatchQueue.main.async {
                        withAnimation(.easeOut(duration: 0.2)) {
                            proxy.scrollTo("streaming-row", anchor: .bottom)
                        }
                    }
                }
            }
            .onPreferenceChange(WorkspaceScrollViewHeightKey.self) { height in
                scrollViewHeight = height
            }
            .onPreferenceChange(WorkspaceBottomAnchorOffsetKey.self) { bottomY in
                let distanceFromBottom = bottomY - scrollViewHeight
                isAtBottom = distanceFromBottom <= bottomThreshold
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            inlineComposer
        }
    }

    private var inlineComposer: some View {
        VStack(spacing: 0) {
            MessageComposerView(
                project: project,
                conversationId: isNewThreadMode ? nil : currentConversation.id,
                conversationTitle: isNewThreadMode ? nil : currentConversation.title,
                initialAgentPubkey: isNewThreadMode ? nil : lastAgentPubkey,
                displayStyle: .inline,
                inlineLayoutStyle: .workspace,
                onSend: isNewThreadMode ? { result in
                    onThreadCreated?(result.eventId)
                } : nil
            )
            .environmentObject(coreManager)
            .background(
                RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .fill(Color.systemBackground)
                    .overlay(
                        RoundedRectangle(cornerRadius: 16, style: .continuous)
                            .stroke(Color.secondary.opacity(0.15), lineWidth: 1)
                    )
            )
            .clipShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
            .padding(.horizontal, 14)
            .padding(.top, 12)
            .padding(.bottom, 12)
        }
        .background(.ultraThinMaterial)
    }

    private var inspectorColumn: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                inspectorPrimaryMetadata
                    .padding(.bottom, 6)
                if viewModel.aggregatedTodoStats.hasTodos {
                    inspectorTodoSection
                }
                if !viewModel.delegations.isEmpty {
                    inspectorDelegationsSection
                }
                if !viewModel.referencedReports.isEmpty {
                    inspectorReportsSection
                }
            }
            .padding(14)
        }
        .background(Color.systemGroupedBackground.opacity(0.32))
    }

    private var inspectorPrimaryMetadata: some View {
        WorkspaceInspectorCard(title: nil, tone: .primary) {
            VStack(alignment: .leading, spacing: 12) {
                Text(conversationTitle)
                    .font(.headline)
                    .foregroundStyle(.primary)
                    .fixedSize(horizontal: false, vertical: true)

                if let summary = currentConversation.summary, !summary.isEmpty {
                    Text(summary)
                        .font(.subheadline)
                        .foregroundStyle(Color.secondary.opacity(0.86))
                        .padding(.top, 4)
                        .fixedSize(horizontal: false, vertical: true)
                }

                HStack(alignment: .center, spacing: 10) {
                    HStack(spacing: 8) {
                        StatusBadge(status: statusText, isActive: isActiveState)

                        if let project {
                            ProjectBadge(projectTitle: project.title)
                        }
                    }

                    Text(runtimeText)
                        .font(.callout.weight(.medium))
                        .monospacedDigit()
                        .foregroundStyle(Color.secondary.opacity(0.9))
                        .frame(maxWidth: .infinity, alignment: .trailing)
                }

                if let currentActivity = currentActivityText, !currentActivity.isEmpty {
                    Text(currentActivity)
                        .font(.caption)
                        .foregroundStyle(Color.statusWaiting.opacity(0.72))
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }

    private var inspectorTodoSection: some View {
        WorkspaceInspectorCard(title: "Todos", tone: .secondary) {
            VStack(alignment: .leading, spacing: 12) {
                TodoProgressView(stats: viewModel.aggregatedTodoStats)

                if !viewModel.todoState.items.isEmpty {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(viewModel.todoState.items) { todo in
                            TodoRowView(todo: todo)
                        }
                    }
                }
            }
        }
    }

    private var inspectorDelegationsSection: some View {
        WorkspaceInspectorCard(title: "Delegations (\(viewModel.delegations.count))", tone: .secondary) {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(viewModel.delegations) { delegation in
                    if let delegatedConversation = delegationConversation(for: delegation) {
                        NavigationLink {
                            ConversationAdaptiveDetailView(conversation: delegatedConversation)
                                .environmentObject(coreManager)
                        } label: {
                            delegationRowLabel(delegation)
                        }
                        .buttonStyle(.plain)
                    } else {
                        Button {
                            openDelegation(byId: delegation.conversationId)
                        } label: {
                            delegationRowLabel(delegation)
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
    }

    private var inspectorReportsSection: some View {
        WorkspaceInspectorCard(title: "Reports (\(viewModel.referencedReports.count))", tone: .secondary) {
            if viewModel.referencedReports.isEmpty {
                Text("No report references found.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                VStack(alignment: .leading, spacing: 8) {
                    ForEach(viewModel.referencedReports) { reportRef in
                        Button {
                            openReport(reportRef)
                        } label: {
                            HStack(spacing: 8) {
                                Image(systemName: "doc.text")
                                    .font(.subheadline)
                                    .foregroundStyle(Color.agentBrand)

                                VStack(alignment: .leading, spacing: 2) {
                                    Text(reportRef.title)
                                        .font(.subheadline)
                                        .foregroundStyle(.primary)
                                        .lineLimit(2)

                                    Text(reportRef.slug)
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                }

                                Spacer()
                                Image(systemName: "chevron.right")
                                    .font(.caption2)
                                    .foregroundStyle(.tertiary)
                            }
                            .padding(.vertical, 4)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
    }

    private func initializeWorkspace() async {
        if !isNewThreadMode {
            viewModel.setCoreManager(coreManager)
            await viewModel.loadData()
        }
        refreshAvailableAgents()
    }

    private func refreshAvailableAgents() {
        if let projectId = project?.id {
            availableAgents = coreManager.onlineAgents[projectId] ?? []
        } else {
            availableAgents = []
        }
    }

    private func openDelegation(byId delegationId: String) {
        if let cached = coreManager.conversations.first(where: { $0.id == delegationId }) {
            selectedDelegationConversation = cached
            return
        }

        if let child = viewModel.childConversation(for: delegationId) {
            selectedDelegationConversation = child
            return
        }

        Task {
            let convs = await coreManager.safeCore.getConversationsByIds(conversationIds: [delegationId])
            await MainActor.run {
                if let conv = convs.first {
                    selectedDelegationConversation = conv
                } else {
                    navigationErrorMessage = "Unable to load the selected delegated conversation."
                }
            }
        }
    }

    private func openReport(_ reportRef: ReferencedReportItem) {
        if let report = reportRef.report ?? resolveReport(aTag: reportRef.aTag) {
            selectedReportDestination = SelectedReportDestination(report: report)
        } else {
            navigationErrorMessage = "Unable to load the selected report."
        }
    }

    private func resolveReport(aTag: String) -> ReportInfo? {
        coreManager.reports.first { report in
            guard let authorHex = Bech32.npubToHex(report.authorNpub) else { return false }
            return "30023:\(authorHex):\(report.id)" == aTag
        }
    }

    private func delegationConversation(for delegation: DelegationItem) -> ConversationFullInfo? {
        if let child = viewModel.childConversation(for: delegation.conversationId) {
            return child
        }
        return coreManager.conversations.first { $0.id == delegation.conversationId }
    }

    @ViewBuilder
    private func delegationRowLabel(_ delegation: DelegationItem) -> some View {
        HStack(spacing: 8) {
            AgentAvatarView(
                agentName: delegation.recipient,
                pubkey: delegation.recipientPubkey,
                size: 24,
                fontSize: 9,
                showBorder: false
            )
            .environmentObject(coreManager)

            VStack(alignment: .leading, spacing: 2) {
                Text(AgentNameFormatter.format(delegation.recipient))
                    .font(.subheadline)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Text(delegation.messagePreview)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }

            Spacer()
            Image(systemName: "chevron.right")
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 4)
        .contentShape(Rectangle())
    }
}

private struct WorkspaceInspectorCard<Content: View>: View {
    enum Tone {
        case primary
        case secondary
    }

    let title: String?
    let tone: Tone
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 11) {
            if let title, !title.isEmpty {
                Text(title)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(Color.primary.opacity(0.66))
                    .textCase(.uppercase)
            }

            content
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 13)
        .background(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .fill(cardFill)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .stroke(cardBorder, lineWidth: 1)
                .allowsHitTesting(false)
        )
    }

    private var cardFill: Color {
        switch tone {
        case .primary:
            return Color.systemBackground.opacity(0.9)
        case .secondary:
            return Color.systemBackground.opacity(0.78)
        }
    }

    private var cardBorder: Color {
        switch tone {
        case .primary:
            return Color.secondary.opacity(0.2)
        case .secondary:
            return Color.secondary.opacity(0.14)
        }
    }
}

private struct WorkspaceScrollViewHeightKey: PreferenceKey {
    static var defaultValue: CGFloat = 0

    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

private struct WorkspaceBottomAnchorOffsetKey: PreferenceKey {
    static var defaultValue: CGFloat = 0

    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}
