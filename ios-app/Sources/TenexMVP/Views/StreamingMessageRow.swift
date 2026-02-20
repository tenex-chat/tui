import SwiftUI

/// Streaming message row matching SlackMessageRow visual style.
/// Shows agent avatar + name header (hidden when consecutive), streaming indicator,
/// accumulated text via MarkdownView, and a block cursor character.
struct StreamingMessageRow: View {
    @EnvironmentObject var coreManager: TenexCoreManager

    let buffer: StreamingBuffer
    let isConsecutive: Bool

    private let avatarSize: CGFloat = 20
    private let avatarFontSize: CGFloat = 8

    private var agentName: String {
        coreManager.displayName(for: buffer.agentPubkey)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            if !isConsecutive {
                HStack(spacing: 6) {
                    AgentAvatarView(
                        agentName: agentName,
                        pubkey: buffer.agentPubkey,
                        size: avatarSize,
                        fontSize: avatarFontSize,
                        showBorder: false
                    )
                    .environmentObject(coreManager)

                    Text(AgentNameFormatter.format(agentName))
                        .font(.subheadline)
                        .fontWeight(.semibold)
                        .foregroundStyle(deterministicColor(for: buffer.agentPubkey))

                    HStack(spacing: 4) {
                        ProgressView()
                            .scaleEffect(0.5)
                            .frame(width: 12, height: 12)
                        Text("streaming")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }

                    Spacer()
                }
            }

            if !buffer.text.isEmpty {
                HStack(alignment: .lastTextBaseline, spacing: 0) {
                    MarkdownView(content: buffer.text)
                        .font(.body)
                        .foregroundStyle(.primary)
                    Text("\u{258C}")
                        .font(.body)
                        .foregroundStyle(.secondary)
                        .opacity(0.6)
                }
            }
        }
        .padding(.vertical, isConsecutive ? 3 : 8)
    }
}
