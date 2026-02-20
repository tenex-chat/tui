import Foundation

struct AgentDefinitionListItem: Identifiable, Hashable {
    let agent: AgentDefinition
    let authorDisplayName: String
    let authorPictureURL: String?

    var id: String { agent.id }
}

@MainActor
final class AgentDefinitionsViewModel: ObservableObject {
    @Published private(set) var mine: [AgentDefinitionListItem] = []
    @Published private(set) var community: [AgentDefinitionListItem] = []
    @Published private(set) var isLoading = false
    @Published var searchText = ""
    @Published var errorMessage: String?

    private weak var coreManager: TenexCoreManager?
    private var hasLoaded = false
    private var authorNameCache: [String: String] = [:]
    private var authorPictureCache: [String: String] = [:]
    private var resolvedAuthorPictures: Set<String> = []
    private var currentUserPubkey: String?

    func configure(with coreManager: TenexCoreManager) {
        self.coreManager = coreManager
    }

    func loadIfNeeded() async {
        guard !hasLoaded else { return }
        await refresh()
    }

    func refresh() async {
        guard let coreManager else { return }

        isLoading = true
        defer { isLoading = false }

        do {
            async let fetchedAgents = coreManager.safeCore.getAllAgents()
            async let fetchedCurrentUser = coreManager.safeCore.getCurrentUser()

            let (agents, user) = try await (fetchedAgents, fetchedCurrentUser)
            currentUserPubkey = user?.pubkey

            let dedupedAgents = deduplicateLatest(agents)
            await resolveAuthors(for: dedupedAgents, coreManager: coreManager)

            let items = dedupedAgents.map { agent in
                AgentDefinitionListItem(
                    agent: agent,
                    authorDisplayName: authorNameCache[agent.pubkey] ?? fallbackAuthorDisplay(pubkey: agent.pubkey),
                    authorPictureURL: authorPictureCache[agent.pubkey]
                )
            }

            mine = items
                .filter { item in
                    guard let currentUserPubkey else { return false }
                    return item.agent.pubkey == currentUserPubkey
                }
            community = items
                .filter { item in
                    guard let currentUserPubkey else { return true }
                    return item.agent.pubkey != currentUserPubkey
                }

            errorMessage = nil
            hasLoaded = true
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    var filteredMine: [AgentDefinitionListItem] {
        filterRows(mine)
    }

    var filteredCommunity: [AgentDefinitionListItem] {
        filterRows(community)
    }

    func listItem(for agent: AgentDefinition) -> AgentDefinitionListItem? {
        let allItems = mine + community
        if let exact = allItems.first(where: { $0.agent.id == agent.id }) {
            return exact
        }

        return AgentDefinitionListItem(
            agent: agent,
            authorDisplayName: authorNameCache[agent.pubkey] ?? fallbackAuthorDisplay(pubkey: agent.pubkey),
            authorPictureURL: authorPictureCache[agent.pubkey]
        )
    }

    func canDelete(_ item: AgentDefinitionListItem) -> Bool {
        guard let currentUserPubkey else { return false }
        return item.agent.pubkey.caseInsensitiveCompare(currentUserPubkey) == .orderedSame
    }

    @discardableResult
    func deleteAgentDefinition(id: String) async -> Bool {
        guard let coreManager else {
            errorMessage = "Core manager unavailable."
            return false
        }

        let allItems = mine + community
        guard let item = allItems.first(where: { $0.agent.id == id }) else {
            errorMessage = "Agent definition not found."
            return false
        }

        guard canDelete(item) else {
            errorMessage = "You can only delete agent definitions you authored."
            return false
        }

        do {
            try await coreManager.safeCore.deleteAgentDefinition(agentId: id)

            mine.removeAll { $0.id == id }
            community.removeAll { $0.id == id }
            return true
        } catch {
            errorMessage = error.localizedDescription
            return false
        }
    }

    private func filterRows(_ rows: [AgentDefinitionListItem]) -> [AgentDefinitionListItem] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !query.isEmpty else { return rows }

        return rows.filter { item in
            let haystacks: [String] = [
                item.agent.name,
                item.agent.description,
                item.agent.role,
                item.agent.model ?? "",
                item.agent.dTag,
                item.authorDisplayName
            ]

            return haystacks.contains { $0.lowercased().contains(query) }
        }
    }

    private func deduplicateLatest(_ agents: [AgentDefinition]) -> [AgentDefinition] {
        var latestByKey: [String: AgentDefinition] = [:]

        for agent in agents {
            let identifier = canonicalIdentifier(for: agent)
            let key = "\(agent.pubkey.lowercased()):\(identifier.lowercased())"

            guard let existing = latestByKey[key] else {
                latestByKey[key] = agent
                continue
            }

            if shouldReplace(existing: existing, with: agent) {
                latestByKey[key] = agent
            }
        }

        return latestByKey.values.sorted { lhs, rhs in
            if lhs.createdAt != rhs.createdAt {
                return lhs.createdAt > rhs.createdAt
            }
            if lhs.name != rhs.name {
                return lhs.name.localizedCaseInsensitiveCompare(rhs.name) == .orderedAscending
            }
            return lhs.id > rhs.id
        }
    }

    private func shouldReplace(existing: AgentDefinition, with candidate: AgentDefinition) -> Bool {
        if candidate.createdAt != existing.createdAt {
            return candidate.createdAt > existing.createdAt
        }

        let existingVersion = Int(existing.version ?? "") ?? 0
        let candidateVersion = Int(candidate.version ?? "") ?? 0
        if candidateVersion != existingVersion {
            return candidateVersion > existingVersion
        }

        return candidate.id > existing.id
    }

    private func canonicalIdentifier(for agent: AgentDefinition) -> String {
        if !agent.dTag.isEmpty {
            return agent.dTag
        }
        if !agent.name.isEmpty {
            return agent.name
        }
        return agent.id
    }

    private func resolveAuthors(for agents: [AgentDefinition], coreManager: TenexCoreManager) async {
        let uniquePubkeys = Set(agents.map(\.pubkey))

        for pubkey in uniquePubkeys {
            if authorNameCache[pubkey] == nil {
                let profileName = await coreManager.safeCore.getProfileName(pubkey: pubkey)
                    .trimmingCharacters(in: .whitespacesAndNewlines)

                if !profileName.isEmpty, profileName != pubkey {
                    authorNameCache[pubkey] = profileName
                } else {
                    authorNameCache[pubkey] = fallbackAuthorDisplay(pubkey: pubkey)
                }
            }

            if !resolvedAuthorPictures.contains(pubkey) {
                if let picture = try? await coreManager.safeCore.getProfilePicture(pubkey: pubkey),
                   !picture.isEmpty {
                    authorPictureCache[pubkey] = picture
                }
                resolvedAuthorPictures.insert(pubkey)
            }
        }
    }

    private func fallbackAuthorDisplay(pubkey: String) -> String {
        if let npub = Bech32.hexToNpub(pubkey) {
            return npub
        }
        if pubkey.count > 16 {
            return "\(pubkey.prefix(8))...\(pubkey.suffix(8))"
        }
        return pubkey
    }
}
