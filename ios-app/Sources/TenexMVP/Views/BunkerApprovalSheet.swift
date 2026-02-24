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
    @State private var previewMode: EventPreviewMode = .raw

    private enum EventPreviewMode: String, CaseIterable, Identifiable {
        case preview = "Preview"
        case raw = "Raw"

        var id: String { rawValue }
    }

    private var previewModel: BunkerSignPreviewModel {
        BunkerSignPreviewModel(request: request)
    }

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
        .onAppear {
            previewMode = previewModel.isAgentDefinition4199 ? .preview : .raw
            startCountdown()
        }
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
        let preview = previewModel
        let resolvedKind = preview.kind ?? request.eventKind

        return VStack(alignment: .leading, spacing: 10) {
            if let kind = resolvedKind {
                HStack {
                    Label(kindName(kind), systemImage: kindIcon(kind))
                        .font(.subheadline.bold())
                    Text("(\(kind))")
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                    Spacer()
                }
            }

            if preview.isAgentDefinition4199 {
                Picker("Event View", selection: $previewMode) {
                    Text(EventPreviewMode.preview.rawValue).tag(EventPreviewMode.preview)
                    Text(EventPreviewMode.raw.rawValue).tag(EventPreviewMode.raw)
                }
                .pickerStyle(.segmented)
            }

            if preview.isAgentDefinition4199,
               previewMode == .preview,
               let agent = preview.agentDefinition {
                richAgentDefinitionPreview(agent)
            } else {
                rawEventSection(preview.rawEventJson)
            }
        }
    }

    @ViewBuilder
    private func richAgentDefinitionPreview(
        _ agent: BunkerSignPreviewModel.AgentDefinitionPreview
    ) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                Text(agent.title ?? "Untitled Agent Definition")
                    .font(.headline)

                let metadataChips = [
                    agent.role.map { "Role: \($0)" },
                    agent.category.map { "Category: \($0)" },
                    agent.version.map { "v\($0)" }
                ].compactMap { $0 }

                if !metadataChips.isEmpty {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: 120), spacing: 6)], spacing: 6) {
                        ForEach(metadataChips, id: \.self) { chip in
                            metadataChip(chip)
                        }
                    }
                }

                if let dTag = agent.dTag, !dTag.isEmpty {
                    metadataRow(title: "d-tag", value: dTag)
                }

                if let description = agent.description, !description.isEmpty {
                    richPreviewSection(title: "Description") {
                        Text(description)
                            .font(.subheadline)
                            .foregroundStyle(.primary)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }

                if !agent.instructionsFromTags.isEmpty {
                    richPreviewSection(title: "Instructions (tag)") {
                        VStack(alignment: .leading, spacing: 8) {
                            ForEach(Array(agent.instructionsFromTags.enumerated()), id: \.offset) { entry in
                                MarkdownView(content: entry.element)
                            }
                        }
                    }
                }

                if !agent.contentMarkdown.isEmpty {
                    richPreviewSection(title: "Content") {
                        MarkdownView(content: agent.contentMarkdown)
                    }
                }

                if !agent.useCriteria.isEmpty {
                    richPreviewSection(title: "Use Criteria") {
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(Array(agent.useCriteria.enumerated()), id: \.offset) { entry in
                                HStack(alignment: .top, spacing: 6) {
                                    Text("•")
                                        .foregroundStyle(.secondary)
                                    Text(entry.element)
                                        .font(.subheadline)
                                        .frame(maxWidth: .infinity, alignment: .leading)
                                }
                            }
                        }
                    }
                }

                if !agent.tools.isEmpty {
                    richPreviewSection(title: "Tools") {
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 100), spacing: 6)], spacing: 6) {
                            ForEach(agent.tools, id: \.self) { tool in
                                metadataChip(tool)
                            }
                        }
                    }
                }

                if !agent.mcpServers.isEmpty {
                    richPreviewSection(title: "MCP Servers") {
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(agent.mcpServers, id: \.self) { server in
                                Text(server)
                                    .font(.caption.monospaced())
                                    .textSelection(.enabled)
                            }
                        }
                    }
                }

                if !agent.fileEventIds.isEmpty {
                    richPreviewSection(title: "File References (e-tags)") {
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(agent.fileEventIds, id: \.self) { fileId in
                                Text(fileId)
                                    .font(.caption.monospaced())
                                    .textSelection(.enabled)
                            }
                        }
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(maxHeight: 300)
        .padding(8)
        .background(.quaternary)
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }

    private func rawEventSection(_ rawJson: String) -> some View {
        ScrollView {
            Text(rawJson)
                .font(.caption)
                .fontDesign(.monospaced)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(maxHeight: 300)
        .padding(8)
        .background(.quaternary)
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }

    private func metadataChip(_ text: String) -> some View {
        Text(text)
            .font(.caption)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(.tertiary, in: RoundedRectangle(cornerRadius: 6))
    }

    private func metadataRow(title: String, value: String) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Text(title)
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(width: 70, alignment: .leading)

            Text(value)
                .font(.caption.monospaced())
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func richPreviewSection<Content: View>(
        title: String,
        @ViewBuilder content: () -> Content
    ) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .font(.caption)
                .foregroundStyle(.secondary)
            content()
        }
    }

    // MARK: - Auto-Approve Toggle

    private var autoApproveToggle: some View {
        Group {
            if let kind = request.eventKind {
                #if os(macOS)
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

        if approved {
            coreManager.approveBunkerRequest(requestId: request.requestId)
        } else {
            coreManager.rejectBunkerRequest(requestId: request.requestId)
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
