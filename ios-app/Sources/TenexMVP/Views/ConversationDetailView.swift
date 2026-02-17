import SwiftUI

// MARK: - Conversation Detail View

/// Main detail view for a conversation showing overview, agents, runtime, latest reply, and delegations
/// Based on the approved wireframe at wireframes/ios-conversation-detail.html
struct ConversationDetailView: View {
    let conversation: ConversationFullInfo
    @EnvironmentObject var coreManager: TenexCoreManager

    @StateObject private var viewModel: ConversationDetailViewModel
    @State private var selectedDelegation: DelegationItem?
    @State private var selectedDelegationConv: ConversationFullInfo?
    @State private var showFullConversation = false
    @State private var showComposer = false
    @State private var isLatestReplyExpanded = false
    @State private var latestReplyContentHeight: CGFloat = 0
    #if os(macOS)
    @Environment(\.openWindow) private var openWindow
    #endif

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
            .navigationDestination(item: $selectedDelegationConv) { conv in
                ConversationDetailView(conversation: conv)
                    .environmentObject(coreManager)
            }
            .task {
                await initializeAndLoad()
            }
            #if os(iOS)
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
            #endif
    }

    @ViewBuilder
    private var contentView: some View {
        ScrollView {
            VStack(spacing: 0) {
                // Header Section (includes status, avatars, and runtime)
                // Always renders immediately with known conversation data
                headerSection

                // Todo List Section - renders when todos are available
                if viewModel.todoState.hasTodos {
                    todoListSection
                }

                // Latest Reply Section - renders when messages are available
                if let reply = viewModel.latestReply {
                    latestReplySection(reply)
                }

                // Streaming Section - shows live agent output from local socket
                if let buffer = coreManager.streamingBuffers[conversation.id] {
                    streamingSection(buffer)
                }

                // Delegations Section - renders when delegations are available
                if !viewModel.delegations.isEmpty {
                    delegationsSection
                }

                // Full Conversation Button
                fullConversationButton
            }
            .padding(.bottom, 20)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(Color.systemBackground)
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

            // Status badge, project badge, avatars, and runtime row
            HStack(alignment: .center, spacing: 12) {
                // Status badge
                StatusBadge(status: viewModel.currentStatus, isActive: viewModel.currentIsActive)

                // Project badge
                if let project = project {
                    ProjectBadge(projectTitle: project.title)
                }

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
                    .font(.headline)
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

    /// Maximum height before collapsing (roughly 60% of screen height)
    private let maxCollapsedHeight: CGFloat = 400

    /// Whether the latest reply content needs collapsing
    private var latestReplyNeedsCollapsing: Bool {
        latestReplyContentHeight > maxCollapsedHeight
    }

    /// Find the project for this conversation
    private var project: ProjectInfo? {
        coreManager.projects.first { $0.id == conversation.extractedProjectId }
    }

    /// Find the last agent that spoke in the conversation (hex pubkey format)
    /// Filters by role to exclude user messages and only selects from available agents
    private var lastAgentPubkey: String? {
        let availableAgents = project.flatMap { coreManager.onlineAgents[$0.id] } ?? []
        return LastAgentFinder.findLastAgentPubkey(
            messages: viewModel.messages,
            availableAgents: availableAgents,
            npubToHex: { Bech32.npubToHex($0) }
        )
    }

    private func latestReplySection(_ reply: MessageInfo) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            // Header with author and timestamp
            HStack {
                // Author avatar and name
                HStack(spacing: 6) {
                    AgentAvatarView(
                        agentName: reply.author,
                        pubkey: reply.authorNpub.isEmpty ? nil : Bech32.npubToHex(reply.authorNpub),
                        size: 20,
                        fontSize: 8,
                        showBorder: false
                    )
                    .environmentObject(coreManager)

                    Text(AgentNameFormatter.format(reply.author))
                        .font(.subheadline)
                        .fontWeight(.semibold)
                        .foregroundStyle(deterministicColor(for: reply.authorNpub))
                }

                Spacer()

                Text(ConversationFormatters.formatRelativeTime(reply.createdAt))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            // Collapsible content
            VStack(alignment: .leading, spacing: 0) {
                // Content with height measurement
                MarkdownView(content: reply.content)
                    .font(.body)
                    .foregroundStyle(.primary)
                    .textSelection(.enabled)
                    .background(
                        GeometryReader { geometry in
                            Color.clear
                                .onAppear {
                                    latestReplyContentHeight = geometry.size.height
                                }
                                .onChange(of: reply.content) {
                                    latestReplyContentHeight = geometry.size.height
                                }
                        }
                    )
                    .frame(maxHeight: isLatestReplyExpanded || !latestReplyNeedsCollapsing ? nil : maxCollapsedHeight, alignment: .top)
                    .clipped()

                // Gradient fade and "Read more" button when collapsed
                if latestReplyNeedsCollapsing && !isLatestReplyExpanded {
                    VStack(spacing: 0) {
                        // Gradient fade overlay
                        LinearGradient(
                            gradient: Gradient(colors: [
                                Color.systemBackground.opacity(0),
                                Color.systemBackground
                            ]),
                            startPoint: .top,
                            endPoint: .bottom
                        )
                        .frame(height: 40)
                        .offset(y: -40)

                        // Read more button
                        Button {
                            withAnimation(.easeInOut(duration: 0.3)) {
                                isLatestReplyExpanded = true
                            }
                        } label: {
                            HStack(spacing: 4) {
                                Text("Read more")
                                    .font(.subheadline)
                                    .fontWeight(.medium)
                                Image(systemName: "chevron.down")
                                    .font(.caption)
                            }
                            .foregroundStyle(Color.composerAction)
                        }
                        .buttonStyle(.plain)
                        .padding(.top, 4)
                    }
                }

                // Collapse button when expanded
                if latestReplyNeedsCollapsing && isLatestReplyExpanded {
                    Button {
                        withAnimation(.easeInOut(duration: 0.3)) {
                            isLatestReplyExpanded = false
                        }
                    } label: {
                        HStack(spacing: 4) {
                            Text("Show less")
                                .font(.subheadline)
                                .fontWeight(.medium)
                            Image(systemName: "chevron.up")
                                .font(.caption)
                        }
                        .foregroundStyle(Color.composerAction)
                    }
                    .buttonStyle(.plain)
                    .padding(.top, 8)
                }
            }

            // Reply button
            Button {
                showComposer = true
            } label: {
                HStack(spacing: 6) {
                    Image(systemName: "arrowshape.turn.up.left.fill")
                        .font(.caption)
                    Text("Reply")
                        .font(.subheadline)
                        .fontWeight(.medium)
                }
                .foregroundStyle(.white)
                .padding(.horizontal, 16)
                .padding(.vertical, 8)
                .background(Color.agentBrand)
                .clipShape(Capsule())
            }
            .buttonStyle(.plain)
            .padding(.top, 4)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 20)
        .overlay(alignment: .top) {
            Divider()
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

    // MARK: - Streaming Section

    private func streamingSection(_ buffer: StreamingBuffer) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 6) {
                ProgressView()
                    .scaleEffect(0.7)
                Text("Agent is typing...")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .italic()
                Spacer()
            }
            if !buffer.text.isEmpty {
                HStack(alignment: .lastTextBaseline, spacing: 0) {
                    MarkdownView(content: buffer.text)
                        .font(.body)
                    Text("\u{258C}")
                        .font(.body)
                        .foregroundStyle(.secondary)
                        .opacity(0.6)
                }
            }
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 16)
        .overlay(alignment: .top) { Divider() }
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
                    #if os(macOS)
                    selectedDelegationConv = viewModel.childConversation(for: delegation.conversationId)
                    #else
                    selectedDelegation = delegation
                    #endif
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
        Button(action: {
            #if os(macOS)
            openWindow(id: "full-conversation", value: conversation.id)
            #else
            showFullConversation = true
            #endif
        }) {
            Text("View Full Conversation")
                .font(.headline)
                .foregroundStyle(.white)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 16)
                .background(Color.agentBrand)
                .clipShape(RoundedRectangle(cornerRadius: 14))
        }
        .buttonStyle(.plain)
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
    @State private var isAtBottom = true
    @State private var scrollViewHeight: CGFloat = 0

    private let bottomAnchorId = "full-conversation-bottom"
    private let bottomThreshold: CGFloat = 60

    /// Compute consecutive message flags for each message
    private var messagesWithConsecutive: [(message: MessageInfo, isConsecutive: Bool)] {
        messages.enumerated().map { index, msg in
            let isConsecutive = index > 0 && messages[index - 1].authorNpub == msg.authorNpub
            return (msg, isConsecutive)
        }
    }

    /// Find the project for this conversation
    private var project: ProjectInfo? {
        coreManager.projects.first { $0.id == conversation.extractedProjectId }
    }

    /// Find the last agent that spoke in the conversation (like TUI's get_most_recent_agent_from_conversation)
    /// Returns hex pubkey format for use with MessageComposerView
    private var lastAgentPubkey: String? {
        LastAgentFinder.findLastAgentPubkey(
            messages: messages,
            availableAgents: availableAgents,
            npubToHex: { Bech32.npubToHex($0) }
        )
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
                                    projectId: conversation.extractedProjectId,
                                    onDelegationTap: { delegationId in
                                        selectedDelegation = delegationId
                                    }
                                )
                                .environmentObject(coreManager)
                                .id(item.message.id)
                            }

                            if let buffer = coreManager.streamingBuffers[conversation.id] {
                                StreamingMessageRow(
                                    buffer: buffer,
                                    isConsecutive: messages.last?.authorNpub == buffer.agentPubkey
                                )
                                .environmentObject(coreManager)
                                .id("streaming-row")
                            }
                        }
                        .padding()
                        .padding(.bottom, 80) // Space for compose button

                        Color.clear
                            .frame(height: 1)
                            .id(bottomAnchorId)
                            .background(
                                GeometryReader { geo in
                                    Color.clear.preference(
                                        key: BottomAnchorOffsetKey.self,
                                        value: geo.frame(in: .named("fullConversationScroll")).maxY
                                    )
                                }
                            )
                    }
                    .coordinateSpace(name: "fullConversationScroll")
                    .background(
                        GeometryReader { geo in
                            Color.clear.preference(
                                key: ScrollViewHeightKey.self,
                                value: geo.size.height
                            )
                        }
                    )
                    .onAppear {
                        // Scroll to the last message
                        if let lastMessage = messages.last {
                            proxy.scrollTo(lastMessage.id, anchor: .bottom)
                        }
                    }
                    .onChange(of: messages.last?.id) { _ in
                        guard let lastMessage = messages.last else { return }
                        if isAtBottom {
                            DispatchQueue.main.async {
                                withAnimation(.easeOut(duration: 0.2)) {
                                    proxy.scrollTo(lastMessage.id, anchor: .bottom)
                                }
                            }
                        }
                    }
                    .onChange(of: coreManager.streamingBuffers[conversation.id]?.text.count) { _ in
                        if isAtBottom {
                            DispatchQueue.main.async {
                                withAnimation(.easeOut(duration: 0.2)) {
                                    proxy.scrollTo("streaming-row", anchor: .bottom)
                                }
                            }
                        }
                    }
                    .onPreferenceChange(ScrollViewHeightKey.self) { height in
                        scrollViewHeight = height
                    }
                    .onPreferenceChange(BottomAnchorOffsetKey.self) { bottomY in
                        let distanceFromBottom = bottomY - scrollViewHeight
                        isAtBottom = distanceFromBottom <= bottomThreshold
                    }
                    .task {
                        // Load available agents for the project to determine last agent
                        if let projectId = project?.id {
                            availableAgents = coreManager.onlineAgents[projectId] ?? []
                        }
                    }
                    .onReceive(coreManager.$onlineAgents) { cache in
                        if let projectId = project?.id {
                            availableAgents = cache[projectId] ?? []
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
                    .background(Color.agentBrand)
                    .clipShape(Capsule())
                    .shadow(color: .black.opacity(0.15), radius: 8, x: 0, y: 4)
                }
                .buttonStyle(.plain)
                .padding(.bottom, 16)
            }
            .navigationTitle("Full Conversation")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
            #if os(iOS)
            .sheet(item: $selectedDelegation) { delegationId in
                DelegationSheetFromId(delegationId: delegationId)
                    .environmentObject(coreManager)
                    .presentationDetents([.large])
                    .presentationDragIndicator(.visible)
            }
            #endif
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

private struct ScrollViewHeightKey: PreferenceKey {
    static var defaultValue: CGFloat = 0

    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

private struct BottomAnchorOffsetKey: PreferenceKey {
    static var defaultValue: CGFloat = 0

    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
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
                if let conv = conversation {
                    ConversationDetailView(conversation: conv)
                        .environmentObject(coreManager)
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

// MARK: - ConversationFullInfo Extensions

extension ConversationFullInfo: Identifiable {}

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
