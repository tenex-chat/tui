import SwiftUI
import UIKit
import CryptoKit

/// A premium message composition view for both new conversations and replies.
/// Supports project selection (for new conversations), agent selection, draft persistence, and markdown input.
struct MessageComposerView: View {
    // MARK: - Properties

    /// The project this message belongs to (nil for new conversations with project selection)
    let initialProject: ProjectInfo?

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
    @Environment(\.scenePhase) private var scenePhase
    @EnvironmentObject private var coreManager: TenexCoreManager

    // MARK: - State

    @State private var selectedProject: ProjectInfo?
    @State private var availableProjects: [ProjectInfo] = []
    @State private var showProjectSelector = false
    @State private var draft: Draft
    @State private var availableAgents: [AgentInfo] = []
    @State private var agentsLoadError: String?
    @State private var showAgentSelector = false
    @State private var isSending = false
    @State private var sendError: String?
    @State private var showSendError = false
    @State private var isDirty = false // Track if user has made edits before load completes
    @State private var showLoadFailedAlert = false
    @State private var isLoadingDraft = true // Track if draft is still loading
    @State private var isSwitchingProject = false // Track if project switch is in progress
    @State private var showSaveFailedAlert = false
    @State private var saveFailedError: String?

    // MARK: - Computed

    private var draftManager: DraftManager {
        DraftManager.shared
    }

    private var isNewConversation: Bool {
        conversationId == nil
    }

    private var canSend: Bool {
        // Project is required for all messages (new conversations and replies)
        guard selectedProject != nil else {
            return false
        }

        // CRITICAL DATA SAFETY: Block sending if draft load failed
        guard !draftManager.loadFailed else {
            return false
        }

        // BLOCKER #1 FIX: Block sending until draft load completes
        guard !isLoadingDraft else {
            return false
        }

        // HIGH #2 FIX: Block sending during project switch to prevent stale draft + new project
        guard !isSwitchingProject else {
            return false
        }

        // Draft must be valid (has required content) and not currently sending
        return draft.isValid && !isSending
    }

    private var selectedAgent: AgentInfo? {
        guard let pubkey = draft.agentPubkey else { return nil }
        return availableAgents.first { $0.pubkey == pubkey }
    }

    // MARK: - Initialization

    init(
        project: ProjectInfo? = nil,
        conversationId: String? = nil,
        conversationTitle: String? = nil,
        onSend: ((SendMessageResult) -> Void)? = nil,
        onDismiss: (() -> Void)? = nil
    ) {
        self.initialProject = project
        self.conversationId = conversationId
        self.conversationTitle = conversationTitle
        self.onSend = onSend
        self.onDismiss = onDismiss

        // Initialize state with project if provided
        _selectedProject = State(initialValue: project)

        // Initialize draft (will be updated in onAppear)
        if let conversationId = conversationId, let projectId = project?.id {
            _draft = State(initialValue: Draft(conversationId: conversationId, projectId: projectId))
        } else if let projectId = project?.id {
            _draft = State(initialValue: Draft(projectId: projectId))
        } else {
            // No project yet - will be set when project is selected
            _draft = State(initialValue: Draft(projectId: ""))
        }
    }

    // MARK: - Body

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // Project chip (for new conversations)
                if isNewConversation {
                    if let project = selectedProject {
                        projectChipView(project)
                    } else {
                        projectPromptView
                    }
                }

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
                        // CRITICAL DATA SAFETY: Use background task to guarantee save completes
                        var backgroundTaskID: UIBackgroundTaskIdentifier = .invalid

                        backgroundTaskID = UIApplication.shared.beginBackgroundTask {
                            print("[MessageComposerView] WARNING: Background save time expired on cancel")
                            if backgroundTaskID != .invalid {
                                UIApplication.shared.endBackgroundTask(backgroundTaskID)
                                backgroundTaskID = .invalid
                            }
                        }

                        Task {
                            // MEDIUM #3 FIX: Catch save errors and alert user
                            do {
                                try await draftManager.saveNow()

                                if backgroundTaskID != .invalid {
                                    UIApplication.shared.endBackgroundTask(backgroundTaskID)
                                    backgroundTaskID = .invalid
                                }

                                onDismiss?()
                                dismiss()
                            } catch {
                                if backgroundTaskID != .invalid {
                                    UIApplication.shared.endBackgroundTask(backgroundTaskID)
                                    backgroundTaskID = .invalid
                                }

                                // Show error alert and block dismissal
                                saveFailedError = error.localizedDescription
                                showSaveFailedAlert = true
                            }
                        }
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
                loadProjects()
                // BLOCKER #1 FIX: If no project selected, no draft to load - mark as not loading
                if selectedProject == nil {
                    isLoadingDraft = false
                } else {
                    loadDraft()
                    loadAgents()
                }
            }
            .sheet(isPresented: $showProjectSelector) {
                ProjectSelectorSheet(
                    projects: availableProjects,
                    selectedProject: $selectedProject,
                    onDone: {
                        projectChanged()
                    }
                )
            }
            .sheet(isPresented: $showAgentSelector) {
                AgentSelectorSheet(
                    agents: availableAgents,
                    selectedPubkey: $draft.agentPubkey,
                    onDone: {
                        isDirty = true // Mark as dirty when user selects agent
                        if let projectId = selectedProject?.id {
                            Task {
                                await draftManager.updateAgent(draft.agentPubkey, conversationId: conversationId, projectId: projectId)
                            }
                        }
                    }
                )
            }
            .alert("Send Failed", isPresented: $showSendError) {
                Button("OK") { }
            } message: {
                Text(sendError ?? "Unknown error")
            }
            .alert("Draft Load Failed", isPresented: $showLoadFailedAlert) {
                Button("OK") {
                    // Dismiss the composer when user acknowledges the error
                    onDismiss?()
                    dismiss()
                }
            } message: {
                Text("Failed to load existing drafts. The corrupted file has been quarantined for recovery. Editing is blocked to prevent data loss. Please fix the corrupted file or restore from backup.")
            }
            .alert("Save Failed", isPresented: $showSaveFailedAlert) {
                Button("OK") { }
            } message: {
                Text("Failed to save your draft: \(saveFailedError ?? "Unknown error"). Your changes may be lost if you dismiss now. Please try again or contact support.")
            }
            .onChange(of: scenePhase) { oldPhase, newPhase in
                // CRITICAL DATA SAFETY: Flush drafts immediately when app goes to background
                // Use proper background task to guarantee completion even if app is suspended
                if newPhase == .background || newPhase == .inactive {
                    var backgroundTaskID: UIBackgroundTaskIdentifier = .invalid

                    backgroundTaskID = UIApplication.shared.beginBackgroundTask {
                        // Background time expired - clean up
                        print("[MessageComposerView] WARNING: Background save time expired")
                        if backgroundTaskID != .invalid {
                            UIApplication.shared.endBackgroundTask(backgroundTaskID)
                            backgroundTaskID = .invalid
                        }
                    }

                    Task {
                        // MEDIUM #3 FIX: Catch save errors (but can't block backgrounding - just log)
                        do {
                            try await draftManager.saveNow()
                            print("[MessageComposerView] Flushed drafts due to scene phase change: \(oldPhase) -> \(newPhase)")
                        } catch {
                            print("[MessageComposerView] ERROR: Failed to save on background: \(error)")
                            // Note: Can't show alert here as app is backgrounding
                            // Error will be surfaced on next foreground if critical
                        }

                        // End background task after save completes
                        if backgroundTaskID != .invalid {
                            UIApplication.shared.endBackgroundTask(backgroundTaskID)
                            backgroundTaskID = .invalid
                        }
                    }
                }
            }
        }
    }

    // MARK: - Subviews

    private func projectChipView(_ project: ProjectInfo) -> some View {
        HStack(spacing: 8) {
            ProjectChipView(project: project) {
                showProjectSelector = true
            }
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(Color(.systemGray6))
    }

    private var projectPromptView: some View {
        Button(action: { showProjectSelector = true }) {
            HStack(spacing: 12) {
                Image(systemName: "folder")
                    .foregroundStyle(.blue)
                Text("Select a project to start")
                    .foregroundStyle(.secondary)
                Spacer()
                Image(systemName: "chevron.right")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .background(Color(.systemGray6))
        }
        .buttonStyle(.plain)
    }

    private func agentChipView(_ agent: AgentInfo) -> some View {
        HStack(spacing: 8) {
            AgentChipView(agent: agent) {
                isDirty = true // Mark as dirty when user changes agent
                draft.clearAgent()
                if let projectId = selectedProject?.id {
                    Task {
                        await draftManager.updateAgent(nil, conversationId: conversationId, projectId: projectId)
                    }
                }
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
                isDirty = true // Mark as dirty when user edits
                draft.updateTitle(newValue)
                if let projectId = selectedProject?.id {
                    Task {
                        await draftManager.updateTitle(newValue, projectId: projectId)
                    }
                }
            }
        ))
        .font(.headline)
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .disabled((isNewConversation && selectedProject == nil) || draftManager.loadFailed || isLoadingDraft || isSwitchingProject)
        .opacity((isNewConversation && selectedProject == nil) || draftManager.loadFailed || isLoadingDraft || isSwitchingProject ? 0.5 : 1.0)
    }

    private var contentEditorView: some View {
        ZStack(alignment: .topLeading) {
            TextEditor(text: Binding(
                get: { draft.content },
                set: { newValue in
                    isDirty = true // Mark as dirty when user edits
                    draft.updateContent(newValue)
                    if let projectId = selectedProject?.id {
                        Task {
                            await draftManager.updateContent(newValue, conversationId: conversationId, projectId: projectId)
                        }
                    }
                }
            ))
            .font(.body)
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .scrollContentBackground(.hidden)
            .disabled((isNewConversation && selectedProject == nil) || draftManager.loadFailed || isLoadingDraft || isSwitchingProject)
            .opacity((isNewConversation && selectedProject == nil) || draftManager.loadFailed || isLoadingDraft || isSwitchingProject ? 0.5 : 1.0)

            if draft.content.isEmpty {
                Text(isNewConversation && selectedProject == nil
                     ? "Select a project to start composing"
                     : (isLoadingDraft ? "Loading draft..." : (isSwitchingProject ? "Switching project..." : (isNewConversation ? "What would you like to discuss?" : "Type your reply..."))))
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
            // Project selector button (only for new conversations)
            if isNewConversation {
                Button(action: { showProjectSelector = true }) {
                    HStack(spacing: 4) {
                        Image(systemName: "folder.fill")
                        if selectedProject != nil {
                            Image(systemName: "checkmark")
                                .font(.caption)
                                .fontWeight(.medium)
                        }
                    }
                    .foregroundColor(selectedProject == nil ? .secondary : .blue)
                }
                .buttonStyle(.plain)
                .padding(.vertical, 8)
                .padding(.horizontal, 12)
                .background(
                    RoundedRectangle(cornerRadius: 8)
                        .fill(selectedProject == nil ? Color.clear : Color.blue.opacity(0.1))
                )
            }

            // Agent selector button (disabled if no project selected for new conversations)
            Button(action: {
                if isNewConversation && selectedProject == nil {
                    // Show project selector first
                    showProjectSelector = true
                } else {
                    showAgentSelector = true
                }
            }) {
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
            .opacity(isNewConversation && selectedProject == nil ? 0.5 : 1.0)

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

    private func loadProjects() {
        // Only load projects if we're in new conversation mode without a project
        guard isNewConversation && initialProject == nil else { return }

        DispatchQueue.global(qos: .userInitiated).async {
            let projects = coreManager.core.getProjects()
            DispatchQueue.main.async {
                availableProjects = projects
            }
        }
    }

    private func loadDraft() {
        guard let projectId = selectedProject?.id else { return }
        // Get or create draft for this project/conversation
        // This will preserve any existing draft content for this specific project
        Task {
            let loadedDraft = await draftManager.getOrCreateDraft(conversationId: conversationId, projectId: projectId)

            // CRITICAL DATA SAFETY: Check if load failed and alert user
            if draftManager.loadFailed {
                showLoadFailedAlert = true
                isLoadingDraft = false
                return
            }

            // CRITICAL DATA SAFETY: Only apply loaded draft if user hasn't made edits yet
            // This prevents async load from overwriting live user typing
            if !isDirty {
                draft = loadedDraft
            } else {
                print("[MessageComposerView] Skipping draft load - user has already made edits (isDirty=true)")
            }

            // BLOCKER #1 FIX: Mark loading as complete to enable editing
            isLoadingDraft = false
        }
    }

    private func loadAgents() {
        guard let projectId = selectedProject?.id else { return }

        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let agents = try coreManager.core.getAgents(projectId: projectId)
                DispatchQueue.main.async {
                    availableAgents = agents
                    agentsLoadError = nil
                    if agents.isEmpty {
                        print("[MessageComposerView] No agents for this project")
                    }
                }
            } catch {
                DispatchQueue.main.async {
                    // Don't clear availableAgents on error - preserve existing data
                    // Only update error state so UI can show warning
                    agentsLoadError = error.localizedDescription
                    print("[MessageComposerView] Failed to load agents: \(error)")
                }
            }
        }
    }

    private func projectChanged() {
        guard let project = selectedProject else { return }

        Task {
            // HIGH #1 FIX: Store previous project to revert on save failure
            let previousProject = availableProjects.first { $0.id == draft.projectId }

            // BLOCKER #2 FIX: Disable editing during project switch
            isSwitchingProject = true

            // Track if user made edits during the switch operation
            let wasDirtyAtStart = isDirty

            // Save any pending changes to the current draft before switching
            if !draft.projectId.isEmpty {
                // MEDIUM #3 FIX: Catch save errors during project switch
                do {
                    try await draftManager.saveNow()
                } catch {
                    print("[MessageComposerView] ERROR: Failed to save before project switch: \(error)")
                    // HIGH #1 FIX: Revert selectedProject to prevent wrong-project editing/sending
                    selectedProject = previousProject
                    // Show error alert and abort project switch
                    saveFailedError = error.localizedDescription
                    showSaveFailedAlert = true
                    isSwitchingProject = false
                    return
                }
            }

            // Clear current in-memory state
            availableAgents = []
            agentsLoadError = nil
            isDirty = false // Reset dirty flag when switching projects

            // MEDIUM FIX: ALWAYS load fresh draft for new project to prevent cross-project content leakage
            // This is the safest approach - content should never silently carry across projects
            // Load or create draft for the new project
            let projectDraft = await draftManager.getOrCreateDraft(conversationId: conversationId, projectId: project.id)

            // Always replace draft with project-specific draft (no wasDirtyAtStart check)
            // This ensures content from Project A never persists under Project B
            draft = projectDraft
            print("[MessageComposerView] Loaded draft for project '\(project.id)' (absolute data safety: no cross-project content leakage)")

            // Validate and clear agent if it doesn't belong to this project
            // (will be validated again before sending)
            draft.clearAgent()
            await draftManager.updateAgent(nil, conversationId: conversationId, projectId: project.id)

            // Load agents for the new project
            loadAgents()

            // BLOCKER #2 FIX: Re-enable editing after project switch completes
            isSwitchingProject = false
        }
    }

    private func sendMessage() {
        guard canSend, let project = selectedProject else { return }

        // Validate agent pubkey against current project's agents before sending
        // CRITICAL: Only validate if we have a successful agent list (no load error)
        // Don't clear agent selection on transient errors - preserve user's choice
        var validatedAgentPubkey: String? = draft.agentPubkey
        if let agentPubkey = draft.agentPubkey, agentsLoadError == nil {
            // Only validate if we successfully loaded agents
            let agentExists = availableAgents.contains { $0.pubkey == agentPubkey }
            if !agentExists && !availableAgents.isEmpty {
                // Only clear if we have agents but this one isn't in the list
                // If availableAgents is empty, the project might have no agents (valid state)
                print("[MessageComposerView] Warning: Agent pubkey '\(agentPubkey)' not found in current project's agents. Clearing agent selection.")
                // Clear invalid agent from draft
                draft.clearAgent()
                Task {
                    await draftManager.updateAgent(nil, conversationId: conversationId, projectId: project.id)
                }
                validatedAgentPubkey = nil
            }
        }

        isSending = true
        sendError = nil

        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let result: SendMessageResult

                if isNewConversation {
                    result = try coreManager.core.sendThread(
                        projectId: project.id,
                        title: draft.title,
                        content: draft.content,
                        agentPubkey: validatedAgentPubkey
                    )
                } else {
                    result = try coreManager.core.sendMessage(
                        conversationId: conversationId!,
                        projectId: project.id,
                        content: draft.content,
                        agentPubkey: validatedAgentPubkey
                    )
                }

                DispatchQueue.main.async {
                    isSending = false

                    // Clear draft on success
                    Task {
                        await draftManager.deleteDraft(conversationId: conversationId, projectId: project.id)
                        // Notify and dismiss after delete completes
                        onSend?(result)
                        dismiss()
                    }
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
        // CRITICAL DATA SAFETY: Do NOT reset isDirty on clear
        // This prevents async load from overwriting the user's intentional clear (delete)
        // isDirty = false // REMOVED - keep isDirty=true to protect against async load
        draft.clear()
        if let projectId = selectedProject?.id {
            Task {
                await draftManager.clearDraft(conversationId: conversationId, projectId: projectId)
            }
        }
    }
}

// MARK: - Project Chip View

struct ProjectChipView: View {
    let project: ProjectInfo
    let onChange: () -> Void

    var body: some View {
        HStack(spacing: 6) {
            // Project icon
            RoundedRectangle(cornerRadius: 4)
                .fill(projectColor.gradient)
                .frame(width: 24, height: 24)
                .overlay {
                    Image(systemName: "folder.fill")
                        .font(.caption2)
                        .foregroundStyle(.white)
                }

            // Project title
            Text(project.title)
                .font(.subheadline)
                .fontWeight(.medium)
                .foregroundStyle(.primary)

            // Change button
            Button(action: onChange) {
                Image(systemName: "arrow.triangle.2.circlepath")
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

    /// Deterministic color using shared utility (stable across app launches)
    private var projectColor: Color {
        deterministicColor(for: project.id)
    }
}

// MARK: - Agent Chip View

struct AgentChipView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    let agent: AgentInfo
    let onRemove: () -> Void

    var body: some View {
        HStack(spacing: 6) {
            // Agent avatar using unified component
            AgentAvatarView(
                agentName: agent.name,
                pubkey: agent.pubkey,
                fallbackPictureUrl: agent.picture,
                size: 24,
                showBorder: false
            )
            .environmentObject(coreManager)

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
