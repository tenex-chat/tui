import SwiftUI

/// Streaming message row matching SlackMessageRow visual style.
/// Shows agent avatar + name header (hidden when consecutive), streaming indicator,
/// and character-by-character text reveal with a trailing fade gradient.
struct StreamingMessageRow: View {
    let buffer: StreamingBuffer
    let isConsecutive: Bool
    /// Pre-resolved agent display name (avoids @EnvironmentObject dependency on coreManager).
    let agentName: String

    @State private var visibleTextChars: Int = 0

    private let avatarSize: CGFloat = 20
    private let avatarFontSize: CGFloat = 8
    private static let revealCharsPerTick: Int = 3
    private static let revealTickNs: UInt64 = 33_000_000
    private static let fadeTailChars: Int = 5

    private var messageBodyFont: Font {
        #if os(macOS)
        .system(size: 14)
        #else
        .body
        #endif
    }

    private var targetTextChars: Int {
        buffer.text.count
    }

    private var visibleText: String {
        String(buffer.text.prefix(visibleTextChars))
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

            (fadedStreamingText(visibleText) + cursorText)
                .font(messageBodyFont)
                #if os(macOS)
                .textSelection(.disabled)
                #else
                .textSelection(.enabled)
                #endif
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, isConsecutive ? 3 : 8)
        .onAppear {
            visibleTextChars = min(visibleTextChars, targetTextChars)
        }
        .onChange(of: buffer.text) { oldText, newText in
            if newText.hasPrefix(oldText) {
                visibleTextChars = min(visibleTextChars, newText.count)
                return
            }
            let common = Self.commonPrefixLength(oldText, newText)
            visibleTextChars = min(visibleTextChars, common)
        }
        .task(id: buffer.text) {
            await revealToTarget()
        }
    }

    @MainActor
    private func revealToTarget() async {
        let target = targetTextChars
        if visibleTextChars > target {
            visibleTextChars = target
            return
        }
        while visibleTextChars < target {
            visibleTextChars = min(target, visibleTextChars + Self.revealCharsPerTick)
            try? await Task.sleep(nanoseconds: Self.revealTickNs)
        }
    }

    private var cursorText: Text {
        Text("\u{258C}")
            .foregroundColor(.secondary.opacity(0.6))
    }

    private func fadedStreamingText(_ text: String) -> Text {
        guard !text.isEmpty else { return Text("") }

        let fadeCount = min(Self.fadeTailChars, text.count)
        let splitIndex = text.index(text.endIndex, offsetBy: -fadeCount)
        let stablePart = String(text[..<splitIndex])
        let tailPart = text[splitIndex...]

        var rendered = Text(stablePart).foregroundColor(.primary)
        for (offset, ch) in tailPart.enumerated() {
            let distanceFromEnd = fadeCount - offset - 1
            rendered = rendered + Text(String(ch))
                .foregroundColor(.primary.opacity(Self.fadeOpacity(for: distanceFromEnd)))
        }
        return rendered
    }

    private static func fadeOpacity(for distanceFromEnd: Int) -> Double {
        switch distanceFromEnd {
        case 0:
            return 0.0
        case 1:
            return 0.24
        case 2:
            return 0.46
        case 3:
            return 0.68
        case 4:
            return 0.85
        default:
            return 1.0
        }
    }

    private static func commonPrefixLength(_ lhs: String, _ rhs: String) -> Int {
        zip(lhs, rhs).prefix { $0 == $1 }.count
    }
}
