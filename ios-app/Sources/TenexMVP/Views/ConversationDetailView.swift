import SwiftUI

// MARK: - Conversation Detail View

/// Main detail view for a conversation showing overview, agents, runtime, latest reply, and delegations
/// Based on the approved wireframe at wireframes/ios-conversation-detail.html
struct ConversationDetailView: View {
    let conversation: ConversationFullInfo
    @EnvironmentObject var coreManager: TenexCoreManager

    @StateObject private var viewModel: ConversationDetailViewModel
    @State private var selectedDelegation: DelegationItem?
    @State private var showFullConversation = false

    /// Initialize the view with a conversation and core manager
    /// Note: coreManager is passed explicitly to support @StateObject initialization
    init(conversation: ConversationFullInfo, coreManager: TenexCoreManager? = nil) {
        self.conversation = conversation
        // Create the view model with a placeholder coreManager initially
        // The actual coreManager will be set via onAppear when using @EnvironmentObject
        self._viewModel = StateObject(wrappedValue: ConversationDetailViewModel(conversation: conversation))
    }

    var body: some View {
        contentView
            .navigationTitle(conversation.title)
            .navigationBarTitleDisplayMode(.inline)
            .task {
                await initializeAndLoad()
            }
            .refreshable {
                await viewModel.loadData()
            }
            .sheet(item: $selectedDelegation) { delegation in
                if let childConv = viewModel.childConversation(for: delegation.conversationId) {
                    NavigationStack {
                        ConversationDetailView(conversation: childConv)
                            .environmentObject(coreManager)
                            .toolbar {
                                ToolbarItem(placement: .topBarTrailing) {
                                    Button("Done") {
                                        selectedDelegation = nil
                                    }
                                }
                            }
                    }
                    .presentationDetents([.large])
                    .presentationDragIndicator(.visible)
                } else {
                    DelegationPreviewSheet(delegation: delegation)
                        .presentationDetents([.medium, .large])
                        .presentationDragIndicator(.visible)
                }
            }
            .sheet(isPresented: $showFullConversation) {
                FullConversationSheet(
                    conversation: conversation,
                    messages: viewModel.messages
                )
                .environmentObject(coreManager)
                .presentationDetents([.large])
                .presentationDragIndicator(.visible)
            }
    }

    @ViewBuilder
    private var contentView: some View {
        if viewModel.isLoading && viewModel.messages.isEmpty {
            ProgressView("Loading...")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            ScrollView {
                VStack(spacing: 0) {
                    // Header Section (includes status, avatars, and runtime)
                    headerSection

                    // Todo List Section
                    if viewModel.todoState.hasTodos {
                        todoListSection
                    }

                    // Delegations Section
                    if !viewModel.delegations.isEmpty {
                        delegationsSection
                    }

                    // Latest Reply Section
                    if let reply = viewModel.latestReply {
                        latestReplySection(reply)
                    }

                    // Full Conversation Button
                    fullConversationButton
                }
                .padding(.bottom, 20)
            }
            .background(Color(.systemBackground))
        }
    }

    /// Initializes the view model with the core manager and loads data
    private func initializeAndLoad() async {
        viewModel.setCoreManager(coreManager)
        await viewModel.loadData()
    }

    // MARK: - Header Section

    private var headerSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Summary (no truncation)
            if let summary = conversation.summary, !summary.isEmpty {
                Text(summary)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }

            // Status badge, avatars, and runtime row
            HStack(alignment: .center, spacing: 12) {
                // Status badge
                StatusBadge(status: viewModel.currentStatus, isActive: viewModel.currentIsActive)

                Spacer()

                // Avatar group (author + participants)
                if let authorInfo = viewModel.authorInfo {
                    ConversationAvatarGroup(
                        authorInfo: authorInfo,
                        pTaggedRecipientInfo: viewModel.pTaggedRecipientInfo,
                        otherParticipants: viewModel.otherParticipantsInfo,
                        avatarSize: 20,
                        fontSize: 8,
                        maxVisibleAvatars: 5
                    )
                    .environmentObject(coreManager)
                }

                // Runtime
                Text(viewModel.formattedRuntime)
                    .font(.system(size: 18, weight: .medium))
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 20)
        .padding(.top, 20)
        .padding(.bottom, 16)
        .overlay(alignment: .bottom) {
            Divider()
        }
    }

    // MARK: - Latest Reply Section

    private func latestReplySection(_ reply: MessageInfo) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text(AgentNameFormatter.format(reply.author))
                    .font(.subheadline)
                    .fontWeight(.semibold)

                Spacer()

                Text(ConversationFormatters.formatRelativeTime(reply.createdAt))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            MarkdownView(content: reply.content)
                .font(.body)
                .foregroundStyle(.primary)
                .textSelection(.enabled)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 20)
        .overlay(alignment: .top) {
            Divider()
        }
    }

    // MARK: - Todo List Section

    private var todoListSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Header with progress indicator
            HStack {
                Text("Todos")
                    .font(.headline)
                    .foregroundStyle(.primary)

                Spacer()

                TodoProgressView(stats: viewModel.aggregatedTodoStats)
            }
            .padding(.horizontal, 20)

            // Todo items
            VStack(spacing: 8) {
                ForEach(viewModel.todoState.items) { todo in
                    TodoRowView(todo: todo)
                }
            }
            .padding(.horizontal, 20)
        }
        .padding(.vertical, 16)
        .overlay(alignment: .bottom) {
            Divider()
        }
    }

    // MARK: - Delegations Section

    private var delegationsSection: some View {
        VStack(spacing: 0) {
            ForEach(viewModel.delegations) { delegation in
                DelegationRowView(delegation: delegation) {
                    selectedDelegation = delegation
                }
                .environmentObject(coreManager)

                if delegation.id != viewModel.delegations.last?.id {
                    Divider()
                        .padding(.leading, 68)
                }
            }
        }
        .padding(.vertical, 16)
        .overlay(alignment: .bottom) {
            Divider()
        }
    }

    // MARK: - Full Conversation Button

    private var fullConversationButton: some View {
        Button(action: { showFullConversation = true }) {
            Text("View Full Conversation")
                .font(.headline)
                .foregroundStyle(.white)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 16)
                .background(Color.accentColor)
                .clipShape(RoundedRectangle(cornerRadius: 14))
        }
        .padding(.horizontal, 20)
        .padding(.top, 20)
    }
}

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
                .font(.system(size: 16))
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
                        .foregroundStyle(.orange)
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
                .font(.system(size: style == .large ? 12 : 10))
            Text("\(count)")
                .font(style == .large ? .caption : .caption2)
                .fontWeight(style == .large ? .medium : .regular)
        }
        .foregroundStyle(.green)
        .if(style == .large) { view in
            view
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(Color.green.opacity(0.15))
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

// MARK: - Delegation Row View

/// Tappable delegation row without card styling
struct DelegationRowView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    let delegation: DelegationItem
    let onTap: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            // Smaller avatar for compact look
            AgentAvatarView(agentName: delegation.recipient, pubkey: delegation.recipientPubkey, size: 32, fontSize: 11)
                .environmentObject(coreManager)

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
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }
}

/// Full conversation chat view sheet with Slack-style messages
struct FullConversationSheet: View {
    let conversation: ConversationFullInfo
    let messages: [MessageInfo]
    @EnvironmentObject var coreManager: TenexCoreManager
    @Environment(\.dismiss) private var dismiss

    @State private var selectedDelegation: String?
    @State private var showComposer = false
    @State private var availableAgents: [OnlineAgentInfo] = []

    /// Compute consecutive message flags for each message
    private var messagesWithConsecutive: [(message: MessageInfo, isConsecutive: Bool)] {
        messages.enumerated().map { index, msg in
            let isConsecutive = index > 0 && messages[index - 1].authorNpub == msg.authorNpub
            return (msg, isConsecutive)
        }
    }

    /// Extract project ID from projectATag
    private var projectId: String {
        conversation.projectATag.isEmpty ? conversation.id : conversation.projectATag
    }

    /// Find the project for this conversation
    private var project: ProjectInfo? {
        coreManager.safeCore.getProjects().first { $0.id == conversation.projectATag }
    }

    /// Find the last agent that spoke in the conversation (like TUI's get_most_recent_agent_from_conversation)
    private var lastAgentPubkey: String? {
        // Get set of agent pubkeys for quick lookup
        let agentPubkeys = Set(availableAgents.map { $0.pubkey })

        // Find the most recent message from an agent (not the user)
        // Messages are sorted by createdAt, iterate to find the latest agent message
        var latestAgentPubkey: String?
        var latestTimestamp: UInt64 = 0

        for msg in messages {
            // Skip user messages
            if msg.role == "user" {
                continue
            }

            // Check if this message is from a known agent (authorNpub is actually hex pubkey)
            if agentPubkeys.contains(msg.authorNpub) && msg.createdAt >= latestTimestamp {
                latestTimestamp = msg.createdAt
                latestAgentPubkey = msg.authorNpub
            }
        }

        return latestAgentPubkey
    }

    var body: some View {
        NavigationStack {
            ZStack(alignment: .bottom) {
                ScrollViewReader { proxy in
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 0) {
                            ForEach(messagesWithConsecutive, id: \.message.id) { item in
                                SlackMessageRow(
                                    message: item.message,
                                    isConsecutive: item.isConsecutive,
                                    conversationId: conversation.id,
                                    projectId: projectId,
                                    onDelegationTap: { delegationId in
                                        selectedDelegation = delegationId
                                    }
                                )
                                .environmentObject(coreManager)
                                .id(item.message.id)
                            }
                        }
                        .padding()
                        .padding(.bottom, 80) // Space for compose button
                    }
                    .onAppear {
                        // Scroll to the last message
                        if let lastMessage = messages.last {
                            proxy.scrollTo(lastMessage.id, anchor: .bottom)
                        }
                    }
                    .task {
                        // Load available agents for the project to determine last agent
                        if let projectId = project?.id {
                            do {
                                availableAgents = try await coreManager.safeCore.getOnlineAgents(projectId: projectId)
                            } catch {
                                print("[FullConversationSheet] Failed to load agents: \(error)")
                            }
                        }
                    }
                }

                // Floating compose button
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
                    .background(Color.accentColor)
                    .clipShape(Capsule())
                    .shadow(color: .black.opacity(0.15), radius: 8, x: 0, y: 4)
                }
                .padding(.bottom, 16)
            }
            .navigationTitle("Full Conversation")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            .sheet(item: $selectedDelegation) { delegationId in
                DelegationSheetFromId(delegationId: delegationId)
                    .environmentObject(coreManager)
                    .presentationDetents([.large])
                    .presentationDragIndicator(.visible)
            }
            .sheet(isPresented: $showComposer) {
                MessageComposerView(
                    project: project,
                    conversationId: conversation.id,
                    conversationTitle: conversation.title,
                    initialAgentPubkey: lastAgentPubkey
                )
                .environmentObject(coreManager)
            }
        }
    }
}

// MARK: - Delegation Sheet From ID

/// Helper view to load and display a delegation conversation by ID
private struct DelegationSheetFromId: View {
    let delegationId: String
    @EnvironmentObject var coreManager: TenexCoreManager
    @Environment(\.dismiss) private var dismiss

    @State private var conversation: ConversationFullInfo?
    @State private var isLoading = true

    var body: some View {
        NavigationStack {
            Group {
                if isLoading {
                    ProgressView("Loading...")
                } else if let conv = conversation {
                    ConversationDetailView(conversation: conv)
                        .environmentObject(coreManager)
                } else {
                    ContentUnavailableView(
                        "Conversation Not Found",
                        systemImage: "doc.questionmark",
                        description: Text("Unable to load delegation details.")
                    )
                }
            }
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
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

// MARK: - ConversationFullInfo Identifiable

extension ConversationFullInfo: Identifiable {}

#Preview {
    ConversationDetailView(conversation: ConversationFullInfo(
        id: "test-123",
        title: "Implement User Authentication",
        author: "architect-orchestrator",
        authorPubkey: "abc123def456",
        summary: "Add OAuth2 authentication flow with Google and GitHub providers",
        messageCount: 15,
        lastActivity: UInt64(Date().timeIntervalSince1970) - 3600,
        effectiveLastActivity: UInt64(Date().timeIntervalSince1970) - 60,
        parentId: nil,
        status: "In Progress",
        currentActivity: "Reviewing security requirements",
        isActive: true,
        isArchived: false,
        hasChildren: true,
        projectATag: "project-123",
        isScheduled: false,
        pTags: []
    ))
    .environmentObject(TenexCoreManager())
}
