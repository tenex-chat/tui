import SwiftUI

/// Debug view for inspecting saved audio notifications — shows original text,
/// massaged text, and allows playback of generated audio files.
struct AudioNotificationsLogView: View {
    @Environment(TenexCoreManager.self) var coreManager
    @StateObject private var player = AudioNotificationPlayer.shared

    @State private var notifications: [AudioNotificationInfo] = []
    @State private var isLoading = true
    @State private var errorMessage: String?
    @State private var showError = false

    var body: some View {
        Group {
            if isLoading {
                ProgressView("Loading notifications...")
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if notifications.isEmpty {
                ContentUnavailableView {
                    Label("No Audio Notifications", systemImage: "waveform.slash")
                } description: {
                    Text("Audio notifications will appear here after they are generated.")
                }
            } else {
                List {
                    ForEach(notifications, id: \.id) { notification in
                        NavigationLink {
                            AudioNotificationDetailView(
                                notification: notification,
                                player: player,
                                coreManager: coreManager
                            )
                        } label: {
                            AudioNotificationRow(notification: notification, coreManager: coreManager)
                        }
                    }
                    .onDelete(perform: deleteNotifications)
                }
            }
        }
        .navigationTitle("Audio Debug Log")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        .onAppear { loadNotifications() }
        .refreshable { loadNotifications() }
        .alert("Error", isPresented: $showError) {
            Button("OK") { errorMessage = nil }
        } message: {
            if let error = errorMessage {
                Text(error)
            }
        }
    }

    private func loadNotifications() {
        Task {
            do {
                let result = try await coreManager.safeCore.listAudioNotifications()
                notifications = result.sorted { $0.createdAt > $1.createdAt }
                isLoading = false
            } catch {
                isLoading = false
                errorMessage = "Failed to load notifications: \(error.localizedDescription)"
                showError = true
            }
        }
    }

    private func deleteNotifications(at offsets: IndexSet) {
        let toDelete = offsets.map { notifications[$0] }
        notifications.remove(atOffsets: offsets)

        Task {
            for notification in toDelete {
                do {
                    try await coreManager.safeCore.deleteAudioNotification(id: notification.id)
                } catch {
                    errorMessage = "Failed to delete notification: \(error.localizedDescription)"
                    showError = true
                }
            }
        }
    }
}

// MARK: - Row View

private struct AudioNotificationRow: View {
    let notification: AudioNotificationInfo
    let coreManager: TenexCoreManager

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(notification.conversationTitle)
                .font(.body)
                .lineLimit(1)

            Text(notification.originalText)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)

            HStack {
                Text(coreManager.displayName(for: notification.agentPubkey))
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                Spacer()
                Text(relativeTimestamp(notification.createdAt))
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 2)
    }
}

// MARK: - Detail View

private struct AudioNotificationDetailView: View {
    let notification: AudioNotificationInfo
    @ObservedObject var player: AudioNotificationPlayer
    let coreManager: TenexCoreManager

    var body: some View {
        List {
            Section("Playback") {
                Button {
                    try? player.play(path: notification.audioFilePath)
                } label: {
                    let isThisPlaying = player.isPlaying && player.currentTextSnippet == notification.massagedText
                    Label(
                        isThisPlaying ? "Playing..." : "Play Audio",
                        systemImage: isThisPlaying ? "speaker.wave.3.fill" : "play.circle.fill"
                    )
                }

                if player.isPlaying {
                    Button(role: .destructive) {
                        player.stop()
                    } label: {
                        Label("Stop", systemImage: "stop.circle.fill")
                    }
                }
            }

            Section("Original Text") {
                Text(notification.originalText)
                    .font(.body)
                    .textSelection(.enabled)
            }

            Section("Massaged Text") {
                Text(notification.massagedText)
                    .font(.body)
                    .textSelection(.enabled)
            }

            Section("Details") {
                LabeledContent("Conversation", value: notification.conversationTitle)
                LabeledContent("Voice ID", value: notification.voiceId)
                LabeledContent("Agent", value: coreManager.displayName(for: notification.agentPubkey))
                LabeledContent("Agent Pubkey") {
                    Text(notification.agentPubkey)
                        .font(.caption2)
                        .monospaced()
                        .textSelection(.enabled)
                }
                LabeledContent("Created", value: relativeTimestamp(notification.createdAt))
                LabeledContent("Audio File") {
                    Text(notification.audioFilePath)
                        .font(.caption2)
                        .monospaced()
                        .textSelection(.enabled)
                }
            }
        }
        .navigationTitle("Notification Detail")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
    }
}

// MARK: - Helpers

private func relativeTimestamp(_ epoch: UInt64) -> String {
    let date = Date(timeIntervalSince1970: TimeInterval(epoch))
    let formatter = RelativeDateTimeFormatter()
    formatter.unitsStyle = .short
    return formatter.localizedString(for: date, relativeTo: Date())
}
