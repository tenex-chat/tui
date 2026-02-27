import SwiftUI
#if os(iOS)
import UIKit
#endif
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
    let initialProject: Project?

    /// The conversation ID if replying to an existing thread (nil for new thread)
    let conversationId: String?

    /// Optional conversation title for display
    let conversationTitle: String?

    /// Initial agent pubkey to pre-select (e.g., last agent that spoke in conversation)
    let initialAgentPubkey: String?

    /// Initial content to pre-populate the composer with (e.g., context message for conversation reference)
    let initialContent: String?

    /// Initial text attachments to seed in the draft (TUI-style [Text Attachment N] payloads)
    let initialTextAttachments: [TextAttachment]

    /// Reference conversation ID for context tagging (adds ["context", "<id>"] tag when sent)
    let referenceConversationId: String?

    /// Reference report a-tag for context tagging (adds ["context", "<a-tag>"] tag when sent)
    /// Format: "30023:<pubkey>:<slug>" - the standard Nostr a-tag for addressable events
    /// Used by "Chat with Author" feature to reference the report being discussed
    let referenceReportATag: String?

    /// Callback when message is sent successfully
    var onSend: ((SendMessageResult) -> Void)?

    /// Callback when "reference conversation" is requested from workspace composer
    var onReferenceConversationRequested: ((ReferenceConversationLaunchPayload) -> Void)?

    /// Callback when the view is dismissed
    var onDismiss: (() -> Void)?

    /// Rendering style for the composer container
    let displayStyle: DisplayStyle

    /// Inline layout variant (used to keep iPhone behavior unchanged while improving workspace)
    let inlineLayoutStyle: InlineLayoutStyle

    // MARK: - Environment

    @Environment(\.dismiss) var dismiss
    @Environment(\.scenePhase) var scenePhase
    @Environment(TenexCoreManager.self) var coreManager

    // MARK: - State

    @State var selectedProject: Project?
    @State var draft: Draft
    @State var availableAgents: [ProjectAgent] = []
    @State var agentsLoadError: String?
    @State var showAgentSelector = false
    @State var replyTargetAgentName: String?  // Agent name for reply target (resolved from initialAgentPubkey)
    @State var availableNudges: [Nudge] = []
    @State var availableSkills: [Skill] = []
    @State var showNudgeSkillSelector = false
    @State var nudgeSkillSelectorInitialMode: NudgeSkillSelectorMode = .all
    @State var nudgeSkillSelectorInitialQuery: String = ""
    @State var agentSelectorInitialQuery: String = ""
    @State var isSending = false
    @State var sendError: String?
    @State var showSendError = false
    @State var isDirty = false // Track if user has made edits before load completes
    @State var showLoadFailedAlert = false
    @State var isSwitchingProject = false // Track if project switch is in progress
    @State var showSaveFailedAlert = false
    @State var saveFailedError: String?
    @State var dictationManager = DictationManager()
    /// Captures localText before dictation starts, so partial results can replace from this point
    @State var preDictationText: String?
    @State var showDraftBrowser = false
    @State var draftSavedConfirmation = false
    @State var pinnedPromptManager = PinnedPromptManager.shared
    @State var showPinPromptTitleSheet = false
    @State var showPinnedPromptBrowser = false
    @State var pinPromptTitle = ""
    @State var pinnedPromptSaveError: String?
    @State var showPinnedPromptSaveError = false
    @State var messageHistory = MessageHistory()
    @State var showModalComposerDeprecationAlert = false

    // Image attachment state
    @State var showImagePicker = false
    @State var isUploadingImage = false
    @State var imageUploadError: String?
    @State var showImageUploadError = false
    @State var isDropTargeted = false
    /// Local image attachments synced with draft for UI display
    @State var localImageAttachments: [ImageAttachment] = []
    /// Local text attachments synced with draft for UI display
    @State var localTextAttachments: [TextAttachment] = []

    // PERFORMANCE FIX: Local text state for instant typing response
    // This decouples TextEditor binding from draft persistence to eliminate per-keystroke lag
    @State var localText: String = ""
    @State var contentSyncTask: Task<Void, Never>?

    // Flag to suppress onChange sync during programmatic localText updates (load/switch/dictation)
    @State var isProgrammaticUpdate: Bool = false
    @State var triggerDetectionTask: Task<Void, Never>?
    @State var workspaceAgentToConfig: ProjectAgent?
    @State var showWorkspaceAgentPopover = false
    @FocusState var composerFieldFocused: Bool

    // Workspace inline layout metrics
    @ScaledMetric(relativeTo: .body) var workspaceContextRowHeight: CGFloat = 34
    @ScaledMetric(relativeTo: .body) var workspaceBottomRowHeight: CGFloat = 52
    @ScaledMetric(relativeTo: .body) var workspaceIconSize: CGFloat = 14
    @ScaledMetric(relativeTo: .body) var workspaceSendButtonSize: CGFloat = 38
    @ScaledMetric(relativeTo: .body) var workspaceAccessoryButtonSize: CGFloat = 24

    // MARK: - Computed

    var draftManager: DraftManager {
        DraftManager.shared
    }

    var composerDependencies: ComposerDependencies {
        ComposerDependencies.live(
            core: coreManager,
            drafts: draftManager,
            credentials: KeychainService.shared,
            notifications: NotificationService.shared
        )
    }

    var composerViewModel: ComposerViewModel {
        ComposerViewModel(dependencies: composerDependencies)
    }

    var attachmentUploadService: AttachmentUploadService {
        AttachmentUploadService(core: composerDependencies.core)
    }

    var isNewConversation: Bool {
        conversationId == nil
    }

    var canSend: Bool {
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
        let hasTextAttachmentContent = !localTextAttachments.isEmpty
        // Either text or images is sufficient to send
        return (hasTextContent || hasImageContent || hasTextAttachmentContent) && !isSending && !isUploadingImage
    }

    var selectedAgent: ProjectAgent? {
        guard let pubkey = draft.agentPubkey else { return nil }
        return availableAgents.first { $0.pubkey == pubkey }
    }

    var selectedNudges: [Nudge] {
        availableNudges.filter { draft.selectedNudgeIds.contains($0.id) }
    }

    var selectedSkills: [Skill] {
        availableSkills.filter { draft.selectedSkillIds.contains($0.id) }
    }

    var isInlineComposer: Bool {
        displayStyle == .inline
    }

    var usesWorkspaceInlineLayout: Bool {
        isInlineComposer && inlineLayoutStyle == .workspace
    }

    var workspaceComposerShellColor: Color {
        #if os(macOS)
        return .conversationComposerShellMac
        #else
        return .systemBackground
        #endif
    }

    var workspaceComposerFooterColor: Color {
        #if os(macOS)
        return .conversationComposerFooterMac
        #else
        return .systemBackground
        #endif
    }

    var workspaceComposerStrokeColor: Color {
        #if os(macOS)
        return .conversationComposerStrokeMac
        #else
        return Color.secondary.opacity(0.2)
        #endif
    }

    /// Find the project with the most recent conversation activity.
    /// Respects the global scheduled event filter.
    func projectWithMostRecentActivity() -> Project? {
        composerViewModel.projectWithMostRecentActivity(
            scheduledFilter: coreManager.appFilterScheduledEvent
        )
    }

    // MARK: - Initialization

    init(
        project: Project? = nil,
        conversationId: String? = nil,
        conversationTitle: String? = nil,
        initialAgentPubkey: String? = nil,
        initialContent: String? = nil,
        initialTextAttachments: [TextAttachment] = [],
        referenceConversationId: String? = nil,
        referenceReportATag: String? = nil,
        displayStyle: DisplayStyle = .modal,
        inlineLayoutStyle: InlineLayoutStyle = .standard,
        onSend: ((SendMessageResult) -> Void)? = nil,
        onReferenceConversationRequested: ((ReferenceConversationLaunchPayload) -> Void)? = nil,
        onDismiss: (() -> Void)? = nil
    ) {
        self.initialProject = project
        self.conversationId = conversationId
        self.conversationTitle = conversationTitle
        self.initialAgentPubkey = initialAgentPubkey
        self.initialContent = initialContent
        self.initialTextAttachments = initialTextAttachments
        self.referenceConversationId = referenceConversationId
        self.referenceReportATag = referenceReportATag
        self.displayStyle = displayStyle
        self.inlineLayoutStyle = inlineLayoutStyle
        self.onSend = onSend
        self.onReferenceConversationRequested = onReferenceConversationRequested
        self.onDismiss = onDismiss

        // Initialize state with project if provided
        _selectedProject = State(initialValue: project)

        // Initialize draft (will be updated in onAppear)
        if let conversationId = conversationId, let projectId = project?.id {
            var seededDraft = Draft(
                conversationId: conversationId,
                projectId: projectId,
                referenceConversationId: referenceConversationId,
                referenceReportATag: referenceReportATag
            )
            seededDraft.setTextAttachments(initialTextAttachments)
            _draft = State(initialValue: seededDraft)
        } else if let projectId = project?.id {
            var seededDraft = Draft(
                projectId: projectId,
                content: initialContent ?? "",
                referenceConversationId: referenceConversationId,
                referenceReportATag: referenceReportATag
            )
            seededDraft.setTextAttachments(initialTextAttachments)
            _draft = State(initialValue: seededDraft)
        } else {
            // No project yet - will be set when project is selected
            var seededDraft = Draft(
                projectId: "",
                content: initialContent ?? "",
                referenceConversationId: referenceConversationId,
                referenceReportATag: referenceReportATag
            )
            seededDraft.setTextAttachments(initialTextAttachments)
            _draft = State(initialValue: seededDraft)
        }

        // Initialize localText with initial content if provided
        if let content = initialContent {
            _localText = State(initialValue: content)
        }
        _localTextAttachments = State(initialValue: initialTextAttachments)
    }

    // MARK: - Body

    var body: some View {
        // TODO(#modal-composer-deprecation): remove modal flow after all call sites migrate to inline.
        if displayStyle == .modal {
            deprecatedModalComposerView
        } else {
            composerViewWithLifecycle
        }
    }

    var deprecatedModalComposerView: some View {
        Color.clear
            .onAppear {
                if !showModalComposerDeprecationAlert {
                    showModalComposerDeprecationAlert = true
                }
            }
            .alert("Modal Composer Deprecated", isPresented: $showModalComposerDeprecationAlert) {
                Button("OK") {
                    dismissModalComposerDeprecationAlert()
                }
            } message: {
                Text("this used to open a composer modal, now it doesnt")
            }
    }

    func dismissModalComposerDeprecationAlert() {
        onDismiss?()
        dismiss()
    }

    @ViewBuilder
    var composerBaseView: some View {
        Group {
            if isInlineComposer {
                composerContent
            } else {
                // TODO(#modal-composer-deprecation): delete this legacy modal composer UI when callers migrate.
                NavigationStack {
                    composerContent
                        .navigationTitle(isNewConversation ? "New Conversation" : "Reply")
                        #if os(iOS)
                        .navigationBarTitleDisplayMode(.inline)
                        #else
                        .toolbarTitleDisplayMode(.inline)
                        #endif
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

    var composerViewWithDialogs: some View {
        composerBaseView
        .onAppear {
            // Auto-select project with most recent activity if none provided
            if selectedProject == nil && initialProject == nil {
                if let mostActiveProject = projectWithMostRecentActivity() {
                    selectedProject = mostActiveProject
                    draft = Draft(projectId: mostActiveProject.id)
                }
            }
        }
        .task(id: selectedProject?.id) {
            await refreshComposerContextForSelectedProject()
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
        .sheet(item: $workspaceAgentToConfig) { agent in
            AgentConfigSheet(agent: agent, projectId: selectedProject?.id ?? "")
                .environment(coreManager)
        }
        .sheet(isPresented: $showNudgeSkillSelector) {
            NudgeSkillSelectorSheet(
                nudges: availableNudges,
                skills: availableSkills,
                selectedNudgeIds: $draft.selectedNudgeIds,
                selectedSkillIds: $draft.selectedSkillIds,
                bookmarkedIds: coreManager.bookmarkedIds,
                initialMode: nudgeSkillSelectorInitialMode,
                initialSearchQuery: nudgeSkillSelectorInitialQuery,
                onDone: {
                    isDirty = true // Mark as dirty when user selects nudges/skills
                    persistSelectedNudgeIds()
                    persistSelectedSkillIds()
                },
                onToggleBookmark: { itemId in
                    Task {
                        _ = try? await coreManager.safeCore.toggleBookmark(itemId: itemId)
                    }
                }
            )
        }
        .sheet(isPresented: $showDraftBrowser) {
            DraftBrowserSheet(projectId: selectedProject?.id ?? "") { draft in
                isProgrammaticUpdate = true
                localText = draft.text
                isDirty = true
                if let projectId = selectedProject?.id {
                    Task {
                        await draftManager.updateContent(draft.text, conversationId: conversationId, projectId: projectId)
                    }
                }
            }
        }
        .sheet(isPresented: $showPinPromptTitleSheet) {
            PinPromptTitleSheet(
                title: $pinPromptTitle,
                promptText: localText.trimmingCharacters(in: .whitespacesAndNewlines),
                onSave: { title in
                    savePinnedPrompt(with: title)
                }
            )
        }
        .sheet(isPresented: $showPinnedPromptBrowser) {
            PinnedPromptBrowserSheet { prompt in
                applyPinnedPrompt(prompt)
            }
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
        .alert("Pinned Prompt Save Failed", isPresented: $showPinnedPromptSaveError) {
            Button("OK", role: .cancel) { }
        } message: {
            Text(pinnedPromptSaveError ?? "Unknown error")
        }
    }

    var composerViewWithLifecycle: some View {
        composerViewWithDialogs
        .task(id: scenePhase) {
            await persistDraftForScenePhase(scenePhase)
        }
        .onChange(of: initialAgentPubkey) { _, newValue in
            guard !isDirty else { return }
            guard !isNewConversation else { return }
            guard let newValue, !newValue.isEmpty else { return }
            guard draft.agentPubkey != newValue else { return }

            Task {
                await loadAgents()
            }
        }
        .onChange(of: coreManager.onlineAgents) { _, newAgents in
            // Reactively update availableAgents when centralized state changes
            // This eliminates the need for manual refresh() calls
            if let projectId = selectedProject?.id {
                let agents = newAgents[projectId] ?? []
                availableAgents = agents
            }
        }
    }

    @ViewBuilder
    var composerContent: some View {
        VStack(spacing: 0) {
            if !usesWorkspaceInlineLayout {
                // Project and Agent row (for new conversations)
                if isNewConversation {
                    HStack(spacing: 12) {
                        if let project = selectedProject {
                            projectChipView(project)
                        } else {
                            projectPromptButton
                        }

                        if selectedProject != nil {
                            if let agent = selectedAgent {
                                OnlineAgentChipView(agent: agent) {
                                    openAgentSelector()
                                }
                                .environment(coreManager)
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

            // Text attachment chips (for all conversations)
            if !localTextAttachments.isEmpty && selectedProject != nil {
                textAttachmentChipsView
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
        .onChange(of: dictationManager.state) { _, newState in
            // Stream dictated text directly into the editor
            switch newState {
            case .recording(let partialText):
                guard !partialText.isEmpty else { return }
                isProgrammaticUpdate = true
                let prefix = preDictationText ?? ""
                localText = prefix + (prefix.isEmpty ? "" : " ") + partialText
                isDirty = true
            case .idle:
                // Recording stopped — commit whatever text is in the editor
                if preDictationText != nil {
                    let currentText = localText
                    preDictationText = nil
                    if let projectId = selectedProject?.id {
                        Task {
                            await draftManager.updateContent(currentText, conversationId: conversationId, projectId: projectId)
                        }
                    }
                    dictationManager.reset()
                }
            }
        }
        #if os(macOS)
        .onDrop(of: [UTType.fileURL], isTargeted: $isDropTargeted) { providers in
            handleFileDrop(providers: providers)
        }
        #endif
    }
}
