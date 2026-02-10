import SwiftUI

// MARK: - Inline Delegation Card

/// Tappable card for q-tagged delegations within a conversation.
/// Shows recipient avatar, preview text, status badge, and navigation chevron.
struct InlineDelegationCard: View {
    let conversationId: String
    let recipientPubkeys: [String]
    let onTap: () -> Void

    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var conversationInfo: ConversationFullInfo?
    @State private var isLoading = true

    /// Avatar size for recipient
    private let avatarSize: CGFloat = 32

    var body: some View {
        Button(action: onTap) {
            HStack(spacing: 10) {
                // Recipient avatar
                if let info = conversationInfo {
                    AgentAvatarView(
                        agentName: info.author,
                        pubkey: info.authorPubkey,
                        size: avatarSize,
                        fontSize: 11,
                        showBorder: false
                    )
                    .environmentObject(coreManager)
                } else if let firstPubkey = recipientPubkeys.first {
                    // Fallback: use first p-tag pubkey
                    AgentAvatarView(
                        agentName: "delegate",
                        pubkey: firstPubkey,
                        size: avatarSize,
                        fontSize: 11,
                        showBorder: false
                    )
                    .environmentObject(coreManager)
                } else {
                    // Generic placeholder
                    Circle()
                        .fill(Color.gray.opacity(0.3))
                        .frame(width: avatarSize, height: avatarSize)
                        .overlay {
                            Image(systemName: "person.fill")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                }

                // Content
                VStack(alignment: .leading, spacing: 4) {
                    // Title or preview
                    if let info = conversationInfo {
                        Text(info.title)
                            .font(.subheadline)
                            .fontWeight(.medium)
                            .foregroundStyle(.primary)
                            .lineLimit(1)

                        // Author with colored name
                        HStack(spacing: 4) {
                            Text("@\(AgentNameFormatter.format(info.author))")
                                .font(.caption)
                                .foregroundStyle(deterministicColor(for: info.authorPubkey))

                            if let status = info.status {
                                StatusBadge(status: status, isActive: info.isActive)
                            }
                        }

                        // Summary preview
                        if let summary = info.summary, !summary.isEmpty {
                            Text(summary)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                    } else if isLoading {
                        Text("Loading delegation...")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    } else {
                        Text("Delegation")
                            .font(.subheadline)
                            .fontWeight(.medium)
                            .foregroundStyle(.primary)

                        Text(conversationId.prefix(12) + "...")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }

                Spacer()

                // Chevron
                Image(systemName: "chevron.right")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
            .padding(12)
            .background(Color.systemGray6)
            .clipShape(RoundedRectangle(cornerRadius: 10))
        }
        .buttonStyle(.plain)
        .task {
            await loadConversationInfo()
        }
    }

    // MARK: - Data Loading

    private func loadConversationInfo() async {
        isLoading = true
        let infos = await coreManager.safeCore.getConversationsByIds(conversationIds: [conversationId])
        await MainActor.run {
            conversationInfo = infos.first
            isLoading = false
        }
    }
}

// MARK: - Preview

#Preview {
    VStack(spacing: 12) {
        InlineDelegationCard(
            conversationId: "test-conversation-id",
            recipientPubkeys: ["abc123"]
        ) {
            print("Tapped delegation")
        }
        .environmentObject(TenexCoreManager())
    }
    .padding()
}
