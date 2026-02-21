import SwiftUI

// MARK: - View Extension

extension View {
    /// Adds a NowPlayingBar as a bottom safe area inset inside tab content,
    /// positioning it above the tab bar (not on top of it).
    func nowPlayingInset(coreManager: TenexCoreManager) -> some View {
        self.safeAreaInset(edge: .bottom) {
            NowPlayingBar()
                .environment(coreManager)
                .animation(.spring(duration: 0.3), value: AudioNotificationPlayer.shared.playbackState)
        }
    }
}

/// Apple Music-style Now Playing bar that sits above the tab bar.
/// Shows conversation title, agent avatar + name, text snippet, progress bar, and controls.
struct NowPlayingBar: View {
    @Environment(TenexCoreManager.self) var coreManager
    @ObservedObject var player = AudioNotificationPlayer.shared

    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @State private var showQueueSheet = false
    @State private var showConversationDetail = false

    /// Look up the conversation from coreManager data
    private var conversation: ConversationFullInfo? {
        guard let id = player.currentConversationId else { return nil }
        return coreManager.conversationById[id]
    }

    /// Real conversation title from live data
    private var conversationTitle: String {
        conversation?.thread.title ?? player.currentConversationTitle ?? "Audio Notification"
    }

    /// Project title for the subtitle
    private var projectTitle: String? {
        guard let aTag = conversation?.projectATag else { return nil }
        let projectId = TenexCoreManager.projectId(fromATag: aTag)
        return coreManager.projects.first { $0.id == projectId }?.title
    }

    /// Resolve agent display name from pubkey
    private var agentName: String? {
        guard let pubkey = player.currentAgentPubkey else { return nil }
        return AgentNameFormatter.format(coreManager.displayName(for: pubkey))
    }

    var body: some View {
        if player.playbackState != .idle {
            VStack(spacing: 0) {
                HStack(spacing: 12) {
                    // Tappable area: avatar + text opens conversation detail
                    Button {
                        showConversationDetail = true
                    } label: {
                        HStack(spacing: 12) {
                            AgentAvatarView(
                                agentName: agentName ?? "Agent",
                                pubkey: player.currentAgentPubkey,
                                size: 36,
                                fontSize: 13,
                                showBorder: false
                            )
                            .environment(coreManager)

                            VStack(alignment: .leading, spacing: 2) {
                                Text(conversationTitle)
                                    .font(.subheadline)
                                    .fontWeight(.semibold)
                                    .lineLimit(1)

                                if let projectTitle {
                                    Text(projectTitle)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                }
                            }
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.borderless)

                    Spacer()

                    // Queue button
                    Button {
                        showQueueSheet = true
                    } label: {
                        Image(systemName: "list.bullet")
                            .font(.title3)
                            .frame(width: 44, height: 44)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.borderless)

                    // Play/Pause button
                    Button {
                        player.togglePlayPause()
                    } label: {
                        Image(systemName: player.isPlaying ? "pause.fill" : "play.fill")
                            .font(.title3)
                            .frame(width: 44, height: 44)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.borderless)

                    // Skip to next (only when queue has items)
                    if player.hasQueue {
                        Button {
                            player.skipToNext()
                        } label: {
                            Image(systemName: "forward.fill")
                                .font(.title3)
                                .frame(width: 44, height: 44)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.borderless)
                    }

                    // Stop button (stops all + clears queue)
                    Button {
                        player.stop()
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(.title3)
                            .foregroundStyle(.secondary)
                            .frame(width: 44, height: 44)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.borderless)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 6)

                // Progress bar
                GeometryReader { geometry in
                    Rectangle()
                        .fill(Color.accentColor)
                        .frame(width: geometry.size.width * player.playbackProgress, height: 3)
                }
                .frame(height: 3)
                .background(Color.primary.opacity(0.1))
            }
            .contentShape(Rectangle())
            .clipShape(RoundedRectangle(cornerRadius: 16))
            .modifier(AvailableGlassEffect(reduceTransparency: reduceTransparency))
            .shadow(color: .black.opacity(0.1), radius: 8, x: 0, y: -2)
            .padding(.horizontal, 8)
            .padding(.bottom, 4)
            .transition(.move(edge: .bottom).combined(with: .opacity))
            .sheet(isPresented: $showQueueSheet) {
                AudioQueueSheet()
                    .environment(coreManager)
            }
            .sheet(isPresented: $showConversationDetail) {
                if let conversation {
                    NavigationStack {
                        ConversationDetailView(conversation: conversation)
                            .environmentObject(coreManager)
                            .toolbar {
                                ToolbarItem(placement: .topBarTrailing) {
                                    Button("Done") {
                                        showConversationDetail = false
                                    }
                                }
                            }
                    }
                    .tenexModalPresentation(detents: [.large])
                }
            }
        }
    }
}

// MARK: - Audio Queue Sheet

struct AudioQueueSheet: View {
    @Environment(TenexCoreManager.self) var coreManager
    @ObservedObject var player = AudioNotificationPlayer.shared
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                // Currently playing
                if player.playbackState != .idle {
                    Section("Now Playing") {
                        HStack(spacing: 12) {
                            Image(systemName: "speaker.wave.2.fill")
                                .foregroundStyle(Color.agentBrand)
                                .frame(width: 24)

                            VStack(alignment: .leading, spacing: 2) {
                                Text(resolveConversationTitle(conversationId: player.currentConversationId, fallback: player.currentConversationTitle))
                                    .font(.subheadline)
                                    .fontWeight(.semibold)
                                    .lineLimit(1)

                                if let project = resolveProjectTitle(conversationId: player.currentConversationId) {
                                    Text(project)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                }
                            }
                        }
                    }
                }

                // Queue
                if !player.queue.isEmpty {
                    Section("Up Next (\(player.queue.count))") {
                        ForEach(player.queue) { item in
                            HStack(spacing: 12) {
                                AgentAvatarView(
                                    agentName: resolveAgentName(pubkey: item.notification.agentPubkey) ?? "Agent",
                                    pubkey: item.notification.agentPubkey,
                                    size: 28,
                                    fontSize: 11,
                                    showBorder: false
                                )
                                .environment(coreManager)

                                VStack(alignment: .leading, spacing: 2) {
                                    Text(resolveConversationTitle(conversationId: item.conversationId, fallback: item.notification.conversationTitle))
                                        .font(.subheadline)
                                        .lineLimit(1)

                                    if let project = resolveProjectTitle(conversationId: item.conversationId) {
                                        Text(project)
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                            .lineLimit(1)
                                    }
                                }

                                Spacer()

                                Button {
                                    player.removeFromQueue(id: item.id)
                                } label: {
                                    Image(systemName: "xmark.circle.fill")
                                        .foregroundStyle(.secondary)
                                }
                                .buttonStyle(.borderless)
                            }
                        }
                    }
                } else {
                    Section {
                        Text("Queue is empty")
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .navigationTitle("Audio Queue")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .automatic) {
                    if !player.queue.isEmpty {
                        Button("Clear All") {
                            player.clearQueue()
                        }
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") {
                        dismiss()
                    }
                }
            }
        }
        .tenexModalPresentation(detents: [.medium, .large])
    }

    private func resolveAgentName(pubkey: String?) -> String? {
        guard let pubkey else { return nil }
        return AgentNameFormatter.format(coreManager.displayName(for: pubkey))
    }

    private func resolveConversationTitle(conversationId: String?, fallback: String?) -> String {
        if let id = conversationId,
           let conv = coreManager.conversationById[id] {
            return conv.thread.title
        }
        return fallback ?? "Audio Notification"
    }

    private func resolveProjectTitle(conversationId: String?) -> String? {
        guard let id = conversationId,
              let conv = coreManager.conversationById[id] else { return nil }
        let projectId = TenexCoreManager.projectId(fromATag: conv.projectATag)
        return coreManager.projects.first { $0.id == projectId }?.title
    }
}
