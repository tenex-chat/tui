import SwiftUI

// MARK: - Conversation Detail View

/// Main detail view for a conversation showing overview, agents, runtime, latest reply, and delegations
/// Based on the approved wireframe at wireframes/ios-conversation-detail.html
struct ConversationDetailView: View {
    let conversation: ConversationFullInfo
    @EnvironmentObject var coreManager: TenexCoreManager
    @Environment(\.dismiss) private var dismiss

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
        NavigationStack {
            contentView
                .background(Color(.systemGroupedBackground))
                .navigationTitle("Conversation")
                .navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .topBarLeading) {
                        Button("Back") { dismiss() }
                    }
                    ToolbarItem(placement: .topBarTrailing) {
                        Button(action: { Task { await viewModel.loadData() } }) {
                            Image(systemName: "arrow.clockwise")
                        }
                        .disabled(viewModel.isLoading)
                    }
                }
                .task {
                    await initializeAndLoad()
                }
                .refreshable {
                    await viewModel.loadData()
                }
                .sheet(item: $selectedDelegation) { delegation in
                    if let childConv = viewModel.childConversation(for: delegation.conversationId) {
                        ConversationDetailView(conversation: childConv)
                            .environmentObject(coreManager)
                    } else {
                        DelegationPreviewSheet(delegation: delegation)
                    }
                }
                .sheet(isPresented: $showFullConversation) {
                    FullConversationSheet(
                        conversation: conversation,
                        messages: viewModel.messages
                    )
                    .environmentObject(coreManager)
                }
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
                    // Header Section
                    headerSection

                    // Agents Section
                    agentsSection

                    // Runtime Section
                    runtimeSection

                    // Latest Reply Section
                    if let reply = viewModel.latestReply {
                        latestReplySection(reply)
                    }

                    // Delegations Section
                    if !viewModel.delegations.isEmpty {
                        delegationsSection
                    }

                    // Full Conversation Button
                    fullConversationButton
                }
                .padding(.bottom, 20)
            }
        }
    }

    /// Initializes the view model with the core manager and loads data
    private func initializeAndLoad() async {
        viewModel.setCoreManager(coreManager)
        await viewModel.loadData()
    }

    // MARK: - Header Section

    private var headerSection: some View {
        SharedCardView {
            VStack(alignment: .leading, spacing: 12) {
                // Title
                Text(conversation.title)
                    .font(.title2)
                    .fontWeight(.bold)
                    .foregroundStyle(.primary)

                // Summary
                if let summary = conversation.summary {
                    Text(summary)
                        .font(.body)
                        .foregroundStyle(.secondary)
                        .lineLimit(3)
                }

                // Status row - uses refreshable metadata from view model
                HStack(spacing: 12) {
                    Text("Status:")
                        .font(.subheadline)
                        .fontWeight(.medium)
                        .foregroundStyle(.secondary)

                    StatusBadge(status: viewModel.currentStatus, isActive: viewModel.currentIsActive)
                }

                // Activity row
                if let activity = viewModel.currentActivity {
                    HStack(spacing: 12) {
                        Text("Activity:")
                            .font(.subheadline)
                            .fontWeight(.medium)
                            .foregroundStyle(.secondary)

                        Text(activity)
                            .font(.subheadline)
                            .foregroundStyle(.primary)
                    }
                }
            }
        }
    }

    // MARK: - Agents Section

    private var agentsSection: some View {
        SharedCardView(title: "AGENTS") {
            HStack(spacing: 16) {
                ForEach(viewModel.participatingAgents.prefix(8), id: \.self) { agent in
                    VStack(spacing: 6) {
                        SharedAgentAvatar(agentName: agent, size: 44, fontSize: 14)
                        Text(AgentNameFormatter.format(agent))
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }

                if viewModel.participatingAgents.count > 8 {
                    VStack(spacing: 6) {
                        Circle()
                            .fill(Color(.systemGray4))
                            .frame(width: 44, height: 44)
                            .overlay {
                                Text("+\(viewModel.participatingAgents.count - 8)")
                                    .font(.subheadline)
                                    .fontWeight(.medium)
                                    .foregroundStyle(.secondary)
                            }
                        Text("More")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }

                Spacer()
            }
        }
    }

    // MARK: - Runtime Section

    private var runtimeSection: some View {
        SharedCardView(title: "EFFECTIVE RUNTIME") {
            RuntimeDisplayView(isActive: viewModel.currentIsActive) { currentTime in
                viewModel.formatEffectiveRuntime(currentTime: currentTime)
            }
        }
    }

    // MARK: - Latest Reply Section

    private func latestReplySection(_ reply: MessageInfo) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            // Card without header
            VStack(alignment: .leading, spacing: 10) {
                HStack {
                    Text(AgentNameFormatter.format(reply.author))
                        .font(.subheadline)
                        .fontWeight(.semibold)

                    Spacer()

                    Text(ConversationFormatters.formatRelativeTime(reply.createdAt))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Text(TextUtilities.truncate(reply.content, maxLength: 200))
                    .font(.body)
                    .foregroundStyle(.secondary)
                    .lineLimit(4)
            }
            .padding(16)
            .background(Color(.systemBackground))
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .padding(.horizontal, 16)
            .padding(.top, 16)
        }
    }

    // MARK: - Delegations Section

    private var delegationsSection: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header
            Text("DELEGATIONS")
                .font(.caption)
                .fontWeight(.semibold)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 16)
                .padding(.top, 20)
                .padding(.bottom, 8)

            // Delegation items
            VStack(spacing: 0) {
                ForEach(viewModel.delegations) { delegation in
                    SharedDelegationRow(delegation: delegation) {
                        selectedDelegation = delegation
                    }

                    if delegation.id != viewModel.delegations.last?.id {
                        Divider()
                            .padding(.leading, 68)
                    }
                }
            }
            .background(Color(.systemBackground))
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .padding(.horizontal, 16)
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
                .clipShape(RoundedRectangle(cornerRadius: 12))
        }
        .padding(.horizontal, 16)
        .padding(.top, 16)
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

/// Full conversation chat view sheet
struct FullConversationSheet: View {
    let conversation: ConversationFullInfo
    let messages: [MessageInfo]
    @EnvironmentObject var coreManager: TenexCoreManager
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    ForEach(messages, id: \.id) { message in
                        SharedMessageBubble(message: message)
                    }
                }
                .padding()
            }
            .navigationTitle("Full Conversation")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }
}

// MARK: - ConversationFullInfo Identifiable

extension ConversationFullInfo: Identifiable {}

#Preview {
    ConversationDetailView(conversation: ConversationFullInfo(
        id: "test-123",
        title: "Implement User Authentication",
        author: "architect-orchestrator",
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
        isScheduled: false
    ))
    .environmentObject(TenexCoreManager())
}
