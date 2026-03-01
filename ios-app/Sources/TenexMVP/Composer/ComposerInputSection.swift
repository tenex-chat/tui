import SwiftUI

extension MessageComposerView {
    enum PinControlMode: Equatable {
        case hidden
        case menu
        case pinAction
    }

    static func canPinCurrentPrompt(forInputText text: String) -> Bool {
        !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    static func pinControlMode(forInputText text: String, pinnedPromptCount: Int) -> PinControlMode {
        if canPinCurrentPrompt(forInputText: text) {
            return .pinAction
        }
        return pinnedPromptCount > 0 ? .menu : .hidden
    }

    var composerPlaceholderText: String {
        if isNewConversation && selectedProject == nil {
            return "Select a project to start composing"
        }
        if isSwitchingProject {
            return "Switching project..."
        }
        if usesWorkspaceInlineLayout && !isNewConversation {
            return "Ask for follow-up changes"
        }
        return isNewConversation ? "What would you like to discuss?" : "Type your reply..."
    }

    var isComposerInputDisabled: Bool {
        (isNewConversation && selectedProject == nil) || draftManager.loadFailed || isSwitchingProject
    }

    var workspaceContentLineCount: Int {
        max(localText.split(separator: "\n", omittingEmptySubsequences: false).count, 1)
    }

    var isWorkspaceEditorExpanded: Bool {
        localText.count > 200 || workspaceContentLineCount > 4
    }

    var workspaceEditorBaseHeight: CGFloat {
        max(workspaceBottomRowHeight + 88, 140)
    }

    var workspaceEditorHeight: CGFloat {
        guard isWorkspaceEditorExpanded else { return workspaceEditorBaseHeight }

        let charGrowth = CGFloat(max(localText.count - 200, 0) / 80) * 24
        let lineGrowth = CGFloat(max(workspaceContentLineCount - 4, 0)) * 22
        let expandedTarget = workspaceEditorBaseHeight + 96 + max(charGrowth, lineGrowth)
        return min(max(expandedTarget, workspaceEditorBaseHeight + 96), 460)
    }

    @ViewBuilder
    var contentEditorView: some View {
        if usesWorkspaceInlineLayout {
            workspaceTextField
                .contentShape(Rectangle())
                .onTapGesture {
                    composerFieldFocused = true
                }
                .frame(height: workspaceEditorHeight, alignment: .top)
        } else {
            ZStack(alignment: .topLeading) {
                TextEditor(text: $localText)
                    .font(.body)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .scrollContentBackground(.hidden)
                    .disabled((isNewConversation && selectedProject == nil) || draftManager.loadFailed || isSwitchingProject)
                    .opacity((isNewConversation && selectedProject == nil) || draftManager.loadFailed || isSwitchingProject ? 0.5 : 1.0)
                    .onChange(of: localText) { oldValue, newValue in
                        scheduleTriggerDetection(previousValue: oldValue, newValue: newValue)
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
            .frame(minHeight: 200)
        }
    }

    @ViewBuilder
    private var workspaceTextField: some View {
        #if os(macOS)
        ZStack(alignment: .topLeading) {
            WorkspaceComposerTextView(
                text: $localText,
                isFocused: Binding(
                    get: { composerFieldFocused },
                    set: { composerFieldFocused = $0 }
                ),
                isEnabled: !isComposerInputDisabled,
                onSubmit: {
                    if canSend {
                        sendMessage()
                    }
                },
                useNewlineForReturn: isWorkspaceEditorExpanded,
                onHistoryPrevious: {
                    guard localText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                        return false
                    }
                    guard let previous = messageHistory.previous(currentText: localText) else {
                        return false
                    }
                    isProgrammaticUpdate = true
                    localText = previous
                    return true
                },
                onHistoryNext: {
                    guard messageHistory.currentIndex != nil else { return false }
                    guard let next = messageHistory.next() else { return false }
                    isProgrammaticUpdate = true
                    localText = next
                    return true
                },
                transformPaste: { pastedText in
                    transformWorkspacePasteText(pastedText)
                }
            )
            .padding(.horizontal, 20)
            .padding(.top, 16)
            .padding(.bottom, 14)
            .opacity(isComposerInputDisabled ? 0.5 : 1.0)

            if localText.isEmpty {
                Text(composerPlaceholderText)
                    .font(.title3)
                    .foregroundStyle(.secondary.opacity(0.6))
                    .padding(.horizontal, 24)
                    .padding(.top, 19)
                    .allowsHitTesting(false)
            }
        }
        .onChange(of: localText) { oldValue, newValue in
            scheduleTriggerDetection(previousValue: oldValue, newValue: newValue)
            scheduleContentSync(newValue)
        }
        #else
        TextField(
            "",
            text: $localText,
            prompt: Text(composerPlaceholderText).foregroundStyle(.secondary.opacity(0.6)),
            axis: .vertical
        )
        .focused($composerFieldFocused)
        .textFieldStyle(.plain)
        .font(.title3)
        .foregroundStyle(.primary.opacity(0.94))
        .lineLimit(1...8)
        .padding(.horizontal, 20)
        .padding(.top, 16)
        .padding(.bottom, 14)
        .disabled((isNewConversation && selectedProject == nil) || draftManager.loadFailed || isSwitchingProject)
        .opacity((isNewConversation && selectedProject == nil) || draftManager.loadFailed || isSwitchingProject ? 0.5 : 1.0)
        .onChange(of: localText) { oldValue, newValue in
            scheduleTriggerDetection(previousValue: oldValue, newValue: newValue)
            scheduleContentSync(newValue)
        }
        #endif
    }

    #if os(macOS)
    func transformWorkspacePasteText(_ pastedText: String) -> String {
        if WorkspacePasteBehavior.shouldBeAttachment(pastedText) {
            let id = draft.addTextAttachment(content: pastedText)
            let attachment = TextAttachment(id: id, content: pastedText)
            localTextAttachments.append(attachment)
            isDirty = true

            if let projectId = selectedProject?.id {
                Task {
                    await draftManager.updateTextAttachments(
                        localTextAttachments,
                        conversationId: conversationId,
                        projectId: projectId
                    )
                }
            }

            return "[Text Attachment \(id)]"
        }

        return WorkspacePasteBehavior.smartFormatPaste(pastedText)
    }
    #endif

    /// Immediately flush localText to DraftManager, canceling any pending debounced sync.
    /// Call this before saveNow() to prevent data loss from the last ~300ms of typing.
    func flushLocalTextToDraftManager() async {
        let projectId = draft.projectId
        guard !projectId.isEmpty else { return }
        await flushLocalTextToDraftManager(projectId: projectId)
    }

    /// Immediately flush localText to DraftManager, canceling any pending debounced sync.
    /// - Parameter projectId: Explicit project ID to flush to.
    func flushLocalTextToDraftManager(projectId: String) async {
        contentSyncTask?.cancel()
        contentSyncTask = nil
        await draftManager.updateContent(localText, conversationId: conversationId, projectId: projectId)
    }

    /// Debounce content sync to DraftManager to avoid per-keystroke lag.
    func scheduleContentSync(_ content: String) {
        if isProgrammaticUpdate {
            isProgrammaticUpdate = false
            return
        }

        isDirty = true
        contentSyncTask?.cancel()

        guard let capturedProjectId = selectedProject?.id else { return }
        let capturedConversationId = conversationId

        contentSyncTask = Task {
            try? await Task.sleep(for: .milliseconds(300))
            guard !Task.isCancelled else { return }
            await draftManager.updateContent(content, conversationId: capturedConversationId, projectId: capturedProjectId)
        }
    }

    func scheduleTriggerDetection(previousValue: String, newValue: String) {
        triggerDetectionTask?.cancel()

        guard !isProgrammaticUpdate else { return }
        guard newValue.count >= previousValue.count else { return }
        guard selectedProject != nil else { return }
        guard !showAgentSelector && !showNudgeSkillSelector else { return }

        triggerDetectionTask = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(120))
            guard !Task.isCancelled else { return }
            guard let trigger = composerViewModel.detectInlineTrigger(in: localText) else { return }

            localText.removeSubrange(trigger.range)

            switch trigger.kind {
            case .agent:
                openAgentSelector(initialQuery: trigger.query)
            case .nudgeSkill:
                openNudgeSkillSelector(mode: .all, initialQuery: trigger.query)
            }
        }
    }

    var toolbarView: some View {
        standardToolbarView
    }

    var standardToolbarView: some View {
        HStack(spacing: 16) {
            if dictationManager.state.isRecording {
                DictationRecordingBar(
                    audioLevelSamples: dictationManager.audioLevelSamples,
                    recordingStartDate: dictationManager.recordingStartDate,
                    error: dictationManager.error,
                    onStop: {
                        Task {
                            await dictationManager.stopRecording()
                        }
                    }
                )
            } else {
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
                        preDictationText = localText
                        try? await dictationManager.startRecording()
                    }
                } label: {
                    Image(systemName: "mic.fill")
                        .foregroundStyle(Color.composerAction)
                }
                .buttonStyle(.borderless)
                .disabled(!dictationManager.state.isIdle || selectedProject == nil)
                #endif

                pinnedPromptsToolbarButton

                if !localText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    Button {
                        saveDraftAsNamed()
                    } label: {
                        Image(systemName: draftSavedConfirmation ? "bookmark.fill" : "bookmark")
                            .foregroundStyle(draftSavedConfirmation ? Color.accentColor : Color.composerAction)
                    }
                    .buttonStyle(.borderless)
                    .disabled(selectedProject == nil)
                    .help("Save as reusable draft")
                }

                if selectedProject != nil {
                    Button {
                        showDraftBrowser = true
                    } label: {
                        Image(systemName: "doc.text.magnifyingglass")
                            .foregroundStyle(Color.composerAction)
                    }
                    .buttonStyle(.borderless)
                    .help("Browse saved drafts")
                }

                if selectedProject != nil {
                    Button {
                        openNudgeSkillSelector(mode: .all)
                    } label: {
                        Text("/")
                            .font(.body.weight(.bold).monospaced())
                            .foregroundStyle(Color.composerAction)
                            .frame(width: 24, height: 24)
                            .background(
                                RoundedRectangle(cornerRadius: 6)
                                    .strokeBorder(Color.composerAction.opacity(0.5), lineWidth: 1)
                            )
                    }
                    .buttonStyle(.borderless)
                    .help("Nudges & Skills")
                }

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

                if !localTextAttachments.isEmpty {
                    HStack(spacing: 4) {
                        Image(systemName: "doc.text.fill")
                            .font(.caption2)
                        Text("\(localTextAttachments.count)")
                            .font(.caption)
                    }
                    .foregroundStyle(Color.composerAction)
                }

                if localText.count > 0 {
                    Text("\(localText.count)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                if !localText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !localImageAttachments.isEmpty || !localTextAttachments.isEmpty {
                    Button(action: clearDraft) {
                        Image(systemName: "trash")
                            .foregroundStyle(Color.composerDestructive)
                    }
                    .buttonStyle(.borderless)
                }
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

    var pinnedPromptsToolbarButton: some View {
        Menu {
            pinnedPromptsMenuContent()
        } label: {
            Image(systemName: recentPinnedPrompts.isEmpty ? "pin" : "pin.fill")
                .foregroundStyle(Color.composerAction)
        }
        .buttonStyle(.borderless)
        .help("Pinned prompts")
    }

    var canPinCurrentPrompt: Bool {
        Self.canPinCurrentPrompt(forInputText: localText)
    }

    var pinControlMode: PinControlMode {
        Self.pinControlMode(forInputText: localText, pinnedPromptCount: recentPinnedPrompts.count)
    }

    var recentPinnedPrompts: [PinnedPrompt] {
        Array(pinnedPromptManager.all().prefix(8))
    }

    @ViewBuilder
    func pinnedPromptsMenuContent() -> some View {
        if canPinCurrentPrompt {
            Button {
                pinCurrentPrompt()
            } label: {
                Label("Pin this prompt", systemImage: "pin")
            }
            Divider()
        }

        if recentPinnedPrompts.isEmpty {
            Text("No pinned prompts yet")
        } else {
            ForEach(recentPinnedPrompts) { prompt in
                Button {
                    applyPinnedPrompt(prompt)
                } label: {
                    Text(prompt.title)
                }
            }
        }

        Divider()

        Button {
            showPinnedPromptBrowser = true
        } label: {
            Label("Manage Pinned Prompts", systemImage: "list.bullet")
        }
    }

    func pinCurrentPrompt() {
        pinPromptTitle = ""
        showPinPromptTitleSheet = true
    }

    func savePinnedPrompt(with title: String) {
        let text = localText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }

        Task { @MainActor in
            _ = await pinnedPromptManager.pin(title: title, text: text)
            if let error = pinnedPromptManager.lastSaveError {
                pinnedPromptSaveError = error.localizedDescription
                showPinnedPromptSaveError = true
                return
            }

            pinPromptTitle = ""
            showPinPromptTitleSheet = false
        }
    }

    func applyPinnedPrompt(_ prompt: PinnedPrompt) {
        isProgrammaticUpdate = true
        localText = prompt.text
        isDirty = true

        if let projectId = selectedProject?.id {
            Task {
                await draftManager.updateContent(prompt.text, conversationId: conversationId, projectId: projectId)
            }
        }

        Task {
            await pinnedPromptManager.markUsed(prompt.id)
        }
    }

    func persistSelectedNudgeIds() {
        guard let projectId = selectedProject?.id else { return }
        Task {
            await draftManager.updateNudgeIds(draft.selectedNudgeIds, conversationId: conversationId, projectId: projectId)
        }
    }

    func persistSelectedSkillIds() {
        guard let projectId = selectedProject?.id else { return }
        Task {
            await draftManager.updateSkillIds(draft.selectedSkillIds, conversationId: conversationId, projectId: projectId)
        }
    }

    func openNudgeSkillSelector(mode: NudgeSkillSelectorMode, initialQuery: String = "") {
        nudgeSkillSelectorInitialMode = mode
        nudgeSkillSelectorInitialQuery = initialQuery
        showNudgeSkillSelector = true
    }

    func openAgentSelector(initialQuery: String = "") {
        agentSelectorInitialQuery = initialQuery
        showAgentSelector = true
    }

    func saveDraftAsNamed() {
        guard let projectId = selectedProject?.id else { return }
        let text = localText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }

        Task {
            await NamedDraftManager.shared.save(text, projectId: projectId)
            draftSavedConfirmation = true
            try? await Task.sleep(for: .seconds(1.5))
            draftSavedConfirmation = false
        }
    }
}
