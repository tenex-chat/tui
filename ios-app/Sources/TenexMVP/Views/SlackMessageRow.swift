import SwiftUI

// MARK: - Slack Message Row

/// Slack-style message component with left-aligned layout.
/// Features:
/// - 36pt avatar on left (hidden when consecutive)
/// - Author name colored via deterministicColor
/// - Relative timestamp
/// - Consecutive message handling (hides avatar/header when same author)
/// - Tool call rendering via ToolCallRow
/// - Q-tag handling for delegations and ask events
struct SlackMessageRow: View {
    @EnvironmentObject var coreManager: TenexCoreManager

    let message: MessageInfo
    let isConsecutive: Bool
    let conversationId: String
    let projectId: String

    /// Callback when a delegation card is tapped
    var onDelegationTap: ((String) -> Void)?

    /// State for expanded/collapsed content
    @State private var isExpanded = false
    @State private var contentHeight: CGFloat = 0

    /// Avatar size
    private let avatarSize: CGFloat = 20

    /// Font size for avatar initials
    private let avatarFontSize: CGFloat = 8

    /// Maximum height before collapsing (roughly 60% of screen height)
    private let maxCollapsedHeight: CGFloat = 400

    /// Whether content needs collapsing
    private var needsCollapsing: Bool {
        contentHeight > maxCollapsedHeight
    }

    /// Get author color using deterministic hash
    private var authorColor: Color {
        deterministicColor(for: message.authorNpub)
    }

    /// Denylist of tools that use q-tags internally (not for delegations)
    private static let qTagDenylist = [
        "report_write", "report_read", "report_delete",
        "lesson_learn", "lesson_get"
    ]

    /// Check if q-tags should be rendered for this tool
    private func shouldRenderQTags(_ toolName: String?) -> Bool {
        guard let name = toolName?.lowercased() else { return true }
        return !Self.qTagDenylist.contains(where: { name.contains($0) })
    }

    /// Check if this is an ask or delegate tool (rendered via q-tags, not ToolCallRow)
    private func isAskOrDelegateTool(_ toolName: String?) -> Bool {
        guard let name = toolName?.lowercased() else { return false }
        return name.contains("ask") || name.contains("delegate")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            // Header with inline avatar (hidden when consecutive)
            if !isConsecutive {
                HStack(spacing: 6) {
                    AgentAvatarView(
                        agentName: message.author,
                        pubkey: message.authorNpub.isEmpty ? nil : npubToHex(message.authorNpub),
                        size: avatarSize,
                        fontSize: avatarFontSize,
                        showBorder: false
                    )
                    .environmentObject(coreManager)

                    Text(AgentNameFormatter.format(message.author))
                        .font(.subheadline)
                        .fontWeight(.semibold)
                        .foregroundStyle(authorColor)

                    Text(ConversationFormatters.formatRelativeTime(message.createdAt))
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    Spacer()
                }
            }

            // Tool call (if applicable and not ask/delegate)
            if message.isToolCall && !isAskOrDelegateTool(message.toolName) {
                ToolCallRow(
                    toolName: message.toolName,
                    toolArgs: message.toolArgs
                )
            }

            // Message content (if not empty)
            if !message.content.isEmpty {
                collapsibleContent
            }

            // Q-tag previews (ask events and delegations)
            if shouldRenderQTags(message.toolName) {
                qTagContent
            }
        }
        .padding(.vertical, isConsecutive ? 1 : 6)
    }

    // MARK: - Collapsible Content

    @ViewBuilder
    private var collapsibleContent: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Content with height measurement
            MarkdownView(content: message.content)
                .font(.body)
                .foregroundStyle(.primary)
                .textSelection(.enabled)
                .background(
                    GeometryReader { geometry in
                        Color.clear
                            .onAppear {
                                contentHeight = geometry.size.height
                            }
                            .onChange(of: message.content) {
                                contentHeight = geometry.size.height
                            }
                    }
                )
                .frame(maxHeight: isExpanded || !needsCollapsing ? nil : maxCollapsedHeight, alignment: .top)
                .clipped()

            // Gradient fade and "Read more" button when collapsed
            if needsCollapsing && !isExpanded {
                VStack(spacing: 0) {
                    // Gradient fade overlay
                    LinearGradient(
                        gradient: Gradient(colors: [
                            Color.systemBackground.opacity(0),
                            Color.systemBackground
                        ]),
                        startPoint: .top,
                        endPoint: .bottom
                    )
                    .frame(height: 40)
                    .offset(y: -40)

                    // Read more button
                    Button {
                        withAnimation(.easeInOut(duration: 0.3)) {
                            isExpanded = true
                        }
                    } label: {
                        HStack(spacing: 4) {
                            Text("Read more")
                                .font(.subheadline)
                                .fontWeight(.medium)
                            Image(systemName: "chevron.down")
                                .font(.caption)
                        }
                        .foregroundStyle(Color.composerAction)
                    }
                    .buttonStyle(.borderless)
                    .padding(.top, 4)
                }
            }

            // Collapse button when expanded
            if needsCollapsing && isExpanded {
                Button {
                    withAnimation(.easeInOut(duration: 0.3)) {
                        isExpanded = false
                    }
                } label: {
                    HStack(spacing: 4) {
                        Text("Show less")
                            .font(.subheadline)
                            .fontWeight(.medium)
                        Image(systemName: "chevron.up")
                            .font(.caption)
                    }
                    .foregroundStyle(Color.composerAction)
                }
                .buttonStyle(.borderless)
                .padding(.top, 8)
            }
        }
    }

    // MARK: - Q-Tag Content

    @ViewBuilder
    private var qTagContent: some View {
        // Inline ask event (if present)
        // Only render if we can convert npub to hex - required for answerAsk FFI call
        if let askEvent = message.askEvent, let hexPubkey = npubToHex(message.authorNpub) {
            InlineAskView(
                askEvent: askEvent,
                askEventId: message.id,
                askAuthorPubkey: hexPubkey,
                conversationId: conversationId,
                projectId: projectId
            )
            .environmentObject(coreManager)
        }

        // Delegation cards for q-tags (only if no ask event)
        if message.askEvent == nil && !message.qTags.isEmpty {
            ForEach(message.qTags, id: \.self) { qTag in
                InlineDelegationCard(
                    conversationId: qTag,
                    recipientPubkeys: message.pTags
                ) {
                    onDelegationTap?(qTag)
                }
                .environmentObject(coreManager)
            }
        }
    }

    // MARK: - Helpers

    /// Convert npub (bech32) to hex pubkey format for use with AgentAvatarView and other components
    private func npubToHex(_ npub: String) -> String? {
        guard !npub.isEmpty else { return nil }
        return Bech32.npubToHex(npub)
    }
}

// MARK: - Preview

#Preview {
    VStack(spacing: 0) {
        SlackMessageRow(
            message: MessageInfo(
                id: "1",
                content: "Hello, this is a test message with some content.",
                author: "claude-code",
                authorNpub: "abc123def456",
                createdAt: UInt64(Date().timeIntervalSince1970) - 300,
                isToolCall: false,
                role: "assistant",
                qTags: [],
                aTags: [],
                pTags: [],
                askEvent: nil,
                toolName: nil,
                toolArgs: nil
            ),
            isConsecutive: false,
            conversationId: "test",
            projectId: "test"
        )

        SlackMessageRow(
            message: MessageInfo(
                id: "2",
                content: "This is a consecutive message from the same author.",
                author: "claude-code",
                authorNpub: "abc123def456",
                createdAt: UInt64(Date().timeIntervalSince1970) - 60,
                isToolCall: false,
                role: "assistant",
                qTags: [],
                aTags: [],
                pTags: [],
                askEvent: nil,
                toolName: nil,
                toolArgs: nil
            ),
            isConsecutive: true,
            conversationId: "test",
            projectId: "test"
        )
    }
    .padding()
    .environmentObject(TenexCoreManager())
}
