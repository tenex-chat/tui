import Foundation

struct NudgeListItem: Identifiable, Hashable {
    let nudge: Nudge
    let authorDisplayName: String
    let authorPictureURL: String?

    var id: String { nudge.id }
}

@MainActor
final class NudgesViewModel: ObservableObject {
    @Published private(set) var mine: [NudgeListItem] = []
    @Published private(set) var community: [NudgeListItem] = []
    @Published private(set) var availableTools: [String] = []
    @Published private(set) var isLoading = false
    @Published var searchText = ""
    @Published var errorMessage: String?

    private weak var coreManager: TenexCoreManager?
    private var hasLoaded = false
    private var currentUserPubkey: String?
    private var authorPictureCache: [String: String] = [:]
    private var resolvedAuthorPictures: Set<String> = []

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
            async let fetchedNudges = coreManager.safeCore.getNudges()
            async let fetchedCurrentUser = coreManager.safeCore.getCurrentUser()

            let (nudges, currentUser) = try await (fetchedNudges, fetchedCurrentUser)
            currentUserPubkey = currentUser?.pubkey

            let projectIds = coreManager.projects.map(\.id)
            availableTools = await loadAvailableTools(projectIds: projectIds, coreManager: coreManager)

            await resolveAuthorPictures(for: nudges, coreManager: coreManager)

            let sorted = nudges.sorted { lhs, rhs in
                if lhs.createdAt != rhs.createdAt {
                    return lhs.createdAt > rhs.createdAt
                }
                if lhs.title != rhs.title {
                    return lhs.title.localizedCaseInsensitiveCompare(rhs.title) == .orderedAscending
                }
                return lhs.id > rhs.id
            }

            let items = sorted.map { nudge in
                NudgeListItem(
                    nudge: nudge,
                    authorDisplayName: coreManager.displayName(for: nudge.pubkey),
                    authorPictureURL: authorPictureCache[nudge.pubkey]
                )
            }

            mine = items.filter { item in
                guard let currentUserPubkey else { return false }
                return item.nudge.pubkey.caseInsensitiveCompare(currentUserPubkey) == .orderedSame
            }

            community = items.filter { item in
                guard let currentUserPubkey else { return true }
                return item.nudge.pubkey.caseInsensitiveCompare(currentUserPubkey) != .orderedSame
            }

            errorMessage = nil
            hasLoaded = true
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    var filteredMine: [NudgeListItem] {
        filterRows(mine)
    }

    var filteredCommunity: [NudgeListItem] {
        filterRows(community)
    }

    func canDelete(_ item: NudgeListItem) -> Bool {
        guard let currentUserPubkey else { return false }
        return item.nudge.pubkey.caseInsensitiveCompare(currentUserPubkey) == .orderedSame
    }

    @discardableResult
    func createNudge(
        title: String,
        description: String,
        content: String,
        hashtags: [String],
        allowTools: [String],
        denyTools: [String],
        onlyTools: [String]
    ) async -> Bool {
        guard let coreManager else {
            errorMessage = "Core manager unavailable."
            return false
        }

        let trimmedTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedContent = content.trimmingCharacters(in: .whitespacesAndNewlines)
        let trimmedDescription = description.trimmingCharacters(in: .whitespacesAndNewlines)

        guard !trimmedTitle.isEmpty else {
            errorMessage = "Nudge title cannot be empty."
            return false
        }

        guard !trimmedContent.isEmpty else {
            errorMessage = "Nudge content cannot be empty."
            return false
        }

        let (sanitizedAllow, sanitizedDeny, sanitizedOnly) = sanitizePermissionPayload(
            allowTools: allowTools,
            denyTools: denyTools,
            onlyTools: onlyTools
        )

        do {
            try await coreManager.safeCore.createNudge(
                title: trimmedTitle,
                description: trimmedDescription,
                content: trimmedContent,
                hashtags: orderedUnique(
                    hashtags
                        .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                        .filter { !$0.isEmpty }
                ),
                allowTools: sanitizedAllow,
                denyTools: sanitizedDeny,
                onlyTools: sanitizedOnly
            )

            await refresh()
            return true
        } catch {
            errorMessage = error.localizedDescription
            return false
        }
    }

    @discardableResult
    func deleteNudge(id: String) async -> Bool {
        guard let coreManager else {
            errorMessage = "Core manager unavailable."
            return false
        }

        let all = mine + community
        guard let item = all.first(where: { $0.id == id }) else {
            errorMessage = "Nudge not found."
            return false
        }

        guard canDelete(item) else {
            errorMessage = "You can only delete nudges you authored."
            return false
        }

        do {
            try await coreManager.safeCore.deleteNudge(nudgeId: id)
            mine.removeAll { $0.id == id }
            community.removeAll { $0.id == id }
            await refresh()
            return true
        } catch {
            errorMessage = error.localizedDescription
            return false
        }
    }

    private func filterRows(_ rows: [NudgeListItem]) -> [NudgeListItem] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !query.isEmpty else { return rows }

        return rows.filter { item in
            let haystacks: [String] = [
                item.nudge.title,
                item.nudge.description,
                item.nudge.content,
                item.authorDisplayName,
                item.nudge.hashtags.joined(separator: " "),
                item.nudge.allowedTools.joined(separator: " "),
                item.nudge.deniedTools.joined(separator: " "),
                item.nudge.onlyTools.joined(separator: " ")
            ]

            return haystacks.contains { $0.lowercased().contains(query) }
        }
    }

    private func resolveAuthorPictures(for nudges: [Nudge], coreManager: TenexCoreManager) async {
        let uniquePubkeys = Set(nudges.map(\.pubkey))

        for pubkey in uniquePubkeys {
            if !resolvedAuthorPictures.contains(pubkey) {
                if let picture = try? await coreManager.safeCore.getProfilePicture(pubkey: pubkey),
                   !picture.isEmpty {
                    authorPictureCache[pubkey] = picture
                }
                resolvedAuthorPictures.insert(pubkey)
            }
        }
    }

    private func loadAvailableTools(projectIds: [String], coreManager: TenexCoreManager) async -> [String] {
        guard !projectIds.isEmpty else { return [] }

        let uniqueProjectIds = Array(Set(projectIds))
        var mergedTools: Set<String> = []

        await withTaskGroup(of: [String].self) { group in
            for projectId in uniqueProjectIds {
                group.addTask {
                    guard let options = try? await coreManager.safeCore.getProjectConfigOptions(projectId: projectId) else {
                        return []
                    }
                    return options.allTools
                }
            }

            for await tools in group {
                for tool in tools {
                    let trimmed = tool.trimmingCharacters(in: .whitespacesAndNewlines)
                    if !trimmed.isEmpty {
                        mergedTools.insert(trimmed)
                    }
                }
            }
        }

        return mergedTools.sorted { lhs, rhs in
            lhs.localizedCaseInsensitiveCompare(rhs) == .orderedAscending
        }
    }

    private func sanitizePermissionPayload(
        allowTools: [String],
        denyTools: [String],
        onlyTools: [String]
    ) -> ([String], [String], [String]) {
        let cleanedOnly = orderedUnique(
            onlyTools
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
        )

        if !cleanedOnly.isEmpty {
            return ([], [], cleanedOnly)
        }

        let cleanedDeny = orderedUnique(
            denyTools
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
        )

        let denySet = Set(cleanedDeny)
        let cleanedAllow = orderedUnique(
            allowTools
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }
                .filter { !denySet.contains($0) }
        )

        return (cleanedAllow, cleanedDeny, [])
    }

    private func orderedUnique(_ values: [String]) -> [String] {
        var seen: Set<String> = []
        var ordered: [String] = []

        for value in values {
            if seen.insert(value).inserted {
                ordered.append(value)
            }
        }

        return ordered
    }
}
