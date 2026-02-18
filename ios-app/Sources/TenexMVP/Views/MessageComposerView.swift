import SwiftUI
#if os(iOS)
import UIKit
#endif
import CryptoKit
import UniformTypeIdentifiers

/// A premium message composition view for both new conversations and replies.
/// Supports project selection (for new conversations), agent selection, draft persistence, and markdown input.
struct MessageComposerView: View {
    enum DisplayStyle {
        case modal
        case inline
    }

    enum InlineLayoutStyle {
        case standard
        case workspace
    }

    // MARK: - Properties

    /// The project this message belongs to (nil for new conversations with project selection)
    let initialProject: ProjectInfo?

    /// The conversation ID if replying to an existing thread (nil for new thread)
    let conversationId: String?

    /// Optional conversation title for display
    let conversationTitle: String?

    /// Initial agent pubkey to pre-select (e.g., last agent that spoke in conversation)
    let initialAgentPubkey: String?

    /// Initial content to pre-populate the composer with (e.g., context message for conversation reference)
    let initialContent: String?

    /// Reference conversation ID for context tagging (adds ["context", "<id>"] tag when sent)
    let referenceConversationId: String?

    /// Reference report a-tag for context tagging (adds ["context", "<a-tag>"] tag when sent)
    /// Format: "30023:<pubkey>:<slug>" - the standard Nostr a-tag for addressable events
    /// Used by "Chat with Author" feature to reference the report being discussed
    let referenceReportATag: String?

    /// Callback when message is sent successfully
    var onSend: ((SendMessageResult) -> Void)?

    /// Callback when the view is dismissed
    var onDismiss: (() -> Void)?

    /// Rendering style for the composer container
    let displayStyle: DisplayStyle

    /// Inline layout variant (used to keep iPhone behavior unchanged while improving workspace)
    let inlineLayoutStyle: InlineLayoutStyle

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
    @State private var availableSkills: [SkillInfo] = []
    @State private var showNudgeSkillSelector = false
    @State private var nudgeSkillSelectorInitialMode: NudgeSkillSelectorMode = .all
    @State private var nudgeSkillSelectorInitialQuery: String = ""
    @State private var agentSelectorInitialQuery: String = ""
    @State private var isSending = false
    @State private var sendError: String?
    @State private var showSendError = false
    @State private var isDirty = false // Track if user has made edits before load completes
    @State private var showLoadFailedAlert = false
    @State private var isSwitchingProject = false // Track if project switch is in progress
    @State private var showSaveFailedAlert = false
    @State private var saveFailedError: String?
    @State private var dictationManager = DictationManager()
    @State private var showDictationOverlay = false

    // Image attachment state
    @State private var showImagePicker = false
    @State private var isUploadingImage = false
    @State private var imageUploadError: String?
    @State private var showImageUploadError = false
    @State private var isDropTargeted = false
    /// Local image attachments synced with draft for UI display
    @State private var localImageAttachments: [ImageAttachment] = []

    // PERFORMANCE FIX: Local text state for instant typing response
    // This decouples TextEditor binding from draft persistence to eliminate per-keystroke lag
    @State private var localText: String = ""
    @State private var contentSyncTask: Task<Void, Never>?

    // Flag to suppress onChange sync during programmatic localText updates (load/switch/dictation)
    @State private var isProgrammaticUpdate: Bool = false
    @State private var triggerDetectionTask: Task<Void, Never>?

    // Workspace inline layout metrics
    @ScaledMetric(relativeTo: .body) private var workspaceContextRowHeight: CGFloat = 44
    @ScaledMetric(relativeTo: .body) private var workspaceBottomRowHeight: CGFloat = 46
    @ScaledMetric(relativeTo: .body) private var workspaceIconBoxSize: CGFloat = 18
    @ScaledMetric(relativeTo: .body) private var workspaceIconSize: CGFloat = 14
    @ScaledMetric(relativeTo: .body) private var workspaceSendButtonSize: CGFloat = 28

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

        // HIGH #2 FIX: Block sending during project switch to prevent stale draft + new project
        guard !isSwitchingProject else {
            return false
        }

        // PERFORMANCE FIX: Use localText for instant send button feedback
        // Draft sync may be pending, but localText always has current content
        let hasTextContent = !localText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        let hasImageContent = !localImageAttachments.isEmpty
        // Either text or images is sufficient to send
        return (hasTextContent || hasImageContent) && !isSending && !isUploadingImage
    }

    private var selectedAgent: OnlineAgentInfo? {
        guard let pubkey = draft.agentPubkey else { return nil }
        return availableAgents.first { $0.pubkey == pubkey }
    }

    private var selectedNudges: [NudgeInfo] {
        availableNudges.filter { draft.selectedNudgeIds.contains($0.id) }
    }

    private var selectedSkills: [SkillInfo] {
        availableSkills.filter { draft.selectedSkillIds.contains($0.id) }
    }

    private var isInlineComposer: Bool {
        displayStyle == .inline
    }

    private var usesWorkspaceInlineLayout: Bool {
        isInlineComposer && inlineLayoutStyle == .workspace
    }

    /// Hide scheduled conversations preference (synced with ConversationsTabView)
    @AppStorage("hideScheduled") private var hideScheduled = true

    /// Find the project with the most recent conversation activity
    /// Respects hideScheduled preference to match prior behavior
    private func projectWithMostRecentActivity() -> ProjectInfo? {
        var candidates = coreManager.conversations

        // When hideScheduled is enabled, exclude scheduled conversations
        // to match the filtering behavior in ConversationsTabView
        if hideScheduled {
            candidates = candidates.filter { !$0.isScheduled }
        }

        guard !candidates.isEmpty else { return nil }

        let mostRecentConv = candidates
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
        initialContent: String? = nil,
        referenceConversationId: String? = nil,
        referenceReportATag: String? = nil,
        displayStyle: DisplayStyle = .modal,
        inlineLayoutStyle: InlineLayoutStyle = .standard,
        onSend: ((SendMessageResult) -> Void)? = nil,
        onDismiss: (() -> Void)? = nil
    ) {
        self.initialProject = project
        self.conversationId = conversationId
        self.conversationTitle = conversationTitle
        self.initialAgentPubkey = initialAgentPubkey
        self.initialContent = initialContent
        self.referenceConversationId = referenceConversationId
        self.referenceReportATag = referenceReportATag
        self.displayStyle = displayStyle
        self.inlineLayoutStyle = inlineLayoutStyle
        self.onSend = onSend
        self.onDismiss = onDismiss

        // Initialize state with project if provided
        _selectedProject = State(initialValue: project)

        // Initialize draft (will be updated in onAppear)
        if let conversationId = conversationId, let projectId = project?.id {
            _draft = State(initialValue: Draft(conversationId: conversationId, projectId: projectId, referenceConversationId: referenceConversationId, referenceReportATag: referenceReportATag))
        } else if let projectId = project?.id {
            _draft = State(initialValue: Draft(projectId: projectId, content: initialContent ?? "", referenceConversationId: referenceConversationId, referenceReportATag: referenceReportATag))
        } else {
            // No project yet - will be set when project is selected
            _draft = State(initialValue: Draft(projectId: "", content: initialContent ?? "", referenceConversationId: referenceConversationId, referenceReportATag: referenceReportATag))
        }

        // Initialize localText with initial content if provided
        if let content = initialContent {
            _localText = State(initialValue: content)
        }
    }

    // MARK: - Body

    var body: some View {
        composerViewWithLifecycle
    }

    @ViewBuilder
    private var composerBaseView: some View {
        Group {
            if isInlineComposer {
                composerContent
            } else {
                NavigationStack {
                    composerContent
                        .navigationTitle(isNewConversation ? "New Conversation" : "Reply")
                        .navigationBarTitleDisplayMode(.inline)
                        .toolbar {
                            ToolbarItem(placement: .cancellationAction) {
                                Button("Cancel") {
                                    #if os(iOS)
                                    // CRITICAL DATA SAFETY: Use background task to guarantee save completes
                                    var backgroundTaskID: UIBackgroundTaskIdentifier = .invalid

                                    backgroundTaskID = UIApplication.shared.beginBackgroundTask {
                                        if backgroundTaskID != .invalid {
                                            UIApplication.shared.endBackgroundTask(backgroundTaskID)
                                            backgroundTaskID = .invalid
                                        }
                                    }

                                    Task {
                                        do {
                                            // HIGH FIX: Flush localText to DraftManager before saving
                                            // This prevents losing the last ~300ms of typing
                                            await flushLocalTextToDraftManager()
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
                                            // HIGH FIX: Flush localText to DraftManager before saving
                                            await flushLocalTextToDraftManager()
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
                }
                .tenexModalPresentation(detents: [.large])
            }
        }
    }

    private var composerViewWithDialogs: some View {
        composerBaseView
        .onAppear {
            // Auto-select project with most recent activity if none provided
            if selectedProject == nil && initialProject == nil {
                if let mostActiveProject = projectWithMostRecentActivity() {
                    selectedProject = mostActiveProject
                    draft = Draft(projectId: mostActiveProject.id)
                }
            }

            if selectedProject != nil {
                loadDraft()
                loadAgents()
                loadNudges()
                loadSkills()
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
                },
                initialSearchQuery: agentSelectorInitialQuery
            )
        }
        .sheet(isPresented: $showNudgeSkillSelector) {
            NudgeSkillSelectorSheet(
                nudges: availableNudges,
                skills: availableSkills,
                selectedNudgeIds: $draft.selectedNudgeIds,
                selectedSkillIds: $draft.selectedSkillIds,
                initialMode: nudgeSkillSelectorInitialMode,
                initialSearchQuery: nudgeSkillSelectorInitialQuery,
                onDone: {
                    isDirty = true // Mark as dirty when user selects nudges/skills
                    persistSelectedNudgeIds()
                    persistSelectedSkillIds()
                }
            )
        }
        #if os(iOS)
        .sheet(isPresented: $showImagePicker) {
            ImagePicker { imageData, mimeType in
                handleImageSelected(data: imageData, mimeType: mimeType)
            }
        }
        #endif
        .alert("Image Upload Failed", isPresented: $showImageUploadError) {
            Button("OK") { }
        } message: {
            Text(imageUploadError ?? "Unknown error")
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
    }

    private var composerViewWithLifecycle: some View {
        composerViewWithDialogs
        .onChange(of: scenePhase) { oldPhase, newPhase in
            // Flush drafts immediately when app goes to background
            if newPhase == .background || newPhase == .inactive {
                #if os(iOS)
                var backgroundTaskID: UIBackgroundTaskIdentifier = .invalid

                backgroundTaskID = UIApplication.shared.beginBackgroundTask {
                    if backgroundTaskID != .invalid {
                        UIApplication.shared.endBackgroundTask(backgroundTaskID)
                        backgroundTaskID = .invalid
                    }
                }

                Task {
                    do {
                        // HIGH FIX: Flush localText to DraftManager before saving
                        await flushLocalTextToDraftManager()
                        try await draftManager.saveNow()
                    } catch {
                    }

                    if backgroundTaskID != .invalid {
                        UIApplication.shared.endBackgroundTask(backgroundTaskID)
                        backgroundTaskID = .invalid
                    }
                }
                #else
                Task {
                    do {
                        // HIGH FIX: Flush localText to DraftManager before saving
                        await flushLocalTextToDraftManager()
                        try await draftManager.saveNow()
                    } catch {
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
                availableAgents = agents
            }
        }
    }

    @ViewBuilder
    private var composerContent: some View {
        VStack(spacing: 0) {
            if !usesWorkspaceInlineLayout {
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
                                    openAgentSelector()
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
                    .background(.bar)
                }

                // Agent chip for replies (not new conversations)
                if !isNewConversation {
                    if let agent = selectedAgent {
                        agentChipView(agent)
                    } else if let targetPubkey = initialAgentPubkey, let targetName = replyTargetAgentName {
                        // Show the reply target even if they're not in online agents list
                        replyTargetChipView(name: targetName, pubkey: targetPubkey) {
                            openAgentSelector()
                        }
                    } else if selectedProject != nil {
                        agentPromptView
                    }
                }

                // Nudge chips (for all conversations)
                if selectedProject != nil {
                    nudgeChipsView
                }

                // Skill chips (for all conversations)
                if selectedProject != nil {
                    skillChipsView
                }
            }

            // Image attachment chips (for all conversations)
            if !localImageAttachments.isEmpty && selectedProject != nil {
                imageAttachmentChipsView
            }

            if usesWorkspaceInlineLayout {
                // Workspace mode uses explicit rows and avoids stacked divider artifacts.
                contentEditorView
                workspaceInlineControlRow
            } else {
                Divider()

                // Content editor
                contentEditorView

                Divider()

                // Toolbar
                toolbarView
            }
        }
        #if os(iOS)
        .overlay {
            if showDictationOverlay {
                DictationOverlayView(
                    manager: dictationManager,
                    onComplete: { text in
                        // Append dictated text to localText (instant update)
                        let appendedText = localText + (localText.isEmpty ? "" : " ") + text
                        localText = appendedText
                        // Dictation is user-initiated content, so mark dirty and sync
                        isDirty = true
                        // Sync to draft directly (bypass onChange which is suppressed during programmatic updates)
                        if let projectId = selectedProject?.id {
                            Task {
                                await draftManager.updateContent(appendedText, conversationId: conversationId, projectId: projectId)
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
        #if os(macOS)
        .onDrop(of: [UTType.fileURL], isTargeted: $isDropTargeted) { providers in
            handleFileDrop(providers: providers)
        }
        #endif
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
        .background(.bar)
    }

    private var projectPromptView: some View {
        Button(action: { showProjectSelector = true }) {
            HStack(spacing: 12) {
                Image(systemName: "folder")
                    .foregroundStyle(Color.composerAction)
                Text("Select a project to start")
                    .foregroundStyle(.secondary)
                Spacer()
                Image(systemName: "chevron.right")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .background(.bar)
        }
        .buttonStyle(.borderless)
    }

    private var agentPromptView: some View {
        Button(action: { openAgentSelector() }) {
            HStack(spacing: 12) {
                Image(systemName: "person")
                    .foregroundStyle(Color.composerAction)
                Text("Select an agent (optional)")
                    .foregroundStyle(.secondary)
                Spacer()
                Image(systemName: "chevron.right")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .background(.bar)
        }
        .buttonStyle(.borderless)
    }

    /// Compact project prompt button for horizontal layout
    private var projectPromptButton: some View {
        Button(action: { showProjectSelector = true }) {
            HStack(spacing: 6) {
                Image(systemName: "folder")
                    .font(.caption)
                    .foregroundStyle(Color.composerAction)
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
        .buttonStyle(.borderless)
    }

    /// Compact agent prompt button for horizontal layout
    private var agentPromptButton: some View {
        Button(action: { openAgentSelector() }) {
            HStack(spacing: 6) {
                Image(systemName: "person")
                    .font(.caption)
                    .foregroundStyle(Color.composerAction)
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
        .buttonStyle(.borderless)
    }

    private func agentChipView(_ agent: OnlineAgentInfo) -> some View {
        HStack(spacing: 8) {
            OnlineAgentChipView(agent: agent) {
                openAgentSelector()
            }
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(.bar)
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
            .buttonStyle(.borderless)
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(.bar)
    }

    private var nudgeChipsView: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                // Selected nudge chips
                ForEach(selectedNudges, id: \.id) { nudge in
                    NudgeChipView(nudge: nudge) {
                        isDirty = true
                        draft.removeNudge(nudge.id)
                        persistSelectedNudgeIds()
                    }
                }

                // Add nudge button
                Button(action: { openNudgeSkillSelector(mode: .nudges) }) {
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
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 16)
        }
        .padding(.vertical, 12)
        .background(.bar)
    }

    private var skillChipsView: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                // Selected skill chips
                ForEach(selectedSkills, id: \.id) { skill in
                    SkillChipView(skill: skill) {
                        isDirty = true
                        draft.removeSkill(skill.id)
                        persistSelectedSkillIds()
                    }
                }

                // Add skill button
                Button(action: { openNudgeSkillSelector(mode: .skills) }) {
                    HStack(spacing: 4) {
                        Image(systemName: "plus")
                            .font(.caption)
                        Text("Add Skill")
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
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 16)
        }
        .padding(.vertical, 12)
        .background(.bar)
    }

    private var workspaceInlineControlRow: some View {
        HStack(spacing: 12) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 10) {
                    if let project = selectedProject {
                        inlineContextToken(icon: "folder.fill", text: project.title) {
                            if isNewConversation {
                                showProjectSelector = true
                            }
                        }
                    } else if isNewConversation {
                        inlineContextToken(icon: "folder.badge.questionmark", text: "Select project") {
                            showProjectSelector = true
                        }
                    }

                    if let agent = selectedAgent {
                        inlineContextToken(icon: "person.crop.circle", text: agentContextSummary(agent: agent)) {
                            openAgentSelector()
                        }
                    } else if let targetPubkey = initialAgentPubkey, let targetName = replyTargetAgentName {
                        inlineContextToken(icon: "person.crop.circle", text: targetName) {
                            // Keep this reply path explicit even when target is offline.
                            draft.setAgent(targetPubkey)
                            openAgentSelector()
                        }
                    } else if selectedProject != nil {
                        inlineContextToken(icon: "person.crop.circle.badge.questionmark", text: "Agent") {
                            openAgentSelector()
                        }
                    }

                    inlineContextToken(text: nudgeSkillContextSummary) {
                        openNudgeSkillSelector(mode: .all)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            Button(action: sendMessage) {
                Image(systemName: "arrow.up")
                    .font(.system(size: workspaceIconSize, weight: .semibold))
                    .foregroundStyle(canSend ? Color.primary : Color.secondary.opacity(0.9))
                    .frame(width: workspaceSendButtonSize, height: workspaceSendButtonSize)
                    .background(
                        Circle()
                            .fill(canSend ? Color.secondary.opacity(0.28) : Color.secondary.opacity(0.16))
                    )
            }
            .buttonStyle(.borderless)
            .disabled(!canSend)
            #if os(macOS)
            .keyboardShortcut(.return, modifiers: [.command])
            #endif
            .help("Send")
        }
        .frame(height: max(workspaceContextRowHeight, workspaceBottomRowHeight))
        .padding(.horizontal, 16)
        .background(Color.systemBackground)
    }

    private func inlineContextToken(icon: String? = nil, text: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 6) {
                if let icon {
                    Image(systemName: icon)
                        .font(.system(size: workspaceIconSize, weight: .medium))
                        .frame(width: workspaceIconBoxSize, height: workspaceIconBoxSize)
                        .foregroundStyle(.secondary)
                }
                Text(text)
                    .font(.caption)
                    .lineLimit(1)
                    .foregroundStyle(.primary)
            }
            .padding(.horizontal, 2)
            .padding(.vertical, 2)
            .frame(height: workspaceContextRowHeight - 16)
            .contentShape(Rectangle())
        }
        .buttonStyle(.borderless)
    }

    private func agentContextSummary(agent: OnlineAgentInfo) -> String {
        if let model = agent.model, !model.isEmpty {
            return "\(agent.name) (\(model))"
        }
        return agent.name
    }

    private var nudgeSkillContextSummary: String {
        let selectedCount = selectedNudges.count + selectedSkills.count
        guard selectedCount > 0 else { return "Shortcuts" }
        return selectedCount == 1 ? "1 selected" : "\(selectedCount) selected"
    }

    private var composerPlaceholderText: String {
        if isNewConversation && selectedProject == nil {
            return "Select a project to start composing"
        }
        if isSwitchingProject {
            return "Switching project..."
        }
        return isNewConversation ? "What would you like to discuss?" : "Type your reply..."
    }

    private var imageAttachmentChipsView: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(localImageAttachments) { attachment in
                    ImageAttachmentChipView(attachment: attachment) {
                        removeImageAttachment(id: attachment.id)
                    }
                }
            }
            .padding(.horizontal, 16)
        }
        .padding(.vertical, 12)
        .background(.bar)
    }

    private var contentEditorView: some View {
        ZStack(alignment: .topLeading) {
            if usesWorkspaceInlineLayout {
                // Native multiline TextField keeps caret and prompt baseline aligned.
                TextField(
                    "",
                    text: $localText,
                    prompt: Text(composerPlaceholderText).foregroundStyle(.tertiary),
                    axis: .vertical
                )
                .textFieldStyle(.plain)
                .font(.body)
                .lineLimit(1...6)
                .padding(.horizontal, 16)
                .padding(.vertical, 12)
                .disabled((isNewConversation && selectedProject == nil) || draftManager.loadFailed || isSwitchingProject)
                .opacity((isNewConversation && selectedProject == nil) || draftManager.loadFailed || isSwitchingProject ? 0.5 : 1.0)
                .onChange(of: localText) { oldValue, newValue in
                    scheduleTriggerDetection(previousValue: oldValue, newValue: newValue)
                    scheduleContentSync(newValue)
                }
            } else {
                // PERFORMANCE FIX: Bind directly to localText for instant response
                // Draft sync is debounced to prevent per-keystroke lag
                TextEditor(text: $localText)
                .font(.body)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .scrollContentBackground(.hidden)
                .disabled((isNewConversation && selectedProject == nil) || draftManager.loadFailed || isSwitchingProject)
                .opacity((isNewConversation && selectedProject == nil) || draftManager.loadFailed || isSwitchingProject ? 0.5 : 1.0)
                .onChange(of: localText) { oldValue, newValue in
                    scheduleTriggerDetection(previousValue: oldValue, newValue: newValue)
                    // PERFORMANCE FIX: Debounce draft sync to avoid per-keystroke mutations
                    scheduleContentSync(newValue)
                }

                if localText.isEmpty {
                    Text(composerPlaceholderText)
                        .foregroundStyle(.tertiary)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 16)
                        .allowsHitTesting(false)
                }
            }
        }
        .frame(
            minHeight: usesWorkspaceInlineLayout ? 40 : 200,
            idealHeight: usesWorkspaceInlineLayout ? 56 : nil,
            maxHeight: usesWorkspaceInlineLayout ? 120 : nil,
            alignment: usesWorkspaceInlineLayout ? .topLeading : .center
        )
    }

    // MARK: - Content Sync (Debounced)

    /// Immediately flush localText to DraftManager, canceling any pending debounced sync.
    /// Call this before saveNow() to prevent data loss from the last ~300ms of typing.
    ///
    /// MEDIUM FIX: Uses draft.projectId instead of selectedProject?.id to prevent data leakage.
    /// draft.projectId represents where the content CAME FROM, not where it SHOULD GO based on
    /// current selection. This is critical for scene-phase flushes: if user taps a different project
    /// in the project sheet and app backgrounds BEFORE projectChanged() runs, we must flush
    /// old localText to the OLD project, not the newly selected one.
    ///
    /// For explicit project switches, use `flushLocalTextToDraftManager(projectId:)` with the
    /// captured previous project ID.
    private func flushLocalTextToDraftManager() async {
        // Use draft.projectId (where content came from) NOT selectedProject?.id (current binding)
        let projectId = draft.projectId
        guard !projectId.isEmpty else { return }
        await flushLocalTextToDraftManager(projectId: projectId)
    }

    /// Immediately flush localText to DraftManager, canceling any pending debounced sync.
    /// Call this before saveNow() to prevent data loss from the last ~300ms of typing.
    ///
    /// - Parameter projectId: Explicit project ID to flush to. Required to avoid flushing to wrong project
    ///   during project switches (when selectedProject has already changed to the new project).
    private func flushLocalTextToDraftManager(projectId: String) async {
        // Cancel any pending debounced sync
        contentSyncTask?.cancel()
        contentSyncTask = nil

        // Immediately sync current localText to DraftManager using explicit projectId
        await draftManager.updateContent(localText, conversationId: conversationId, projectId: projectId)
    }

    /// Debounce content sync to DraftManager to avoid per-keystroke lag
    /// Uses 300ms debounce - typing feels instant, persistence catches up
    ///
    /// NOTE: We intentionally don't update draft.content during typing.
    /// - localText is the source of truth for the UI and for sending
    /// - DraftManager handles persistence
    /// - draft.content is only used for initial loading
    private func scheduleContentSync(_ content: String) {
        // Skip sync during programmatic updates (load/switch/dictation)
        // LOW PRIORITY FIX: Consume the flag here instead of at call sites
        // SwiftUI's onChange runs AFTER the transaction completes, so resetting
        // the flag immediately after setting localText doesn't work - the flag
        // must be consumed inside the handler that runs asynchronously
        if isProgrammaticUpdate {
            isProgrammaticUpdate = false
            return
        }

        // Mark dirty immediately (cheap operation)
        isDirty = true

        // Cancel any pending sync
        contentSyncTask?.cancel()

        // MEDIUM FIX: Capture projectId at schedule time to prevent cross-project content leakage
        // If project changes during the debounce window, this captured ID ensures we don't
        // write old content to the new project's draft
        guard let capturedProjectId = selectedProject?.id else { return }
        let capturedConversationId = conversationId

        // Schedule debounced sync
        contentSyncTask = Task {
            // Wait for 300ms of inactivity before syncing
            try? await Task.sleep(for: .milliseconds(300))

            // Check if cancelled (user typed more, or project changed)
            guard !Task.isCancelled else { return }

            // Sync to DraftManager using captured IDs (safe from project changes)
            await draftManager.updateContent(content, conversationId: capturedConversationId, projectId: capturedProjectId)
        }
    }

    // MARK: - Inline Trigger Detection

    private enum InlineTriggerKind {
        case agent
        case nudgeSkill
    }

    private struct InlineTrigger {
        let kind: InlineTriggerKind
        let query: String
        let range: Range<String.Index>
    }

    private func scheduleTriggerDetection(previousValue: String, newValue: String) {
        triggerDetectionTask?.cancel()

        // Only trigger when user is adding text and no selector is already open.
        guard !isProgrammaticUpdate else { return }
        guard newValue.count >= previousValue.count else { return }
        guard selectedProject != nil else { return }
        guard !showAgentSelector && !showNudgeSkillSelector else { return }

        triggerDetectionTask = Task {
            try? await Task.sleep(for: .milliseconds(120))
            guard !Task.isCancelled else { return }

            await MainActor.run {
                guard let trigger = detectInlineTrigger(in: localText) else { return }

                // Remove trigger token from the editor; selections are represented by chips.
                localText.removeSubrange(trigger.range)

                switch trigger.kind {
                case .agent:
                    openAgentSelector(initialQuery: trigger.query)
                case .nudgeSkill:
                    openNudgeSkillSelector(mode: .all, initialQuery: trigger.query)
                }
            }
        }
    }

    private func detectInlineTrigger(in text: String) -> InlineTrigger? {
        guard !text.isEmpty else { return nil }

        let tokenStart = text.lastIndex(where: { $0.isWhitespace })
            .map { text.index(after: $0) } ?? text.startIndex
        guard tokenStart < text.endIndex else { return nil }

        let token = text[tokenStart..<text.endIndex]
        guard let prefix = token.first else { return nil }
        guard prefix == "@" || prefix == "/" else { return nil }

        let queryPart = token.dropFirst()
        if !queryPart.isEmpty && !queryPart.allSatisfy(isValidTriggerQueryCharacter(_:)) {
            return nil
        }

        return InlineTrigger(
            kind: prefix == "@" ? .agent : .nudgeSkill,
            query: String(queryPart),
            range: tokenStart..<text.endIndex
        )
    }

    private func isValidTriggerQueryCharacter(_ character: Character) -> Bool {
        character.isLetter || character.isNumber || character == "-" || character == "_"
    }

    private var toolbarView: some View {
        standardToolbarView
    }

    private var standardToolbarView: some View {
        HStack(spacing: 16) {
            if agentsLoadError != nil {
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(Color.composerWarning)
                    .font(.caption)
            }

            #if os(iOS)
            Button {
                showImagePicker = true
            } label: {
                if isUploadingImage {
                    ProgressView()
                        .scaleEffect(0.8)
                } else {
                    Image(systemName: "photo")
                        .foregroundStyle(Color.composerAction)
                }
            }
            .buttonStyle(.borderless)
            .disabled(selectedProject == nil || isUploadingImage)

            Button {
                Task {
                    showDictationOverlay = true
                    try? await dictationManager.startRecording()
                }
            } label: {
                Image(systemName: "mic.fill")
                    .foregroundStyle(Color.composerAction)
            }
            .buttonStyle(.borderless)
            .disabled(!dictationManager.state.isIdle || selectedProject == nil)
            #endif

            Spacer()

            if !localImageAttachments.isEmpty {
                HStack(spacing: 4) {
                    Image(systemName: "photo.fill")
                        .font(.caption2)
                    Text("\(localImageAttachments.count)")
                        .font(.caption)
                }
                .foregroundStyle(Color.composerAction)
            }

            if localText.count > 0 {
                Text("\(localText.count)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            if !localText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !localImageAttachments.isEmpty {
                Button(action: clearDraft) {
                    Image(systemName: "trash")
                        .foregroundStyle(Color.composerDestructive)
                }
                .buttonStyle(.borderless)
            }

            if isInlineComposer {
                Button(action: sendMessage) {
                    Image(systemName: "arrow.up")
                        .font(.headline.weight(.semibold))
                        .foregroundStyle(canSend ? Color.primary : Color.secondary.opacity(0.9))
                        .frame(width: 30, height: 30)
                        .background(
                            Circle()
                                .fill(canSend ? Color.secondary.opacity(0.28) : Color.secondary.opacity(0.16))
                        )
                }
                .buttonStyle(.borderless)
                .disabled(!canSend)
                #if os(macOS)
                .keyboardShortcut(.return, modifiers: [.command])
                .help("Send")
                #else
                .help("Send")
                #endif
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(Color.systemBackground)
    }

    // MARK: - Selection Sync

    /// Persists selected nudge IDs to DraftManager.
    /// Call after modifying draft.selectedNudgeIds to persist changes.
    private func persistSelectedNudgeIds() {
        guard let projectId = selectedProject?.id else { return }
        Task {
            await draftManager.updateNudgeIds(draft.selectedNudgeIds, conversationId: conversationId, projectId: projectId)
        }
    }

    /// Persists selected skill IDs to DraftManager.
    /// Call after modifying draft.selectedSkillIds to persist changes.
    private func persistSelectedSkillIds() {
        guard let projectId = selectedProject?.id else { return }
        Task {
            await draftManager.updateSkillIds(draft.selectedSkillIds, conversationId: conversationId, projectId: projectId)
        }
    }

    private func openNudgeSkillSelector(mode: NudgeSkillSelectorMode, initialQuery: String = "") {
        nudgeSkillSelectorInitialMode = mode
        nudgeSkillSelectorInitialQuery = initialQuery
        showNudgeSkillSelector = true
    }

    private func openAgentSelector(initialQuery: String = "") {
        agentSelectorInitialQuery = initialQuery
        showAgentSelector = true
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
                return
            }

            // CRITICAL DATA SAFETY: Only apply loaded draft if user hasn't made edits yet
            // This prevents async load from overwriting live user typing
            if !isDirty {
                // CONVERSATION/REPORT REFERENCE: If we have initial content (e.g., from "Reference this conversation"
                // or "Chat with Author"), use it instead of the loaded draft content. This ensures the context message is shown.
                if let initialContent = initialContent, !initialContent.isEmpty {
                    var modifiedDraft = loadedDraft
                    modifiedDraft.updateContent(initialContent)
                    modifiedDraft.setReferenceConversation(referenceConversationId)
                    modifiedDraft.setReferenceReportATag(referenceReportATag)
                    draft = modifiedDraft

                    // Sync localText with initial content
                    if initialContent != localText {
                        isProgrammaticUpdate = true
                        localText = initialContent
                    }

                    // Persist the initial content and reference IDs to draft storage
                    await draftManager.updateContent(initialContent, conversationId: conversationId, projectId: projectId)
                    await draftManager.updateReferenceConversation(referenceConversationId, conversationId: conversationId, projectId: projectId)
                    await draftManager.updateReferenceReportATag(referenceReportATag, conversationId: conversationId, projectId: projectId)

                } else {
                    draft = loadedDraft
                    // PERFORMANCE FIX: Sync localText with loaded draft content
                    // Flag is consumed by scheduleContentSync to suppress isDirty marking
                    // LOW FIX: Only set flag when value will actually change, otherwise onChange
                    // won't fire and the flag remains true, causing first user keystroke to be ignored
                    if loadedDraft.content != localText {
                        isProgrammaticUpdate = true
                        localText = loadedDraft.content
                    }
                    // Sync image attachments
                    localImageAttachments = loadedDraft.imageAttachments
                }
            } else {

                // RACE CONDITION FIX: Persist reference IDs even if user typed before load completed.
                // The reference IDs must always be saved regardless of dirty state, otherwise they can be lost
                // when scheduleContentSync flips isDirty before this async block runs.
                if let refId = referenceConversationId {
                    await draftManager.updateReferenceConversation(refId, conversationId: conversationId, projectId: projectId)
                }
                if let refATag = referenceReportATag {
                    await draftManager.updateReferenceReportATag(refATag, conversationId: conversationId, projectId: projectId)
                }
            }

        }
    }

    private func loadAgents() {
        guard let projectId = selectedProject?.id else { return }

        Task {
            // Use centralized cached agents instead of fetching on-demand
            // This eliminates multi-second FFI delays
            let agents = coreManager.onlineAgents[projectId] ?? []
            availableAgents = agents
            agentsLoadError = nil

            // If initialAgentPubkey is provided, use it (works for both new conversations and replies)
            // This supports features like "Chat with Author" where we want to direct the conversation to a specific agent
            if let initialPubkey = initialAgentPubkey {
                draft.setAgent(initialPubkey)
                await draftManager.updateAgent(initialPubkey, conversationId: conversationId, projectId: projectId)

                // Resolve the agent name for display (even if not online)
                let name = await coreManager.safeCore.getProfileName(pubkey: initialPubkey)
                replyTargetAgentName = name.isEmpty ? "Agent" : name
            } else if draft.agentPubkey == nil {
                // No initial agent specified: auto-select PM agent if available (for new conversations)
                if let pmAgent = agents.first(where: { $0.isPm }) {
                    draft.setAgent(pmAgent.pubkey)
                    await draftManager.updateAgent(pmAgent.pubkey, conversationId: conversationId, projectId: projectId)
                }
            }

            if agents.isEmpty {
            }
        }
    }

    private func loadNudges() {
        Task {
            // No refresh needed - use data already available from centralized state
            do {
                availableNudges = try await coreManager.safeCore.getNudges()
            } catch {
            }
        }
    }

    private func loadSkills() {
        Task {
            // No refresh needed - use data already available from centralized state
            do {
                availableSkills = try await coreManager.safeCore.getSkills()
            } catch {
            }
        }
    }

    private func projectChanged() {
        guard let project = selectedProject else { return }

        Task {
            // HIGH #1 FIX: Store previous project to revert on save failure
            let previousProject = coreManager.projects.first { $0.id == draft.projectId }
            // HIGH PRIORITY FIX: Capture previous project ID BEFORE any changes
            // This ensures flushLocalTextToDraftManager writes to the OLD project, not the NEW one
            let previousProjectId = draft.projectId

            // BLOCKER #2 FIX: Disable editing during project switch
            isSwitchingProject = true

            // Save any pending changes to the current draft before switching
            if !previousProjectId.isEmpty {
                // MEDIUM #3 FIX: Catch save errors during project switch
                do {
                    // HIGH FIX: Flush localText to DraftManager before saving
                    // Uses explicit previousProjectId to avoid writing old content to new project's draft
                    await flushLocalTextToDraftManager(projectId: previousProjectId)
                    try await draftManager.saveNow()
                } catch {
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
            // PERFORMANCE FIX: Sync localText with loaded draft content
            // Flag is consumed by scheduleContentSync to suppress isDirty marking
            // LOW FIX: Only set flag when value will actually change, otherwise onChange
            // won't fire and the flag remains true, causing first user keystroke to be ignored
            if projectDraft.content != localText {
                isProgrammaticUpdate = true
                localText = projectDraft.content
            }
            // Cancel any pending sync from previous project
            contentSyncTask?.cancel()

            // Validate and clear agent if it doesn't belong to this project
            // (will be validated again before sending)
            draft.clearAgent()
            await draftManager.updateAgent(nil, conversationId: conversationId, projectId: project.id)

            // Load agents and nudges for the new project
            loadAgents()
            loadNudges()
            loadSkills()

            // BLOCKER #2 FIX: Re-enable editing after project switch completes
            isSwitchingProject = false
        }
    }

    private func sendMessage() {
        guard canSend, let project = selectedProject else { return }

        // Validate agent pubkey against current project's agents before sending
        // CRITICAL: Only validate if we have a successful agent list (no load error)
        // Don't clear agent selection on transient errors - preserve user's choice
        // EXCEPTION: Skip validation for direct chats initiated via initialAgentPubkey (e.g., "Chat with Author")
        // These should address the target even if they're offline - standard messaging behavior
        var validatedAgentPubkey: String? = draft.agentPubkey
        if let agentPubkey = draft.agentPubkey, agentsLoadError == nil {
            // Skip online-agent validation if this agent was explicitly set via initialAgentPubkey
            // This supports "Chat with Author" and similar features where the recipient may be offline
            let isDirectChat = initialAgentPubkey != nil && agentPubkey == initialAgentPubkey

            if !isDirectChat {
                // Only validate agents selected from the online list
                let agentExists = availableAgents.contains { $0.pubkey == agentPubkey }
                if !agentExists && !availableAgents.isEmpty {
                    // Only clear if we have agents but this one isn't in the list
                    // If availableAgents is empty, the project might have no agents (valid state)
                    // Clear invalid agent from draft
                    draft.clearAgent()
                    Task {
                        await draftManager.updateAgent(nil, conversationId: conversationId, projectId: project.id)
                    }
                    validatedAgentPubkey = nil
                }
            }
        }

        isSending = true
        sendError = nil

        // PERFORMANCE FIX: Cancel any pending sync and use localText directly
        // This ensures we send what the user typed, even if sync hasn't caught up
        contentSyncTask?.cancel()
        triggerDetectionTask?.cancel()

        // Build full content including image URLs (replaces [Image #N] markers with actual URLs)
        var contentToSend = localText
        for attachment in localImageAttachments {
            let marker = "[Image #\(attachment.id)]"
            contentToSend = contentToSend.replacingOccurrences(of: marker, with: attachment.url)
        }

        Task {
            do {
                let result: SendMessageResult

                if isNewConversation {
                    result = try await coreManager.safeCore.sendThread(
                        projectId: project.id,
                        title: "",
                        content: contentToSend,
                        agentPubkey: validatedAgentPubkey,
                        nudgeIds: Array(draft.selectedNudgeIds),
                        skillIds: Array(draft.selectedSkillIds)
                    )
                } else {
                    result = try await coreManager.safeCore.sendMessage(
                        conversationId: conversationId!,
                        projectId: project.id,
                        content: contentToSend,
                        agentPubkey: validatedAgentPubkey,
                        nudgeIds: Array(draft.selectedNudgeIds),
                        skillIds: Array(draft.selectedSkillIds)
                    )
                }

                isSending = false

                // Record user activity for TTS inactivity gating
                if let convId = conversationId {
                    coreManager.recordUserActivity(conversationId: convId)
                }

                // Clear draft on success
                onSend?(result)
                if isInlineComposer {
                    await clearDraftAfterInlineSend(projectId: project.id)
                } else {
                    await draftManager.deleteDraft(conversationId: conversationId, projectId: project.id)
                    dismiss()
                }
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

        // PERFORMANCE FIX: Clear localText immediately for instant feedback
        // Flag is consumed by scheduleContentSync to suppress isDirty marking
        // LOW FIX: Only set flag when value will actually change, otherwise onChange
        // won't fire and the flag remains true, causing first user keystroke to be ignored
        if !localText.isEmpty {
            isProgrammaticUpdate = true
            localText = ""
        }
        // Cancel any pending sync since we're clearing
        contentSyncTask?.cancel()
        triggerDetectionTask?.cancel()

        // Clear image attachments
        localImageAttachments = []

        draft.clear()
        if let projectId = selectedProject?.id {
            Task {
                await draftManager.clearDraft(conversationId: conversationId, projectId: projectId)
            }
        }
    }

    /// Clears only typed content after a successful inline send while preserving routing controls.
    private func clearDraftAfterInlineSend(projectId: String) async {
        draft.updateContent("")
        draft.clearImageAttachments()
        localText = ""
        localImageAttachments = []
        isDirty = false
        contentSyncTask?.cancel()
        triggerDetectionTask?.cancel()
        await draftManager.updateContent("", conversationId: conversationId, projectId: projectId)
        await draftManager.updateImageAttachments([], conversationId: conversationId, projectId: projectId)
    }

    // MARK: - Image Attachment Handling

    /// Handle image selected from picker - upload to Blossom
    private func handleImageSelected(data: Data, mimeType: String) {
        isUploadingImage = true
        imageUploadError = nil

        Task {
            do {
                try await uploadImageAttachment(data: data, mimeType: mimeType)
                isUploadingImage = false
            } catch {
                isUploadingImage = false
                imageUploadError = error.localizedDescription
                showImageUploadError = true
            }
        }
    }

    private func uploadImageAttachment(data: Data, mimeType: String) async throws {
        let url = try await coreManager.safeCore.uploadImage(data: data, mimeType: mimeType)

        // Add to draft and local state
        let imageId = draft.addImageAttachment(url: url)
        let attachment = ImageAttachment(id: imageId, url: url)
        localImageAttachments.append(attachment)

        // Insert marker at cursor position (matching TUI behavior)
        let marker = "[Image #\(imageId)] "
        localText.append(marker)

        // Mark dirty and save
        isDirty = true
        if let projectId = selectedProject?.id {
            await draftManager.updateContent(localText, conversationId: conversationId, projectId: projectId)
            await draftManager.updateImageAttachments(localImageAttachments, conversationId: conversationId, projectId: projectId)
        }
    }

    /// Remove an image attachment
    private func removeImageAttachment(id: Int) {
        // Remove from local state
        localImageAttachments.removeAll { $0.id == id }

        // Remove marker from text
        let marker = "[Image #\(id)]"
        localText = localText.replacingOccurrences(of: marker + " ", with: "")
        localText = localText.replacingOccurrences(of: marker, with: "")

        // Update draft
        draft.removeImageAttachment(id: id)

        // Mark dirty and save
        isDirty = true
        if let projectId = selectedProject?.id {
            Task {
                await draftManager.updateContent(localText, conversationId: conversationId, projectId: projectId)
                await draftManager.updateImageAttachments(localImageAttachments, conversationId: conversationId, projectId: projectId)
            }
        }
    }

    #if os(macOS)
    private enum FileDropError: LocalizedError {
        case noReadableFileURL
        case unsupportedFileType(String)
        case readFailed(String)

        var errorDescription: String? {
            switch self {
            case .noReadableFileURL:
                return "Could not read dropped file URL."
            case .unsupportedFileType(let name):
                return "Unsupported file '\(name)'. Supported: png, jpg, jpeg, gif, webp, bmp."
            case .readFailed(let name):
                return "Failed to read '\(name)'."
            }
        }
    }

    private func handleFileDrop(providers: [NSItemProvider]) -> Bool {
        guard selectedProject != nil else {
            imageUploadError = "Select a project before dropping files."
            showImageUploadError = true
            return false
        }

        let fileProviders = providers.filter {
            $0.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier)
        }
        guard !fileProviders.isEmpty else { return false }

        Task {
            await uploadDroppedFiles(from: fileProviders)
        }
        return true
    }

    private func uploadDroppedFiles(from providers: [NSItemProvider]) async {
        isUploadingImage = true
        imageUploadError = nil

        var uploadedCount = 0
        var failures: [String] = []

        for provider in providers {
            do {
                let fileURL = try await loadDroppedFileURL(from: provider)
                try await uploadDroppedFile(at: fileURL)
                uploadedCount += 1
            } catch {
                failures.append(error.localizedDescription)
            }
        }

        isUploadingImage = false

        if !failures.isEmpty {
            let prefix = uploadedCount > 0 ? "Some files were skipped:\n" : ""
            imageUploadError = prefix + failures.joined(separator: "\n")
            showImageUploadError = true
        }
    }

    private func loadDroppedFileURL(from provider: NSItemProvider) async throws -> URL {
        try await withCheckedThrowingContinuation { continuation in
            provider.loadItem(forTypeIdentifier: UTType.fileURL.identifier, options: nil) { item, error in
                if let error {
                    continuation.resume(throwing: error)
                    return
                }
                if let url = item as? URL {
                    continuation.resume(returning: url)
                    return
                }
                if let data = item as? Data,
                   let url = URL(dataRepresentation: data, relativeTo: nil) {
                    continuation.resume(returning: url)
                    return
                }
                if let text = item as? String,
                   let url = URL(string: text) {
                    continuation.resume(returning: url)
                    return
                }
                continuation.resume(throwing: FileDropError.noReadableFileURL)
            }
        }
    }

    private func uploadDroppedFile(at fileURL: URL) async throws {
        guard let mimeType = mimeTypeForDroppedImage(url: fileURL) else {
            throw FileDropError.unsupportedFileType(fileURL.lastPathComponent)
        }

        let hasSecurityScope = fileURL.startAccessingSecurityScopedResource()
        defer {
            if hasSecurityScope {
                fileURL.stopAccessingSecurityScopedResource()
            }
        }

        let data: Data
        do {
            data = try Data(contentsOf: fileURL)
        } catch {
            throw FileDropError.readFailed(fileURL.lastPathComponent)
        }

        try await uploadImageAttachment(data: data, mimeType: mimeType)
    }

    private func mimeTypeForDroppedImage(url: URL) -> String? {
        switch url.pathExtension.lowercased() {
        case "png":
            return "image/png"
        case "jpg", "jpeg":
            return "image/jpeg"
        case "gif":
            return "image/gif"
        case "webp":
            return "image/webp"
        case "bmp":
            return "image/bmp"
        default:
            return nil
        }
    }
    #endif
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
        .buttonStyle(.borderless)
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
        .buttonStyle(.borderless)
        .contentShape(Capsule())
    }
}

// MARK: - Image Attachment Chip View

struct ImageAttachmentChipView: View {
    let attachment: ImageAttachment
    let onRemove: () -> Void

    var body: some View {
        HStack(spacing: 6) {
            // Image icon
            Image(systemName: "photo.fill")
                .font(.caption)
                .foregroundStyle(Color.composerAction)

            // Image label
            Text("Image #\(attachment.id)")
                .font(.caption)
                .foregroundStyle(.primary)

            // Remove button
            Button(action: onRemove) {
                Image(systemName: "xmark.circle.fill")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.borderless)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(
            Capsule()
                .fill(Color.composerAction.opacity(0.1))
        )
    }
}

// MARK: - Image Picker

#if os(iOS)
import PhotosUI

struct ImagePicker: UIViewControllerRepresentable {
    let onImageSelected: (Data, String) -> Void

    func makeUIViewController(context: Context) -> PHPickerViewController {
        var config = PHPickerConfiguration()
        config.filter = .images
        config.selectionLimit = 1

        let picker = PHPickerViewController(configuration: config)
        picker.delegate = context.coordinator
        return picker
    }

    func updateUIViewController(_ uiViewController: PHPickerViewController, context: Context) {}

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    class Coordinator: NSObject, PHPickerViewControllerDelegate {
        let parent: ImagePicker

        init(_ parent: ImagePicker) {
            self.parent = parent
        }

        func picker(_ picker: PHPickerViewController, didFinishPicking results: [PHPickerResult]) {
            picker.dismiss(animated: true)

            guard let result = results.first else { return }

            // Load the image data
            if result.itemProvider.canLoadObject(ofClass: UIImage.self) {
                result.itemProvider.loadObject(ofClass: UIImage.self) { [weak self] object, error in
                    guard let image = object as? UIImage else { return }

                    // Convert to PNG or JPEG data
                    let imageData: Data?
                    let mimeType: String

                    // Prefer PNG for transparency, fallback to JPEG for smaller size
                    if let pngData = image.pngData() {
                        imageData = pngData
                        mimeType = "image/png"
                    } else if let jpegData = image.jpegData(compressionQuality: 0.9) {
                        imageData = jpegData
                        mimeType = "image/jpeg"
                    } else {
                        return
                    }

                    guard let data = imageData else { return }

                    // Call on main thread
                    DispatchQueue.main.async {
                        self?.parent.onImageSelected(data, mimeType)
                    }
                }
            }
        }
    }
}
#endif

// MARK: - Preview

#Preview("New Conversation") {
    MessageComposerView(
        project: ProjectInfo(
            id: "test-project",
            title: "Test Project",
            description: "A test project",
            repoUrl: nil,
            pictureUrl: nil,
            createdAt: 0,
            agentIds: [],
            mcpToolIds: [],
            isDeleted: false
        )
    )
    .environmentObject(TenexCoreManager())
}

#Preview("Reply") {
    MessageComposerView(
        project: ProjectInfo(
            id: "test-project",
            title: "Test Project",
            description: "A test project",
            repoUrl: nil,
            pictureUrl: nil,
            createdAt: 0,
            agentIds: [],
            mcpToolIds: [],
            isDeleted: false
        ),
        conversationId: "conv-123",
        conversationTitle: "Test Conversation"
    )
    .environmentObject(TenexCoreManager())
}
