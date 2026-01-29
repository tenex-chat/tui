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

                    // Agents and Runtime Row
                    agentsAndRuntimeSection

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
            // Title
            Text(conversation.title)
                .font(.system(size: 28, weight: .bold))
                .foregroundStyle(.primary)
                .lineLimit(3)

            // Status badge
            StatusBadge(status: viewModel.currentStatus, isActive: viewModel.currentIsActive)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 20)
        .padding(.top, 20)
        .padding(.bottom, 16)
    }

    // MARK: - Agents Section

    private var agentsAndRuntimeSection: some View {
        HStack(alignment: .center, spacing: 12) {
            ForEach(viewModel.participatingAgents.prefix(8), id: \.self) { agent in
                AgentAvatarView(agentName: agent, size: 44, fontSize: 14)
                    .environmentObject(coreManager)
            }

            if viewModel.participatingAgents.count > 8 {
                Circle()
                    .fill(Color(.systemGray4))
                    .frame(width: 44, height: 44)
                    .overlay {
                        Text("+\(viewModel.participatingAgents.count - 8)")
                            .font(.subheadline)
                            .fontWeight(.medium)
                            .foregroundStyle(.secondary)
                    }
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 20)
        .padding(.vertical, 16)
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

            Text(reply.content)
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

// MARK: - Delegation Row View

/// Tappable delegation row without card styling
struct DelegationRowView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    let delegation: DelegationItem
    let onTap: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            AgentAvatarView(agentName: delegation.recipient, size: 40, fontSize: 13)
                .environmentObject(coreManager)

            VStack(alignment: .leading, spacing: 4) {
                Text(AgentNameFormatter.format(delegation.recipient))
                    .font(.subheadline)
                    .fontWeight(.semibold)

                Text(delegation.messagePreview)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }

            Spacer()

            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 14)
        .contentShape(Rectangle())
        .onTapGesture {
            onTap()
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
