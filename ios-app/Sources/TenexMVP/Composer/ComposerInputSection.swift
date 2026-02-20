import SwiftUI

extension MessageComposerView {
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

    @ViewBuilder
    var contentEditorView: some View {
        if usesWorkspaceInlineLayout {
            workspaceTextField
                .contentShape(Rectangle())
                .onTapGesture {
                    composerFieldFocused = true
                }
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

    private var workspaceTextField: some View {
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
        #if os(macOS)
        .onKeyPress(.return) { keyPress in
            if keyPress.modifiers.contains(.shift) {
                localText += "\n"
                return .handled
            }
            if canSend {
                sendMessage()
            }
            return .handled
        }
        #endif
    }

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
}
