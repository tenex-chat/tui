import Foundation

struct SkillListItem: Identifiable, Hashable {
    let skill: Skill
    let authorDisplayName: String
    let authorPictureURL: String?

    var id: String { skill.id }
}

@MainActor
final class SkillsViewModel: ObservableObject {
    @Published private(set) var mine: [SkillListItem] = []
    @Published private(set) var community: [SkillListItem] = []
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
            async let fetchedSkills = coreManager.safeCore.getSkills()
            async let fetchedCurrentUser = coreManager.safeCore.getCurrentUser()

            let (skills, currentUser) = try await (fetchedSkills, fetchedCurrentUser)
            currentUserPubkey = currentUser?.pubkey

            await resolveAuthorPictures(for: skills, coreManager: coreManager)

            let sorted = skills.sorted { lhs, rhs in
                if lhs.createdAt != rhs.createdAt {
                    return lhs.createdAt > rhs.createdAt
                }
                if lhs.title != rhs.title {
                    return lhs.title.localizedCaseInsensitiveCompare(rhs.title) == .orderedAscending
                }
                return lhs.id > rhs.id
            }

            let items = sorted.map { skill in
                SkillListItem(
                    skill: skill,
                    authorDisplayName: coreManager.displayName(for: skill.pubkey),
                    authorPictureURL: authorPictureCache[skill.pubkey]
                )
            }

            mine = items.filter { item in
                guard let currentUserPubkey else { return false }
                return item.skill.pubkey.caseInsensitiveCompare(currentUserPubkey) == .orderedSame
            }

            community = items.filter { item in
                guard let currentUserPubkey else { return true }
                return item.skill.pubkey.caseInsensitiveCompare(currentUserPubkey) != .orderedSame
            }

            errorMessage = nil
            hasLoaded = true
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    var filteredMine: [SkillListItem] {
        filterRows(mine)
    }

    var filteredCommunity: [SkillListItem] {
        filterRows(community)
    }

    private func filterRows(_ rows: [SkillListItem]) -> [SkillListItem] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !query.isEmpty else { return rows }

        return rows.filter { item in
            let haystacks: [String] = [
                item.skill.title,
                item.skill.description,
                item.skill.content,
                item.authorDisplayName,
                item.skill.hashtags.joined(separator: " "),
                item.skill.fileIds.joined(separator: " ")
            ]

            return haystacks.contains { $0.lowercased().contains(query) }
        }
    }

    private func resolveAuthorPictures(for skills: [Skill], coreManager: TenexCoreManager) async {
        let uniquePubkeys = Set(skills.map(\.pubkey))

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
}
