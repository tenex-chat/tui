import SwiftUI

// MARK: - Shared Status Badge

/// Unified status badge component used across conversation views
struct StatusBadge: View {
    let status: String
    let isActive: Bool

    private var displayText: String {
        if isActive { return "Active" }
        switch status.lowercased() {
        case "active", "in progress": return "Active"
        case "waiting", "blocked": return "Waiting"
        case "completed", "done": return "Completed"
        default: return status.capitalized
        }
    }

    private var backgroundColor: Color {
        if isActive { return Color.green.opacity(0.15) }
        switch status.lowercased() {
        case "active", "in progress": return Color.green.opacity(0.15)
        case "waiting", "blocked": return Color.orange.opacity(0.15)
        case "completed", "done": return Color.systemGray5
        default: return Color.blue.opacity(0.15)
        }
    }

    private var textColor: Color {
        if isActive { return .green }
        switch status.lowercased() {
        case "active", "in progress": return .green
        case "waiting", "blocked": return .orange
        case "completed", "done": return .secondary
        default: return .blue
        }
    }

    var body: some View {
        Text(displayText)
            .font(.caption)
            .fontWeight(.medium)
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .background(backgroundColor)
            .foregroundStyle(textColor)
            .clipShape(Capsule())
    }
}

// MARK: - Shared Message Bubble

/// Unified message bubble component used in conversation detail and full conversation views
struct SharedMessageBubble: View {
    let message: MessageInfo
    let showAvatar: Bool

    init(message: MessageInfo, showAvatar: Bool = true) {
        self.message = message
        self.showAvatar = showAvatar
    }

    private var isUser: Bool {
        message.role == "user"
    }

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            if !isUser && showAvatar {
                SharedAgentAvatar(agentName: message.author, size: 32, fontSize: 11)
            }

            if isUser { Spacer(minLength: 60) }

            VStack(alignment: isUser ? .trailing : .leading, spacing: 4) {
                // Header with author name and time
                HStack(spacing: 6) {
                    if !isUser {
                        Text(AgentNameFormatter.format(message.author))
                            .font(.caption)
                            .fontWeight(.medium)
                            .foregroundStyle(.secondary)
                    }

                    Text(ConversationFormatters.formatRelativeTime(message.createdAt))
                        .font(.caption2)
                        .foregroundStyle(.tertiary)

                    if isUser {
                        Text("You")
                            .font(.caption)
                            .fontWeight(.medium)
                            .foregroundStyle(.secondary)
                    }
                }

                // Message content with markdown rendering
                MarkdownView(content: message.content)
                    .font(.body)
                    .padding(12)
                    .background(isUser ? Color.accentColor : Color.systemGray6)
                    .foregroundStyle(isUser ? .white : .primary)
                    .clipShape(RoundedRectangle(cornerRadius: 16))
            }

            if !isUser { Spacer(minLength: 60) }

            if isUser && showAvatar {
                Circle()
                    .fill(Color.green.gradient)
                    .frame(width: 32, height: 32)
                    .overlay {
                        Image(systemName: "person.fill")
                            .font(.caption)
                            .foregroundStyle(.white)
                    }
            }
        }
    }
}

// MARK: - Shared Card View

/// Generic card container view with optional title header
struct SharedCardView<Content: View>: View {
    var title: String?
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let title = title {
                Text(title)
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 16)
                    .padding(.top, 20)
                    .padding(.bottom, 8)
            }

            content
                .padding(16)
                .background(Color.systemBackground)
                .clipShape(RoundedRectangle(cornerRadius: 12))
                .padding(.horizontal, 16)
                .padding(.top, title == nil ? 16 : 0)
        }
    }
}

// MARK: - Delegation Row

/// Row view for displaying a delegation item with consistent styling
struct SharedDelegationRow: View {
    let delegation: DelegationItem
    let onTap: (() -> Void)?

    init(delegation: DelegationItem, onTap: (() -> Void)? = nil) {
        self.delegation = delegation
        self.onTap = onTap
    }

    var body: some View {
        HStack(spacing: 12) {
            SharedAgentAvatar(agentName: delegation.recipient, size: 40, fontSize: 13)

            VStack(alignment: .leading, spacing: 4) {
                Text(AgentNameFormatter.format(delegation.recipient))
                    .font(.subheadline)
                    .fontWeight(.semibold)

                Text(delegation.messagePreview)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }

            Spacer()

            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .contentShape(Rectangle())
        .onTapGesture {
            onTap?()
        }
    }
}

// MARK: - Conversation Row (Shared)

/// Unified conversation row component for use in lists
struct SharedConversationRow: View {
    let conversation: ConversationFullInfo
    let onSelect: (ConversationFullInfo) -> Void

    private var statusColor: Color {
        if conversation.isActive { return .green }
        switch conversation.status?.lowercased() ?? "" {
        case "active", "in progress": return .green
        case "waiting", "blocked": return .orange
        case "completed", "done": return .gray
        default: return .blue
        }
    }

    var body: some View {
        HStack(spacing: 12) {
            // Status indicator with activity pulse
            ZStack {
                Circle()
                    .fill(statusColor)
                    .frame(width: 10, height: 10)

                if conversation.isActive {
                    Circle()
                        .stroke(statusColor.opacity(0.5), lineWidth: 2)
                        .frame(width: 16, height: 16)
                }
            }

            VStack(alignment: .leading, spacing: 6) {
                // Row 1: Title and effective last active time
                HStack(alignment: .top) {
                    Text(conversation.title)
                        .font(.headline)
                        .lineLimit(2)

                    Spacer()

                    Text(ConversationFormatters.formatRelativeTime(conversation.effectiveLastActivity))
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                // Row 2: Summary or current activity
                HStack(alignment: .top) {
                    if let activity = conversation.currentActivity, conversation.isActive {
                        HStack(spacing: 4) {
                            Image(systemName: "bolt.fill")
                                .font(.caption2)
                                .foregroundStyle(.orange)
                            Text(activity)
                                .font(.subheadline)
                                .foregroundStyle(.orange)
                                .lineLimit(1)
                        }
                    } else if let summary = conversation.summary {
                        Text(summary)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                    } else {
                        Text("No summary")
                            .font(.subheadline)
                            .foregroundStyle(.tertiary)
                            .italic()
                    }

                    Spacer()

                    // Show message count
                    if conversation.messageCount > 0 {
                        HStack(spacing: 2) {
                            Image(systemName: "bubble.left")
                                .font(.caption2)
                            Text("\(conversation.messageCount)")
                        }
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                    }
                }

                // Row 3: Author avatar and status badge
                HStack(spacing: -8) {
                    SharedAgentAvatar(agentName: conversation.author)

                    Spacer()

                    // Status badge
                    if let status = conversation.status {
                        StatusBadge(status: status, isActive: conversation.isActive)
                    }
                }
            }

            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 10)
        .contentShape(Rectangle())
        .onTapGesture {
            onSelect(conversation)
        }
    }
}

// MARK: - Runtime Display View

/// Displays runtime with TimelineView for efficient live updates
struct RuntimeDisplayView: View {
    let isActive: Bool
    let computeRuntime: (Date) -> String

    var body: some View {
        if isActive {
            // Use TimelineView for active conversations to update every second
            TimelineView(.periodic(from: .now, by: 1.0)) { context in
                runtimeContent(currentTime: context.date)
            }
        } else {
            // Static display for inactive conversations
            runtimeContent(currentTime: Date())
        }
    }

    @ViewBuilder
    private func runtimeContent(currentTime: Date) -> some View {
        VStack(spacing: 8) {
            Text("⏱️")
                .font(.system(size: 32))

            Text(computeRuntime(currentTime))
                .font(.system(size: 32, weight: .bold, design: .rounded))
                .monospacedDigit()

            Text("Total active time")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 8)
    }
}
