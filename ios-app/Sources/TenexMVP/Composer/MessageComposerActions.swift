import SwiftUI
#if os(iOS)
import UIKit
#endif

extension MessageComposerView {
    func loadDraft() async {
        guard let projectId = selectedProject?.id else { return }

        let result = await composerViewModel.loadDraft(
            projectId: projectId,
            conversationId: conversationId,
            initialContent: initialContent,
            referenceConversationId: referenceConversationId,
            referenceReportATag: referenceReportATag,
            isDirty: isDirty
        )

        if result.shouldShowLoadFailedAlert {
            showLoadFailedAlert = true
            return
        }

        guard let loadedDraft = result.draft else { return }
        draft = loadedDraft

        if let loadedText = result.localText, loadedText != localText {
            isProgrammaticUpdate = true
            localText = loadedText
        }
        localImageAttachments = result.imageAttachments
    }

    func loadAgents() async {
        guard let projectId = selectedProject?.id else { return }

        let result = await composerViewModel.loadAgentContext(
            projectId: projectId,
            conversationId: conversationId,
            initialAgentPubkey: initialAgentPubkey,
            currentAgentPubkey: draft.agentPubkey
        )

        availableAgents = result.availableAgents
        agentsLoadError = nil
        replyTargetAgentName = result.replyTargetAgentName

        if draft.agentPubkey != result.selectedAgentPubkey {
            draft.setAgent(result.selectedAgentPubkey)
        }
    }

    func loadNudges() async {
        availableNudges = await composerViewModel.loadNudges()
    }

    func loadSkills() async {
        availableSkills = await composerViewModel.loadSkills()
    }

    func refreshComposerContextForSelectedProject() async {
        guard selectedProject != nil else { return }
        await loadDraft()
        await loadAgents()
        await loadNudges()
        await loadSkills()
    }

    func projectChanged() {
        guard let project = selectedProject else { return }
        isSwitchingProject = true

        Task {
            let previousProject = coreManager.projects.first { $0.id == draft.projectId }
            let previousProjectId = draft.projectId

            if !previousProjectId.isEmpty {
                do {
                    await flushLocalTextToDraftManager(projectId: previousProjectId)
                    try await draftManager.saveNow()
                } catch {
                    selectedProject = previousProject
                    saveFailedError = error.localizedDescription
                    showSaveFailedAlert = true
                    isSwitchingProject = false
                    return
                }
            }

            availableAgents = []
            agentsLoadError = nil
            isDirty = false

            let projectDraft = await draftManager.getOrCreateDraft(
                conversationId: conversationId,
                projectId: project.id
            )
            draft = projectDraft
            if projectDraft.content != localText {
                isProgrammaticUpdate = true
                localText = projectDraft.content
            }
            localImageAttachments = projectDraft.imageAttachments
            contentSyncTask?.cancel()

            draft.clearAgent()
            await draftManager.updateAgent(nil, conversationId: conversationId, projectId: project.id)

            await loadAgents()
            await loadNudges()
            await loadSkills()
            isSwitchingProject = false
        }
    }

    func sendMessage() {
        guard canSend, let project = selectedProject else { return }

        isSending = true
        sendError = nil
        contentSyncTask?.cancel()
        triggerDetectionTask?.cancel()

        var contentToSend = localText
        for attachment in localImageAttachments {
            let marker = "[Image #\(attachment.id)]"
            contentToSend = contentToSend.replacingOccurrences(of: marker, with: attachment.url)
        }

        Task {
            do {
                let validatedAgentPubkey = await composerViewModel.validatedAgentPubkey(
                    candidate: draft.agentPubkey,
                    initialAgentPubkey: initialAgentPubkey,
                    agentsLoadError: agentsLoadError,
                    availableAgents: availableAgents,
                    conversationId: conversationId,
                    projectId: project.id
                )
                if draft.agentPubkey != validatedAgentPubkey {
                    draft.setAgent(validatedAgentPubkey)
                }

                let result = try await composerViewModel.sendMessage(
                    isNewConversation: isNewConversation,
                    conversationId: conversationId,
                    projectId: project.id,
                    content: contentToSend,
                    agentPubkey: validatedAgentPubkey,
                    nudgeIds: Array(draft.selectedNudgeIds),
                    skillIds: Array(draft.selectedSkillIds)
                )

                isSending = false
                messageHistory.add(contentToSend)
                messageHistory.reset()

                if let convId = conversationId {
                    coreManager.recordUserActivity(conversationId: convId)
                }

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

    func clearDraft() {
        if !localText.isEmpty {
            isProgrammaticUpdate = true
            localText = ""
        }
        contentSyncTask?.cancel()
        triggerDetectionTask?.cancel()
        localImageAttachments = []

        draft.clear()
        if let projectId = selectedProject?.id {
            Task {
                await draftManager.clearDraft(conversationId: conversationId, projectId: projectId)
            }
        }
    }

    func persistDraftForScenePhase(_ phase: ScenePhase) async {
        guard phase == .background || phase == .inactive else { return }

        #if os(iOS)
        var backgroundTaskID: UIBackgroundTaskIdentifier = .invalid
        backgroundTaskID = UIApplication.shared.beginBackgroundTask {
            if backgroundTaskID != .invalid {
                UIApplication.shared.endBackgroundTask(backgroundTaskID)
                backgroundTaskID = .invalid
            }
        }
        defer {
            if backgroundTaskID != .invalid {
                UIApplication.shared.endBackgroundTask(backgroundTaskID)
            }
        }
        #endif

        do {
            await flushLocalTextToDraftManager()
            try await draftManager.saveNow()
        } catch {
        }
    }

    /// Clears only typed content after a successful inline send while preserving routing controls.
    func clearDraftAfterInlineSend(projectId: String) async {
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
}
