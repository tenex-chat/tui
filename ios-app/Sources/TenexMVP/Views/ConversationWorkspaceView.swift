import SwiftUI
import UniformTypeIdentifiers

#if os(macOS)
import AppKit
#elseif os(iOS)
import UIKit
#endif

struct ConversationAdaptiveDetailView: View {
    let conversation: ConversationFullInfo
    let onOpenConversationId: ((String) -> Void)?
    let onReferenceConversationRequested: ((ReferenceConversationLaunchPayload) -> Void)?
    @Environment(TenexCoreManager.self) private var coreManager

    init(
        conversation: ConversationFullInfo,
        onOpenConversationId: ((String) -> Void)? = nil,
        onReferenceConversationRequested: ((ReferenceConversationLaunchPayload) -> Void)? = nil
    ) {
        self.conversation = conversation
        self.onOpenConversationId = onOpenConversationId
        self.onReferenceConversationRequested = onReferenceConversationRequested
    }

    var body: some View {
        ConversationWorkspaceView(
            source: .existing(conversation: conversation),
            onReferenceConversationRequested: onReferenceConversationRequested,
            onOpenConversationId: onOpenConversationId
        )
            .environment(coreManager)
    }
}

/// Resolves a conversation by ID and presents the adaptive conversation destination.
/// Useful for entry points that only carry conversation IDs (e.g. inbox items).
struct ConversationByIdAdaptiveDetailView: View {
    let conversationId: String
    let onOpenConversationId: ((String) -> Void)?
    let onReferenceConversationRequested: ((ReferenceConversationLaunchPayload) -> Void)?
    @Environment(TenexCoreManager.self) private var coreManager

    @State private var conversation: ConversationFullInfo?
    @State private var isLoading = true

    init(
        conversationId: String,
        onOpenConversationId: ((String) -> Void)? = nil,
        onReferenceConversationRequested: ((ReferenceConversationLaunchPayload) -> Void)? = nil
    ) {
        self.conversationId = conversationId
        self.onOpenConversationId = onOpenConversationId
        self.onReferenceConversationRequested = onReferenceConversationRequested
    }

    var body: some View {
        Group {
            if let conversation {
                ConversationAdaptiveDetailView(
                    conversation: conversation,
                    onOpenConversationId: onOpenConversationId,
                    onReferenceConversationRequested: onReferenceConversationRequested
                )
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
                isLoading = false
            }
        }
    }

    private func resolveConversation() async {
        isLoading = true

        if let cached = coreManager.conversationById[conversationId] {
            conversation = cached
            isLoading = false
            return
        }

        // A newly published thread can take a moment to flow from NostrDB stream
        // processing into the in-memory conversation index.
        let maxAttempts = 12
        let retryDelayNs: UInt64 = 150_000_000
        var forcedRefresh = false

        for attempt in 0..<maxAttempts {
            if Task.isCancelled { return }

            if let cached = coreManager.conversationById[conversationId] {
                conversation = cached
                isLoading = false
                return
            }

            let fetched = await coreManager.core.getConversationsByIds(conversationIds: [conversationId])
            if let resolved = fetched.first {
                conversation = resolved
                isLoading = false
                return
            }

            if !forcedRefresh {
                _ = await coreManager.core.refresh()
                forcedRefresh = true
            }

            if attempt < maxAttempts - 1 {
                try? await Task.sleep(nanoseconds: retryDelayNs)
            }
        }

        conversation = nil
        isLoading = false
    }
}

private struct RawEventDestination: Identifiable, Hashable {
    let eventId: String
    let json: String

    var id: String { eventId }
}

struct NewThreadComposerSeed: Equatable {
    let launchId: UUID
    let projectId: String
    let agentPubkey: String?
    let initialContent: String
    let textAttachments: [TextAttachment]
    let referenceConversationId: String?

    init(
        launchId: UUID = UUID(),
        projectId: String,
        agentPubkey: String? = nil,
        initialContent: String,
        textAttachments: [TextAttachment],
        referenceConversationId: String?
    ) {
        self.launchId = launchId
        self.projectId = projectId
        self.agentPubkey = agentPubkey
        self.initialContent = initialContent
        self.textAttachments = textAttachments
        self.referenceConversationId = referenceConversationId
    }

    var identity: String {
        launchId.uuidString
    }
}

struct ReferenceConversationLaunchPayload: Equatable, Identifiable {
    let seed: NewThreadComposerSeed

    var id: String {
        seed.identity
    }
}

enum ConversationWorkspaceSource {
    case existing(conversation: ConversationFullInfo)
    case newThread(
        project: Project,
        agentPubkey: String? = nil,
        composerSeed: NewThreadComposerSeed? = nil
    )

    var identity: String {
        switch self {
        case .existing(let conversation):
            return "existing-\(conversation.thread.id)"
        case .newThread(let project, let agentPubkey, let composerSeed):
            return "new-thread-\(project.id)-\(agentPubkey ?? "none")-\(composerSeed?.identity ?? "none")"
        }
    }

    fileprivate var seedConversation: ConversationFullInfo {
        switch self {
        case .existing(let conversation):
            return conversation
        case .newThread(let project, _, _):
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
                isScheduled: false,
                isInterventionReview: false
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

/// Native workspace for a conversation:
/// full Slack-style transcript + inline composer.
struct ConversationWorkspaceView: View {
    let source: ConversationWorkspaceSource
    let onThreadCreated: ((String) -> Void)?
    let onReferenceConversationRequested: ((ReferenceConversationLaunchPayload) -> Void)?
    let onOpenConversationId: ((String) -> Void)?

    @Environment(TenexCoreManager.self) private var coreManager

    private let seedConversation: ConversationFullInfo
    @State private var viewModel: ConversationDetailViewModel
    @State private var selectedDelegationConversation: ConversationFullInfo?
    @State private var visibleMessageWindow: Int = 30
    @State private var transcriptDropProviders: [NSItemProvider]? = nil
    @State private var isTranscriptDropTargeted = false
    @State private var navigationErrorMessage: String?
    @State private var rawEventDestination: RawEventDestination?
    @State private var localReferenceLaunchPayload: ReferenceConversationLaunchPayload?
    /// Shared minute-level transcript clock used by all rows to avoid per-row TimelineView schedulers.
    @State private var transcriptRelativeTimeNow = Date()
    @State private var isTranscriptAtBottom = true
    private let profiler = PerformanceProfiler.shared

    private let bottomAnchorId = "workspace-bottom-anchor"

    init(
        source: ConversationWorkspaceSource,
        onThreadCreated: ((String) -> Void)? = nil,
        onReferenceConversationRequested: ((ReferenceConversationLaunchPayload) -> Void)? = nil,
        onOpenConversationId: ((String) -> Void)? = nil
    ) {
        self.source = source
        self.onThreadCreated = onThreadCreated
        self.onReferenceConversationRequested = onReferenceConversationRequested
        self.onOpenConversationId = onOpenConversationId
        self.seedConversation = source.seedConversation
        _viewModel = State(initialValue: ConversationDetailViewModel(conversation: source.seedConversation))
    }

    init(
        conversation: ConversationFullInfo,
        onOpenConversationId: ((String) -> Void)? = nil
    ) {
        self.init(
            source: .existing(conversation: conversation),
            onOpenConversationId: onOpenConversationId
        )
    }

    private var isNewThreadMode: Bool {
        if case .newThread = source {
            return true
        }
        return false
    }

    private var newThreadAgentPubkey: String? {
        if case .newThread(_, let agentPubkey, _) = source {
            return agentPubkey
        }
        return nil
    }

    private var newThreadComposerSeed: NewThreadComposerSeed? {
        if case .newThread(_, _, let composerSeed) = source {
            return composerSeed
        }
        return nil
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
        case .newThread(let project, _, _):
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

    /// Display items for the transcript: every message renders inline, with `isConsecutive` set
    /// when the previous message has the same pubkey (used to suppress repeated headers).
    private var transcriptDisplayItems: [TranscriptDisplayItem] {
        var items: [TranscriptDisplayItem] = []
        var lastPubkey: String? = nil
        for index in messageIndices {
            let message = transcriptMessages[index]
            items.append(TranscriptDisplayItem(index: index, isConsecutive: lastPubkey == message.pubkey))
            lastPubkey = message.pubkey
        }
        return items
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
        .navigationDestination(item: $selectedDelegationConversation) { delegatedConversation in
            ConversationAdaptiveDetailView(
                conversation: delegatedConversation,
                onOpenConversationId: onOpenConversationId,
                onReferenceConversationRequested: onReferenceConversationRequested
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
        .task(id: "\(source.identity)-minute-tick") {
            await runTranscriptMinuteTicker()
        }
        // Observation isolation: onChange handlers for coreManager live in a
        // separate lightweight view so this body doesn't observe
        // coreManager.conversations/messagesByConversation/streamingBuffers.
        // Without this, ANY change to those dictionaries triggers a full body
        // re-evaluation including the ForEach over 30+ message rows.
        .background(CoreManagerObserver(viewModel: viewModel))
        .sheet(item: $rawEventDestination) { destination in
            RawEventInspectorSheet(
                eventId: destination.eventId,
                json: destination.json
            )
        }
        .sheet(item: $localReferenceLaunchPayload) { payload in
            referenceComposerSheet(for: payload)
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
        transcriptColumn
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(workspaceBackdropColor)
    }

    private var transcriptColumn: some View {
        ScrollViewReader { proxy in
            Group {
                #if os(macOS)
                List {
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
                        .transcriptListRow()
                    }

                    ForEach(transcriptDisplayItems) { item in
                        transcriptRowView(for: item)
                            .transcriptListRow()
                    }

                    if !isNewThreadMode {
                        TranscriptStreamingSection(
                            conversationId: currentConversation.thread.id,
                            lastMessagePubkey: allMessages.last?.pubkey,
                            scrollProxy: proxy
                        )
                        .transcriptListRow()
                    }

                    Color.clear
                        .frame(height: 1)
                        .id(bottomAnchorId)
                        .transcriptListRow()
                }
                .listStyle(.plain)
                .scrollContentBackground(.hidden)
                #else
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
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

                        ForEach(transcriptDisplayItems) { item in
                            transcriptRowView(for: item)
                        }

                        // Streaming buffer lives in a separate view struct so that
                        // coreManager.streamingBuffers observation is isolated here.
                        // The parent transcript body does NOT observe streaming changes.
                        if !isNewThreadMode {
                            TranscriptStreamingSection(
                                conversationId: currentConversation.thread.id,
                                lastMessagePubkey: allMessages.last?.pubkey,
                                scrollProxy: proxy
                            )
                        }

                        Color.clear
                            .frame(height: 1)
                            .id(bottomAnchorId)
                    }
                    .padding()
                    .padding(.bottom, 12)
                }
                .transcriptBottomVisibilityTracking(isAtBottom: $isTranscriptAtBottom)
                #endif
            }
            .background(workspaceBackdropColor)
            #if os(iOS)
            .overlay(alignment: .bottomTrailing) {
                if !isTranscriptAtBottom && !isNewThreadMode {
                    TranscriptJumpToBottomButton {
                        scrollToBottom(proxy, animated: true)
                    }
                    .padding(.trailing, 18)
                    .padding(.bottom, 82)
                    .transition(.scale(scale: 0.88).combined(with: .opacity))
                }
            }
            #endif
            .onAppear {
                logTranscriptRenderBoundary(reason: "appear")
                scrollToBottomAfterLayout(proxy, animated: false)
            }
            .onChange(of: transcriptMessages.count) { _, _ in
                logTranscriptRenderBoundary(reason: "visible-window-change")
            }
            .onChange(of: allMessages.count) { _, _ in
                logTranscriptRenderBoundary(reason: "message-count-change")
            }
            .onChange(of: transcriptMessages.last?.id) { _, _ in
                scrollToBottomAfterLayout(proxy, animated: true)
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            inlineComposer
        }
        #if os(macOS)
        .onDrop(of: [.fileURL], isTargeted: $isTranscriptDropTargeted) { providers in
            guard !providers.isEmpty else { return false }
            transcriptDropProviders = providers
            return true
        }
        .overlay {
            if isTranscriptDropTargeted {
                RoundedRectangle(cornerRadius: 8)
                    .stroke(Color.accentColor, lineWidth: 2)
                    .padding(4)
                    .allowsHitTesting(false)
            }
        }
        #endif
    }

    private func scrollToBottomAfterLayout(_ proxy: ScrollViewProxy, animated: Bool) {
        DispatchQueue.main.async {
            scrollToBottom(proxy, animated: animated)
            DispatchQueue.main.async {
                scrollToBottom(proxy, animated: animated)
            }
        }
    }

    private func scrollToBottom(_ proxy: ScrollViewProxy, animated: Bool) {
        // Update state outside the animation block — state mutations inside withAnimation
        // run inside a CAAnimation context, which can interfere with UIKit focus/keyboard.
        isTranscriptAtBottom = true
        if animated {
            withAnimation(.easeOut(duration: 0.2)) {
                proxy.scrollTo(bottomAnchorId, anchor: .bottom)
            }
        } else {
            proxy.scrollTo(bottomAnchorId, anchor: .bottom)
        }
    }

    private func transcriptRowView(for item: TranscriptDisplayItem) -> some View {
        let message = transcriptMessages[item.index]
        return SlackMessageRow(
            message: message,
            isConsecutive: item.isConsecutive,
            conversationId: currentConversation.thread.id,
            projectId: currentConversation.extractedProjectId,
            relativeTimeNow: transcriptRelativeTimeNow,
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
        #if os(macOS)
        .frame(maxWidth: 800, alignment: .leading)
        #endif
    }

    private var inlineComposer: some View {
        VStack(spacing: 0) {
            MessageComposerView(
                project: project,
                conversationId: isNewThreadMode ? nil : currentConversation.thread.id,
                conversationTitle: isNewThreadMode ? nil : currentConversation.thread.title,
                initialAgentPubkey: isNewThreadMode ? newThreadAgentPubkey : viewModel.lastAgentPubkey,
                initialContent: isNewThreadMode ? newThreadComposerSeed?.initialContent : nil,
                initialTextAttachments: isNewThreadMode ? (newThreadComposerSeed?.textAttachments ?? []) : [],
                referenceConversationId: isNewThreadMode ? newThreadComposerSeed?.referenceConversationId : nil,
                displayStyle: .inline,
                inlineLayoutStyle: .workspace,
                onSend: isNewThreadMode ? { result in
                    onThreadCreated?(result.eventId)
                } : nil,
                onReferenceConversationRequested: isNewThreadMode ? nil : handleReferenceConversationRequested
            )
            .environment(coreManager)
            #if os(macOS)
            .background(
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .fill(workspaceComposerShellColor)
                    .overlay(
                        RoundedRectangle(cornerRadius: 24, style: .continuous)
                            .stroke(workspaceComposerStrokeColor, lineWidth: 1)
                    )
            )
            .clipShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
            .shadow(color: .black.opacity(0.24), radius: 12, x: 0, y: 4)
            .padding(.horizontal, 14)
            .padding(.top, 8)
            .padding(.bottom, 8)
            .frame(maxWidth: 800, alignment: .leading)
            #endif
        }
        #if os(macOS)
        .background(workspaceBackdropColor)
        #endif
    }

    private func initializeWorkspace() async {
        let startedAt = CFAbsoluteTimeGetCurrent()
        visibleMessageWindow = 30
        if !isNewThreadMode {
            viewModel.setCoreManager(coreManager)
            await viewModel.loadData()
        }
        let currentUserPubkey = await coreManager.core.getCurrentUser()?.pubkey
        viewModel.setCurrentUserPubkey(currentUserPubkey)
        // Warm display-name and profile-picture caches so the first transcript
        // render doesn't hit cold FFI lookups for every unique author.
        let uniquePubkeys = Array(Set(viewModel.messages.map(\.pubkey).filter { !$0.isEmpty }))
        coreManager.prefetchProfilePictures(uniquePubkeys)
        for pubkey in uniquePubkeys {
            _ = coreManager.displayName(for: pubkey)
        }
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "workspace initialized source=\(source.identity) messages=\(viewModel.messages.count) children=\(viewModel.childConversations.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 250 ? .error : .info
        )
    }

    @MainActor
    private func runTranscriptMinuteTicker() async {
        transcriptRelativeTimeNow = Date()

        while !Task.isCancelled {
            let now = Date()
            let nextBoundary = Self.nextMinuteBoundary(after: now)
            let sleepSeconds = max(nextBoundary.timeIntervalSince(now), 0.05)
            let sleepNs = UInt64(sleepSeconds * 1_000_000_000)
            do {
                try await Task.sleep(nanoseconds: sleepNs)
            } catch {
                break
            }

            if Task.isCancelled { break }
            transcriptRelativeTimeNow = Date()
        }
    }

    private static func nextMinuteBoundary(after date: Date) -> Date {
        if let interval = Calendar.current.dateInterval(of: .minute, for: date) {
            return interval.end
        }
        return date.addingTimeInterval(60)
    }

    private func logTranscriptRenderBoundary(reason: String) {
        profiler.logEvent(
            "workspace transcript boundary reason=\(reason) conversationId=\(currentConversation.thread.id) visibleRows=\(transcriptMessages.count) totalRows=\(allMessages.count) actionAffordance=context-menu-only timestampMode=minute-tick",
            category: .swiftUI,
            level: allMessages.count >= 400 ? .error : .info
        )
    }

    private func viewRawEvent(for messageId: String) {
        Task {
            let startedAt = CFAbsoluteTimeGetCurrent()
            let rawEvent = await coreManager.core.getRawEventJson(eventId: messageId)

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

    private func handleReferenceConversationRequested(_ payload: ReferenceConversationLaunchPayload) {
        if let onReferenceConversationRequested {
            onReferenceConversationRequested(payload)
        } else {
            localReferenceLaunchPayload = payload
        }
    }

    @ViewBuilder
    private func referenceComposerSheet(for payload: ReferenceConversationLaunchPayload) -> some View {
        if let project = coreManager.projects.first(where: { $0.id == payload.seed.projectId }) {
            MessageComposerView(
                project: project,
                initialAgentPubkey: payload.seed.agentPubkey,
                initialContent: payload.seed.initialContent,
                initialTextAttachments: payload.seed.textAttachments,
                referenceConversationId: payload.seed.referenceConversationId
            )
            .environment(coreManager)
            .tenexModalPresentation(detents: [.large])
        } else {
            ContentUnavailableView(
                "Project Not Found",
                systemImage: "exclamationmark.triangle",
                description: Text("Unable to open a reference composer because the project is unavailable.")
            )
            .padding()
        }
    }

    private func openDelegation(byId delegationId: String) {
        if let cached = coreManager.conversationById[delegationId] {
            navigateToDelegation(cached)
            profiler.logEvent(
                "delegation navigation cache-hit id=\(delegationId)",
                category: .general,
                level: .debug
            )
            return
        }

        if let child = viewModel.childConversation(for: delegationId) {
            navigateToDelegation(child)
            profiler.logEvent(
                "delegation navigation child-cache-hit id=\(delegationId)",
                category: .general,
                level: .debug
            )
            return
        }

        Task {
            let startedAt = CFAbsoluteTimeGetCurrent()
            let convs = await coreManager.core.getConversationsByIds(conversationIds: [delegationId])
            await MainActor.run {
                if let conv = convs.first {
                    navigateToDelegation(conv)
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

    private func navigateToDelegation(_ conversation: ConversationFullInfo) {
        if let onOpenConversationId {
            onOpenConversationId(conversation.thread.id)
        } else {
            selectedDelegationConversation = conversation
        }
    }
}

private extension View {
    func transcriptListRow() -> some View {
        self
            .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 16))
            .listRowSeparator(.hidden)
            .listRowBackground(Color.clear)
    }
}

// MARK: - Observation Isolation

/// Lightweight observer that bridges coreManager changes to the viewModel.
/// Lives as a background view so that ConversationWorkspaceView's body does NOT
/// observe coreManager.conversations / messagesByConversation directly.
/// This eliminates the root cause of the 100% CPU observation cascade:
/// previously, any change to messagesByConversation (including messages for OTHER
/// conversations) triggered a full body re-evaluation of the workspace including
/// the ForEach over 30+ SlackMessageRows.
private struct CoreManagerObserver: View {
    let viewModel: ConversationDetailViewModel
    @Environment(TenexCoreManager.self) private var coreManager

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .onChange(of: coreManager.conversations) { _, _ in
                viewModel.handleConversationsChanged(coreManager.conversations)
            }
            .onChange(of: coreManager.messagesByConversation) { _, _ in
                viewModel.handleMessagesChanged(coreManager.messagesByConversation)
            }
    }
}

/// Extracted streaming buffer rendering so that coreManager.streamingBuffers
/// observation is scoped to this view only. During active streaming, this
/// dictionary changes 10+ times/sec — without isolation, each change would
/// trigger the parent transcript ForEach to re-evaluate all 30+ message rows.
private struct TranscriptStreamingSection: View {
    let conversationId: String
    let lastMessagePubkey: String?
    let scrollProxy: ScrollViewProxy
    @Environment(TenexCoreManager.self) private var coreManager
    @State private var lastScrollAt: CFAbsoluteTime = 0

    var body: some View {
        if let buffer = coreManager.streamingBuffers[conversationId] {
            StreamingMessageRow(
                buffer: buffer,
                isConsecutive: lastMessagePubkey == buffer.agentPubkey,
                agentName: coreManager.displayName(for: buffer.agentPubkey)
            )
            .environment(coreManager)
            .id("streaming-row")
            .onChange(of: buffer.text.count) { _, _ in
                maybeScrollToStreamingRow()
            }
        }
    }

    private func maybeScrollToStreamingRow() {
        let now = CFAbsoluteTimeGetCurrent()
        guard now - lastScrollAt >= 0.10 else { return }
        lastScrollAt = now

        var transaction = Transaction()
        transaction.disablesAnimations = true
        withTransaction(transaction) {
            scrollProxy.scrollTo("streaming-row", anchor: .bottom)
        }
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
