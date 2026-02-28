import SwiftUI

struct ShellConversationListStyle: ViewModifier {
    let isShellColumn: Bool

    @ViewBuilder
    func body(content: Content) -> some View {
        if isShellColumn {
            #if os(macOS)
            content.listStyle(.inset)
            #else
            content.listStyle(.plain)
            #endif
        } else {
            content.listStyle(.sidebar)
        }
    }
}

/// Conversation row that uses ConversationFullInfo's rich data.
/// PERFORMANCE: Uses cached hierarchy data instead of per-row FFI calls.
/// The cache is preloaded in ConversationsTabView for visible conversations.
struct ConversationRowFull: View, Equatable {
    static func == (lhs: ConversationRowFull, rhs: ConversationRowFull) -> Bool {
        lhs.conversation == rhs.conversation &&
            lhs.projectTitle == rhs.projectTitle &&
            lhs.isHierarchicallyActive == rhs.isHierarchicallyActive &&
            lhs.pTaggedRecipientInfo == rhs.pTaggedRecipientInfo &&
            lhs.delegationAgentInfos == rhs.delegationAgentInfos &&
            lhs.isPlayingAudio == rhs.isPlayingAudio &&
            lhs.isAudioPlaying == rhs.isAudioPlaying &&
            lhs.showsChevron == rhs.showsChevron
    }

    let conversation: ConversationFullInfo
    let projectTitle: String?
    /// Whether this conversation or any of its descendants has active work
    let isHierarchicallyActive: Bool
    let pTaggedRecipientInfo: AgentAvatarInfo?
    let delegationAgentInfos: [AgentAvatarInfo]
    let isPlayingAudio: Bool
    let isAudioPlaying: Bool
    let showsChevron: Bool
    let onSelect: ((ConversationFullInfo) -> Void)?
    let onToggleArchive: ((ConversationFullInfo) -> Void)?

    #if os(macOS)
    @Environment(\.openWindow) private var openWindow
    @State private var isHovered = false
    #else
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    @Environment(TenexCoreManager.self) private var coreManager
    @State private var showDelegationTree = false
    #endif

    private var statusColor: Color {
        Color.conversationStatus(for: conversation.thread.statusLabel, isActive: isHierarchicallyActive)
    }

    private func shouldShowStatusBadge(_ status: String) -> Bool {
        let normalizedStatus = status.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if conversation.isActive && (normalizedStatus == "waiting" || normalizedStatus == "blocked") {
            return false
        }
        return true
    }

    private var rowContent: some View {
        HStack(spacing: 12) {
            // Status indicator with activity pulse (shows pulse if hierarchically active)
            ZStack {
                Circle()
                    .fill(statusColor)
                    .frame(width: 10, height: 10)

                if isHierarchicallyActive {
                    Circle()
                        .stroke(statusColor.opacity(0.5), lineWidth: 2)
                        .frame(width: 16, height: 16)
                }
            }

            VStack(alignment: .leading, spacing: 6) {
                // Row 1: Title and effective last active time
                HStack(alignment: .top) {
                    Text(conversation.thread.title)
                        .font(.headline)
                        .lineLimit(2)

                    if isPlayingAudio {
                        Image(systemName: "speaker.wave.2.fill")
                            .font(.caption)
                            .foregroundStyle(Color.agentBrand)
                            .symbolEffect(.variableColor.iterative, isActive: isAudioPlaying)
                    }

                    Spacer()

                    #if os(macOS)
                    if isHovered, let onToggleArchive {
                        Button {
                            onToggleArchive(conversation)
                        } label: {
                            Image(systemName: conversation.isArchived ? "tray.and.arrow.up" : "archivebox")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                        .buttonStyle(.borderless)
                        .help(conversation.isArchived ? "Unarchive conversation" : "Archive conversation")
                    } else {
                        RelativeTimeText(timestamp: conversation.thread.effectiveLastActivity, style: .localizedAbbreviated)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    #else
                    RelativeTimeText(timestamp: conversation.thread.effectiveLastActivity, style: .localizedAbbreviated)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    #endif
                }

                // Row 2: Summary or current activity
                HStack(alignment: .top) {
                    // Show current activity if directly active (not hierarchically via descendants)
                    if let activity = conversation.thread.statusCurrentActivity, conversation.isActive {
                        HStack(spacing: 4) {
                            Image(systemName: "bolt.fill")
                                .font(.caption2)
                                .foregroundStyle(Color.skillBrand)
                            Text(activity)
                                .font(.subheadline)
                                .foregroundStyle(Color.skillBrand)
                                .lineLimit(1)
                        }
                    // Show "Delegation active" if hierarchically active but not directly active
                    } else if isHierarchicallyActive && !conversation.isActive {
                        HStack(spacing: 4) {
                            Image(systemName: "arrow.triangle.branch")
                                .font(.caption2)
                                .foregroundStyle(Color.presenceOnline)
                            Text("Delegation active")
                                .font(.subheadline)
                                .foregroundStyle(Color.presenceOnline)
                                .lineLimit(1)
                        }
                    } else if let summary = conversation.thread.summary {
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

                // Row 3: Avatars (author + p-tagged overlapping, then delegation agents) + badges
                HStack(spacing: 0) {
                    ConversationAvatarGroup(
                        authorInfo: AgentAvatarInfo(name: conversation.author, pubkey: conversation.thread.pubkey),
                        pTaggedRecipientInfo: pTaggedRecipientInfo,
                        otherParticipants: delegationAgentInfos,
                        maxVisibleAvatars: maxVisibleAvatars
                    )

                    Spacer()

                    // Scheduled badge (shows when conversation has scheduled-task-id tag)
                    if conversation.thread.isScheduled {
                        HStack(spacing: 2) {
                            Image(systemName: "clock")
                                .font(.caption2)
                            Text("Scheduled")
                        }
                        .font(.caption2)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Color.projectBrandBackground)
                        .foregroundStyle(Color.projectBrand)
                        .clipShape(Capsule())
                    }

                    if conversation.isActive {
                        HStack(spacing: 3) {
                            ZStack {
                                Circle()
                                    .fill(Color.presenceOnline)
                                    .frame(width: 6, height: 6)
                                Circle()
                                    .stroke(Color.presenceOnline.opacity(0.45), lineWidth: 1.5)
                                    .frame(width: 10, height: 10)
                            }
                            Text("Working")
                        }
                        .font(.caption2.weight(.medium))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Color.presenceOnline.opacity(0.16))
                        .foregroundStyle(Color.presenceOnline)
                        .clipShape(Capsule())
                    }

                    // Status badge
                    if let status = conversation.thread.statusLabel, shouldShowStatusBadge(status) {
                        Text(status)
                            .font(.caption2)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(statusColor.opacity(0.15))
                            .foregroundStyle(statusColor)
                            .clipShape(Capsule())
                    }

                    // Show project title badge if available
                    if let projectTitle = projectTitle {
                        Text(projectTitle)
                            .font(.caption2)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(Color.messageBubbleUserBackground)
                            .foregroundStyle(Color.agentBrand)
                            .clipShape(Capsule())
                    }

                    // Delegation tree button (Mac/iPad only)
                    #if os(macOS)
                    if conversation.hasChildren {
                        Button {
                            openWindow(id: "delegation-tree", value: conversation.thread.id)
                        } label: {
                            Image(systemName: "arrow.triangle.branch")
                                .font(.system(size: 13))
                                .foregroundStyle(.secondary)
                        }
                        .buttonStyle(.borderless)
                        .opacity(isHovered ? 1 : 0)
                        .help("View delegation tree")
                    }
                    #else
                    if conversation.hasChildren && horizontalSizeClass == .regular {
                        Button {
                            showDelegationTree = true
                        } label: {
                            Image(systemName: "arrow.triangle.branch")
                                .font(.system(size: 13))
                                .foregroundStyle(.secondary)
                        }
                        .buttonStyle(.borderless)
                        .help("View delegation tree")
                    }
                    #endif
                }
            }

            if showsChevron {
                Image(systemName: "chevron.right")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
        }
    }

    var body: some View {
        Group {
            if let onSelect {
                rowContent
                    .contentShape(Rectangle())
                    .onTapGesture {
                        onSelect(conversation)
                    }
            } else {
                rowContent
            }
        }
        .padding(.vertical, 10)
        #if os(macOS)
        .onHover { hovering in
            isHovered = hovering
        }
        #else
        .fullScreenCover(isPresented: $showDelegationTree) {
            NavigationStack {
                DelegationTreeView(rootConversationId: conversation.thread.id)
                    .environment(coreManager)
                    .toolbar {
                        ToolbarItem(placement: .topBarTrailing) {
                            Button("Done") { showDelegationTree = false }
                        }
                    }
            }
        }
        #endif
        // PERFORMANCE: Removed per-row .task that called loadDelegationAgentInfos()
        // Hierarchy data is now preloaded in batch by ConversationsTabView
    }
}
