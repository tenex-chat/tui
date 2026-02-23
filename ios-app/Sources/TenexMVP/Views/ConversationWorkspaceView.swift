import SwiftUI

#if os(macOS)
import AppKit
#elseif os(iOS)
import UIKit
#endif

/// Adaptive conversation destination:
/// - macOS: always workspace layout
/// - iPad (regular width): workspace layout
/// - iPhone (compact): existing overview-first detail layout
struct ConversationAdaptiveDetailView: View {
    let conversation: ConversationFullInfo
    @Environment(TenexCoreManager.self) private var coreManager
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    var body: some View {
        #if os(macOS)
        ConversationWorkspaceView(source: .existing(conversation: conversation))
            .environment(coreManager)
        #else
        if horizontalSizeClass == .regular {
            ConversationWorkspaceView(source: .existing(conversation: conversation))
                .environment(coreManager)
        } else {
            ConversationDetailView(conversation: conversation)
                .environment(coreManager)
        }
        #endif
    }
}

/// Resolves a conversation by ID and presents the adaptive conversation destination.
/// Useful for entry points that only carry conversation IDs (e.g. inbox items).
struct ConversationByIdAdaptiveDetailView: View {
    let conversationId: String
    @Environment(TenexCoreManager.self) private var coreManager

    @State private var conversation: ConversationFullInfo?
    @State private var isLoading = true

    var body: some View {
        Group {
            if let conversation {
                ConversationAdaptiveDetailView(conversation: conversation)
                    .environment(coreManager)
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
        .onChange(of: coreManager.conversations) { _, _ in
            if let updated = coreManager.conversationById[conversationId] {
                conversation = updated
            }
        }
    }

    private func resolveConversation() async {
        if let cached = coreManager.conversationById[conversationId] {
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
    let report: Report
    var id: String { "\(report.projectATag):\(report.slug)" }
}

private struct RawEventDestination: Identifiable, Hashable {
    let eventId: String
    let json: String

    var id: String { eventId }
}

enum ConversationWorkspaceSource {
    case existing(conversation: ConversationFullInfo)
    case newThread(project: Project)

    var identity: String {
        switch self {
        case .existing(let conversation):
            return "existing-\(conversation.thread.id)"
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
            let thread = Thread(
                id: "new-thread-\(project.id)",
                title: "New Conversation",
                content: "",
                pubkey: "",
                lastActivity: now,
                effectiveLastActivity: now,
                statusLabel: "draft",
                statusCurrentActivity: nil,
                summary: nil,
                hashtags: [],
                parentConversationId: nil,
                pTags: [],
                askEvent: nil,
                isScheduled: false
            )
            return ConversationFullInfo(
                thread: thread,
                author: "You",
                messageCount: 0,
                isActive: false,
                isArchived: false,
                hasChildren: false,
                projectATag: "31922:local:\(project.id)"
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

    @Environment(TenexCoreManager.self) private var coreManager

    private let seedConversation: ConversationFullInfo
    @StateObject private var viewModel: ConversationDetailViewModel
    @State private var inspectorUserPreference = true
    /// Tracks the workspace width so the inspector auto-hides when space is tight.
    @State private var workspaceWidth: CGFloat = .infinity
    @State private var selectedDelegationConversation: ConversationFullInfo?
    @State private var selectedReportDestination: SelectedReportDestination?
    @State private var availableAgents: [ProjectAgent] = []
    @State private var visibleMessageWindow: Int = 30
    /// Defers ForEach rendering until after the @Published initialization storm settles.
    /// Without this, 15-20 body re-evaluations each process 30+ rows during loadData().
    @State private var isTranscriptReady = false
    @State private var cachedLastAgentPubkey: String?
    @State private var lastStreamingAutoScrollAt: CFAbsoluteTime = 0
    @State private var navigationErrorMessage: String?
    @State private var rawEventDestination: RawEventDestination?
    private let profiler = PerformanceProfiler.shared

    private let bottomAnchorId = "workspace-bottom-anchor"

    init(source: ConversationWorkspaceSource, onThreadCreated: ((String) -> Void)? = nil) {
        self.source = source
        self.onThreadCreated = onThreadCreated
        self.seedConversation = source.seedConversation
        _viewModel = StateObject(wrappedValue: ConversationDetailViewModel(conversation: source.seedConversation))
    }

    init(conversation: ConversationFullInfo) {
        self.init(source: .existing(conversation: conversation))
    }

    /// Minimum workspace width to show both transcript and inspector side-by-side.
    /// Transcript minWidth (560) + inspector minWidth (320) + some breathing room.
    private static let inspectorWidthThreshold: CGFloat = 900

    /// Inspector shows only when the user wants it AND there's enough horizontal space.
    private var inspectorVisible: Bool {
        inspectorUserPreference && workspaceWidth >= Self.inspectorWidthThreshold
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
            return coreManager.conversationById[conversation.thread.id] ?? conversation
        case .newThread:
            return seedConversation
        }
    }

    private var project: Project? {
        switch source {
        case .existing:
            return coreManager.projects.first { $0.id == currentConversation.extractedProjectId }
        case .newThread(let project):
            return coreManager.projects.first { $0.id == project.id } ?? project
        }
    }

    private var conversationTitle: String {
        isNewThreadMode ? "New Conversation" : currentConversation.thread.title
    }

    private var allMessages: [Message] {
        isNewThreadMode ? [] : viewModel.messages
    }

    /// Windowed slice of messages — only the last N are rendered initially.
    /// Older messages load progressively as the user scrolls up.
    private var transcriptMessages: ArraySlice<Message> {
        let all = allMessages
        guard all.count > visibleMessageWindow else { return all[...] }
        return all.suffix(visibleMessageWindow)
    }

    private var hasOlderMessages: Bool {
        allMessages.count > visibleMessageWindow
    }

    /// Keep row iteration lightweight by avoiding tuple arrays with full Message copies.
    private var messageIndices: Range<Int> {
        transcriptMessages.indices
    }

    private var streamingBuffer: StreamingBuffer? {
        guard !isNewThreadMode else { return nil }
        return coreManager.streamingBuffers[currentConversation.thread.id]
    }

    private var streamingTextCount: Int? {
        streamingBuffer?.text.count
    }

    private var lastAgentPubkey: String? {
        cachedLastAgentPubkey
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

    private var workspaceBackdropColor: Color {
        #if os(macOS)
        return .conversationWorkspaceBackdropMac
        #else
        return .systemBackground
        #endif
    }

    private var workspaceSurfaceColor: Color {
        #if os(macOS)
        return .conversationWorkspaceSurfaceMac
        #else
        return .systemBackground
        #endif
    }

    private var workspaceBorderColor: Color {
        #if os(macOS)
        return .conversationWorkspaceBorderMac
        #else
        return Color.secondary.opacity(0.15)
        #endif
    }

    private var workspaceComposerShellColor: Color {
        #if os(macOS)
        return .conversationComposerShellMac
        #else
        return workspaceSurfaceColor
        #endif
    }

    private var workspaceComposerStrokeColor: Color {
        #if os(macOS)
        return .conversationComposerStrokeMac
        #else
        return workspaceBorderColor
        #endif
    }

    var body: some View {
        workspaceLayout
        .navigationTitle(conversationTitle)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .automatic) {
                inspectorToggleButton
            }
        }
        .navigationDestination(item: $selectedDelegationConversation) { delegatedConversation in
            ConversationAdaptiveDetailView(conversation: delegatedConversation)
                .environment(coreManager)
        }
        .navigationDestination(item: $selectedReportDestination) { destination in
            ReportsTabDetailView(
                report: destination.report,
                project: coreManager.projects.first { $0.id == TenexCoreManager.projectId(fromATag: destination.report.projectATag) }
            )
            .environment(coreManager)
        }
        .task(id: source.identity) {
            profiler.logEvent(
                "workspace task start source=\(source.identity) mode=\(isNewThreadMode ? "new-thread" : "existing")",
                category: .general
            )
            await initializeWorkspace()
        }
        .onChange(of: coreManager.onlineAgents) { _, _ in
            refreshAvailableAgents()
        }
        .onChange(of: coreManager.conversations) { _, _ in
            refreshAvailableAgents()
            viewModel.handleConversationsChanged(coreManager.conversations)
        }
        .onChange(of: coreManager.messagesByConversation) { _, _ in
            viewModel.handleMessagesChanged(coreManager.messagesByConversation)
        }
        .onChange(of: coreManager.reports) { _, _ in
            viewModel.handleReportsChanged()
        }
        .onChange(of: viewModel.messages.count) { _, _ in
            recomputeLastAgentPubkey()
        }
        .sheet(item: $rawEventDestination) { destination in
            RawEventInspectorSheet(
                eventId: destination.eventId,
                json: destination.json
            )
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
        .background(workspaceBackdropColor)
    }

    @ViewBuilder
    private var workspaceLayout: some View {
        Group {
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
        .background(
            GeometryReader { geo in
                Color.clear.preference(key: WorkspaceWidthKey.self, value: geo.size.width)
            }
        )
        .onPreferenceChange(WorkspaceWidthKey.self) { width in
            workspaceWidth = width
        }
        .background(workspaceBackdropColor)
    }

    private var inspectorToggleButton: some View {
        Button {
            withAnimation(.easeInOut(duration: 0.2)) {
                inspectorUserPreference.toggle()
            }
        } label: {
            Image(systemName: "sidebar.right")
        }
        .accessibilityLabel(inspectorUserPreference ? "Hide Inspector" : "Show Inspector")
    }

    private var transcriptColumn: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    if isTranscriptReady {
                        if hasOlderMessages {
                            Button {
                                visibleMessageWindow += 30
                            } label: {
                                Text("Load earlier messages")
                                    .font(.subheadline)
                                    .foregroundStyle(.secondary)
                                    .frame(maxWidth: .infinity)
                                    .padding(.vertical, 12)
                            }
                            .buttonStyle(.borderless)
                            .onAppear {
                                visibleMessageWindow += 30
                            }
                        }

                        ForEach(messageIndices, id: \.self) { index in
                            let message = transcriptMessages[index]
                            SlackMessageRow(
                                message: message,
                                isConsecutive: index > transcriptMessages.startIndex && transcriptMessages[index - 1].pubkey == message.pubkey,
                                conversationId: currentConversation.thread.id,
                                projectId: currentConversation.extractedProjectId,
                                authorDisplayName: coreManager.displayName(for: message.pubkey),
                                directedRecipientsText: message.pTags.isEmpty ? "" : message.pTags
                                    .map { AgentNameFormatter.format(coreManager.displayName(for: $0)) }
                                    .map { "@\($0)" }
                                    .joined(separator: ", "),
                                onDelegationTap: { delegationId in
                                    openDelegation(byId: delegationId)
                                },
                                onViewRawEvent: { messageId in
                                    viewRawEvent(for: messageId)
                                }
                            )
                            .equatable()
                            .environment(coreManager)
                            .id(message.id)
                        }

                        if let buffer = streamingBuffer {
                            StreamingMessageRow(
                                buffer: buffer,
                                isConsecutive: allMessages.last?.pubkey == buffer.agentPubkey,
                                agentName: coreManager.displayName(for: buffer.agentPubkey)
                            )
                            .environment(coreManager)
                            .id("streaming-row")
                        }
                    }

                    Color.clear
                        .frame(height: 1)
                        .id(bottomAnchorId)
                }
                .padding()
                .padding(.bottom, 12)
                #if os(macOS)
                .frame(maxWidth: 800, alignment: .leading)
                #endif
            }
            .background(workspaceBackdropColor)
            .onChange(of: isTranscriptReady) { _, ready in
                if ready, let lastMessage = transcriptMessages.last {
                    DispatchQueue.main.async {
                        proxy.scrollTo(lastMessage.id, anchor: .bottom)
                    }
                }
            }
            .onChange(of: transcriptMessages.last?.id) { _, _ in
                guard isTranscriptReady, let lastMessage = transcriptMessages.last else { return }
                DispatchQueue.main.async {
                    withAnimation(.easeOut(duration: 0.2)) {
                        proxy.scrollTo(lastMessage.id, anchor: .bottom)
                    }
                }
            }
            .onChange(of: streamingTextCount) { _, _ in
                guard isTranscriptReady else { return }
                DispatchQueue.main.async {
                    maybeScrollToStreamingRow(with: proxy)
                }
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
                conversationId: isNewThreadMode ? nil : currentConversation.thread.id,
                conversationTitle: isNewThreadMode ? nil : currentConversation.thread.title,
                initialAgentPubkey: isNewThreadMode ? nil : lastAgentPubkey,
                displayStyle: .inline,
                inlineLayoutStyle: .workspace,
                onSend: isNewThreadMode ? { result in
                    onThreadCreated?(result.eventId)
                } : nil
            )
            .environment(coreManager)
            .background(
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .fill(workspaceComposerShellColor)
                    .overlay(
                        RoundedRectangle(cornerRadius: 24, style: .continuous)
                            .stroke(workspaceComposerStrokeColor, lineWidth: 1)
                    )
            )
            .clipShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
            #if os(macOS)
            .shadow(color: .black.opacity(0.24), radius: 12, x: 0, y: 4)
            #endif
            .padding(.horizontal, 14)
            .padding(.top, 8)
            .padding(.bottom, 8)
            #if os(macOS)
            .frame(maxWidth: 800, alignment: .leading)
            #endif
        }
        #if os(macOS)
        .background(workspaceBackdropColor)
        #else
        .background(.ultraThinMaterial)
        #endif
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
        #if os(macOS)
        .background(workspaceBackdropColor)
        #else
        .background(Color.systemGroupedBackground.opacity(0.32))
        #endif
    }

    private var inspectorPrimaryMetadata: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(conversationTitle)
                .font(.headline)
                .foregroundStyle(.primary)
                .fixedSize(horizontal: false, vertical: true)

            if let summary = currentConversation.thread.summary, !summary.isEmpty {
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

    private var inspectorTodoSection: some View {
        VStack(alignment: .leading, spacing: 11) {
            Text("Todos")
                .font(.caption.weight(.semibold))
                .foregroundStyle(Color.primary.opacity(0.66))
                .textCase(.uppercase)

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
                    let isWorking = viewModel.delegationActivityByConversationId[delegation.conversationId] ?? false
                    if let delegatedConversation = delegationConversation(for: delegation) {
                        NavigationLink {
                            ConversationAdaptiveDetailView(conversation: delegatedConversation)
                                .environment(coreManager)
                        } label: {
                            delegationRowLabel(delegation, isWorking: isWorking)
                        }
                        .buttonStyle(.plain)
                    } else {
                        Button {
                            openDelegation(byId: delegation.conversationId)
                        } label: {
                            delegationRowLabel(delegation, isWorking: isWorking)
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
    }

    private var inspectorReportsSection: some View {
        VStack(alignment: .leading, spacing: 11) {
            Text("Reports (\(viewModel.referencedReports.count))")
                .font(.caption.weight(.semibold))
                .foregroundStyle(Color.primary.opacity(0.66))
                .textCase(.uppercase)

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
        let startedAt = CFAbsoluteTimeGetCurrent()
        visibleMessageWindow = 30
        isTranscriptReady = false
        if !isNewThreadMode {
            viewModel.setCoreManager(coreManager)
            await viewModel.loadData()
        }
        refreshAvailableAgents()
        // Warm caches for message avatars and display names before rendering the transcript.
        let uniquePubkeys = Array(Set(viewModel.messages.map(\.pubkey).filter { !$0.isEmpty }))
        coreManager.prefetchProfilePictures(uniquePubkeys)
        for pubkey in uniquePubkeys {
            _ = coreManager.displayName(for: pubkey)
        }
        // Flip AFTER all @Published properties have settled.
        // This ensures the ForEach is empty during the initialization storm
        // (15-20 body re-evaluations), then renders once with final data.
        isTranscriptReady = true
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "workspace initialized source=\(source.identity) messages=\(viewModel.messages.count) children=\(viewModel.childConversations.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 250 ? .error : .info
        )
    }

    private func refreshAvailableAgents() {
        if let projectId = project?.id {
            availableAgents = coreManager.onlineAgents[projectId] ?? []
        } else {
            availableAgents = []
        }
        recomputeLastAgentPubkey()
    }

    private func recomputeLastAgentPubkey() {
        guard !isNewThreadMode else {
            cachedLastAgentPubkey = nil
            return
        }
        cachedLastAgentPubkey = LastAgentFinder.findLastAgentPubkey(
            messages: viewModel.messages,
            availableAgents: availableAgents
        )
    }

    private func maybeScrollToStreamingRow(with proxy: ScrollViewProxy) {
        let now = CFAbsoluteTimeGetCurrent()
        guard now - lastStreamingAutoScrollAt >= 0.10 else { return }
        lastStreamingAutoScrollAt = now

        var transaction = Transaction()
        transaction.disablesAnimations = true
        withTransaction(transaction) {
            proxy.scrollTo("streaming-row", anchor: .bottom)
        }
    }

    private func viewRawEvent(for messageId: String) {
        Task {
            let startedAt = CFAbsoluteTimeGetCurrent()
            let rawEvent = await coreManager.safeCore.getRawEventJson(eventId: messageId)

            await MainActor.run {
                if let rawEvent, !rawEvent.isEmpty {
                    rawEventDestination = RawEventDestination(
                        eventId: messageId,
                        json: rawEvent
                    )
                } else {
                    navigationErrorMessage = "Unable to load raw event for message \(shortId(messageId))."
                }

                let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
                profiler.logEvent(
                    "raw event lookup message=\(shortId(messageId)) found=\(rawEvent != nil) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                    category: .general,
                    level: elapsedMs >= 120 ? .error : .info
                )
            }
        }
    }

    private func shortId(_ value: String, prefix: Int = 8, suffix: Int = 6) -> String {
        guard value.count > prefix + suffix else { return value }
        return "\(value.prefix(prefix))...\(value.suffix(suffix))"
    }

    private func openDelegation(byId delegationId: String) {
        if let cached = coreManager.conversationById[delegationId] {
            selectedDelegationConversation = cached
            profiler.logEvent(
                "delegation navigation cache-hit id=\(delegationId)",
                category: .general,
                level: .debug
            )
            return
        }

        if let child = viewModel.childConversation(for: delegationId) {
            selectedDelegationConversation = child
            profiler.logEvent(
                "delegation navigation child-cache-hit id=\(delegationId)",
                category: .general,
                level: .debug
            )
            return
        }

        Task {
            let startedAt = CFAbsoluteTimeGetCurrent()
            let convs = await coreManager.safeCore.getConversationsByIds(conversationIds: [delegationId])
            await MainActor.run {
                if let conv = convs.first {
                    selectedDelegationConversation = conv
                } else {
                    navigationErrorMessage = "Unable to load the selected delegated conversation."
                }
                let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
                profiler.logEvent(
                    "delegation navigation fetch id=\(delegationId) found=\(convs.first != nil) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                    category: .general,
                    level: elapsedMs >= 120 ? .error : .info
                )
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

    private func resolveReport(aTag: String) -> Report? {
        coreManager.reports.first { report in
            "30023:\(report.author):\(report.slug)" == aTag
        }
    }

    private func delegationConversation(for delegation: DelegationItem) -> ConversationFullInfo? {
        if let child = viewModel.childConversation(for: delegation.conversationId) {
            return child
        }
        return coreManager.conversationById[delegation.conversationId]
    }

    @ViewBuilder
    private func delegationRowLabel(_ delegation: DelegationItem, isWorking: Bool) -> some View {
        HStack(spacing: 8) {
            AgentAvatarView(
                agentName: delegation.recipient,
                pubkey: delegation.recipientPubkey,
                size: 24,
                fontSize: 9,
                showBorder: false
            )
            .environment(coreManager)

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

            if isWorking {
                WorkingActivityBadge()
            }

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
        #if os(macOS)
        switch tone {
        case .primary:
            return Color.conversationWorkspaceSurfaceMac.opacity(0.95)
        case .secondary:
            return Color.conversationWorkspaceSurfaceMac.opacity(0.82)
        }
        #else
        switch tone {
        case .primary:
            return Color.systemBackground.opacity(0.9)
        case .secondary:
            return Color.systemBackground.opacity(0.78)
        }
        #endif
    }

    private var cardBorder: Color {
        #if os(macOS)
        switch tone {
        case .primary:
            return Color.conversationWorkspaceBorderMac.opacity(0.92)
        case .secondary:
            return Color.conversationWorkspaceBorderMac.opacity(0.74)
        }
        #else
        switch tone {
        case .primary:
            return Color.secondary.opacity(0.2)
        case .secondary:
            return Color.secondary.opacity(0.14)
        }
        #endif
    }
}

private struct WorkspaceWidthKey: PreferenceKey {
    static let defaultValue: CGFloat = .infinity
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

private struct RawEventInspectorSheet: View {
    let eventId: String
    let json: String

    @Environment(\.dismiss) private var dismiss
    @State private var copyFeedbackMessage: String?
    @State private var highlightedJson: AttributedString

    init(eventId: String, json: String) {
        self.eventId = eventId
        self.json = json
        _highlightedJson = State(initialValue: JsonSyntaxHighlighter.highlight(json))
    }

    var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 12) {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Event ID")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Text(eventId)
                        .font(.caption.monospaced())
                        .textSelection(.enabled)
                }

                ScrollView([.vertical, .horizontal], showsIndicators: true) {
                    Text(highlightedJson)
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(10)
                }
                .background(Color.systemGray6)
                .clipShape(RoundedRectangle(cornerRadius: 10))
            }
            .padding()
            .navigationTitle("Raw Event")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Close") {
                        dismiss()
                    }
                }

                ToolbarItem(placement: .automatic) {
                    Button {
                        copyToClipboard(json)
                        copyFeedbackMessage = "Raw event JSON copied."
                    } label: {
                        Label("Copy JSON", systemImage: "doc.on.doc")
                    }
                }
            }
            .alert(
                "Raw Event",
                isPresented: Binding(
                    get: { copyFeedbackMessage != nil },
                    set: { shown in
                        if !shown {
                            copyFeedbackMessage = nil
                        }
                    }
                )
            ) {
                Button("OK", role: .cancel) { }
            } message: {
                Text(copyFeedbackMessage ?? "")
            }
        }
        .frame(minWidth: 680, minHeight: 460)
    }

    private func copyToClipboard(_ text: String) {
        #if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        #elseif os(iOS)
        UIPasteboard.general.string = text
        #endif
    }
}

private enum JsonSyntaxHighlighter {
    static func highlight(_ source: String) -> AttributedString {
        var highlighted = AttributedString(source)
        highlighted.font = .caption.monospaced()
        highlighted.foregroundColor = .primary

        apply(pattern: #"[{}\[\],:]"#, color: .secondary, to: &highlighted, source: source)
        apply(pattern: #"\b(?:true|false|null)\b"#, color: .purple, to: &highlighted, source: source)
        apply(
            pattern: #"-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?"#,
            color: .orange,
            to: &highlighted,
            source: source
        )
        apply(pattern: #""(?:\\.|[^"\\])*""#, color: .green, to: &highlighted, source: source)
        apply(pattern: #""(?:\\.|[^"\\])*"(?=\s*:)"#, color: .blue, to: &highlighted, source: source)

        return highlighted
    }

    private static func apply(
        pattern: String,
        color: Color,
        to highlighted: inout AttributedString,
        source: String
    ) {
        guard let regex = try? NSRegularExpression(pattern: pattern) else {
            return
        }

        let fullRange = NSRange(source.startIndex..<source.endIndex, in: source)
        for match in regex.matches(in: source, range: fullRange) {
            guard let sourceRange = Range(match.range, in: source),
                  let highlightedRange = attributedRange(
                      for: sourceRange,
                      in: highlighted,
                      source: source
                  )
            else {
                continue
            }
            highlighted[highlightedRange].foregroundColor = color
        }
    }

    private static func attributedRange(
        for sourceRange: Range<String.Index>,
        in highlighted: AttributedString,
        source: String
    ) -> Range<AttributedString.Index>? {
        let startOffset = source.distance(from: source.startIndex, to: sourceRange.lowerBound)
        let endOffset = source.distance(from: source.startIndex, to: sourceRange.upperBound)
        let start = highlighted.index(highlighted.startIndex, offsetByCharacters: startOffset)
        let end = highlighted.index(highlighted.startIndex, offsetByCharacters: endOffset)
        return start..<end
    }
}
