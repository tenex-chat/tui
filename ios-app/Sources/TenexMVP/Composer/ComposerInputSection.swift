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
            #if os(iOS)
            // Telegram-style: single-line input that grows up to a few lines
            TextField(
                "",
                text: $localText,
                prompt: Text(composerPlaceholderText).foregroundStyle(.tertiary),
                axis: .vertical
            )
            .focused($composerFieldFocused)
            .textFieldStyle(.plain)
            .font(.body)
            .lineLimit(1...6)
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(
                Capsule(style: .continuous)
                    .fill(Color.systemGray6)
                    .overlay(
                        Capsule(style: .continuous)
                            .stroke(Color.systemGray4.opacity(0.55), lineWidth: 1)
                    )
            )
            .disabled(isComposerInputDisabled)
            .opacity(isComposerInputDisabled ? 0.5 : 1.0)
            .onChange(of: localText) { oldValue, newValue in
                scheduleTriggerDetection(previousValue: oldValue, newValue: newValue)
                scheduleContentSync(newValue)
            }
            #else
            ZStack(alignment: .topLeading) {
                TextEditor(text: $localText)
                    .font(.body)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .scrollContentBackground(.hidden)
                    .disabled(isComposerInputDisabled)
                    .opacity(isComposerInputDisabled ? 0.5 : 1.0)
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
            .frame(minHeight: 80)
            #endif
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
        guard !showAgentSelector && !showSkillSelector else { return }

        triggerDetectionTask = Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(120))
            guard !Task.isCancelled else { return }
            guard let trigger = composerViewModel.detectInlineTrigger(in: localText) else { return }

            localText.removeSubrange(trigger.range)

            switch trigger.kind {
            case .agent:
                openAgentSelector(initialQuery: trigger.query)
            case .skill:
                openSkillSelector(initialQuery: trigger.query)
            }
        }
    }

    /// Telegram-style single-row composer: attach (left), text field (center, expandable), mic↔send (right).
    var telegramStyleComposerRow: some View {
        Group {
            if dictationManager.state.isRecording {
                HStack(spacing: 12) {
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
                }
                .padding(.horizontal, 12)
                .padding(.top, 8)
                .padding(.bottom, 10)
                .modifier(ToolbarGlassBackground())
            } else {
                HStack(alignment: .bottom, spacing: 8) {
                    composerLeadingAttachButton
                    contentEditorView
                        .frame(maxWidth: .infinity)
                    composerTrailingActionButton
                }
                .padding(.horizontal, 12)
                .padding(.top, 8)
                .padding(.bottom, 10)
                .modifier(ToolbarGlassBackground())
            }
        }
    }

    @ViewBuilder
    var composerLeadingAttachButton: some View {
        #if os(iOS)
        Button {
            showImagePicker = true
        } label: {
            if isUploadingImage {
                ProgressView()
                    .scaleEffect(0.8)
                    .frame(width: 32, height: 32)
            } else {
                Image(systemName: "plus")
                    .font(.title3.weight(.medium))
                    .foregroundStyle(Color.composerAction)
                    .frame(width: 32, height: 32)
            }
        }
        .buttonStyle(.borderless)
        .disabled(selectedProject == nil || isUploadingImage)
        .help("Attach")
        #else
        EmptyView()
        #endif
    }

    @ViewBuilder
    var composerTrailingActionButton: some View {
        #if os(iOS)
        // Single stable Button — avoids structural type change in the HStack that hosts the
        // focused TextField. Swapping Button types (if/else) destroys/recreates the sibling
        // view, causing SwiftUI's focus system to call becomeFirstResponder() again, which
        // interrupts iOS native keyboard dictation.
        let showSend = isInlineComposer && canSend
        Button {
            if showSend {
                sendMessage()
            } else if dictationManager.state.isIdle && selectedProject != nil {
                Task {
                    preDictationText = localText
                    try? await dictationManager.startRecording()
                }
            }
        } label: {
            if showSend {
                Image(systemName: "arrow.up")
                    .font(.headline.weight(.semibold))
                    .foregroundStyle(Color.white)
                    .frame(width: 32, height: 32)
                    .background(Circle().fill(Color.accentColor))
            } else {
                Image(systemName: "mic.fill")
                    .font(.title3)
                    .foregroundStyle(Color.composerAction)
                    .frame(width: 32, height: 32)
            }
        }
        .buttonStyle(.borderless)
        .disabled(!showSend && (!dictationManager.state.isIdle || selectedProject == nil))
        .help(showSend ? "Send" : "Voice message")
        #else
        if isInlineComposer && canSend {
            Button(action: sendMessage) {
                Image(systemName: "arrow.up")
                    .font(.headline.weight(.semibold))
                    .foregroundStyle(Color.white)
                    .frame(width: 32, height: 32)
                    .background(Circle().fill(Color.accentColor))
            }
            .buttonStyle(.borderless)
            .keyboardShortcut(.return, modifiers: [.command])
            .help("Send")
        }
        #endif
    }

    /// Toolbar button that opens the skill selector sheet.
    /// Shows a [/] glyph with a count badge when skills are selected.
    var skillToolbarButton: some View {
        Button {
            openSkillSelector()
        } label: {
            HStack(spacing: 3) {
                Text("/")
                    .font(.body.monospaced().weight(.semibold))
                    .foregroundStyle(Color.composerAction)
                if draft.selectedSkillIds.count > 0 {
                    Text("\(draft.selectedSkillIds.count)")
                        .font(.caption2.weight(.semibold))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 5)
                        .padding(.vertical, 1)
                        .background(Capsule().fill(Color.composerAction))
                }
            }
        }
        .buttonStyle(.borderless)
        .disabled(selectedProject == nil)
        .help("Skills")
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

    func persistSelectedSkillIds() {
        guard let projectId = selectedProject?.id else { return }
        Task {
            await draftManager.updateSkillIds(draft.selectedSkillIds, conversationId: conversationId, projectId: projectId)
        }
    }

    func openSkillSelector(initialQuery: String = "") {
        skillSelectorInitialQuery = initialQuery
        showSkillSelector = true
    }

    func openAgentSelector(initialQuery: String = "") {
        agentSelectorInitialQuery = initialQuery
        if usesWorkspaceInlineLayout {
            showWorkspaceAgentPopover = true
        } else {
            showAgentSelector = true
        }
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

// MARK: - Liquid Glass Toolbar Background

/// Applies Liquid Glass styling to the composer toolbar on iOS 26+,
/// falling back to a translucent material on earlier versions.
private struct ToolbarGlassBackground: ViewModifier {
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    func body(content: Content) -> some View {
        if reduceTransparency {
            content
                .background(Color.systemBackground)
        } else if #available(iOS 26.0, macOS 26.0, *) {
            content
                .background(Color.systemBackground.opacity(0.82))
        } else {
            content
                .background(Color.systemBackground.opacity(0.94))
        }
    }
}
