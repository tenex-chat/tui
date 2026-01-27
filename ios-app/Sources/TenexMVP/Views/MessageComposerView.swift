import SwiftUI

/// A premium message composition view for both new conversations and replies.
/// Supports single agent selection, draft persistence, and markdown input.
struct MessageComposerView: View {
    // MARK: - Properties

    /// The project this message belongs to
    let project: ProjectInfo

    /// The conversation ID if replying to an existing thread (nil for new thread)
    let conversationId: String?

    /// Optional conversation title for display
    let conversationTitle: String?

    /// Callback when message is sent successfully
    var onSend: ((SendMessageResult) -> Void)?

    /// Callback when the view is dismissed
    var onDismiss: (() -> Void)?

    // MARK: - Environment

    @Environment(\.dismiss) private var dismiss
    @EnvironmentObject private var coreManager: TenexCoreManager

    // MARK: - State

    @State private var draft: Draft
    @State private var draftManager = DraftManager()
    @State private var availableAgents: [AgentInfo] = []
    @State private var agentsLoadError: String?
    @State private var showAgentSelector = false
    @State private var isSending = false
    @State private var sendError: String?
    @State private var showSendError = false

    // MARK: - Computed

    private var isNewConversation: Bool {
        conversationId == nil
    }

    private var canSend: Bool {
        draft.isValid && !isSending
    }

    private var selectedAgent: AgentInfo? {
        guard let pubkey = draft.agentPubkey else { return nil }
        return availableAgents.first { $0.pubkey == pubkey }
    }

    // MARK: - Initialization

    init(
        project: ProjectInfo,
        conversationId: String? = nil,
        conversationTitle: String? = nil,
        onSend: ((SendMessageResult) -> Void)? = nil,
        onDismiss: (() -> Void)? = nil
    ) {
        self.project = project
        self.conversationId = conversationId
        self.conversationTitle = conversationTitle
        self.onSend = onSend
        self.onDismiss = onDismiss

        // Initialize draft (will be updated in onAppear)
        if let conversationId = conversationId {
            _draft = State(initialValue: Draft(conversationId: conversationId, projectId: project.id))
        } else {
            _draft = State(initialValue: Draft(projectId: project.id))
        }
    }

    // MARK: - Body

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // Agent chip (if selected)
                if let agent = selectedAgent {
                    agentChipView(agent)
                }

                // Title field (only for new conversations)
                if isNewConversation {
                    titleFieldView
                }

                Divider()

                // Content editor
                contentEditorView

                Divider()

                // Toolbar
                toolbarView
            }
            .navigationTitle(isNewConversation ? "New Conversation" : "Reply")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        draftManager.saveNow()
                        onDismiss?()
                        dismiss()
                    }
                }

                ToolbarItem(placement: .primaryAction) {
                    Button("Send") {
                        sendMessage()
                    }
                    .disabled(!canSend)
                    .fontWeight(.semibold)
                }
            }
            .onAppear {
                loadDraft()
                loadAgents()
            }
            .sheet(isPresented: $showAgentSelector) {
                AgentSelectorSheet(
                    agents: availableAgents,
                    selectedPubkey: $draft.agentPubkey,
                    onDone: {
                        draftManager.updateAgent(draft.agentPubkey, conversationId: conversationId, projectId: project.id)
                    }
                )
            }
            .alert("Send Failed", isPresented: $showSendError) {
                Button("OK") { }
            } message: {
                Text(sendError ?? "Unknown error")
            }
        }
    }

    // MARK: - Subviews

    private func agentChipView(_ agent: AgentInfo) -> some View {
        HStack(spacing: 8) {
            AgentChipView(agent: agent) {
                draft.clearAgent()
                draftManager.updateAgent(nil, conversationId: conversationId, projectId: project.id)
            }
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(Color(.systemGray6))
    }

    private var titleFieldView: some View {
        TextField("Conversation Title", text: Binding(
            get: { draft.title },
            set: { newValue in
                draft.updateTitle(newValue)
                draftManager.updateTitle(newValue, projectId: project.id)
            }
        ))
        .font(.headline)
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    private var contentEditorView: some View {
        ZStack(alignment: .topLeading) {
            TextEditor(text: Binding(
                get: { draft.content },
                set: { newValue in
                    draft.updateContent(newValue)
                    draftManager.updateContent(newValue, conversationId: conversationId, projectId: project.id)
                }
            ))
            .font(.body)
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .scrollContentBackground(.hidden)

            if draft.content.isEmpty {
                Text(isNewConversation ? "What would you like to discuss?" : "Type your reply...")
                    .foregroundStyle(.tertiary)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 16)
                    .allowsHitTesting(false)
            }
        }
        .frame(minHeight: 200)
    }

    private var toolbarView: some View {
        HStack(spacing: 16) {
            // Agent selector button
            Button(action: { showAgentSelector = true }) {
                HStack(spacing: 4) {
                    Image(systemName: "person.fill")
                    if selectedAgent != nil {
                        Image(systemName: "checkmark")
                            .font(.caption)
                            .fontWeight(.medium)
                    }
                }
                .foregroundColor(selectedAgent == nil ? .secondary : .blue)
            }
            .buttonStyle(.plain)
            .padding(.vertical, 8)
            .padding(.horizontal, 12)
            .background(
                RoundedRectangle(cornerRadius: 8)
                    .fill(selectedAgent == nil ? Color.clear : Color.blue.opacity(0.1))
            )

            // Show error indicator if agents failed to load
            if agentsLoadError != nil {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                    .font(.caption)
            }

            Spacer()

            // Character count
            if draft.content.count > 0 {
                Text("\(draft.content.count)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            // Clear button (if has content)
            if draft.hasContent {
                Button(action: clearDraft) {
                    Image(systemName: "trash")
                        .foregroundStyle(.red)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(Color(.systemBackground))
    }

    // MARK: - Actions

    private func loadDraft() {
        if let existingDraft = draftManager.getDraft(conversationId: conversationId, projectId: project.id) {
            draft = existingDraft
        }
    }

    private func loadAgents() {
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let agents = try coreManager.core.getAgents(projectId: project.id)
                DispatchQueue.main.async {
                    availableAgents = agents
                    agentsLoadError = nil
                    if agents.isEmpty {
                        print("[MessageComposerView] No agents for this project")
                    }
                }
            } catch {
                DispatchQueue.main.async {
                    availableAgents = []
                    agentsLoadError = error.localizedDescription
                    print("[MessageComposerView] Failed to load agents: \(error)")
                }
            }
        }
    }

    private func sendMessage() {
        guard canSend else { return }

        isSending = true
        sendError = nil

        let agentPubkey = draft.agentPubkey

        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let result: SendMessageResult

                if isNewConversation {
                    result = try coreManager.core.sendThread(
                        projectId: project.id,
                        title: draft.title,
                        content: draft.content,
                        agentPubkey: agentPubkey
                    )
                } else {
                    result = try coreManager.core.sendMessage(
                        conversationId: conversationId!,
                        projectId: project.id,
                        content: draft.content,
                        agentPubkey: agentPubkey
                    )
                }

                DispatchQueue.main.async {
                    isSending = false

                    // Clear draft on success
                    draftManager.deleteDraft(conversationId: conversationId, projectId: project.id)

                    // Notify and dismiss
                    onSend?(result)
                    dismiss()
                }
            } catch {
                DispatchQueue.main.async {
                    isSending = false
                    sendError = error.localizedDescription
                    showSendError = true
                }
            }
        }
    }

    private func clearDraft() {
        draft.clear()
        draftManager.clearDraft(conversationId: conversationId, projectId: project.id)
    }
}

// MARK: - Agent Chip View

struct AgentChipView: View {
    let agent: AgentInfo
    let onRemove: () -> Void

    var body: some View {
        HStack(spacing: 6) {
            // Agent avatar
            Circle()
                .fill(agentColor.gradient)
                .frame(width: 24, height: 24)
                .overlay {
                    Text(String(agent.name.prefix(1)).uppercased())
                        .font(.caption2)
                        .fontWeight(.semibold)
                        .foregroundStyle(.white)
                }

            // Agent name
            Text("@\(agent.dTag)")
                .font(.subheadline)
                .fontWeight(.medium)
                .foregroundStyle(.primary)

            // Remove button
            Button(action: onRemove) {
                Image(systemName: "xmark.circle.fill")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(
            Capsule()
                .fill(Color(.systemBackground))
                .shadow(color: .black.opacity(0.1), radius: 2, x: 0, y: 1)
        )
    }

    private var agentColor: Color {
        let colors: [Color] = [.blue, .purple, .orange, .green, .pink, .indigo, .teal, .cyan]
        let hash = agent.pubkey.hashValue
        return colors[abs(hash) % colors.count]
    }
}

// MARK: - Preview

#Preview("New Conversation") {
    MessageComposerView(
        project: ProjectInfo(id: "test-project", title: "Test Project", description: "A test project")
    )
    .environmentObject(TenexCoreManager())
}

#Preview("Reply") {
    MessageComposerView(
        project: ProjectInfo(id: "test-project", title: "Test Project", description: "A test project"),
        conversationId: "conv-123",
        conversationTitle: "Test Conversation"
    )
    .environmentObject(TenexCoreManager())
}
