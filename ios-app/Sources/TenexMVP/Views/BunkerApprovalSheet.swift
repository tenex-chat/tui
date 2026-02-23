import SwiftUI

extension FfiBunkerSignRequest: Identifiable {
    public var id: String { requestId }
}

extension FfiBunkerAutoApproveRule {
    var ruleId: String {
        "\(requesterPubkey):\(eventKind.map { String($0) } ?? "any")"
    }
}

struct BunkerApprovalSheet: View {
    @Environment(TenexCoreManager.self) private var coreManager
    let request: FfiBunkerSignRequest
    let onDismiss: () -> Void

    @State private var remainingSeconds = 60
    @State private var timer: Timer?
    @State private var alwaysApprove = false

    var body: some View {
        VStack(spacing: 16) {
            requesterIdentitySection
            Divider()
            eventDetailsSection
            Spacer(minLength: 0)
            autoApproveToggle
            actionButtons
        }
        .padding(24)
        .frame(minWidth: 420, idealWidth: 500, minHeight: 400)
        .onAppear { startCountdown() }
        .onDisappear { timer?.invalidate() }
    }

    // MARK: - Requester Identity

    private var requesterIdentitySection: some View {
        HStack(spacing: 12) {
            AgentAvatarView(
                agentName: displayName,
                pubkey: request.requesterPubkey,
                size: 44,
                showBorder: false
            )

            VStack(alignment: .leading, spacing: 2) {
                Text(displayName)
                    .font(.headline)
                Text(truncatedPubkey)
                    .font(.caption)
                    .fontDesign(.monospaced)
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
            }

            Spacer()

            Image(systemName: "signature")
                .font(.system(size: 24))
                .foregroundStyle(.orange)
        }
    }

    private var displayName: String {
        coreManager.displayName(for: request.requesterPubkey)
    }

    private var truncatedPubkey: String {
        let pk = request.requesterPubkey
        if pk.count > 16 {
            return "\(pk.prefix(8))...\(pk.suffix(8))"
        }
        return pk
    }

    // MARK: - Event Details

    private var eventDetailsSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let kind = request.eventKind {
                HStack {
                    Label(kindName(kind), systemImage: kindIcon(kind))
                        .font(.subheadline.bold())
                    Text("(\(kind))")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                    Spacer()
                }
            }

            ScrollView {
                Text(rawEventJson)
                    .font(.caption)
                    .fontDesign(.monospaced)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(maxHeight: 260)
            .padding(8)
            .background(.quaternary)
            .clipShape(RoundedRectangle(cornerRadius: 6))
        }
    }

    private var rawEventJson: String {
        var obj: [String: Any] = [:]
        if let kind = request.eventKind { obj["kind"] = kind }
        if let content = request.eventContent { obj["content"] = content }
        if let tagsJson = request.eventTagsJson,
           let data = tagsJson.data(using: .utf8),
           let tags = try? JSONSerialization.jsonObject(with: data) {
            obj["tags"] = tags
        }
        guard let data = try? JSONSerialization.data(withJSONObject: obj, options: [.prettyPrinted, .sortedKeys]),
              let str = String(data: data, encoding: .utf8)
        else { return "(unable to serialize event)" }
        return str
    }

    // MARK: - Auto-Approve Toggle

    private var autoApproveToggle: some View {
        Group {
            if let kind = request.eventKind {
                Toggle(isOn: $alwaysApprove) {
                    Text("Always approve \(kindName(kind)) from \(displayName)")
                        .font(.caption)
                }
                #if os(macOS)
                .toggleStyle(.checkbox)
                #endif
            }
        }
    }

    // MARK: - Actions

    private var actionButtons: some View {
        VStack(spacing: 10) {
            Text("Auto-reject in \(remainingSeconds)s")
                .font(.caption)
                .foregroundStyle(.secondary)

            HStack(spacing: 16) {
                Button(role: .destructive) {
                    respond(approved: false)
                } label: {
                    Text("Reject")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.bordered)
                .controlSize(.large)

                Button {
                    respond(approved: true)
                } label: {
                    Text("Approve")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
            }
        }
    }

    // MARK: - Kind Helpers

    private func kindName(_ kind: UInt16) -> String {
        switch kind {
        case 0: return "Metadata"
        case 1: return "Text Note"
        case 4: return "DM"
        case 7: return "Reaction"
        case 1111: return "Comment"
        case 4199: return "Agent Definition"
        case 4200: return "MCP Tool"
        case 4201: return "Nudge"
        case 14199: return "Sealed Agent Def"
        case 24010: return "Status"
        case 24020: return "Agent Config"
        case 24133: return "Active Conversations"
        case 24134: return "Stop Operations"
        case 30023: return "Article"
        case 31933: return "Project"
        case 34199: return "Team"
        default: return "Kind \(kind)"
        }
    }

    private func kindIcon(_ kind: UInt16) -> String {
        switch kind {
        case 0: return "person.crop.circle"
        case 1: return "text.bubble"
        case 4: return "envelope"
        case 7: return "heart"
        case 1111: return "text.bubble"
        case 4199: return "cpu"
        case 4200: return "wrench"
        case 4201: return "bell"
        case 30023: return "doc.text"
        case 31933: return "folder"
        default: return "doc"
        }
    }

    // MARK: - Countdown & Response

    private func startCountdown() {
        timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { _ in
            if remainingSeconds > 0 {
                remainingSeconds -= 1
            } else {
                timer?.invalidate()
                respond(approved: false)
            }
        }
    }

    private func respond(approved: Bool) {
        timer?.invalidate()

        if approved && alwaysApprove, let kind = request.eventKind {
            persistAutoApproveRule(requesterPubkey: request.requesterPubkey, eventKind: kind)
            Task {
                try? await coreManager.safeCore.addBunkerAutoApproveRule(
                    requesterPubkey: request.requesterPubkey,
                    eventKind: kind
                )
            }
        }

        Task {
            try? await coreManager.safeCore.respondToBunkerRequest(
                requestId: request.requestId,
                approved: approved
            )
        }
        onDismiss()
    }

    // MARK: - Persistence

    private func persistAutoApproveRule(requesterPubkey: String, eventKind: UInt16) {
        var rules = BunkerAutoApproveStorage.loadRules()
        let newRule = BunkerAutoApproveStorage.Rule(requesterPubkey: requesterPubkey, eventKind: eventKind)
        if !rules.contains(where: { $0.requesterPubkey == requesterPubkey && $0.eventKind == eventKind }) {
            rules.append(newRule)
            BunkerAutoApproveStorage.saveRules(rules)
        }
    }
}

// MARK: - Persistence Helper

enum BunkerAutoApproveStorage {
    struct Rule: Codable, Equatable {
        let requesterPubkey: String
        let eventKind: UInt16
    }

    private static let key = "bunker.autoApproveRules"

    static func loadRules() -> [Rule] {
        guard let data = UserDefaults.standard.data(forKey: key),
              let rules = try? JSONDecoder().decode([Rule].self, from: data)
        else { return [] }
        return rules
    }

    static func saveRules(_ rules: [Rule]) {
        if let data = try? JSONEncoder().encode(rules) {
            UserDefaults.standard.set(data, forKey: key)
        }
    }

    static func removeRule(requesterPubkey: String, eventKind: UInt16) {
        var rules = loadRules()
        rules.removeAll { $0.requesterPubkey == requesterPubkey && $0.eventKind == eventKind }
        saveRules(rules)
    }
}
