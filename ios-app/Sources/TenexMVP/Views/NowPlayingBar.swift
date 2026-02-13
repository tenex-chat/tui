import SwiftUI

// MARK: - View Extension

extension View {
    /// Adds a NowPlayingBar as a bottom safe area inset inside tab content,
    /// positioning it above the tab bar (not on top of it).
    func nowPlayingInset(coreManager: TenexCoreManager) -> some View {
        self.safeAreaInset(edge: .bottom) {
            NowPlayingBar()
                .environmentObject(coreManager)
                .animation(.spring(duration: 0.3), value: AudioNotificationPlayer.shared.playbackState)
        }
    }
}

/// Apple Music-style Now Playing bar that sits above the tab bar.
/// Shows conversation title, agent avatar + name, text snippet, progress bar, and controls.
struct NowPlayingBar: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @ObservedObject var player = AudioNotificationPlayer.shared

    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    /// Resolve agent display name from pubkey
    private var agentName: String? {
        guard let pubkey = player.currentAgentPubkey else { return nil }
        let name = coreManager.safeCore.getProfileName(pubkey: pubkey)
        return name.isEmpty ? nil : AgentNameFormatter.format(name)
    }

    /// Build subtitle: "Agent Name: text snippet..." or just text snippet
    private var subtitle: String? {
        let parts: [String] = [
            agentName,
            player.currentTextSnippet.map { "\"\($0)\"" }
        ].compactMap { $0 }

        guard !parts.isEmpty else { return nil }
        return parts.joined(separator: ": ")
    }

    var body: some View {
        if player.playbackState != .idle {
            VStack(spacing: 0) {
                HStack(spacing: 12) {
                    // Agent avatar
                    AgentAvatarView(
                        agentName: agentName ?? "Agent",
                        pubkey: player.currentAgentPubkey,
                        size: 36,
                        fontSize: 13,
                        showBorder: false
                    )
                    .environmentObject(coreManager)

                    // Title + subtitle
                    VStack(alignment: .leading, spacing: 2) {
                        Text(player.currentConversationTitle ?? "Audio Notification")
                            .font(.subheadline)
                            .fontWeight(.semibold)
                            .lineLimit(1)

                        if let subtitle {
                            Text(subtitle)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                    }

                    Spacer()

                    // Play/Pause button
                    Button {
                        player.togglePlayPause()
                    } label: {
                        Image(systemName: player.isPlaying ? "pause.fill" : "play.fill")
                            .font(.title3)
                            .frame(width: 32, height: 32)
                    }
                    .buttonStyle(.plain)

                    // Stop button
                    Button {
                        player.stop()
                    } label: {
                        Image(systemName: "xmark.circle.fill")
                            .font(.title3)
                            .foregroundStyle(.secondary)
                            .frame(width: 32, height: 32)
                    }
                    .buttonStyle(.plain)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 10)

                // Progress bar
                GeometryReader { geometry in
                    Rectangle()
                        .fill(Color.accentColor)
                        .frame(width: geometry.size.width * player.playbackProgress, height: 3)
                }
                .frame(height: 3)
                .background(Color.primary.opacity(0.1))
            }
            .clipShape(RoundedRectangle(cornerRadius: 16))
            .modifier(AvailableGlassEffect(reduceTransparency: reduceTransparency))
            .shadow(color: .black.opacity(0.1), radius: 8, x: 0, y: -2)
            .padding(.horizontal, 8)
            .padding(.bottom, 4)
            .transition(.move(edge: .bottom).combined(with: .opacity))
        }
    }
}
