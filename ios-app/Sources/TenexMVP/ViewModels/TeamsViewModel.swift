import Foundation

struct TeamListItem: Identifiable, Hashable {
    var team: TeamInfo
    var authorDisplayName: String
    var authorPictureURL: String?

    var id: String { team.id }
}

struct TeamCategorySection: Identifiable, Hashable {
    let title: String
    let teams: [TeamListItem]

    var id: String { title.lowercased() }
}

struct TeamCommentRow: Identifiable, Hashable {
    let comment: TeamCommentInfo
    let authorDisplayName: String
    let authorPictureURL: String?

    var id: String { comment.id }
}

struct TeamCommentThread: Identifiable, Hashable {
    let root: TeamCommentRow
    let replies: [TeamCommentRow]

    var id: String { root.id }
}

struct TeamHireResult: Identifiable {
    let id = UUID()
    let title: String
    let message: String
}

@MainActor
final class TeamsViewModel: ObservableObject {
    @Published private(set) var allTeams: [TeamListItem] = []
    @Published private(set) var featuredTeams: [TeamListItem] = []
    @Published private(set) var categorySections: [TeamCategorySection] = []
    @Published private(set) var isLoading = false
    @Published var errorMessage: String?

    private weak var coreManager: TenexCoreManager?
    private var hasLoaded = false
    private var isRefreshInFlight = false
    private var refreshRequestedWhileInFlight = false
    private var authorPictureCache: [String: String] = [:]
    private var resolvedAuthorPictures: Set<String> = []

    private let uncategorizedLabel = "Uncategorized"

    func configure(with coreManager: TenexCoreManager) {
        self.coreManager = coreManager
    }

    func loadIfNeeded() async {
        guard !hasLoaded else { return }
        await refresh()
    }

    func refresh() async {
        guard let coreManager else { return }

        if isRefreshInFlight {
            refreshRequestedWhileInFlight = true
            return
        }

        isRefreshInFlight = true
        defer { isRefreshInFlight = false }

        repeat {
            refreshRequestedWhileInFlight = false
            isLoading = true

            do {
                let teams = try await coreManager.safeCore.getAllTeams()
                await resolveAuthorPictures(pubkeys: Set(teams.map(\.pubkey)), coreManager: coreManager)

                let sortedTeams = teams.sorted { lhs, rhs in
                    if lhs.createdAt != rhs.createdAt {
                        return lhs.createdAt > rhs.createdAt
                    }
                    return lhs.id > rhs.id
                }

                let items = sortedTeams.map { team in
                    TeamListItem(
                        team: team,
                        authorDisplayName: coreManager.displayName(for: team.pubkey),
                        authorPictureURL: authorPictureCache[team.pubkey]
                    )
                }

                apply(items: items)
                hasLoaded = true
                errorMessage = nil
            } catch {
                errorMessage = error.localizedDescription
            }

            isLoading = false
        } while refreshRequestedWhileInFlight
    }

    func item(for teamId: String) -> TeamListItem? {
        allTeams.first(where: { $0.id == teamId })
    }

    @discardableResult
    func toggleLike(teamId: String) async -> Bool {
        guard let coreManager,
              let current = item(for: teamId) else {
            errorMessage = "Team not found."
            return false
        }

        let willLike = !current.team.likedByMe

        do {
            _ = try await coreManager.safeCore.reactToTeam(
                teamCoordinate: current.team.coordinate,
                teamEventId: current.team.id,
                teamPubkey: current.team.pubkey,
                isLike: willLike
            )

            mutateTeam(teamId: teamId) { team in
                let wasLiked = team.likedByMe
                team.likedByMe = willLike

                if willLike && !wasLiked {
                    team.likeCount += 1
                } else if !willLike && wasLiked {
                    team.likeCount = team.likeCount > 0 ? team.likeCount - 1 : 0
                }
            }

            return true
        } catch {
            errorMessage = error.localizedDescription
            return false
        }
    }

    func loadCommentThread(for team: TeamInfo) async throws -> [TeamCommentThread] {
        guard let coreManager else { return [] }

        let comments = try await coreManager.safeCore.getTeamComments(
            teamCoordinate: team.coordinate,
            teamEventId: team.id
        )

        await resolveAuthorPictures(pubkeys: Set(comments.map(\.pubkey)), coreManager: coreManager)

        let rows = comments.map { comment in
            TeamCommentRow(
                comment: comment,
                authorDisplayName: resolvedCommentAuthorDisplay(comment),
                authorPictureURL: authorPictureCache[comment.pubkey]
            )
        }

        return buildThread(rows: rows)
    }

    func loadAgentDefinitions(for team: TeamInfo) async throws -> [AgentDefinitionListItem] {
        guard let coreManager else { return [] }
        guard !team.agentDefinitionIds.isEmpty else { return [] }

        let allAgents = try await coreManager.safeCore.getAllAgents()
        let matchingIds = Set(team.agentDefinitionIds)
        let matching = allAgents.filter { matchingIds.contains($0.id) }

        await resolveAuthorPictures(pubkeys: Set(matching.map(\.pubkey)), coreManager: coreManager)

        return matching.map { agent in
            AgentDefinitionListItem(
                agent: agent,
                authorDisplayName: coreManager.displayName(for: agent.pubkey),
                authorPictureURL: authorPictureCache[agent.pubkey]
            )
        }
    }

    @discardableResult
    func postComment(
        team: TeamInfo,
        content: String,
        parentComment: TeamCommentRow?
    ) async -> Bool {
        guard let coreManager else {
            errorMessage = "Core manager unavailable."
            return false
        }

        let trimmed = content.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else {
            errorMessage = "Comment cannot be empty."
            return false
        }

        do {
            _ = try await coreManager.safeCore.postTeamComment(
                teamCoordinate: team.coordinate,
                teamEventId: team.id,
                teamPubkey: team.pubkey,
                content: trimmed,
                parentCommentId: parentComment?.comment.id,
                parentCommentPubkey: parentComment?.comment.pubkey
            )

            mutateTeam(teamId: team.id) { item in
                item.commentCount += 1
            }

            return true
        } catch {
            errorMessage = error.localizedDescription
            return false
        }
    }

    func hireTeam(_ team: TeamInfo, into project: Project) async -> TeamHireResult {
        guard let coreManager else {
            return TeamHireResult(
                title: "Unable to Hire",
                message: "Core manager is unavailable."
            )
        }

        var mergedAgentIds = project.agentDefinitionIds
        var existing = Set(project.agentDefinitionIds)
        var newlyAdded = 0

        for agentId in team.agentDefinitionIds {
            if existing.insert(agentId).inserted {
                mergedAgentIds.append(agentId)
                newlyAdded += 1
            }
        }

        let alreadyPresent = max(0, team.agentDefinitionIds.count - newlyAdded)

        if newlyAdded == 0 {
            return TeamHireResult(
                title: "Already Hired",
                message: "All \(team.agentDefinitionIds.count) agent definition\(team.agentDefinitionIds.count == 1 ? " is" : "s are") already in \(project.title)."
            )
        }

        do {
            try await coreManager.safeCore.updateProject(
                projectId: project.id,
                title: project.title,
                description: project.description ?? "",
                repoUrl: project.repoUrl,
                pictureUrl: project.pictureUrl,
                agentDefinitionIds: mergedAgentIds,
                mcpToolIds: project.mcpToolIds
            )

            await coreManager.fetchData()

            if alreadyPresent > 0 {
                return TeamHireResult(
                    title: "Partially Hired",
                    message: "Added \(newlyAdded) new agent\(newlyAdded == 1 ? "" : "s") to \(project.title). \(alreadyPresent) were already present."
                )
            }

            return TeamHireResult(
                title: "Team Hired",
                message: "Added \(newlyAdded) agent\(newlyAdded == 1 ? "" : "s") to \(project.title)."
            )
        } catch {
            return TeamHireResult(
                title: "Unable to Hire",
                message: error.localizedDescription
            )
        }
    }

    private func apply(items: [TeamListItem]) {
        allTeams = items
        featuredTeams = items
            .sorted(by: sortByFeatured)
            .prefix(10)
            .map { $0 }

        var buckets: [String: [TeamListItem]] = [:]
        for item in items {
            let normalizedCategories = normalizeCategories(item.team.categories)
            let categories = normalizedCategories.isEmpty ? [uncategorizedLabel] : normalizedCategories
            for category in categories {
                buckets[category, default: []].append(item)
            }
        }

        let sections = buckets.map { key, teams in
            TeamCategorySection(
                title: key,
                teams: teams.sorted(by: sortByCategory)
            )
        }

        categorySections = sections.sorted { lhs, rhs in
            if lhs.teams.count != rhs.teams.count {
                return lhs.teams.count > rhs.teams.count
            }
            return lhs.title.localizedCaseInsensitiveCompare(rhs.title) == .orderedAscending
        }
    }

    private func mutateTeam(teamId: String, _ mutate: (inout TeamInfo) -> Void) {
        var updated = allTeams
        guard let index = updated.firstIndex(where: { $0.id == teamId }) else { return }

        mutate(&updated[index].team)
        apply(items: updated)
    }

    private func normalizeCategories(_ categories: [String]) -> [String] {
        var seen: Set<String> = []
        var normalized: [String] = []

        for raw in categories {
            let value = raw.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !value.isEmpty else { continue }

            let key = value.lowercased()
            if seen.insert(key).inserted {
                normalized.append(value)
            }
        }

        return normalized
    }

    private func sortByFeatured(_ lhs: TeamListItem, _ rhs: TeamListItem) -> Bool {
        if lhs.team.likeCount != rhs.team.likeCount {
            return lhs.team.likeCount > rhs.team.likeCount
        }
        if lhs.team.createdAt != rhs.team.createdAt {
            return lhs.team.createdAt > rhs.team.createdAt
        }
        return lhs.team.id > rhs.team.id
    }

    private func sortByCategory(_ lhs: TeamListItem, _ rhs: TeamListItem) -> Bool {
        if lhs.team.likeCount != rhs.team.likeCount {
            return lhs.team.likeCount > rhs.team.likeCount
        }
        if lhs.team.createdAt != rhs.team.createdAt {
            return lhs.team.createdAt > rhs.team.createdAt
        }
        return lhs.team.title.localizedCaseInsensitiveCompare(rhs.team.title) == .orderedAscending
    }

    private func buildThread(rows: [TeamCommentRow]) -> [TeamCommentThread] {
        let rowById = Dictionary(uniqueKeysWithValues: rows.map { ($0.id, $0) })

        var repliesByParent: [String: [TeamCommentRow]] = [:]
        var roots: [TeamCommentRow] = []

        for row in rows {
            guard let parentId = row.comment.parentCommentId,
                  rowById[parentId] != nil else {
                roots.append(row)
                continue
            }

            repliesByParent[parentId, default: []].append(row)
        }

        roots.sort(by: sortComments)

        return roots.map { root in
            let replies = (repliesByParent[root.id] ?? []).sorted(by: sortComments)
            return TeamCommentThread(root: root, replies: replies)
        }
    }

    private func sortComments(_ lhs: TeamCommentRow, _ rhs: TeamCommentRow) -> Bool {
        if lhs.comment.createdAt != rhs.comment.createdAt {
            return lhs.comment.createdAt < rhs.comment.createdAt
        }
        return lhs.comment.id < rhs.comment.id
    }

    private func resolveAuthorPictures(pubkeys: Set<String>, coreManager: TenexCoreManager) async {
        for pubkey in pubkeys {
            if !resolvedAuthorPictures.contains(pubkey) {
                if let picture = try? await coreManager.safeCore.getProfilePicture(pubkey: pubkey),
                   !picture.isEmpty {
                    authorPictureCache[pubkey] = picture
                }
                resolvedAuthorPictures.insert(pubkey)
            }
        }
    }

    private func resolvedCommentAuthorDisplay(_ comment: TeamCommentInfo) -> String {
        let profile = comment.author.trimmingCharacters(in: .whitespacesAndNewlines)
        if !profile.isEmpty, profile != comment.pubkey {
            return profile
        }
        return coreManager?.displayName(for: comment.pubkey) ?? comment.pubkey
    }
}
