import SwiftUI

struct TeamDetailView: View {
    @Environment(TenexCoreManager.self) private var coreManager

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

    @State private var agentDefinitions: [AgentDefinitionListItem] = []
    @State private var isLoadingAgentDefinitions = false

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

                        agentDefinitionsSection(item)

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
            await refreshAgentDefinitions()
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
                .environment(coreManager)
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
            .adaptiveProminentGlassButtonStyle()
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
            .adaptiveGlassButtonStyle()
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

    @ViewBuilder
    private func agentDefinitionsSection(_ item: TeamListItem) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Agent Definitions")
                    .font(.title3.weight(.semibold))
                Spacer()
                Text("\(item.team.agentDefinitionIds.count)")
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.secondary)
            }

            if isLoadingAgentDefinitions {
                HStack(spacing: 8) {
                    ProgressView()
                        .controlSize(.small)
                    Text("Loading agent definitions...")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .padding(.vertical, 4)
            } else if item.team.agentDefinitionIds.isEmpty {
                Text("This team has no agent definitions.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .padding(.vertical, 6)
            } else if agentDefinitions.isEmpty {
                Text("Agent definitions not synced yet.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .padding(.vertical, 6)
            } else {
                LazyVGrid(
                    columns: [
                        GridItem(
                            .adaptive(
                                minimum: AgentDefinitionVisualCard.gridMinimumWidth,
                                maximum: AgentDefinitionVisualCard.gridMaximumWidth
                            ),
                            spacing: 14,
                            alignment: .top
                        )
                    ],
                    alignment: .leading,
                    spacing: 14
                ) {
                    ForEach(agentDefinitions) { agentItem in
                        AgentDefinitionVisualCard(item: agentItem)
                    }
                }
            }
        }
    }

    private func refreshAgentDefinitions() async {
        guard let item else { return }

        isLoadingAgentDefinitions = true
        defer { isLoadingAgentDefinitions = false }

        do {
            agentDefinitions = try await viewModel.loadAgentDefinitions(for: item.team)
        } catch {
            actionError = error.localizedDescription
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
                .adaptiveProminentGlassButtonStyle()
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
                    Label("\(item.team.agentDefinitionIds.count) agent definitions", systemImage: "person.3")
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
