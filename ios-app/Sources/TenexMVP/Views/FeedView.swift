import SwiftUI

struct FeedView: View {
    @EnvironmentObject var coreManager: TenexCoreManager

    var body: some View {
        VStack(spacing: 0) {
            FeedHeaderView(
                itemCount: coreManager.liveFeed.count,
                lastReceivedAt: coreManager.liveFeedLastReceivedAt,
                onClear: { coreManager.clearLiveFeed() }
            )

            Divider()

            if coreManager.liveFeed.isEmpty {
                FeedEmptyStateView()
            } else {
                List {
                    ForEach(coreManager.liveFeed) { item in
                        FeedRowView(
                            item: item,
                            conversationTitle: conversationTitle(for: item.conversationId),
                            conversationSummary: conversationSummary(for: item.conversationId)
                        )
                    }
                }
                .listStyle(.plain)
            }
        }
        .navigationTitle("Feed")
    }

    private func conversationTitle(for conversationId: String) -> String? {
        coreManager.conversations.first(where: { $0.id == conversationId })?.title
    }

    private func conversationSummary(for conversationId: String) -> String? {
        coreManager.conversations.first(where: { $0.id == conversationId })?.summary
    }
}

private struct FeedHeaderView: View {
    let itemCount: Int
    let lastReceivedAt: Date?
    let onClear: () -> Void

    private let relativeFormatter: RelativeDateTimeFormatter = {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .short
        return formatter
    }()

    var body: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text("Live kind:1 feed")
                    .font(.headline)
                Text(statusLine)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            Button("Clear") {
                onClear()
            }
            .font(.subheadline)
        }
        .padding(.horizontal)
        .padding(.vertical, 10)
        .background(Color.systemBackground)
    }

    private var statusLine: String {
        if itemCount == 0 {
            return "Waiting for events"
        }
        if let last = lastReceivedAt {
            return "Last event " + relativeFormatter.localizedString(for: last, relativeTo: Date())
        }
        return "Events received"
    }
}

private struct FeedEmptyStateView: View {
    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "dot.radiowaves.left.and.right")
                .font(.system(size: 44))
                .foregroundStyle(.secondary)
            Text("No events yet")
                .font(.title3)
                .fontWeight(.semibold)
            Text("This view shows kind:1 events as they arrive from the live stream.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)
        }
        .padding()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color.systemBackground)
    }
}

private struct FeedRowView: View {
    let item: LiveFeedItem
    let conversationTitle: String?
    let conversationSummary: String?

    @EnvironmentObject var coreManager: TenexCoreManager

    private let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd HH:mm:ss"
        return formatter
    }()

    private let relativeFormatter: RelativeDateTimeFormatter = {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .short
        return formatter
    }()

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .firstTextBaseline) {
                AgentAvatarView(
                    agentName: displayAuthor,
                    pubkey: authorPubkey,
                    size: 26,
                    fontSize: 10,
                    showBorder: false
                )
                .environmentObject(coreManager)

                Text(displayAuthor)
                    .font(.subheadline)
                    .fontWeight(.semibold)

                if !item.message.role.isEmpty {
                    Text(item.message.role)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Text(relativeFormatter.localizedString(for: item.receivedAt, relativeTo: Date()))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Text(conversationTitleLine)
                .font(.subheadline)
                .foregroundStyle(.secondary)

            if let summary = conversationSummary, !summary.isEmpty {
                Text(summary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }

            Text(messageContent)
                .font(.body)
                .lineLimit(5)

            HStack(spacing: 8) {
                Text("created " + createdAtString)
                Text("conv " + shortId(item.conversationId))
                Text("event " + shortId(item.id))
            }
            .font(.caption2)
            .foregroundStyle(.secondary)
        }
        .padding(.vertical, 6)
    }

    private var displayAuthor: String {
        let fallback = item.message.author.isEmpty ? fallbackAuthor : item.message.author
        return fallback.isEmpty ? "Unknown" : fallback
    }

    private var messageContent: String {
        item.message.content.isEmpty ? "(empty message)" : item.message.content
    }

    private var createdAtString: String {
        let date = Date(timeIntervalSince1970: TimeInterval(item.message.createdAt))
        return dateFormatter.string(from: date)
    }

    private var conversationTitleLine: String {
        if let title = conversationTitle, !title.isEmpty {
            return title
        }
        return "Conversation " + shortId(item.conversationId)
    }

    private var authorPubkey: String? {
        item.message.authorNpub.isEmpty ? nil : item.message.authorNpub
    }

    private var fallbackAuthor: String {
        if let pubkey = authorPubkey {
            return shortId(pubkey)
        }
        return ""
    }

    private func shortId(_ id: String) -> String {
        if id.count <= 12 { return id }
        let prefix = id.prefix(6)
        let suffix = id.suffix(6)
        return "\(prefix)...\(suffix)"
    }
}

#Preview {
    FeedView()
        .environmentObject(TenexCoreManager())
}
