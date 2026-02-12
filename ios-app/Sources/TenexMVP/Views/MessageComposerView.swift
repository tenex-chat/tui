import SwiftUI
#if os(iOS)
import UIKit
#endif
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

    /// Initial agent pubkey to pre-select (e.g., last agent that spoke in conversation)
    let initialAgentPubkey: String?

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
    @State private var showProjectSelector = false
    @State private var draft: Draft
    @State private var availableAgents: [OnlineAgentInfo] = []
    @State private var agentsLoadError: String?
    @State private var showAgentSelector = false
    @State private var replyTargetAgentName: String?  // Agent name for reply target (resolved from initialAgentPubkey)
    @State private var availableNudges: [NudgeInfo] = []
    @State private var showNudgeSelector = false
    @State private var isSending = false
    @State private var sendError: String?
    @State private var showSendError = false
    @State private var isDirty = false // Track if user has made edits before load completes
    @State private var showLoadFailedAlert = false
    @State private var isLoadingDraft = true // Track if draft is still loading
    @State private var isSwitchingProject = false // Track if project switch is in progress
    @State private var showSaveFailedAlert = false
    @State private var saveFailedError: String?
    @State private var dictationManager = DictationManager()
    @State private var showDictationOverlay = false

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

    private var selectedAgent: OnlineAgentInfo? {
        guard let pubkey = draft.agentPubkey else { return nil }
        return availableAgents.first { $0.pubkey == pubkey }
    }

    private var selectedNudges: [NudgeInfo] {
        availableNudges.filter { draft.selectedNudgeIds.contains($0.id) }
    }

    /// Find the project with the most recent conversation activity
    private func projectWithMostRecentActivity() -> ProjectInfo? {
        guard !coreManager.conversations.isEmpty else { return nil }

        let mostRecentConv = coreManager.conversations
            .max(by: { $0.effectiveLastActivity < $1.effectiveLastActivity })

        guard let conv = mostRecentConv else { return nil }
        // projectATag is "kind:pubkey:d-tag", extract d-tag to match project.id
        let projectId = conv.projectATag.split(separator: ":").dropFirst(2).joined(separator: ":")
        return coreManager.projects.first { $0.id == projectId }
    }

    // MARK: - Initialization

    init(
        project: ProjectInfo? = nil,
        conversationId: String? = nil,
        conversationTitle: String? = nil,
        initialAgentPubkey: String? = nil,
        onSend: ((SendMessageResult) -> Void)? = nil,
        onDismiss: (() -> Void)? = nil
    ) {
        self.initialProject = project
        self.conversationId = conversationId
        self.conversationTitle = conversationTitle
        self.initialAgentPubkey = initialAgentPubkey
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
                // Project and Agent row (for new conversations)
                if isNewConversation {
                    HStack(spacing: 12) {
                        if let project = selectedProject {
                            ProjectChipView(project: project) {
                                showProjectSelector = true
                            }
                        } else {
                            projectPromptButton
                        }

                        if selectedProject != nil {
                            if let agent = selectedAgent {
                                OnlineAgentChipView(agent: agent) {
                                    showAgentSelector = true
                                }
                                .environmentObject(coreManager)
                            } else {
                                agentPromptButton
                            }
                        }

                        Spacer()
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .background(Color.systemGray6)
                }

                // Agent chip for replies (not new conversations)
                if !isNewConversation {
                    if let agent = selectedAgent {
                        agentChipView(agent)
                    } else if let targetPubkey = initialAgentPubkey, let targetName = replyTargetAgentName {
                        // Show the reply target even if they're not in online agents list
                        replyTargetChipView(name: targetName, pubkey: targetPubkey) {
                            showAgentSelector = true
                        }
                    } else if selectedProject != nil {
                        agentPromptView
                    }
                }

                // Nudge chips (for new conversations)
                if isNewConversation && selectedProject != nil {
                    nudgeChipsView
                }

                Divider()

                // Content editor
                contentEditorView

                Divider()

                // Toolbar
                toolbarView
            }
            #if os(iOS)
            .overlay {
                if showDictationOverlay {
                    DictationOverlayView(
                        manager: dictationManager,
                        onComplete: { text in
                            draft.updateContent(draft.content + (draft.content.isEmpty ? "" : " ") + text)
                            if let projectId = selectedProject?.id {
                                Task {
                                    await draftManager.updateContent(draft.content, conversationId: conversationId, projectId: projectId)
                                }
                            }
                            showDictationOverlay = false
                            dictationManager.reset()
                        },
                        onCancel: {
                            dictationManager.cancelRecording()
                            showDictationOverlay = false
                        }
                    )
                }
            }
            #endif
            .navigationTitle(isNewConversation ? "New Conversation" : "Reply")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        #if os(iOS)
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

                                saveFailedError = error.localizedDescription
                                showSaveFailedAlert = true
                            }
                        }
                        #else
                        Task {
                            do {
                                try await draftManager.saveNow()
                                onDismiss?()
                                dismiss()
                            } catch {
                                saveFailedError = error.localizedDescription
                                showSaveFailedAlert = true
                            }
                        }
                        #endif
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
                // Auto-select project with most recent activity if none provided
                if selectedProject == nil && initialProject == nil {
                    if let mostActiveProject = projectWithMostRecentActivity() {
                        selectedProject = mostActiveProject
                        draft = Draft(projectId: mostActiveProject.id)
                    }
                }

                // BLOCKER #1 FIX: If no project selected, no draft to load - mark as not loading
                if selectedProject == nil {
                    isLoadingDraft = false
                } else {
                    loadDraft()
                    loadAgents()
                    loadNudges()
                }
            }
            .sheet(isPresented: $showProjectSelector) {
                ProjectSelectorSheet(
                    projects: coreManager.projects,
                    projectOnlineStatus: coreManager.projectOnlineStatus,
                    selectedProject: $selectedProject,
                    onDone: {
                        projectChanged()
                    }
                )
            }
            .sheet(isPresented: $showAgentSelector) {
                AgentSelectorSheet(
                    agents: availableAgents,
                    projectId: selectedProject?.id ?? "",
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
            .sheet(isPresented: $showNudgeSelector) {
                NudgeSelectorSheet(
                    nudges: availableNudges,
                    selectedNudgeIds: $draft.selectedNudgeIds,
                    onDone: {
                        isDirty = true // Mark as dirty when user selects nudges
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
                // Flush drafts immediately when app goes to background
                if newPhase == .background || newPhase == .inactive {
                    #if os(iOS)
                    var backgroundTaskID: UIBackgroundTaskIdentifier = .invalid

                    backgroundTaskID = UIApplication.shared.beginBackgroundTask {
                        print("[MessageComposerView] WARNING: Background save time expired")
                        if backgroundTaskID != .invalid {
                            UIApplication.shared.endBackgroundTask(backgroundTaskID)
                            backgroundTaskID = .invalid
                        }
                    }

                    Task {
                        do {
                            try await draftManager.saveNow()
                            print("[MessageComposerView] Flushed drafts due to scene phase change: \(oldPhase) -> \(newPhase)")
                        } catch {
                            print("[MessageComposerView] ERROR: Failed to save on background: \(error)")
                        }

                        if backgroundTaskID != .invalid {
                            UIApplication.shared.endBackgroundTask(backgroundTaskID)
                            backgroundTaskID = .invalid
                        }
                    }
                    #else
                    Task {
                        do {
                            try await draftManager.saveNow()
                            print("[MessageComposerView] Flushed drafts due to scene phase change: \(oldPhase) -> \(newPhase)")
                        } catch {
                            print("[MessageComposerView] ERROR: Failed to save on background: \(error)")
                        }
                    }
                    #endif
                }
            }
            .onChange(of: coreManager.onlineAgents) { oldAgents, newAgents in
                // Reactively update availableAgents when centralized state changes
                // This eliminates the need for manual refresh() calls
                if let projectId = selectedProject?.id {
                    let agents = newAgents[projectId] ?? []
                    print("[MessageComposerView] onChange(onlineAgents): projectId='\(projectId)' agents.count=\(agents.count)")
                    availableAgents = agents
                    print("[MessageComposerView] Updated availableAgents to \(availableAgents.count) agents")
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
        .background(Color.systemGray6)
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
            .background(Color.systemGray6)
        }
        .buttonStyle(.plain)
    }

    private var agentPromptView: some View {
        Button(action: { showAgentSelector = true }) {
            HStack(spacing: 12) {
                Image(systemName: "person")
                    .foregroundStyle(.blue)
                Text("Select an agent (optional)")
                    .foregroundStyle(.secondary)
                Spacer()
                Image(systemName: "chevron.right")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .background(Color.systemGray6)
        }
        .buttonStyle(.plain)
    }

    /// Compact project prompt button for horizontal layout
    private var projectPromptButton: some View {
        Button(action: { showProjectSelector = true }) {
            HStack(spacing: 6) {
                Image(systemName: "folder")
                    .font(.caption)
                    .foregroundStyle(.blue)
                Text("Select project")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(
                Capsule()
                    .strokeBorder(Color.secondary.opacity(0.3), lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }

    /// Compact agent prompt button for horizontal layout
    private var agentPromptButton: some View {
        Button(action: { showAgentSelector = true }) {
            HStack(spacing: 6) {
                Image(systemName: "person")
                    .font(.caption)
                    .foregroundStyle(.blue)
                Text("Select agent")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(
                Capsule()
                    .strokeBorder(Color.secondary.opacity(0.3), lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }

    private func agentChipView(_ agent: OnlineAgentInfo) -> some View {
        HStack(spacing: 8) {
            OnlineAgentChipView(agent: agent) {
                showAgentSelector = true
            }
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(Color.systemGray6)
    }

    /// Shows the reply target agent (used when replying and the agent isn't in online agents list)
    private func replyTargetChipView(name: String, pubkey: String, onChange: @escaping () -> Void) -> some View {
        HStack(spacing: 8) {
            Button(action: onChange) {
                HStack(spacing: 6) {
                    AgentAvatarView(
                        agentName: name,
                        pubkey: pubkey,
                        size: 24,
                        showBorder: false
                    )
                    .environmentObject(coreManager)

                    Text("@\(name)")
                        .font(.subheadline)
                        .fontWeight(.medium)
                        .foregroundStyle(.primary)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .background(
                    Capsule()
                        .fill(Color.systemBackground)
                        .shadow(color: .black.opacity(0.1), radius: 2, x: 0, y: 1)
                )
            }
            .buttonStyle(.plain)
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(Color.systemGray6)
    }

    private var nudgeChipsView: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                // Selected nudge chips
                ForEach(selectedNudges, id: \.id) { nudge in
                    NudgeChipView(nudge: nudge) {
                        isDirty = true
                        draft.removeNudge(nudge.id)
                    }
                }

                // Add nudge button
                Button(action: { showNudgeSelector = true }) {
                    HStack(spacing: 4) {
                        Image(systemName: "plus")
                            .font(.caption)
                        Text("Add Nudge")
                            .font(.caption)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background(
                        Capsule()
                            .strokeBorder(Color.secondary.opacity(0.3), lineWidth: 1)
                    )
                    .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 16)
        }
        .padding(.vertical, 12)
        .background(Color.systemGray6)
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
            // Show error indicator if agents failed to load
            if agentsLoadError != nil {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(.orange)
                    .font(.caption)
            }

            #if os(iOS)
            // Voice dictation button
            Button {
                Task {
                    showDictationOverlay = true
                    try? await dictationManager.startRecording()
                }
            } label: {
                Image(systemName: "mic.fill")
                    .foregroundStyle(.blue)
            }
            .buttonStyle(.plain)
            .disabled(!dictationManager.state.isIdle || selectedProject == nil)
            #endif

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
        .background(Color.systemBackground)
    }

    // MARK: - Actions

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

        Task {
            // Use centralized cached agents instead of fetching on-demand
            // This eliminates multi-second FFI delays
            let agents = coreManager.onlineAgents[projectId] ?? []
            print("[MessageComposerView] loadAgents() for projectId='\(projectId)': found \(agents.count) agents in cache")
            availableAgents = agents
            print("[MessageComposerView] Set availableAgents to \(availableAgents.count) agents")
            agentsLoadError = nil

            // For replies: always use initialAgentPubkey - we know who we're replying to
            if !isNewConversation, let initialPubkey = initialAgentPubkey {
                draft.setAgent(initialPubkey)
                await draftManager.updateAgent(initialPubkey, conversationId: conversationId, projectId: projectId)

                // Resolve the agent name for display (even if not online)
                let name = coreManager.safeCore.getProfileName(pubkey: initialPubkey)
                replyTargetAgentName = name.isEmpty ? "Agent" : name
            } else if draft.agentPubkey == nil {
                // New conversation: auto-select PM agent if available
                if let pmAgent = agents.first(where: { $0.isPm }) {
                    draft.setAgent(pmAgent.pubkey)
                    await draftManager.updateAgent(pmAgent.pubkey, conversationId: conversationId, projectId: projectId)
                }
            }

            if agents.isEmpty {
                print("[MessageComposerView] No online agents for this project (using cached state)")
            }
        }
    }

    private func loadNudges() {
        Task {
            // No refresh needed - use data already available from centralized state
            do {
                availableNudges = try await coreManager.safeCore.getNudges()
            } catch {
                print("[MessageComposerView] Failed to load nudges: \(error)")
            }
        }
    }

    private func projectChanged() {
        guard let project = selectedProject else { return }

        Task {
            // HIGH #1 FIX: Store previous project to revert on save failure
            let previousProject = coreManager.projects.first { $0.id == draft.projectId }

            // BLOCKER #2 FIX: Disable editing during project switch
            isSwitchingProject = true

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

            // Always replace draft with project-specific draft
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

        Task {
            do {
                let result: SendMessageResult

                if isNewConversation {
                    result = try await coreManager.safeCore.sendThread(
                        projectId: project.id,
                        title: "",
                        content: draft.content,
                        agentPubkey: validatedAgentPubkey,
                        nudgeIds: Array(draft.selectedNudgeIds)
                    )
                } else {
                    result = try await coreManager.safeCore.sendMessage(
                        conversationId: conversationId!,
                        projectId: project.id,
                        content: draft.content,
                        agentPubkey: validatedAgentPubkey
                    )
                }

                isSending = false

                // Clear draft on success
                await draftManager.deleteDraft(conversationId: conversationId, projectId: project.id)
                // Notify and dismiss after delete completes
                onSend?(result)
                dismiss()
            } catch {
                isSending = false
                sendError = error.localizedDescription
                showSendError = true
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
        Button(action: onChange) {
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
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(
                Capsule()
                    .fill(Color.systemBackground)
                    .shadow(color: .black.opacity(0.1), radius: 2, x: 0, y: 1)
            )
        }
        .buttonStyle(.plain)
        .contentShape(Capsule())
    }

    /// Deterministic color using shared utility (stable across app launches)
    private var projectColor: Color {
        deterministicColor(for: project.id)
    }
}

// MARK: - Online Agent Chip View

struct OnlineAgentChipView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    let agent: OnlineAgentInfo
    let onChange: () -> Void

    var body: some View {
        Button(action: onChange) {
            HStack(spacing: 6) {
                // Agent avatar - uses actual agent pubkey for profile lookup
                AgentAvatarView(
                    agentName: agent.name,
                    pubkey: agent.pubkey,
                    size: 24,
                    showBorder: false
                )
                .environmentObject(coreManager)

                // Agent name
                Text("@\(agent.name)")
                    .font(.subheadline)
                    .fontWeight(.medium)
                    .foregroundStyle(.primary)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(
                Capsule()
                    .fill(Color.systemBackground)
                    .shadow(color: .black.opacity(0.1), radius: 2, x: 0, y: 1)
            )
        }
        .buttonStyle(.plain)
        .contentShape(Capsule())
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
