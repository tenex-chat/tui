import SwiftUI

extension FfiBunkerSignRequest: @retroactive Identifiable {
    public var id: String { requestId }
}

struct BunkerApprovalSheet: View {
    @Environment(TenexCoreManager.self) private var coreManager
    let request: FfiBunkerSignRequest
    let onDismiss: () -> Void

    @State private var remainingSeconds = 60
    @State private var timer: Timer?

    var body: some View {
        VStack(spacing: 20) {
            headerSection
            Divider()
            requestDetailsSection
            Spacer()
            actionButtons
        }
        .padding(24)
        .frame(minWidth: 400, idealWidth: 480, minHeight: 320)
        .onAppear { startCountdown() }
        .onDisappear { timer?.invalidate() }
    }

    private var headerSection: some View {
        VStack(spacing: 8) {
            Image(systemName: "signature")
                .font(.system(size: 36))
                .foregroundStyle(.orange)
            Text("Signing Request")
                .font(.title2.bold())
            Text("An agent is requesting you to sign an event")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
    }

    private var requestDetailsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            detailRow("From", value: truncatedPubkey)
            if let kind = request.eventKind {
                detailRow("Event Kind", value: "\(kindName(kind)) (\(kind))")
            }
            if let content = request.eventContent, !content.isEmpty {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Content")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    ScrollView {
                        Text(content)
                            .font(.callout)
                            .fontDesign(.monospaced)
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .frame(maxHeight: 120)
                    .padding(8)
                    .background(.quaternary)
                    .clipShape(RoundedRectangle(cornerRadius: 6))
                }
            }
        }
    }

    private var actionButtons: some View {
        VStack(spacing: 12) {
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

    private var truncatedPubkey: String {
        let pk = request.requesterPubkey
        if pk.count > 16 {
            return "\(pk.prefix(8))...\(pk.suffix(8))"
        }
        return pk
    }

    private func detailRow(_ label: String, value: String) -> some View {
        HStack(alignment: .top) {
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(width: 80, alignment: .trailing)
            Text(value)
                .font(.callout)
                .fontDesign(.monospaced)
                .textSelection(.enabled)
        }
    }

    private func kindName(_ kind: UInt16) -> String {
        switch kind {
        case 0: return "Metadata"
        case 1: return "Text Note"
        case 4199: return "Agent Definition"
        case 4200: return "MCP Tool"
        case 4201: return "Nudge"
        case 31933: return "Project"
        case 30023: return "Article"
        default: return "Kind \(kind)"
        }
    }

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
        Task {
            try? await coreManager.safeCore.respondToBunkerRequest(
                requestId: request.requestId,
                approved: approved
            )
        }
        onDismiss()
    }
}
