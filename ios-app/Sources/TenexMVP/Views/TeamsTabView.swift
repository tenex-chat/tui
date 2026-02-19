import SwiftUI
import Kingfisher

enum TeamsLayoutMode {
    case adaptive
    case shellList
    case shellDetail
}

struct TeamsTabView: View {
    @EnvironmentObject private var coreManager: TenexCoreManager

    let layoutMode: TeamsLayoutMode
    private let selectedTeamBindingOverride: Binding<TeamInfo?>?

    @StateObject private var viewModel = TeamsViewModel()
    @State private var selectedTeamState: TeamInfo?
    @State private var hasConfiguredViewModel = false
    @State private var navigationPath: [TeamListItem] = []
    @State private var searchText = ""

    init(
        layoutMode: TeamsLayoutMode = .adaptive,
        selectedTeam: Binding<TeamInfo?>? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedTeamBindingOverride = selectedTeam
    }

    private var selectedTeamBinding: Binding<TeamInfo?> {
        selectedTeamBindingOverride ?? $selectedTeamState
    }

    private var query: String {
        searchText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    }

    private var filteredFeaturedTeams: [TeamListItem] {
        viewModel.featuredTeams.filter(matchesQuery)
    }

    private var filteredCategorySections: [TeamCategorySection] {
        viewModel.categorySections.compactMap { section in
            let filtered = section.teams.filter(matchesQuery)
            guard !filtered.isEmpty else { return nil }
            return TeamCategorySection(title: section.title, teams: filtered)
        }
    }

    var body: some View {
        Group {
            switch layoutMode {
            case .shellList, .adaptive:
                navigationListLayout
            case .shellDetail:
                shellDetailLayout
            }
        }
        .task {
            if !hasConfiguredViewModel {
                viewModel.configure(with: coreManager)
                hasConfiguredViewModel = true
            }
            await viewModel.loadIfNeeded()
        }
        .onReceive(coreManager.teamsVersionPublisher) { _ in
            Task { await viewModel.refresh() }
        }
        .alert(
            "Unable to Load Teams",
            isPresented: Binding(
                get: { viewModel.errorMessage != nil },
                set: { isPresented in
                    if !isPresented {
                        viewModel.errorMessage = nil
                    }
                }
            )
        ) {
            Button("OK", role: .cancel) {
                viewModel.errorMessage = nil
            }
        } message: {
            Text(viewModel.errorMessage ?? "Unknown error")
        }
    }

    private var navigationListLayout: some View {
        NavigationStack(path: $navigationPath) {
            listContent
                .navigationTitle("Teams")
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #else
                .toolbarTitleDisplayMode(.inline)
                #endif
                .navigationDestination(for: TeamListItem.self) { item in
                    TeamDetailView(teamId: item.id, viewModel: viewModel)
                }
                .searchable(text: $searchText, placement: .toolbar, prompt: "Search teams")
                .toolbar {
                    ToolbarItem(placement: .topBarTrailing) {
                        if viewModel.isLoading {
                            ProgressView()
                                .controlSize(.small)
                        }
                    }

                    ToolbarItem(placement: .topBarTrailing) {
                        Button {
                            Task { await viewModel.refresh() }
                        } label: {
                            Label("Refresh", systemImage: "arrow.clockwise")
                        }
                        .disabled(viewModel.isLoading)
                    }
                }
        }
        .accessibilityIdentifier("section_list_column")
    }

    private var shellDetailLayout: some View {
        ContentUnavailableView(
            "Teams",
            systemImage: "person.2",
            description: Text("Select a team from Browse to open details.")
        )
        .accessibilityIdentifier("detail_column")
    }

    private var listContent: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 26) {
                TeamsHeroHeader(
                    totalCount: viewModel.allTeams.count,
                    featuredCount: viewModel.featuredTeams.count
                )

                if viewModel.allTeams.isEmpty, !viewModel.isLoading {
                    ContentUnavailableView(
                        "No Teams Yet",
                        systemImage: "person.2.slash",
                        description: Text("Teams from kind:34199 events will appear here.")
                    )
                    .frame(maxWidth: .infinity, minHeight: 260)
                } else {
                    if !filteredFeaturedTeams.isEmpty {
                        featuredRail
                    }

                    if filteredCategorySections.isEmpty,
                       !query.isEmpty,
                       !viewModel.isLoading {
                        ContentUnavailableView(
                            "No Matching Teams",
                            systemImage: "magnifyingglass",
                            description: Text("Try a different search term.")
                        )
                        .frame(maxWidth: .infinity, minHeight: 180)
                    } else {
                        ForEach(filteredCategorySections) { section in
                            categorySection(section)
                        }
                    }
                }
            }
            .frame(maxWidth: 960, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .center)
            .padding(.horizontal, 18)
            .padding(.vertical, 20)
        }
        .background(Color.systemBackground.ignoresSafeArea())
        #if os(iOS)
        .refreshable {
            await viewModel.refresh()
        }
        #endif
    }

    private var featuredRail: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Featured Teams")
                    .font(.title3.weight(.semibold))
                Spacer()
                Text("Top by likes")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 14) {
                    ForEach(filteredFeaturedTeams) { item in
                        Button {
                            open(item)
                        } label: {
                            TeamFeaturedCard(item: item)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.vertical, 2)
            }
        }
    }

    private func categorySection(_ section: TeamCategorySection) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(section.title)
                .font(.title3.weight(.semibold))
                .lineLimit(1)

            LazyVGrid(
                columns: [GridItem(.adaptive(minimum: 290), spacing: 14, alignment: .top)],
                alignment: .leading,
                spacing: 14
            ) {
                ForEach(section.teams) { item in
                    Button {
                        open(item)
                    } label: {
                        TeamGridCard(item: item)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private func open(_ item: TeamListItem) {
        selectedTeamBinding.wrappedValue = item.team
        navigationPath.append(item)
    }

    private func matchesQuery(_ item: TeamListItem) -> Bool {
        guard !query.isEmpty else { return true }

        let haystacks = [
            item.team.title,
            item.team.description,
            item.authorDisplayName,
            item.team.categories.joined(separator: " "),
            item.team.tags.joined(separator: " ")
        ]

        return haystacks.contains { $0.lowercased().contains(query) }
    }
}

private struct TeamsHeroHeader: View {
    let totalCount: Int
    let featuredCount: Int

    var body: some View {
        ZStack(alignment: .leading) {
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .fill(Color.systemGray6.opacity(0.55))

            TeamsPolygonBackdrop()
                .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))

            VStack(alignment: .leading, spacing: 10) {
                Text("Teams")
                    .font(.system(size: 36, weight: .bold, design: .rounded))

                Text("Assemble and hire cross-functional agent squads into your projects.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)

                HStack(spacing: 10) {
                    Label("\(totalCount) total", systemImage: "person.2")
                    Label("\(featuredCount) featured", systemImage: "star")
                }
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
            }
            .padding(20)
        }
        .frame(height: 170)
    }
}

private struct TeamsPolygonBackdrop: View {
    var body: some View {
        GeometryReader { proxy in
            Canvas { context, size in
                let width = size.width
                let height = size.height

                func polygon(_ points: [CGPoint], color: Color, stroke: Color) {
                    var path = Path()
                    if let first = points.first {
                        path.move(to: first)
                        for point in points.dropFirst() {
                            path.addLine(to: point)
                        }
                        path.closeSubpath()
                    }
                    context.fill(path, with: .color(color))
                    context.stroke(path, with: .color(stroke), lineWidth: 1)
                }

                polygon(
                    [
                        CGPoint(x: width * 0.52, y: height * 0.12),
                        CGPoint(x: width * 0.88, y: height * 0.06),
                        CGPoint(x: width * 0.95, y: height * 0.42),
                        CGPoint(x: width * 0.64, y: height * 0.46)
                    ],
                    color: .white.opacity(0.08),
                    stroke: .white.opacity(0.16)
                )

                polygon(
                    [
                        CGPoint(x: width * 0.58, y: height * 0.58),
                        CGPoint(x: width * 0.84, y: height * 0.52),
                        CGPoint(x: width * 0.98, y: height * 0.90),
                        CGPoint(x: width * 0.68, y: height * 0.94)
                    ],
                    color: .white.opacity(0.05),
                    stroke: .white.opacity(0.13)
                )

                polygon(
                    [
                        CGPoint(x: width * 0.73, y: height * 0.20),
                        CGPoint(x: width * 1.00, y: height * 0.18),
                        CGPoint(x: width * 1.00, y: height * 0.62),
                        CGPoint(x: width * 0.82, y: height * 0.62)
                    ],
                    color: .white.opacity(0.04),
                    stroke: .white.opacity(0.11)
                )
            }
            .frame(width: proxy.size.width, height: proxy.size.height)
        }
        .allowsHitTesting(false)
    }
}

private struct TeamFeaturedCard: View {
    let item: TeamListItem

    var body: some View {
        TeamVisualCard(item: item, height: 320, titleFont: .title3.weight(.bold), showPrimaryCategory: true)
            .frame(width: 224)
    }
}

private struct TeamGridCard: View {
    let item: TeamListItem

    var body: some View {
        TeamVisualCard(item: item, height: 244, titleFont: .headline, showPrimaryCategory: false)
    }
}

private struct TeamVisualCard: View {
    let item: TeamListItem
    let height: CGFloat
    let titleFont: Font
    let showPrimaryCategory: Bool

    private var shortDescription: String {
        item.team.description
            .replacingOccurrences(of: "\n", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var primaryCategory: String? {
        item.team.categories
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .first(where: { !$0.isEmpty })
    }

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            TeamCoverImage(imageURL: item.team.image, title: item.team.title)

            Rectangle()
                .fill(.black.opacity(0.4))

            VStack(alignment: .leading, spacing: 8) {
                if showPrimaryCategory, let primaryCategory {
                    Text(primaryCategory)
                        .font(.caption2.weight(.semibold))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(.black.opacity(0.35), in: Capsule())
                }

                Text(item.team.title.isEmpty ? "Untitled Team" : item.team.title)
                    .font(titleFont)
                    .foregroundStyle(.white)
                    .lineLimit(2)

                if !shortDescription.isEmpty {
                    Text(shortDescription)
                        .font(.subheadline)
                        .foregroundStyle(.white.opacity(0.9))
                        .lineLimit(2)
                }

                HStack(spacing: 8) {
                    AgentAvatarView(
                        agentName: item.authorDisplayName,
                        pubkey: item.team.pubkey,
                        fallbackPictureUrl: item.authorPictureURL,
                        size: 20,
                        showBorder: false
                    )
                    Text(item.authorDisplayName)
                        .font(.caption.weight(.medium))
                        .foregroundStyle(.white.opacity(0.95))
                        .lineLimit(1)
                }

                HStack(spacing: 12) {
                    Label("\(item.team.likeCount)", systemImage: "heart")
                    Label("\(item.team.commentCount)", systemImage: "bubble.right")
                }
                .font(.caption)
                .foregroundStyle(.white.opacity(0.9))
            }
            .padding(14)
        }
        .frame(maxWidth: .infinity)
        .frame(height: height)
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .stroke(.white.opacity(0.12), lineWidth: 1)
        )
        .shadow(color: .black.opacity(0.16), radius: 14, y: 8)
    }
}

private struct TeamCoverImage: View {
    let imageURL: String?
    let title: String

    var body: some View {
        GeometryReader { proxy in
            if let imageURL,
               let url = URL(string: imageURL) {
                KFImage(url)
                    .placeholder {
                        TeamImagePlaceholder(title: title)
                    }
                    .retry(maxCount: 2, interval: .seconds(1))
                    .fade(duration: 0.15)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .frame(width: proxy.size.width, height: proxy.size.height)
                    .clipped()
            } else {
                TeamImagePlaceholder(title: title)
            }
        }
    }
}

private struct TeamImagePlaceholder: View {
    let title: String

    var body: some View {
        ZStack {
            Color.systemGray5.opacity(0.6)

            TeamsPolygonBackdrop()

            Text(initials)
                .font(.system(size: 36, weight: .black, design: .rounded))
                .foregroundStyle(.white.opacity(0.26))
        }
    }

    private var initials: String {
        let words = title
            .split(separator: " ")
            .map(String.init)
            .filter { !$0.isEmpty }

        if words.count >= 2 {
            let first = words[0].prefix(1)
            let second = words[1].prefix(1)
            return "\(first)\(second)".uppercased()
        }

        let compact = title.replacingOccurrences(of: " ", with: "")
        return String(compact.prefix(2)).uppercased()
    }
}

private struct TeamDetailView: View {
    @EnvironmentObject private var coreManager: TenexCoreManager

    let teamId: String
    @ObservedObject var viewModel: TeamsViewModel

    @State private var comments: [TeamCommentThread] = []
    @State private var isLoadingComments = false
    @State private var commentDraft = ""
    @State private var replyingTo: TeamCommentRow?
    @State private var isPostingComment = false

    @State private var showHireSheet = false
    @State private var isHiring = false
    @State private var hireResult: TeamHireResult?

    @State private var isTogglingLike = false
    @State private var actionError: String?

    private var item: TeamListItem? {
        viewModel.item(for: teamId)
    }

    var body: some View {
        Group {
            if let item {
                ScrollView {
                    VStack(alignment: .leading, spacing: 22) {
                        TeamDetailHero(item: item)

                        VStack(alignment: .leading, spacing: 12) {
                            actionsRow(item)
                            detailsMetaRow(item)
                        }

                        descriptionSection(item)

                        commentsSection(item)
                    }
                    .frame(maxWidth: 860, alignment: .leading)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .padding(.horizontal, 18)
                    .padding(.vertical, 18)
                }
                .background(Color.systemBackground.ignoresSafeArea())
                .navigationTitle(item.team.title.isEmpty ? "Team" : item.team.title)
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #else
                .toolbarTitleDisplayMode(.inline)
                #endif
            } else {
                ContentUnavailableView(
                    "Team Not Found",
                    systemImage: "person.2.slash",
                    description: Text("This team may have been removed.")
                )
            }
        }
        .task(id: teamId) {
            await refreshComments()
        }
        .sheet(isPresented: $showHireSheet) {
            if let item {
                TeamHireSheet(team: item.team) { project in
                    Task {
                        isHiring = true
                        let result = await viewModel.hireTeam(item.team, into: project)
                        isHiring = false
                        hireResult = result
                    }
                }
                .environmentObject(coreManager)
            }
        }
        .alert(item: $hireResult) { result in
            Alert(
                title: Text(result.title),
                message: Text(result.message),
                dismissButton: .default(Text("OK"))
            )
        }
        .alert(
            "Action Failed",
            isPresented: Binding(
                get: { actionError != nil },
                set: { isPresented in
                    if !isPresented {
                        actionError = nil
                    }
                }
            )
        ) {
            Button("OK", role: .cancel) {
                actionError = nil
            }
        } message: {
            Text(actionError ?? "Unknown error")
        }
    }

    private func actionsRow(_ item: TeamListItem) -> some View {
        HStack(spacing: 10) {
            Button {
                toggleLike()
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: item.team.likedByMe ? "heart.fill" : "heart")
                    Text(item.team.likedByMe ? "Liked" : "Like")
                    Text("\(item.team.likeCount)")
                        .foregroundStyle(.secondary)
                }
                .font(.subheadline.weight(.semibold))
            }
            .buttonStyle(.borderedProminent)
            .tint(item.team.likedByMe ? .pink : .accentColor)
            .disabled(isTogglingLike || isHiring)

            Button {
                showHireSheet = true
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: "person.crop.circle.badge.plus")
                    Text(isHiring ? "Hiring..." : "Hire Into Project")
                }
                .font(.subheadline.weight(.semibold))
            }
            .buttonStyle(.bordered)
            .disabled(isHiring)

            Spacer()
        }
    }

    private func detailsMetaRow(_ item: TeamListItem) -> some View {
        HStack(alignment: .center, spacing: 14) {
            AgentAvatarView(
                agentName: item.authorDisplayName,
                pubkey: item.team.pubkey,
                fallbackPictureUrl: item.authorPictureURL,
                size: 24,
                showBorder: false
            )

            VStack(alignment: .leading, spacing: 2) {
                Text(item.authorDisplayName)
                    .font(.subheadline.weight(.semibold))
                Text(formatDate(item.team.createdAt))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            if !item.team.categories.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(item.team.categories, id: \.self) { category in
                            Text(category)
                                .font(.caption.weight(.medium))
                                .padding(.horizontal, 10)
                                .padding(.vertical, 5)
                                .background(Color.systemGray6, in: Capsule())
                        }
                    }
                }
                .frame(maxWidth: 320)
            }
        }
    }

    private func descriptionSection(_ item: TeamListItem) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Description")
                .font(.title3.weight(.semibold))

            MarkdownView(content: item.team.description.isEmpty ? "No description provided." : item.team.description)
                .padding(14)
                .background(Color.systemGray6.opacity(0.55), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        }
    }

    private func commentsSection(_ item: TeamListItem) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Comments")
                    .font(.title3.weight(.semibold))
                Spacer()
                Text("\(item.team.commentCount)")
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.secondary)
            }

            if isLoadingComments {
                HStack(spacing: 8) {
                    ProgressView()
                        .controlSize(.small)
                    Text("Loading comments...")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
            } else if comments.isEmpty {
                Text("No comments yet. Start the thread.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .padding(.vertical, 6)
            } else {
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(comments) { thread in
                        TeamCommentThreadView(
                            thread: thread,
                            onReply: { row in
                                replyingTo = row
                            }
                        )
                    }
                }
            }

            commentComposer(item)
        }
    }

    private func commentComposer(_ item: TeamListItem) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            if let replyingTo {
                HStack(spacing: 8) {
                    Text("Replying to \(replyingTo.authorDisplayName)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button("Cancel") {
                        self.replyingTo = nil
                    }
                    .font(.caption)
                }
            }

            TextEditor(text: $commentDraft)
                .frame(minHeight: 72)
                .padding(6)
                .background(Color.systemGray6.opacity(0.55), in: RoundedRectangle(cornerRadius: 12, style: .continuous))

            HStack {
                Spacer()
                Button {
                    Task { await postComment(item) }
                } label: {
                    if isPostingComment {
                        ProgressView()
                            .controlSize(.small)
                    } else {
                        Label("Post Comment", systemImage: "paperplane.fill")
                    }
                }
                .buttonStyle(.borderedProminent)
                .disabled(isPostingComment || commentDraft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
    }

    private func refreshComments() async {
        guard let item else { return }

        isLoadingComments = true
        defer { isLoadingComments = false }

        do {
            comments = try await viewModel.loadCommentThread(for: item.team)
        } catch {
            actionError = error.localizedDescription
        }
    }

    private func postComment(_ item: TeamListItem) async {
        let trimmed = commentDraft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        isPostingComment = true
        defer { isPostingComment = false }

        let success = await viewModel.postComment(
            team: item.team,
            content: trimmed,
            parentComment: replyingTo
        )

        guard success else {
            actionError = viewModel.errorMessage ?? "Failed to post comment."
            return
        }

        commentDraft = ""
        replyingTo = nil
        await refreshComments()
    }

    private func toggleLike() {
        guard !isTogglingLike else { return }

        isTogglingLike = true
        Task {
            let success = await viewModel.toggleLike(teamId: teamId)
            isTogglingLike = false
            if !success {
                actionError = viewModel.errorMessage ?? "Unable to react to team."
            }
        }
    }

    private func formatDate(_ timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return Self.dateFormatter.string(from: date)
    }

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter
    }()
}

private struct TeamDetailHero: View {
    let item: TeamListItem

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            TeamCoverImage(imageURL: item.team.image, title: item.team.title)
            Rectangle()
                .fill(.black.opacity(0.38))

            VStack(alignment: .leading, spacing: 8) {
                Text(item.team.title.isEmpty ? "Untitled Team" : item.team.title)
                    .font(.system(size: 30, weight: .bold, design: .rounded))
                    .foregroundStyle(.white)
                    .lineLimit(2)

                HStack(spacing: 12) {
                    Label("\(item.team.agentIds.count) agents", systemImage: "person.3")
                    Label("\(item.team.likeCount) likes", systemImage: "heart")
                    Label("\(item.team.commentCount) comments", systemImage: "bubble.right")
                }
                .font(.caption.weight(.medium))
                .foregroundStyle(.white.opacity(0.92))
            }
            .padding(18)
        }
        .frame(maxWidth: .infinity)
        .frame(height: 260)
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .stroke(.white.opacity(0.12), lineWidth: 1)
        )
    }
}

private struct TeamCommentThreadView: View {
    let thread: TeamCommentThread
    let onReply: (TeamCommentRow) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            TeamCommentRowView(comment: thread.root, onReply: onReply)

            if !thread.replies.isEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    ForEach(thread.replies) { reply in
                        TeamCommentRowView(comment: reply, onReply: onReply)
                    }
                }
                .padding(.leading, 24)
            }
        }
    }
}

private struct TeamCommentRowView: View {
    let comment: TeamCommentRow
    let onReply: (TeamCommentRow) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                AgentAvatarView(
                    agentName: comment.authorDisplayName,
                    pubkey: comment.comment.pubkey,
                    fallbackPictureUrl: comment.authorPictureURL,
                    size: 20,
                    showBorder: false
                )

                Text(comment.authorDisplayName)
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.primary)

                Text(formatDate(comment.comment.createdAt))
                    .font(.caption2)
                    .foregroundStyle(.secondary)

                Spacer()

                Button("Reply") {
                    onReply(comment)
                }
                .font(.caption)
                .buttonStyle(.plain)
            }

            Text(comment.comment.content)
                .font(.subheadline)
                .foregroundStyle(.primary)
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color.systemGray6.opacity(0.6), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        }
    }

    private func formatDate(_ timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return Self.dateFormatter.string(from: date)
    }

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .none
        formatter.timeStyle = .short
        return formatter
    }()
}

private struct TeamHireSheet: View {
    @EnvironmentObject private var coreManager: TenexCoreManager
    @Environment(\.dismiss) private var dismiss

    let team: TeamInfo
    let onConfirm: (ProjectInfo) -> Void

    @State private var selectedProjectId: String?
    @State private var searchText = ""

    private var sortedProjects: [ProjectInfo] {
        coreManager.projects
            .filter { !$0.isDeleted }
            .sorted { lhs, rhs in
                lhs.title.localizedCaseInsensitiveCompare(rhs.title) == .orderedAscending
            }
    }

    private var filteredProjects: [ProjectInfo] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return sortedProjects }

        return sortedProjects.filter { project in
            project.title.localizedCaseInsensitiveContains(query)
                || project.id.localizedCaseInsensitiveContains(query)
                || (project.description?.localizedCaseInsensitiveContains(query) ?? false)
        }
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(team.title.isEmpty ? "Untitled Team" : team.title)
                            .font(.headline)
                        Text("Hire \(team.agentIds.count) agent\(team.agentIds.count == 1 ? "" : "s") into one project")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 2)
                }

                Section {
                    if filteredProjects.isEmpty {
                        ContentUnavailableView(
                            "No Projects",
                            systemImage: "folder.badge.questionmark",
                            description: Text(searchText.isEmpty ? "No projects available." : "No projects match your search.")
                        )
                    } else {
                        ForEach(filteredProjects, id: \.id) { project in
                            Button {
                                selectedProjectId = project.id
                            } label: {
                                HStack(spacing: 10) {
                                    VStack(alignment: .leading, spacing: 4) {
                                        Text(project.title)
                                            .font(.body.weight(.medium))
                                            .foregroundStyle(.primary)

                                        Text(project.id)
                                            .font(.caption2.monospaced())
                                            .foregroundStyle(.secondary)
                                            .lineLimit(1)
                                    }

                                    Spacer()

                                    Image(systemName: selectedProjectId == project.id ? "checkmark.circle.fill" : "circle")
                                        .font(.title3)
                                        .foregroundStyle(selectedProjectId == project.id ? Color.accentColor : .secondary)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }
            #if os(iOS)
            .listStyle(.insetGrouped)
            #else
            .listStyle(.inset)
            #endif
            .searchable(text: $searchText, prompt: "Search projects")
            .navigationTitle("Hire Team")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                }

                ToolbarItem(placement: .primaryAction) {
                    Button("Hire") {
                        guard let selectedProjectId,
                              let project = sortedProjects.first(where: { $0.id == selectedProjectId }) else {
                            return
                        }
                        onConfirm(project)
                        dismiss()
                    }
                    .fontWeight(.semibold)
                    .disabled(selectedProjectId == nil)
                }
            }
        }
        #if os(iOS)
        .tenexModalPresentation(detents: [.medium, .large])
        #else
        .frame(minWidth: 500, idealWidth: 620, minHeight: 420, idealHeight: 560)
        #endif
    }
}
