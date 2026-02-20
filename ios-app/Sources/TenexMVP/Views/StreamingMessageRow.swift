import SwiftUI

/// Streaming message row matching SlackMessageRow visual style.
/// Shows agent avatar + name header (hidden when consecutive), streaming indicator,
/// accumulated text via MarkdownView, and a block cursor character.
struct StreamingMessageRow: View {
    let buffer: StreamingBuffer
    let isConsecutive: Bool
    /// Pre-resolved agent display name (avoids @EnvironmentObject dependency on coreManager).
    let agentName: String

    private let avatarSize: CGFloat = 20
    private let avatarFontSize: CGFloat = 8

    private var messageBodyFont: Font {
        #if os(macOS)
        .system(size: 14)
        #else
        .body
        #endif
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
                        .font(messageBodyFont)
                        .foregroundStyle(.primary)
                        .textSelection(.enabled)
                    Text("\u{258C}")
                        .font(messageBodyFont)
                        .foregroundStyle(.secondary)
                        .opacity(0.6)
                }
            }
        }
        .padding(.vertical, isConsecutive ? 3 : 8)
    }
}
