import Foundation
import Observation

enum ComposerInlineTriggerKind {
    case agent
    case nudgeSkill
}

struct ComposerInlineTrigger {
    let kind: ComposerInlineTriggerKind
    let query: String
    let range: Range<String.Index>
}

struct ComposerDraftLoadResult {
    let draft: Draft?
    let localText: String?
    let imageAttachments: [ImageAttachment]
    let textAttachments: [TextAttachment]
    let shouldShowLoadFailedAlert: Bool
}

struct ComposerAgentLoadResult {
    let availableAgents: [ProjectAgent]
    let selectedAgentPubkey: String?
    let replyTargetAgentName: String?
}

@MainActor
@Observable
final class ComposerViewModel {
    let dependencies: ComposerDependencies

    init(dependencies: ComposerDependencies) {
        self.dependencies = dependencies
    }

    func projectWithMostRecentActivity(scheduledFilter: ScheduledEventFilter = .showAll) -> Project? {
        var candidates = dependencies.core.conversations
        if scheduledFilter != .showAll {
            candidates = candidates.filter { scheduledFilter.allows(isScheduled: $0.thread.isScheduled) }
        }

        guard let mostRecent = candidates.max(by: { $0.thread.effectiveLastActivity < $1.thread.effectiveLastActivity }) else {
            return nil
        }

        let projectId = TenexCoreManager.projectId(fromATag: mostRecent.projectATag)
        return dependencies.core.projects.first { $0.id == projectId }
    }

    func loadDraft(
        projectId: String,
        conversationId: String?,
        initialContent: String?,
        initialTextAttachments: [TextAttachment],
        referenceConversationId: String?,
        referenceReportATag: String?,
        isDirty: Bool
    ) async -> ComposerDraftLoadResult {
        let loadedDraft = await dependencies.drafts.getOrCreateDraft(
            conversationId: conversationId,
            projectId: projectId
        )

        if dependencies.drafts.loadFailed {
            return ComposerDraftLoadResult(
                draft: nil,
                localText: nil,
                imageAttachments: [],
                textAttachments: [],
                shouldShowLoadFailedAlert: true
            )
        }

        if isDirty {
            if let refId = referenceConversationId {
                await dependencies.drafts.updateReferenceConversation(
                    refId,
                    conversationId: conversationId,
                    projectId: projectId
                )
            }
            if let refATag = referenceReportATag {
                await dependencies.drafts.updateReferenceReportATag(
                    refATag,
                    conversationId: conversationId,
                    projectId: projectId
                )
            }
            return ComposerDraftLoadResult(
                draft: nil,
                localText: nil,
                imageAttachments: [],
                textAttachments: [],
                shouldShowLoadFailedAlert: false
            )
        }

        let hasInitialContent = !(initialContent ?? "").isEmpty
        let hasInitialTextAttachments = !initialTextAttachments.isEmpty
        if hasInitialContent || hasInitialTextAttachments {
            let seededContent = initialContent ?? ""
            var modifiedDraft = loadedDraft
            modifiedDraft.updateContent(seededContent)
            modifiedDraft.setReferenceConversation(referenceConversationId)
            modifiedDraft.setReferenceReportATag(referenceReportATag)
            modifiedDraft.setTextAttachments(initialTextAttachments)

            await dependencies.drafts.updateContent(
                seededContent,
                conversationId: conversationId,
                projectId: projectId
            )
            await dependencies.drafts.updateTextAttachments(
                initialTextAttachments,
                conversationId: conversationId,
                projectId: projectId
            )
            await dependencies.drafts.updateReferenceConversation(
                referenceConversationId,
                conversationId: conversationId,
                projectId: projectId
            )
            await dependencies.drafts.updateReferenceReportATag(
                referenceReportATag,
                conversationId: conversationId,
                projectId: projectId
            )

            return ComposerDraftLoadResult(
                draft: modifiedDraft,
                localText: seededContent,
                imageAttachments: modifiedDraft.imageAttachments,
                textAttachments: modifiedDraft.textAttachments,
                shouldShowLoadFailedAlert: false
            )
        }

        return ComposerDraftLoadResult(
            draft: loadedDraft,
            localText: loadedDraft.content,
            imageAttachments: loadedDraft.imageAttachments,
            textAttachments: loadedDraft.textAttachments,
            shouldShowLoadFailedAlert: false
        )
    }

    func loadAgentContext(
        projectId: String,
        conversationId: String?,
        initialAgentPubkey: String?,
        currentAgentPubkey: String?
    ) async -> ComposerAgentLoadResult {
        let agents = dependencies.core.onlineAgents[projectId] ?? []
        var selectedAgentPubkey = currentAgentPubkey
        var replyTargetName: String?

        if let initialAgentPubkey {
            selectedAgentPubkey = initialAgentPubkey
            await dependencies.drafts.updateAgent(
                initialAgentPubkey,
                conversationId: conversationId,
                projectId: projectId
            )
            let name = await dependencies.core.getProfileName(pubkey: initialAgentPubkey)
            replyTargetName = name.isEmpty ? "Agent" : name
        } else if selectedAgentPubkey == nil,
                  let pmAgent = agents.first(where: { $0.isPm }) {
            selectedAgentPubkey = pmAgent.pubkey
            await dependencies.drafts.updateAgent(
                pmAgent.pubkey,
                conversationId: conversationId,
                projectId: projectId
            )
        }

        return ComposerAgentLoadResult(
            availableAgents: agents,
            selectedAgentPubkey: selectedAgentPubkey,
            replyTargetAgentName: replyTargetName
        )
    }

    func loadNudges() async -> [Nudge] {
        (try? await dependencies.core.getNudges()) ?? []
    }

    func loadSkills() async -> [Skill] {
        (try? await dependencies.core.getSkills()) ?? []
    }

    func validatedAgentPubkey(
        candidate: String?,
        initialAgentPubkey: String?,
        agentsLoadError: String?,
        availableAgents: [ProjectAgent],
        conversationId: String?,
        projectId: String
    ) async -> String? {
        guard let candidate else { return nil }
        guard agentsLoadError == nil else { return candidate }

        let isDirectChat = initialAgentPubkey != nil && candidate == initialAgentPubkey
        if isDirectChat {
            return candidate
        }

        let agentExists = availableAgents.contains { $0.pubkey == candidate }
        if !agentExists && !availableAgents.isEmpty {
            await dependencies.drafts.updateAgent(nil, conversationId: conversationId, projectId: projectId)
            return nil
        }
        return candidate
    }

    func sendMessage(
        isNewConversation: Bool,
        conversationId: String?,
        projectId: String,
        content: String,
        agentPubkey: String?,
        nudgeIds: [String],
        skillIds: [String],
        referenceConversationId: String?
    ) async throws -> SendMessageResult {
        if isNewConversation {
            return try await dependencies.core.sendThread(
                projectId: projectId,
                title: "",
                content: content,
                agentPubkey: agentPubkey,
                nudgeIds: nudgeIds,
                skillIds: skillIds,
                referenceConversationId: referenceConversationId
            )
        }

        guard let conversationId else {
            throw ComposerError.missingConversationId
        }

        return try await dependencies.core.sendMessage(
            conversationId: conversationId,
            projectId: projectId,
            content: content,
            agentPubkey: agentPubkey,
            nudgeIds: nudgeIds,
            skillIds: skillIds
        )
    }

    func detectInlineTrigger(in text: String) -> ComposerInlineTrigger? {
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

        return ComposerInlineTrigger(
            kind: prefix == "@" ? .agent : .nudgeSkill,
            query: String(queryPart),
            range: tokenStart..<text.endIndex
        )
    }

    private func isValidTriggerQueryCharacter(_ character: Character) -> Bool {
        character.isLetter || character.isNumber || character == "-" || character == "_"
    }
}

enum ComposerError: LocalizedError {
    case missingConversationId

    var errorDescription: String? {
        switch self {
        case .missingConversationId:
            return "Missing conversation ID for reply send."
        }
    }
}
