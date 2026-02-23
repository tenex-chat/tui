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
struct SlackMessageRow: View, Equatable {
    let message: Message
    let isConsecutive: Bool
    let conversationId: String
    let projectId: String
    /// Pre-resolved author display name (avoids @EnvironmentObject dependency on coreManager).
    let authorDisplayName: String
    /// Pre-formatted "@name1, @name2" string for p-tag recipients (empty when no p-tags).
    let directedRecipientsText: String

    /// Callback when a delegation card is tapped
    var onDelegationTap: ((String) -> Void)?
    /// Callback to inspect the backing raw Nostr event for this message.
    var onViewRawEvent: ((String) -> Void)?

    static func == (lhs: SlackMessageRow, rhs: SlackMessageRow) -> Bool {
        lhs.message == rhs.message &&
        lhs.isConsecutive == rhs.isConsecutive &&
        lhs.conversationId == rhs.conversationId &&
        lhs.projectId == rhs.projectId &&
        lhs.authorDisplayName == rhs.authorDisplayName &&
        lhs.directedRecipientsText == rhs.directedRecipientsText
    }

    /// State for expanded/collapsed content
    @State private var isExpanded = false
    @State private var contentHeight: CGFloat = 0
    @State private var isHovered = false

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
        deterministicColor(for: message.pubkey)
    }

    /// Match TUI semantics: q-tag presence should also classify as tool use.
    private var isToolUse: Bool {
        message.toolName != nil || !message.qTags.isEmpty
    }

    private var hasPTags: Bool {
        !message.pTags.isEmpty
    }

    /// P-tagged messages always show the header even in consecutive groups.
    private var shouldShowHeader: Bool {
        !isConsecutive || hasPTags
    }

    private var readMoreFadeBackground: Color {
        #if os(macOS)
        return .conversationWorkspaceBackdropMac
        #else
        return .systemBackground
        #endif
    }

    var body: some View {
        Group {
            VStack(alignment: .leading, spacing: 6) {
                // Header with inline avatar (hidden for normal consecutive messages)
                if shouldShowHeader {
                    HStack(spacing: 6) {
                        AgentAvatarView(
                            agentName: authorDisplayName,
                            pubkey: message.pubkey,
                            size: avatarSize,
                            fontSize: avatarFontSize,
                            showBorder: false
                        )

                        if hasPTags {
                            Text(AgentNameFormatter.format(authorDisplayName))
                                .font(.subheadline)
                                .fontWeight(.semibold)
                                .foregroundStyle(authorColor)
                            Text("->")
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                            Text(directedRecipientsText)
                                .font(.subheadline)
                                .fontWeight(.medium)
                                .foregroundStyle(Color.agentBrand)
                        } else {
                            Text(AgentNameFormatter.format(authorDisplayName))
                                .font(.subheadline)
                                .fontWeight(.semibold)
                                .foregroundStyle(authorColor)
                        }

                        Text(ConversationFormatters.formatRelativeTime(message.createdAt))
                            .font(.caption)
                            .foregroundStyle(.secondary)

                        Spacer()
                    }
                }

                // Tool-use messages render only compact summaries (no markdown body),
                // matching TUI per-tool selection precedence.
                if isToolUse {
                    ToolCallRow(
                        toolName: message.toolName,
                        toolArgs: message.toolArgs,
                        contentFallback: message.content
                    )
                } else if !message.content.isEmpty {
                    collapsibleContent
                }

                // Inline ask event on the message itself (root ask message).
                if let askEvent = message.askEvent, !message.pubkey.isEmpty {
                    InlineAskView(
                        askEvent: askEvent,
                        askEventId: message.id,
                        askAuthorPubkey: message.pubkey,
                        conversationId: conversationId,
                        projectId: projectId
                    )
                }

                // Q-tags can resolve to either ask events or delegation threads.
                if ConversationRenderPolicy.shouldRenderQTags(toolName: message.toolName), !message.qTags.isEmpty {
                    ForEach(message.qTags, id: \.self) { qTag in
                        QTagReferenceRow(
                            qTag: qTag,
                            recipientPubkeys: message.pTags,
                            conversationId: conversationId,
                            projectId: projectId,
                            onDelegationTap: onDelegationTap
                        )
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, isConsecutive ? 5 : 14)
        .overlay(alignment: .topTrailing) {
            #if os(macOS)
            if let onViewRawEvent {
                messageActionsMenu(onViewRawEvent: onViewRawEvent)
                    .padding(.top, shouldShowHeader ? 0 : 2)
                    .opacity(isHovered ? 1.0 : 0.32)
                    .animation(.easeInOut(duration: 0.14), value: isHovered)
            }
            #endif
        }
        #if os(macOS)
        .onHover { isHovering in
            isHovered = isHovering
        }
        #endif
        .contextMenu {
            if let onViewRawEvent {
                Button {
                    onViewRawEvent(message.id)
                } label: {
                    Label("View Raw Event", systemImage: "doc.text.magnifyingglass")
                }
            }
        }
        #if os(macOS)
        // Large transcripts can trigger expensive accessibility tree traversals on every host update.
        // Keep a compact per-row accessibility representation to avoid deep subtree recomputation.
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(accessibilitySummary)
        #endif
    }

    // MARK: - Collapsible Content

    @ViewBuilder
    private var collapsibleContent: some View {
        #if os(macOS)
        // macOS transcript performance: avoid per-row geometry measurement and collapse state churn.
        // Most rows are short; rendering directly keeps large transcripts responsive.
        MarkdownView(content: message.content)
            .font(.system(size: 14))
            .foregroundStyle(.primary.opacity(hasPTags ? 1.0 : 0.72))
            .textSelection(.enabled)
        #else
        VStack(alignment: .leading, spacing: 0) {
            // Content with height measurement
            MarkdownView(content: message.content)
                .font(hasPTags ? .body : .callout)
                .foregroundStyle(.primary.opacity(hasPTags ? 1.0 : 0.72))
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
                            readMoreFadeBackground.opacity(0),
                            readMoreFadeBackground
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
        #endif
    }

    // MARK: - Helpers

    #if os(macOS)
    @ViewBuilder
    private func messageActionsMenu(onViewRawEvent: @escaping (String) -> Void) -> some View {
        Menu {
            Button {
                onViewRawEvent(message.id)
            } label: {
                Label("View Raw Event", systemImage: "doc.text.magnifyingglass")
            }
        } label: {
            Image(systemName: "ellipsis.circle")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .padding(.horizontal, 4)
                .padding(.vertical, 2)
                .contentShape(Rectangle())
        }
        .menuStyle(.borderlessButton)
        .buttonStyle(.plain)
    }
    #endif

    #if os(macOS)
    private var accessibilitySummary: String {
        var parts: [String] = []
        if !message.pubkey.isEmpty {
            parts.append("From \(AgentNameFormatter.format(authorDisplayName))")
        }

        let normalizedContent = message.content
            .replacingOccurrences(of: "\n", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)

        if !normalizedContent.isEmpty {
            let preview: String
            if normalizedContent.count > 220 {
                preview = String(normalizedContent.prefix(220)) + "..."
            } else {
                preview = normalizedContent
            }
            parts.append(preview)
        }

        if !message.qTags.isEmpty {
            parts.append("\(message.qTags.count) delegation reference\(message.qTags.count == 1 ? "" : "s")")
        }

        return parts.isEmpty ? "Message" : parts.joined(separator: ". ")
    }
    #endif
}

private struct QTagReferenceRow: View {
    @Environment(TenexCoreManager.self) var coreManager

    let qTag: String
    let recipientPubkeys: [String]
    let conversationId: String
    let projectId: String
    let onDelegationTap: ((String) -> Void)?

    @State private var askLookupInfo: AskEventLookupInfo?

    var body: some View {
        Group {
            if let askLookupInfo {
                InlineAskView(
                    askEvent: askLookupInfo.askEvent,
                    askEventId: qTag,
                    askAuthorPubkey: askLookupInfo.authorPubkey,
                    conversationId: conversationId,
                    projectId: projectId
                )
                .environment(coreManager)
            } else {
                InlineDelegationCard(
                    conversationId: qTag,
                    recipientPubkeys: recipientPubkeys
                ) {
                    onDelegationTap?(qTag)
                }
                .environment(coreManager)
            }
        }
        .task(id: qTag) {
            await resolveAskEvent()
        }
    }

    private func resolveAskEvent() async {
        askLookupInfo = await coreManager.safeCore.getAskEventById(eventId: qTag)
    }
}

// MARK: - Preview

#Preview {
    VStack(spacing: 0) {
        SlackMessageRow(
            message: Message(
                id: "1",
                content: "Hello, this is a test message with some content.",
                pubkey: "abc123def456",
                threadId: "test",
                createdAt: UInt64(Date().timeIntervalSince1970) - 300,
                replyTo: nil,
                isReasoning: false,
                askEvent: nil,
                qTags: [],
                aTags: [],
                pTags: [],
                toolName: nil,
                toolArgs: nil,
                llmMetadata: [:],
                delegationTag: nil,
                branch: nil
            ),
            isConsecutive: false,
            conversationId: "test",
            projectId: "test",
            authorDisplayName: "Test Agent",
            directedRecipientsText: "",
            onViewRawEvent: nil
        )

        SlackMessageRow(
            message: Message(
                id: "2",
                content: "This is a consecutive message from the same author.",
                pubkey: "abc123def456",
                threadId: "test",
                createdAt: UInt64(Date().timeIntervalSince1970) - 60,
                replyTo: nil,
                isReasoning: false,
                askEvent: nil,
                qTags: [],
                aTags: [],
                pTags: [],
                toolName: nil,
                toolArgs: nil,
                llmMetadata: [:],
                delegationTag: nil,
                branch: nil
            ),
            isConsecutive: true,
            conversationId: "test",
            projectId: "test",
            authorDisplayName: "Test Agent",
            directedRecipientsText: "",
            onViewRawEvent: nil
        )
    }
    .padding()
    .environment(TenexCoreManager())
}
