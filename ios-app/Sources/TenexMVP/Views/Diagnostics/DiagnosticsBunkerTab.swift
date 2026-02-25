import SwiftUI
import UniformTypeIdentifiers

#if os(macOS)
import AppKit
#elseif os(iOS)
import UIKit
#endif

/// Bunker tab showing full NIP-46 remote signing audit trail.
/// Includes raw request/response payloads and export support.
struct DiagnosticsBunkerTab: View {
    let auditEntries: [FfiBunkerAuditEntry]
    @Environment(TenexCoreManager.self) private var coreManager

    @State private var exportDocument: BunkerAuditExportDocument?
    @State private var isExporting = false
    @State private var copyFeedbackMessage: String?

    private var sortedEntries: [FfiBunkerAuditEntry] {
        auditEntries.sorted { lhs, rhs in
            lhs.timestampMs > rhs.timestampMs
        }
    }

    private var exportFilename: String {
        "tenex-bunker-audit-\(BunkerAuditFormatters.exportFilenameDate.string(from: Date())).json"
    }

    var body: some View {
        VStack(spacing: 16) {
            if auditEntries.isEmpty {
                emptyState
            } else {
                headerControls

                LazyVStack(spacing: 12) {
                    ForEach(sortedEntries, id: \.rowIdentity) { entry in
                        BunkerAuditEntryRow(
                            entry: entry,
                            coreManager: coreManager,
                            onCopyText: copyText(_:feedback:)
                        )
                    }
                }
            }
        }
        .fileExporter(
            isPresented: $isExporting,
            document: exportDocument,
            contentType: .json,
            defaultFilename: exportFilename
        ) { result in
            switch result {
            case .success(let url):
                copyFeedbackMessage = "Exported bunker audit log to \(url.path)."
            case .failure(let error):
                copyFeedbackMessage = "Export failed: \(error.localizedDescription)"
            }
        }
        .alert(
            "Bunker Audit",
            isPresented: Binding(
                get: { copyFeedbackMessage != nil },
                set: { shown in
                    if !shown {
                        copyFeedbackMessage = nil
                    }
                }
            )
        ) {
            Button("OK", role: .cancel) { }
        } message: {
            Text(copyFeedbackMessage ?? "")
        }
    }

    private var headerControls: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("Requests: \(auditEntries.count)")
                    .font(.subheadline)
                    .fontWeight(.semibold)

                Spacer()

                Button {
                    copyText(buildAuditExportJSON(), feedback: "Copied full bunker audit log JSON.")
                } label: {
                    Label("Copy JSON", systemImage: "doc.on.doc")
                }
                .adaptiveGlassButtonStyle()

                Button {
                    exportDocument = BunkerAuditExportDocument(text: buildAuditExportJSON())
                    isExporting = true
                } label: {
                    Label("Export JSON", systemImage: "square.and.arrow.down")
                }
                .adaptiveProminentGlassButtonStyle()
            }

            Text("Each entry includes request payload, response payload, timing, and decision metadata.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private var emptyState: some View {
        VStack(spacing: 16) {
            Image(systemName: "lock.shield")
                .font(.system(.largeTitle))
                .foregroundColor(.secondary)

            Text("No Bunker Activity")
                .font(.headline)
                .foregroundColor(.primary)

            Text("NIP-46 signing requests will appear here when the bunker is active and clients connect.")
                .font(.subheadline)
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 40)
    }

    private func buildAuditExportJSON() -> String {
        let entries = auditEntries
            .sorted { $0.timestampMs < $1.timestampMs }
            .map { entry in
                BunkerAuditExportEntry(
                    requestId: entry.requestId,
                    sourceEventId: entry.sourceEventId,
                    requesterPubkey: entry.requesterPubkey,
                    requesterDisplayName: coreManager.displayName(for: entry.requesterPubkey),
                    requestType: entry.requestType,
                    decision: entry.decision,
                    eventKind: entry.eventKind,
                    timestampMs: entry.timestampMs,
                    timestampIso8601: BunkerAuditFormatters.iso8601String(ms: entry.timestampMs),
                    completedAtMs: entry.completedAtMs,
                    completedAtIso8601: BunkerAuditFormatters.iso8601String(ms: entry.completedAtMs),
                    responseTimeMs: entry.responseTimeMs,
                    eventContentPreview: entry.eventContentPreview,
                    eventContentFull: entry.eventContentFull,
                    eventTagsJson: entry.eventTagsJson,
                    requestPayloadJson: entry.requestPayloadJson,
                    responsePayloadJson: entry.responsePayloadJson
                )
            }

        let envelope = BunkerAuditExportEnvelope(
            generatedAtMs: UInt64(Date().timeIntervalSince1970 * 1000),
            generatedAtIso8601: BunkerAuditFormatters.isoFormatter.string(from: Date()),
            entryCount: entries.count,
            entries: entries
        )

        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]

        guard let data = try? encoder.encode(envelope),
              let json = String(data: data, encoding: .utf8)
        else {
            return "{\"error\":\"Failed to encode bunker audit log\"}"
        }

        return json
    }

    private func copyText(_ text: String, feedback: String) {
        #if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(text, forType: .string)
        #elseif os(iOS)
        UIPasteboard.general.string = text
        #endif
        copyFeedbackMessage = feedback
    }
}

// MARK: - Audit Entry Row

private struct BunkerAuditEntryRow: View {
    let entry: FfiBunkerAuditEntry
    let coreManager: TenexCoreManager
    let onCopyText: (String, String) -> Void

    @State private var isExpanded = false

    private var requesterName: String {
        let resolved = coreManager.displayName(for: entry.requesterPubkey)
        if resolved == entry.requesterPubkey {
            return "\(entry.requesterPubkey.prefix(8))...\(entry.requesterPubkey.suffix(8))"
        }
        return resolved
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Button {
                withAnimation(.easeInOut(duration: 0.2)) {
                    isExpanded.toggle()
                }
            } label: {
                VStack(alignment: .leading, spacing: 10) {
                    HStack(alignment: .top, spacing: 12) {
                        VStack(alignment: .leading, spacing: 6) {
                            Text(BunkerAuditFormatters.localTime(ms: entry.timestampMs))
                                .font(.caption.monospacedDigit())
                                .foregroundStyle(.secondary)

                            HStack(spacing: 8) {
                                Image(systemName: "person.circle")
                                    .foregroundStyle(.secondary)
                                Text(requesterName)
                                    .font(.subheadline)
                                    .fontWeight(.semibold)
                                    .foregroundStyle(.primary)
                            }

                            if let kind = entry.eventKind {
                                HStack(spacing: 6) {
                                    Image(systemName: "doc.text")
                                        .font(.caption2)
                                        .foregroundStyle(.secondary)
                                    Text("Kind \(kind)")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }

                        Spacer()

                        VStack(alignment: .trailing, spacing: 8) {
                            HStack(spacing: 6) {
                                requestTypeBadge
                                decisionBadge
                            }

                            Text("\(entry.responseTimeMs) ms")
                                .font(.caption2.monospacedDigit())
                                .foregroundStyle(.secondary)
                        }
                    }

                    HStack(spacing: 6) {
                        Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                        Text(isExpanded ? "Hide details" : "Show payloads and metadata")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .buttonStyle(.plain)

            if isExpanded {
                VStack(alignment: .leading, spacing: 12) {
                    auditMetadata

                    if let payload = entry.requestPayloadJson, !payload.isEmpty {
                        payloadSection(
                            title: "Request Payload (JSON)",
                            text: payload,
                            copyFeedback: "Copied request payload JSON."
                        )
                    }

                    if let payload = entry.responsePayloadJson, !payload.isEmpty {
                        payloadSection(
                            title: "Response Payload (JSON)",
                            text: payload,
                            copyFeedback: "Copied response payload JSON."
                        )
                    }

                    if let tags = entry.eventTagsJson, !tags.isEmpty {
                        payloadSection(
                            title: "Event Tags (JSON)",
                            text: tags,
                            copyFeedback: "Copied event tags JSON."
                        )
                    }

                    if let content = entry.eventContentFull, !content.isEmpty {
                        payloadSection(
                            title: "Event Content",
                            text: content,
                            copyFeedback: "Copied full event content."
                        )
                    } else if let preview = entry.eventContentPreview, !preview.isEmpty {
                        payloadSection(
                            title: "Event Content Preview",
                            text: preview,
                            copyFeedback: "Copied event content preview."
                        )
                    }
                }
                .transition(.opacity.combined(with: .move(edge: .top)))
            }
        }
        .padding(12)
        .background(Color.systemGray6.opacity(0.65))
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }

    private var auditMetadata: some View {
        VStack(alignment: .leading, spacing: 8) {
            metadataRow(label: "Request ID", value: entry.requestId, copyFeedback: "Copied request ID.")
            metadataRow(label: "Source Event ID", value: entry.sourceEventId, copyFeedback: "Copied source event ID.")
            metadataRow(
                label: "Received",
                value: "\(BunkerAuditFormatters.localDateTime(ms: entry.timestampMs)) (\(BunkerAuditFormatters.iso8601String(ms: entry.timestampMs)))"
            )
            metadataRow(
                label: "Completed",
                value: "\(BunkerAuditFormatters.localDateTime(ms: entry.completedAtMs)) (\(BunkerAuditFormatters.iso8601String(ms: entry.completedAtMs)))"
            )
            metadataRow(label: "Decision", value: entry.decision)
            metadataRow(label: "Request Type", value: entry.requestType)
        }
    }

    private func metadataRow(label: String, value: String, copyFeedback: String? = nil) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Text(label)
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(width: 110, alignment: .leading)

            Text(value)
                .font(.caption.monospaced())
                .foregroundStyle(.primary)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)

            if let copyFeedback {
                Button {
                    onCopyText(value, copyFeedback)
                } label: {
                    Image(systemName: "doc.on.doc")
                        .font(.caption2)
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
            }
        }
    }

    private func payloadSection(title: String, text: String, copyFeedback: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Text(title)
                    .font(.caption)
                    .fontWeight(.semibold)
                    .foregroundStyle(.secondary)

                Spacer()

                Button {
                    onCopyText(text, copyFeedback)
                } label: {
                    Label("Copy", systemImage: "doc.on.doc")
                        .font(.caption2)
                }
                .adaptiveGlassButtonStyle()
            }

            ScrollView([.horizontal, .vertical], showsIndicators: true) {
                Text(text)
                    .font(.caption.monospaced())
                    .foregroundStyle(.primary)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
            }
            .frame(minHeight: 80, maxHeight: 220)
            .background(Color.systemGray5.opacity(0.45))
            .clipShape(RoundedRectangle(cornerRadius: 6))
        }
    }

    private var requestTypeBadge: some View {
        Text(entry.requestType)
            .font(.caption2)
            .fontWeight(.medium)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(requestTypeColor.opacity(0.15))
            .foregroundColor(requestTypeColor)
            .clipShape(Capsule())
    }

    private var requestTypeColor: Color {
        switch entry.requestType {
        case "SignEvent": return .orange
        case "Connect": return .blue
        case "Ping": return .gray
        case "GetPublicKey": return .purple
        default: return .secondary
        }
    }

    private var decisionBadge: some View {
        Text(entry.decision)
            .font(.caption2)
            .fontWeight(.medium)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(decisionColor.opacity(0.15))
            .foregroundColor(decisionColor)
            .clipShape(Capsule())
    }

    private var decisionColor: Color {
        switch entry.decision {
        case "approved": return .green
        case "rejected": return .red
        case "timed-out": return .orange
        case "auto-approved": return .gray
        case "error": return .red
        default: return .secondary
        }
    }
}

// MARK: - Export Models

private struct BunkerAuditExportEnvelope: Codable {
    let generatedAtMs: UInt64
    let generatedAtIso8601: String
    let entryCount: Int
    let entries: [BunkerAuditExportEntry]
}

private struct BunkerAuditExportEntry: Codable {
    let requestId: String
    let sourceEventId: String
    let requesterPubkey: String
    let requesterDisplayName: String
    let requestType: String
    let decision: String
    let eventKind: UInt16?
    let timestampMs: UInt64
    let timestampIso8601: String
    let completedAtMs: UInt64
    let completedAtIso8601: String
    let responseTimeMs: UInt64
    let eventContentPreview: String?
    let eventContentFull: String?
    let eventTagsJson: String?
    let requestPayloadJson: String?
    let responsePayloadJson: String?
}

private struct BunkerAuditExportDocument: FileDocument {
    static var readableContentTypes: [UTType] { [.json] }

    let text: String

    init(text: String) {
        self.text = text
    }

    init(configuration: ReadConfiguration) throws {
        let data = configuration.file.regularFileContents ?? Data()
        self.text = String(data: data, encoding: .utf8) ?? ""
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        FileWrapper(regularFileWithContents: Data(text.utf8))
    }
}

private enum BunkerAuditFormatters {
    static let localTimeFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "HH:mm:ss.SSS"
        return formatter
    }()

    static let localDateTimeFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd HH:mm:ss.SSS"
        return formatter
    }()

    static let isoFormatter: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter
    }()

    static let exportFilenameDate: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "yyyy-MM-dd-HHmmss"
        return formatter
    }()

    static func localTime(ms: UInt64) -> String {
        localTimeFormatter.string(from: date(from: ms))
    }

    static func localDateTime(ms: UInt64) -> String {
        localDateTimeFormatter.string(from: date(from: ms))
    }

    static func iso8601String(ms: UInt64) -> String {
        isoFormatter.string(from: date(from: ms))
    }

    private static func date(from ms: UInt64) -> Date {
        Date(timeIntervalSince1970: TimeInterval(ms) / 1000.0)
    }
}

private extension FfiBunkerAuditEntry {
    var rowIdentity: String {
        "\(requestId)-\(timestampMs)-\(responseTimeMs)"
    }
}
