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
            .navigationTitle(conversation.thread.title)
            .navigationBarTitleDisplayMode(.inline)
            .navigationDestination(item: $selectedDelegationConv) { conv in
                ConversationDetailView(conversation: conv)
                    .environmentObject(coreManager)
            }
            #if os(macOS)
            .navigationDestination(isPresented: $showFullConversation) {
                FullConversationSheet(
                    conversation: conversation,
                    messages: viewModel.messages,
                    presentationStyle: .embedded
                )
                .environmentObject(coreManager)
            }
            #endif
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
                    .tenexModalPresentation(detents: [.large])
                } else {
                    DelegationPreviewSheet(delegation: delegation)
                        .tenexModalPresentation(detents: [.medium, .large])
                }
            }
            .sheet(isPresented: $showFullConversation) {
                FullConversationSheet(
                    conversation: conversation,
                    messages: viewModel.messages
                )
                .environmentObject(coreManager)
                .tenexModalPresentation(detents: [.large])
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
                if let buffer = coreManager.streamingBuffers[conversation.thread.id] {
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
            if let summary = conversation.thread.summary, !summary.isEmpty {
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
    private var project: Project? {
        coreManager.projects.first { $0.id == conversation.extractedProjectId }
    }

    /// Find the last agent that spoke in the conversation (hex pubkey format)
    /// Filters to exclude user messages and only selects from available agents
    private var lastAgentPubkey: String? {
        let availableAgents = project.flatMap { coreManager.onlineAgents[$0.id] } ?? []
        return LastAgentFinder.findLastAgentPubkey(
            messages: viewModel.messages,
            availableAgents: availableAgents
        )
    }

    private func latestReplySection(_ reply: Message) -> some View {
        let replyAuthorName = coreManager.displayName(for: reply.pubkey)
        return VStack(alignment: .leading, spacing: 12) {
            // Header with author and timestamp
            HStack {
                // Author avatar and name
                HStack(spacing: 6) {
                    AgentAvatarView(
                        agentName: replyAuthorName,
                        pubkey: reply.pubkey,
                        size: 20,
                        fontSize: 8,
                        showBorder: false
                    )
                    .environmentObject(coreManager)

                    Text(AgentNameFormatter.format(replyAuthorName))
                        .font(.subheadline)
                        .fontWeight(.semibold)
                        .foregroundStyle(deterministicColor(for: reply.pubkey))
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
                    #if !os(macOS)
                    .textSelection(.enabled)
                    #endif
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
                        .buttonStyle(.borderless)
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
                    .buttonStyle(.borderless)
                    .padding(.top, 8)
                }
            }

            #if os(iOS)
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
            .buttonStyle(.borderless)
            .padding(.top, 4)
            #endif
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 20)
        .overlay(alignment: .top) {
            Divider()
        }
        #if os(iOS)
        .sheet(isPresented: $showComposer) {
            MessageComposerView(
                project: project,
                conversationId: conversation.thread.id,
                conversationTitle: conversation.thread.title,
                initialAgentPubkey: lastAgentPubkey
            )
            .environmentObject(coreManager)
        }
        #endif
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
                DelegationRowView(
                    delegation: delegation,
                    isWorking: viewModel.delegationActivityByConversationId[delegation.conversationId] ?? false
                ) {
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
            showFullConversation = true
        }) {
            Text("View Full Conversation")
                .font(.headline)
                .foregroundStyle(.white)
                .frame(maxWidth: .infinity)
                .padding(.vertical, 16)
                .background(Color.agentBrand)
                .clipShape(RoundedRectangle(cornerRadius: 14))
        }
        .buttonStyle(.borderless)
        .padding(.horizontal, 20)
        .padding(.top, 20)
    }
}
