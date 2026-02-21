import SwiftUI

// MARK: - Inline Delegation Card

/// Tappable card for q-tagged delegations within a conversation.
/// Shows recipient avatar, preview text, status badge, and navigation chevron.
struct InlineDelegationCard: View {
    let conversationId: String
    let recipientPubkeys: [String]
    let onTap: () -> Void

    @Environment(TenexCoreManager.self) var coreManager
    @State private var conversationInfo: ConversationFullInfo?
    @State private var isLoading = true
    private let profiler = PerformanceProfiler.shared

    /// Avatar size for recipient
    private let avatarSize: CGFloat = 32

    /// Resolve recipient pubkey: prefer thread p-tags, fall back to message p-tags, then thread creator.
    private var recipientPubkey: String? {
        conversationInfo?.thread.pTags.first ?? recipientPubkeys.first ?? conversationInfo?.thread.pubkey
    }

    private var recipientName: String {
        guard let pk = recipientPubkey else { return "delegate" }
        return coreManager.displayName(for: pk)
    }

    var body: some View {
        Button(action: onTap) {
            HStack(spacing: 10) {
                // Recipient avatar
                if let pk = recipientPubkey {
                    AgentAvatarView(
                        agentName: recipientName,
                        pubkey: pk,
                        size: avatarSize,
                        fontSize: 11,
                        showBorder: false
                    )
                    .environment(coreManager)
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
                        Text(info.thread.title)
                            .font(.subheadline)
                            .fontWeight(.medium)
                            .foregroundStyle(.primary)
                            .lineLimit(1)

                        // Recipient with colored name
                        HStack(spacing: 4) {
                            if let pk = recipientPubkey {
                                Text("@\(AgentNameFormatter.format(recipientName))")
                                    .font(.caption)
                                    .foregroundStyle(deterministicColor(for: pk))
                            }

                            if let status = info.thread.statusLabel {
                                StatusBadge(status: status, isActive: info.isActive)
                            }
                        }

                        // Summary preview
                        if let summary = info.thread.summary, !summary.isEmpty {
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
        .buttonStyle(.borderless)
        .task {
            await loadConversationInfo()
        }
        .onChange(of: coreManager.conversations) { _, _ in
            if let updated = coreManager.conversationById[conversationId] {
                conversationInfo = updated
                isLoading = false
            }
        }
    }

    // MARK: - Data Loading

    private func loadConversationInfo() async {
        isLoading = true
        let startedAt = CFAbsoluteTimeGetCurrent()
        let infos = await coreManager.safeCore.getConversationsByIds(conversationIds: [conversationId])
        await MainActor.run {
            conversationInfo = infos.first
            isLoading = false
            let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
            profiler.logEvent(
                "inline delegation load conversationId=\(conversationId) found=\(conversationInfo != nil) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                category: .general,
                level: elapsedMs >= 100 ? .error : .info
            )
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
        }
        .environment(TenexCoreManager())
    }
    .padding()
}
