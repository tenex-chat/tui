import SwiftUI

// MARK: - Todo Row View

/// Compact todo item row
struct TodoRowView: View {
    let todo: TodoItem

    private var statusIcon: String {
        switch todo.status {
        case .done, .completed:
            return "checkmark.circle.fill"
        case .inProgress:
            return "circle.circle.fill"
        case .skipped:
            return "xmark.circle.fill"
        case .pending:
            return "circle"
        }
    }

    private var statusColor: Color {
        switch todo.status {
        case .done, .completed:
            return .green
        case .inProgress:
            return .blue
        case .skipped:
            return .gray
        case .pending:
            return .secondary
        }
    }

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: statusIcon)
                .font(.body)
                .foregroundStyle(statusColor)
                .frame(width: 20)

            VStack(alignment: .leading, spacing: 2) {
                Text(todo.title)
                    .font(.callout)
                    .foregroundStyle(.primary)
                    .fixedSize(horizontal: false, vertical: true)

                if let skipReason = todo.skipReason, !skipReason.isEmpty {
                    Text("Skipped: \(skipReason)")
                        .font(.caption)
                        .foregroundStyle(Color.todoSkipped)
                        .italic()
                }
            }

            Spacer()
        }
    }
}

// MARK: - Todo Progress View

/// Shows todo progress as a bar (incomplete) or completion pill (complete)
struct TodoProgressView: View {
    let stats: AggregateTodoStats

    var body: some View {
        if stats.isComplete {
            // Completion pill
            TodoCompletionPill(count: stats.totalCount, style: .large)
        } else {
            // Progress bar with fraction
            HStack(spacing: 8) {
                // Progress bar
                GeometryReader { geometry in
                    ZStack(alignment: .leading) {
                        // Background
                        RoundedRectangle(cornerRadius: 4)
                            .fill(Color.secondary.opacity(0.2))

                        // Progress
                        RoundedRectangle(cornerRadius: 4)
                            .fill(Color.accentColor)
                            .frame(width: geometry.size.width * CGFloat(stats.completedCount) / CGFloat(max(1, stats.totalCount)))
                    }
                }
                .frame(width: 60, height: 8)

                // Fraction label
                Text("\(stats.completedCount)/\(stats.totalCount)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
        }
    }
}

/// Reusable completion pill showing checkmark and count
struct TodoCompletionPill: View {
    let count: Int
    let style: Style

    enum Style {
        case large  // For section headers
        case small  // For inline badges
    }

    var body: some View {
        HStack(spacing: style == .large ? 4 : 2) {
            Image(systemName: "checkmark.circle.fill")
                .font(style == .large ? .caption : .caption2)
            Text("\(count)")
                .font(style == .large ? .caption : .caption2)
                .fontWeight(style == .large ? .medium : .regular)
        }
        .foregroundStyle(Color.todoDone)
        .if(style == .large) { view in
            view
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(Color.todoDoneBackground)
                .clipShape(Capsule())
        }
    }
}

// MARK: - View Extension for Conditional Modifiers

extension View {
    @ViewBuilder
    func `if`<Content: View>(_ condition: Bool, transform: (Self) -> Content) -> some View {
        if condition {
            transform(self)
        } else {
            self
        }
    }
}

struct WorkingActivityBadge: View {
    var body: some View {
        HStack(spacing: 4) {
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
}

// MARK: - Delegation Row View

/// Tappable delegation row without card styling
struct DelegationRowView: View {
    @Environment(TenexCoreManager.self) var coreManager
    let delegation: DelegationItem
    let isWorking: Bool
    let onTap: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            // Smaller avatar for compact look
            AgentAvatarView(agentName: delegation.recipient, pubkey: delegation.recipientPubkey, size: 32, fontSize: 11)
                .environment(coreManager)

            VStack(alignment: .leading, spacing: 2) {
                // Agent name - callout size
                Text(AgentNameFormatter.format(delegation.recipient))
                    .font(.callout)
                    .fontWeight(.semibold)

                // Preview - caption size (equivalent to text-xs)
                HStack(spacing: 6) {
                    Text(delegation.messagePreview)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)

                    // Todo badge if delegation has todos
                    if let stats = delegation.todoStats {
                        TodoBadgeView(stats: stats)
                    }
                }
            }

            Spacer()

            if isWorking {
                WorkingActivityBadge()
            }

            Image(systemName: "chevron.right")
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 10)
        .contentShape(Rectangle())
        .onTapGesture {
            onTap()
        }
    }
}

// MARK: - Todo Badge View

/// Small badge showing todo progress for delegations
struct TodoBadgeView: View {
    let stats: AggregateTodoStats

    var body: some View {
        if stats.isComplete {
            TodoCompletionPill(count: stats.totalCount, style: .small)
        } else {
            Text("\(stats.completedCount)/\(stats.totalCount)")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .monospacedDigit()
        }
    }
}

// MARK: - Supporting Views

/// Preview sheet for delegation when child conversation not found
struct DelegationPreviewSheet: View {
    let delegation: DelegationItem
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            VStack(spacing: 20) {
                SharedAgentAvatar(agentName: delegation.recipient, size: 60, fontSize: 20)

                Text(AgentNameFormatter.format(delegation.recipient))
                    .font(.title2)
                    .fontWeight(.bold)

                Text(delegation.messagePreview)
                    .font(.body)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal)

                Spacer()
            }
            .padding(.top, 40)
            .navigationTitle("Delegation")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }
}

/// Full conversation chat view sheet with Slack-style messages
struct FullConversationSheet: View {
    enum PresentationStyle {
        case sheet
        case embedded
    }

    let conversation: ConversationFullInfo
    let messages: [Message]
    let presentationStyle: PresentationStyle
    @Environment(TenexCoreManager.self) var coreManager
    @Environment(\.dismiss) private var dismiss

    @State private var selectedDelegation: String?
    @State private var showComposer = false
    @State private var availableAgents: [ProjectAgent] = []
    @State private var lastStreamingAutoScrollAt: CFAbsoluteTime = 0
    @State private var visibleMessageLimit = 240

    private let bottomAnchorId = "full-conversation-bottom"
    private let initialMessageWindowSize = 240
    private let messageWindowStepSize = 240

    init(
        conversation: ConversationFullInfo,
        messages: [Message],
        presentationStyle: PresentationStyle = .sheet
    ) {
        self.conversation = conversation
        self.messages = messages
        self.presentationStyle = presentationStyle
    }

    private var isEmbedded: Bool {
        presentationStyle == .embedded
    }

    /// Keep row iteration lightweight by avoiding tuple arrays with full Message copies.
    private var messageStartIndex: Int {
        guard usesMessageWindowing else {
            return 0
        }
        return max(messages.count - visibleMessageLimit, 0)
    }

    /// Keep row iteration lightweight by avoiding tuple arrays with full Message copies.
    private var messageIndices: Range<Int> {
        messageStartIndex..<messages.count
    }

    private var hiddenMessageCount: Int {
        messageStartIndex
    }

    private var usesMessageWindowing: Bool {
        #if os(macOS)
        return messages.count > initialMessageWindowSize
        #else
        return false
        #endif
    }

    /// Find the project for this conversation
    private var project: Project? {
        coreManager.projects.first { $0.id == conversation.extractedProjectId }
    }

    /// Find the last agent that spoke in the conversation (like TUI's get_most_recent_agent_from_conversation)
    /// Returns hex pubkey format for use with MessageComposerView
    private var lastAgentPubkey: String? {
        LastAgentFinder.findLastAgentPubkey(
            messages: messages,
            availableAgents: availableAgents
        )
    }

    private var transcriptBackdropColor: Color {
        #if os(macOS)
        return .conversationWorkspaceBackdropMac
        #else
        return .systemBackground
        #endif
    }

    var body: some View {
        Group {
            if isEmbedded {
                conversationContent
            } else {
                NavigationStack {
                    conversationContent
                        .toolbar {
                            ToolbarItem(placement: .confirmationAction) {
                                Button("Done") { dismiss() }
                            }
                        }
                }
            }
        }
        .navigationTitle("Full Conversation")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        #if os(iOS)
        .sheet(item: $selectedDelegation) { delegationId in
            DelegationSheetFromId(delegationId: delegationId)
                .environment(coreManager)
                .tenexModalPresentation(detents: [.large])
        }
        .sheet(isPresented: $showComposer) {
            MessageComposerView(
                project: project,
                conversationId: conversation.thread.id,
                conversationTitle: conversation.thread.title,
                initialAgentPubkey: lastAgentPubkey
            )
            .environment(coreManager)
        }
        #endif
    }

    private var conversationContent: some View {
        ZStack(alignment: .bottom) {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        if hiddenMessageCount > 0 {
                            Button {
                                loadOlderMessages()
                            } label: {
                                HStack(spacing: 6) {
                                    Image(systemName: "clock.arrow.circlepath")
                                    Text("Load \(hiddenMessageCount) earlier message\(hiddenMessageCount == 1 ? "" : "s")")
                                }
                                .font(.subheadline.weight(.semibold))
                                .padding(.vertical, 8)
                                .padding(.horizontal, 12)
                                .frame(maxWidth: .infinity, alignment: .center)
                            }
                            .buttonStyle(.borderedProminent)
                            .padding(.bottom, 12)
                        }

                        ForEach(messageIndices, id: \.self) { index in
                            let message = messages[index]
                            SlackMessageRow(
                                message: message,
                                isConsecutive: index > 0 && messages[index - 1].pubkey == message.pubkey,
                                conversationId: conversation.thread.id,
                                projectId: conversation.extractedProjectId,
                                authorDisplayName: coreManager.displayName(for: message.pubkey),
                                directedRecipientsText: message.pTags.isEmpty ? "" : message.pTags
                                    .map { AgentNameFormatter.format(coreManager.displayName(for: $0)) }
                                    .map { "@\($0)" }
                                    .joined(separator: ", "),
                                onDelegationTap: { delegationId in
                                    selectedDelegation = delegationId
                                }
                            )
                            .equatable()
                            .environment(coreManager)
                            .id(message.id)
                        }

                        if let buffer = coreManager.streamingBuffers[conversation.thread.id] {
                            StreamingMessageRow(
                                buffer: buffer,
                                isConsecutive: messages.last?.pubkey == buffer.agentPubkey,
                                agentName: coreManager.displayName(for: buffer.agentPubkey)
                            )
                            .environment(coreManager)
                            .id("streaming-row")
                        }
                    }
                    .padding()
                    .padding(.bottom, isEmbedded ? 12 : 80)

                    Color.clear
                        .frame(height: 1)
                        .id(bottomAnchorId)
                }
                .onAppear {
                    if usesMessageWindowing {
                        visibleMessageLimit = min(messages.count, max(initialMessageWindowSize, visibleMessageLimit))
                    }

                    PerformanceProfiler.shared.logEvent(
                        "fullConversation appear conversationId=\(conversation.thread.id) totalMessages=\(messages.count) visibleMessages=\(messageIndices.count) hiddenMessages=\(hiddenMessageCount)",
                        category: .swiftUI,
                        level: messages.count >= 400 ? .error : .info
                    )

                    DispatchQueue.main.async {
                        proxy.scrollTo(bottomAnchorId, anchor: .bottom)
                    }
                }
                .onChange(of: messages.count) { oldCount, newCount in
                    guard usesMessageWindowing else { return }
                    let growth = max(0, newCount - oldCount)
                    if growth > 0 {
                        visibleMessageLimit = min(newCount, max(initialMessageWindowSize, visibleMessageLimit + growth))
                    }
                }
                .onChange(of: messages.last?.id) { _, _ in
                    DispatchQueue.main.async {
                        withAnimation(.easeOut(duration: 0.2)) {
                            proxy.scrollTo(bottomAnchorId, anchor: .bottom)
                        }
                    }
                }
                .onChange(of: coreManager.streamingBuffers[conversation.thread.id]?.text.count) { _, _ in
                    DispatchQueue.main.async {
                        maybeScrollToStreamingRow(with: proxy)
                    }
                }
                .task {
                    // Load available agents for the project to determine last agent
                    if let projectId = project?.id {
                        availableAgents = coreManager.onlineAgents[projectId] ?? []
                    }
                }
                .onChange(of: coreManager.onlineAgents) { _, _ in
                    if let projectId = project?.id {
                        availableAgents = coreManager.onlineAgents[projectId] ?? []
                    }
                }
            }
            .background(transcriptBackdropColor)

            if !isEmbedded {
                // Floating compose button (sheet mode)
                Button {
                    showComposer = true
                } label: {
                    HStack(spacing: 8) {
                        Image(systemName: "square.and.pencil")
                        Text("Reply")
                    }
                    .font(.headline)
                    .foregroundStyle(.white)
                    .padding(.horizontal, 20)
                    .padding(.vertical, 12)
                    .background(Color.agentBrand)
                    .clipShape(Capsule())
                    .shadow(color: .black.opacity(0.15), radius: 8, x: 0, y: 4)
                }
                .buttonStyle(.borderless)
                .padding(.bottom, 16)
            }
        }
        .background(transcriptBackdropColor)
        #if os(macOS)
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if isEmbedded {
                inlineComposer
            }
        }
        #endif
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

    private func loadOlderMessages() {
        let nextLimit = min(messages.count, visibleMessageLimit + messageWindowStepSize)
        guard nextLimit != visibleMessageLimit else { return }
        visibleMessageLimit = nextLimit
        PerformanceProfiler.shared.logEvent(
            "fullConversation loadOlder conversationId=\(conversation.thread.id) visibleMessages=\(messageIndices.count) hiddenMessages=\(hiddenMessageCount)",
            category: .swiftUI,
            level: .info
        )
    }

    #if os(macOS)
    private var inlineComposer: some View {
        VStack(spacing: 0) {
            MessageComposerView(
                project: project,
                conversationId: conversation.thread.id,
                conversationTitle: conversation.thread.title,
                initialAgentPubkey: lastAgentPubkey,
                displayStyle: .inline,
                inlineLayoutStyle: .workspace
            )
            .environment(coreManager)
            .background(
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .fill(Color.conversationComposerShellMac)
                    .overlay(
                        RoundedRectangle(cornerRadius: 24, style: .continuous)
                            .stroke(Color.conversationComposerStrokeMac, lineWidth: 1)
                    )
            )
            .clipShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
            .shadow(color: .black.opacity(0.24), radius: 12, x: 0, y: 4)
            .padding(.horizontal, 14)
            .padding(.top, 8)
            .padding(.bottom, 8)
        }
        .background(transcriptBackdropColor)
    }
    #endif
}

// MARK: - Delegation Sheet From ID

/// Helper view to load and display a delegation conversation by ID
private struct DelegationSheetFromId: View {
    let delegationId: String
    @Environment(TenexCoreManager.self) var coreManager
    @Environment(\.dismiss) private var dismiss

    @State private var conversation: ConversationFullInfo?
    @State private var isLoading = true

    var body: some View {
        NavigationStack {
            Group {
                if let conv = conversation {
                    ConversationDetailView(conversation: conv)
                        .environment(coreManager)
                } else {
                    VStack(spacing: 16) {
                        ContentUnavailableView(
                            "Conversation Not Found",
                            systemImage: "doc.questionmark",
                            description: Text("Unable to load delegation details.")
                        )
                        if isLoading {
                            ProgressView()
                                .padding(.top, 8)
                        }
                    }
                }
            }
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
        .task {
            await loadConversation()
        }
    }

    private func loadConversation() async {
        isLoading = true
        let convs = await coreManager.safeCore.getConversationsByIds(conversationIds: [delegationId])
        await MainActor.run {
            conversation = convs.first
            isLoading = false
        }
    }
}

// MARK: - String Identifiable for Sheet

extension String: @retroactive Identifiable {
    public var id: String { self }
}

// MARK: - ConversationFullInfo Extensions

extension ConversationFullInfo: Identifiable {
    public var id: String { thread.id }
}

extension ConversationFullInfo {
    /// Extracts the project ID (d-tag) from the projectATag.
    ///
    /// The projectATag follows the format `kind:pubkey:d-tag`.
    /// This property extracts the d-tag portion (everything after the first two colon-separated parts).
    /// Returns an empty string if projectATag is empty or malformed.
    var extractedProjectId: String {
        guard !projectATag.isEmpty else { return "" }
        // projectATag format: "kind:pubkey:d-tag"
        // We need to drop the first two components (kind and pubkey) and join the rest
        // This handles d-tags that might contain colons themselves
        let parts = projectATag.split(separator: ":", omittingEmptySubsequences: false)
        guard parts.count >= 3 else { return "" }
        return parts.dropFirst(2).joined(separator: ":")
    }
}

#Preview {
    ConversationDetailView(conversation: ConversationFullInfo(
        thread: Thread(
            id: "test-123",
            title: "Implement User Authentication",
            content: "",
            pubkey: "abc123def456",
            lastActivity: UInt64(Date().timeIntervalSince1970) - 3600,
            effectiveLastActivity: UInt64(Date().timeIntervalSince1970) - 60,
            statusLabel: "In Progress",
            statusCurrentActivity: "Reviewing security requirements",
            summary: "Add OAuth2 authentication flow with Google and GitHub providers",
            parentConversationId: nil,
            pTags: [],
            askEvent: nil,
            isScheduled: false
        ),
        author: "architect-orchestrator",
        messageCount: 15,
        isActive: true,
        isArchived: false,
        hasChildren: true,
        projectATag: "project-123"
    ))
    .environment(TenexCoreManager())
}
