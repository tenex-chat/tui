import SwiftUI

struct AppSettingsView: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.dismiss) private var dismiss
    @StateObject private var viewModel = AppSettingsViewModel()
    @State private var selectedSection: SettingsSection?
    @State private var phonePath: [SettingsSection] = []
    let defaultSection: SettingsSection
    let isEmbedded: Bool

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    init(defaultSection: SettingsSection = .audio, isEmbedded: Bool = false) {
        self.defaultSection = defaultSection
        self.isEmbedded = isEmbedded
        _selectedSection = State(initialValue: defaultSection)
    }

    private var useSplitLayout: Bool {
        #if os(macOS)
        return true
        #else
        return horizontalSizeClass == .regular
        #endif
    }

    var body: some View {
        Group {
            if isEmbedded {
                embeddedSettingsView
            } else if useSplitLayout {
                splitSettingsView
            } else {
                phoneSettingsView
            }
        }
        .task {
            await viewModel.load(coreManager: coreManager)
        }
        .onChange(of: coreManager.diagnosticsVersion) { _, _ in
            Task {
                await viewModel.reloadRelays(coreManager: coreManager)
                await viewModel.reloadBackends(coreManager: coreManager)
            }
        }
        .alert("Error", isPresented: Binding(
            get: { viewModel.errorMessage != nil },
            set: { shown in if !shown { viewModel.errorMessage = nil } }
        )) {
            Button("OK", role: .cancel) { }
        } message: {
            Text(viewModel.errorMessage ?? "")
        }
    }

    private var settingsCategoryList: some View {
        List(SettingsSection.allCases, selection: $selectedSection) { section in
            NavigationLink(value: section) {
                Label(section.title, systemImage: section.icon)
            }
        }
        #if os(macOS)
        .listStyle(.sidebar)
        #endif
    }

    @ViewBuilder
    private var settingsDetailContent: some View {
        if let section = selectedSection {
            sectionContent(section)
        } else {
            ContentUnavailableView("Select a Section", systemImage: "gearshape")
        }
    }

    private var embeddedSettingsView: some View {
        #if os(macOS)
        HSplitView {
            settingsCategoryList
                .frame(minWidth: 160, idealWidth: 190, maxWidth: 240)

            NavigationStack {
                settingsDetailContent
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                    .navigationTitle(selectedSection?.title ?? "Settings")
            }
            .frame(minWidth: 400)
        }
        .onAppear {
            if selectedSection == nil {
                selectedSection = defaultSection
            }
        }
        #else
        HStack(spacing: 0) {
            settingsCategoryList
                .frame(minWidth: 160, idealWidth: 190, maxWidth: 240)

            Divider()

            NavigationStack {
                settingsDetailContent
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                    .navigationTitle(selectedSection?.title ?? "Settings")
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .onAppear {
            if selectedSection == nil {
                selectedSection = defaultSection
            }
        }
        #endif
    }

    private var splitSettingsView: some View {
        NavigationSplitView {
            settingsCategoryList
                .navigationTitle("Settings")
        } detail: {
            settingsDetailContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                .navigationTitle(selectedSection?.title ?? "Settings")
                .toolbar {
                    ToolbarItem(placement: .confirmationAction) {
                        Button("Done") { dismiss() }
                    }
                }
        }
        .onAppear {
            if selectedSection == nil {
                selectedSection = defaultSection
            }
        }
    }

    private var phoneSettingsView: some View {
        NavigationStack(path: $phonePath) {
            List(SettingsSection.allCases) { section in
                NavigationLink(value: section) {
                    Label(section.title, systemImage: section.icon)
                }
            }
            .navigationDestination(for: SettingsSection.self) { section in
                sectionContent(section)
                    .navigationTitle(section.title)
            }
            .navigationTitle("Settings")
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
            .onAppear {
                if phonePath.isEmpty {
                    phonePath = [defaultSection]
                }
            }
        }
    }

    @ViewBuilder
    private func sectionContent(_ section: SettingsSection) -> some View {
        switch section {
        case .relays:
            RelaysSettingsSectionView(viewModel: viewModel)
                .environment(coreManager)
        case .backends:
            BackendsSettingsSectionView(viewModel: viewModel)
                .environment(coreManager)
        case .bunker:
            BunkerSettingsSectionView(viewModel: viewModel)
                .environment(coreManager)
        case .ai:
            AISettingsSectionView(viewModel: viewModel)
                .environment(coreManager)
        case .audio:
            AudioSettingsSectionView(viewModel: viewModel)
                .environment(coreManager)
        }
    }
}

private struct RelaysSettingsSectionView: View {
    @ObservedObject var viewModel: AppSettingsViewModel
    @Environment(TenexCoreManager.self) private var coreManager

    var body: some View {
        Form {
            Section("Configured Relays") {
                if viewModel.relayUrls.isEmpty {
                    Text("No relays configured")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(viewModel.relayUrls, id: \.self) { relay in
                        Text(relay)
                            .fontDesign(.monospaced)
                            .textSelection(.enabled)
                    }
                }
            }

            Section("Connection") {
                let system = viewModel.diagnosticsSnapshot?.system
                HStack {
                    Text("Status")
                    Spacer()
                    Text((system?.relayConnected ?? false) ? "Connected" : "Disconnected")
                        .foregroundStyle((system?.relayConnected ?? false) ? .green : .secondary)
                }
                HStack {
                    Text("Connected Relays")
                    Spacer()
                    Text("\(system?.connectedRelays ?? 0)")
                        .foregroundStyle(.secondary)
                }
            }

            Section("Sync Health") {
                let sync = viewModel.diagnosticsSnapshot?.sync
                HStack {
                    Text("Last Sync")
                    Spacer()
                    Text(lastSyncText(sync?.secondsSinceLastCycle))
                        .foregroundStyle(.secondary)
                }
                HStack {
                    Text("Success Rate")
                    Spacer()
                    Text(successRateText(sync))
                        .foregroundStyle(.secondary)
                }
            }

            Section("Actions") {
                Button("Reconnect Relays") {
                    Task { await viewModel.reconnectRelays(coreManager: coreManager) }
                }
                .buttonStyle(.borderedProminent)

                Button("Sync Now") {
                    Task { await viewModel.syncNow(coreManager: coreManager) }
                }
            }
        }
        #if os(macOS)
        .formStyle(.grouped)
        #endif
    }

    private func lastSyncText(_ seconds: UInt64?) -> String {
        guard let seconds else { return "Never" }
        if seconds < 60 { return "\(seconds)s ago" }
        if seconds < 3600 { return "\(seconds / 60)m ago" }
        return "\(seconds / 3600)h ago"
    }

    private func successRateText(_ sync: NegentropySyncDiagnostics?) -> String {
        guard let sync else { return "N/A" }
        let total = sync.successfulSyncs + sync.failedSyncs
        guard total > 0 else { return "100%" }
        let percent = (Double(sync.successfulSyncs) / Double(total)) * 100
        return String(format: "%.1f%%", percent)
    }
}

private struct BackendsSettingsSectionView: View {
    @ObservedObject var viewModel: AppSettingsViewModel
    @Environment(TenexCoreManager.self) private var coreManager

    var body: some View {
        Form {
            Section {
                Button("Refresh Backend State") {
                    Task { await viewModel.reloadBackends(coreManager: coreManager) }
                }
            }

            Section("Pending") {
                if let snapshot = viewModel.backendSnapshot, !snapshot.pending.isEmpty {
                    ForEach(Array(snapshot.pending.enumerated()), id: \.offset) { _, pending in
                        VStack(alignment: .leading, spacing: 8) {
                            Text(pending.backendPubkey)
                                .font(.caption)
                                .fontDesign(.monospaced)
                                .textSelection(.enabled)
                            Text(pending.projectATag)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .textSelection(.enabled)
                            HStack {
                                Button("Approve") {
                                    Task {
                                        await viewModel.approveBackend(
                                            coreManager: coreManager,
                                            pubkey: pending.backendPubkey
                                        )
                                    }
                                }
                                .buttonStyle(.borderedProminent)
                                Button("Block") {
                                    Task {
                                        await viewModel.blockBackend(
                                            coreManager: coreManager,
                                            pubkey: pending.backendPubkey
                                        )
                                    }
                                }
                                .buttonStyle(.bordered)
                            }
                        }
                        .padding(.vertical, 2)
                    }
                } else {
                    Text("No pending backends")
                        .foregroundStyle(.secondary)
                }
            }

            Section("Approved") {
                if let snapshot = viewModel.backendSnapshot, !snapshot.approved.isEmpty {
                    ForEach(snapshot.approved, id: \.self) { pubkey in
                        backendTrustRow(pubkey: pubkey, primaryActionTitle: "Block") {
                            Task { await viewModel.blockBackend(coreManager: coreManager, pubkey: pubkey) }
                        } removeAction: {
                            Task { await viewModel.removeFromTrustedLists(coreManager: coreManager, pubkey: pubkey) }
                        }
                    }
                } else {
                    Text("No approved backends")
                        .foregroundStyle(.secondary)
                }
            }

            Section("Blocked") {
                if let snapshot = viewModel.backendSnapshot, !snapshot.blocked.isEmpty {
                    ForEach(snapshot.blocked, id: \.self) { pubkey in
                        backendTrustRow(pubkey: pubkey, primaryActionTitle: "Approve") {
                            Task { await viewModel.approveBackend(coreManager: coreManager, pubkey: pubkey) }
                        } removeAction: {
                            Task { await viewModel.removeFromTrustedLists(coreManager: coreManager, pubkey: pubkey) }
                        }
                    }
                } else {
                    Text("No blocked backends")
                        .foregroundStyle(.secondary)
                }
            }
        }
        #if os(macOS)
        .formStyle(.grouped)
        #endif
        .task {
            await viewModel.reloadBackends(coreManager: coreManager)
        }
    }

    @ViewBuilder
    private func backendTrustRow(
        pubkey: String,
        primaryActionTitle: String,
        primaryAction: @escaping () -> Void,
        removeAction: @escaping () -> Void
    ) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(pubkey)
                .font(.caption)
                .fontDesign(.monospaced)
                .textSelection(.enabled)
            HStack {
                Button(primaryActionTitle, action: primaryAction)
                    .buttonStyle(.borderedProminent)
                Button("Remove", role: .destructive, action: removeAction)
                    .buttonStyle(.bordered)
            }
        }
        .padding(.vertical, 2)
    }
}

private struct BunkerSettingsSectionView: View {
    @ObservedObject var viewModel: AppSettingsViewModel
    @Environment(TenexCoreManager.self) private var coreManager

    var body: some View {
        Form {
            Section {
                Toggle(isOn: Binding(
                    get: { viewModel.bunkerRunning },
                    set: { enabled in
                        Task { await viewModel.setBunkerEnabled(coreManager: coreManager, enabled: enabled) }
                    }
                )) {
                    HStack {
                        Text("Enable Bunker")
                        if viewModel.isTogglingBunker {
                            ProgressView()
                                .controlSize(.small)
                        }
                    }
                }
                .disabled(viewModel.isTogglingBunker)
            } header: {
                Text("Remote Signer (NIP-46)")
            } footer: {
                Text("When enabled, agents can request you to sign events. You'll be prompted to approve or reject each signing request.")
            }

            if viewModel.bunkerRunning && !viewModel.bunkerUri.isEmpty {
                Section("Connection URI") {
                    Text(viewModel.bunkerUri)
                        .font(.caption)
                        .fontDesign(.monospaced)
                        .textSelection(.enabled)
                        .lineLimit(nil)

                    Button("Copy URI") {
                        #if os(macOS)
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(viewModel.bunkerUri, forType: .string)
                        #else
                        UIPasteboard.general.string = viewModel.bunkerUri
                        #endif
                    }
                    .buttonStyle(.bordered)
                }
            }

            if viewModel.bunkerRunning {
                Section("Auto-Approve Rules") {
                    if viewModel.bunkerAutoApproveRules.isEmpty {
                        Text("No auto-approve rules")
                            .foregroundStyle(.secondary)
                    } else {
                        ForEach(viewModel.bunkerAutoApproveRules, id: \.ruleId) { rule in
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(bunkerKindName(rule.eventKind))
                                        .font(.body)
                                    Text(truncatedPubkey(rule.requesterPubkey))
                                        .font(.caption)
                                        .fontDesign(.monospaced)
                                        .foregroundStyle(.secondary)
                                }
                                Spacer()
                                Image(systemName: "checkmark.shield.fill")
                                    .foregroundStyle(.green)
                            }
                        }
                        .onDelete { offsets in
                            let rulesToRemove = offsets.map { viewModel.bunkerAutoApproveRules[$0] }
                            for rule in rulesToRemove {
                                Task {
                                    await viewModel.removeBunkerAutoApproveRule(
                                        coreManager: coreManager,
                                        rule: rule
                                    )
                                }
                            }
                        }
                    }
                }

                Section {
                    if viewModel.bunkerAuditLog.isEmpty {
                        Text("No requests this session")
                            .foregroundStyle(.secondary)
                    } else {
                        ForEach(viewModel.bunkerAuditLog.reversed(), id: \.requestId) { entry in
                            HStack(spacing: 10) {
                                Image(systemName: auditOutcomeIcon(entry.decision))
                                    .foregroundStyle(auditOutcomeColor(entry.decision))
                                VStack(alignment: .leading, spacing: 2) {
                                    HStack(spacing: 4) {
                                        Text(entry.requestType)
                                            .font(.body)
                                        if let kind = entry.eventKind {
                                            Text("(\(bunkerKindName(kind)))")
                                                .font(.caption)
                                                .foregroundStyle(.secondary)
                                        }
                                    }
                                    Text(truncatedPubkey(entry.requesterPubkey))
                                        .font(.caption)
                                        .fontDesign(.monospaced)
                                        .foregroundStyle(.secondary)
                                }
                                Spacer()
                                Text("\(entry.responseTimeMs)ms")
                                    .font(.caption2)
                                    .foregroundStyle(.tertiary)
                            }
                        }
                    }
                } header: {
                    HStack {
                        Text("Request Log")
                        Spacer()
                        Button("Refresh") {
                            Task { await viewModel.loadBunkerRulesAndLog(coreManager: coreManager) }
                        }
                        .font(.caption)
                        .buttonStyle(.borderless)
                    }
                }
            }
        }
        #if os(macOS)
        .formStyle(.grouped)
        #endif
        .task {
            if viewModel.bunkerRunning {
                await viewModel.loadBunkerRulesAndLog(coreManager: coreManager)
            }
        }
    }

    private func truncatedPubkey(_ pk: String) -> String {
        if pk.count > 16 {
            return "\(pk.prefix(8))...\(pk.suffix(8))"
        }
        return pk
    }

    private func bunkerKindName(_ kind: UInt16?) -> String {
        guard let kind else { return "Any Kind" }
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

    private func auditOutcomeIcon(_ decision: String) -> String {
        switch decision {
        case "auto-approved": return "checkmark.shield.fill"
        case "approved": return "checkmark.circle.fill"
        case "rejected": return "xmark.circle.fill"
        case "timed-out": return "clock.badge.exclamationmark"
        default: return "questionmark.circle"
        }
    }

    private func auditOutcomeColor(_ decision: String) -> Color {
        switch decision {
        case "auto-approved": return .green
        case "approved": return .green
        case "rejected": return .red
        case "timed-out": return .orange
        default: return .secondary
        }
    }
}

private struct AISettingsSectionView: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @ObservedObject var viewModel: AppSettingsViewModel

    @State private var showModelSelector = false
    @State private var showCredentialSheet = false
    @State private var selectedProvider = ""
    @State private var credentialInput = ""
    @State private var credentialError: String?

    private let providers: [(id: String, name: String, description: String)] = [
        ("openrouter", "OpenRouter", "Required for LLM text processing"),
        ("elevenlabs", "ElevenLabs", "Required for audio synthesis"),
    ]

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            VStack(alignment: .leading, spacing: 8) {
                Text("Providers")
                    .font(.headline)
                    .padding(.horizontal, 16)

                VStack(spacing: 0) {
                    ForEach(Array(providers.enumerated()), id: \.element.id) { index, provider in
                        let hasKey = provider.id == "openrouter" ? viewModel.hasOpenRouterKey : viewModel.hasElevenLabsKey
                        HStack(spacing: 12) {
                            ProviderLogoView(provider.id, size: 24)
                            VStack(alignment: .leading, spacing: 2) {
                                HStack(spacing: 6) {
                                    Text(provider.name)
                                        .font(.body.weight(.medium))
                                    if hasKey {
                                        Image(systemName: "checkmark.circle.fill")
                                            .foregroundStyle(.green)
                                            .font(.body)
                                    }
                                }
                                Text(provider.description)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                            Spacer()
                            Button(hasKey ? "Disconnect" : "Connect") {
                                if hasKey {
                                    Task {
                                        if provider.id == "openrouter" {
                                            await viewModel.deleteOpenRouterKey()
                                        } else {
                                            await viewModel.deleteElevenLabsKey()
                                        }
                                    }
                                } else {
                                    credentialError = nil
                                    credentialInput = ""
                                    selectedProvider = provider.id
                                    showCredentialSheet = true
                                }
                            }
                            .buttonStyle(.bordered)
                        }
                        .padding(12)
                        if index < providers.count - 1 {
                            Divider()
                        }
                    }
                }
                #if os(macOS)
                .background(Color(nsColor: .windowBackgroundColor))
                #else
                .background(Color(.secondarySystemGroupedBackground))
                #endif
                .clipShape(RoundedRectangle(cornerRadius: 10))
                .overlay(
                    RoundedRectangle(cornerRadius: 10)
                        .stroke(.quaternary, lineWidth: 1)
                )
                .padding(.horizontal, 16)
            }

            VStack(alignment: .leading, spacing: 8) {
                Text("Model")
                    .font(.headline)
                    .padding(.horizontal, 16)

                Button {
                    Task {
                        if viewModel.availableModels.isEmpty {
                            await viewModel.fetchModels(coreManager: coreManager)
                        }
                        showModelSelector = true
                    }
                } label: {
                    HStack {
                        Text("OpenRouter Models")
                        Spacer()
                        Text(viewModel.selectedModelsSummary)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                        Image(systemName: "chevron.right")
                            .font(.caption)
                            .foregroundStyle(.tertiary)
                    }
                    .padding(12)
                }
                #if os(macOS)
                .background(Color(nsColor: .windowBackgroundColor))
                #else
                .background(Color(.secondarySystemGroupedBackground))
                #endif
                .clipShape(RoundedRectangle(cornerRadius: 10))
                .overlay(
                    RoundedRectangle(cornerRadius: 10)
                        .stroke(.quaternary, lineWidth: 1)
                )
                .padding(.horizontal, 16)
                .disabled(!viewModel.hasOpenRouterKey)
            }

            Spacer(minLength: 0)
        }
        .padding(.top, 16)
        .sheet(isPresented: $showModelSelector) {
            ModelSelectorSheet(viewModel: viewModel)
                .environment(coreManager)
                #if os(macOS)
                .frame(minWidth: 500, idealWidth: 560, minHeight: 420, idealHeight: 560)
                #endif
        }
        .sheet(isPresented: $showCredentialSheet) {
            credentialSheet
        }
    }

    private var credentialSheet: some View {
        let providerName = providers.first(where: { $0.id == selectedProvider })?.name ?? selectedProvider
        return VStack(alignment: .leading, spacing: 12) {
            Text("Connect \(providerName)")
                .font(.headline)

            SecureField("API key", text: $credentialInput)
                .textFieldStyle(.roundedBorder)
                .font(.system(.body, design: .monospaced))
                .autocorrectionDisabled()
                #if os(iOS)
                .textInputAutocapitalization(.never)
                #endif

            if let credentialError {
                Text(credentialError)
                    .font(.caption)
                    .foregroundStyle(.red)
            }

            HStack {
                Button("Cancel") {
                    showCredentialSheet = false
                }
                Spacer()
                Button("Connect") {
                    let trimmed = credentialInput.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !trimmed.isEmpty else {
                        credentialError = "API key is required."
                        return
                    }
                    let provider = selectedProvider
                    Task {
                        if provider == "openrouter" {
                            await viewModel.saveOpenRouterKey(trimmed)
                        } else {
                            await viewModel.saveElevenLabsKey(trimmed)
                        }
                        credentialInput = ""
                        credentialError = nil
                        showCredentialSheet = false
                    }
                }
                .keyboardShortcut(.defaultAction)
                .disabled(credentialInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || viewModel.isSavingApiKey)
            }
        }
        .padding(16)
        #if os(macOS)
        .frame(width: 420)
        #endif
    }
}

private struct AudioSettingsSectionView: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @ObservedObject var viewModel: AppSettingsViewModel
    @State private var showVoiceBrowser = false

    var body: some View {
        Form {
            Section("Audio Notifications") {
                Toggle("Enable Audio Notifications", isOn: Binding(
                    get: { viewModel.audioEnabled },
                    set: { enabled in
                        Task { await viewModel.setAudioEnabled(coreManager: coreManager, enabled: enabled) }
                    }
                ))

                Stepper(
                    value: Binding(
                        get: { Int(viewModel.ttsInactivityThresholdSecs) },
                        set: { value in
                            Task {
                                await viewModel.setTtsInactivityThreshold(
                                    coreManager: coreManager,
                                    secs: UInt64(value)
                                )
                            }
                        }
                    ),
                    in: 10...600,
                    step: 10
                ) {
                    HStack {
                        Text("Inactivity Threshold")
                        Spacer()
                        Text("\(viewModel.ttsInactivityThresholdSecs)s")
                            .foregroundStyle(.secondary)
                    }
                }
            }

            Section("Voices") {
                Button {
                    Task {
                        if viewModel.availableVoices.isEmpty {
                            await viewModel.fetchVoices(coreManager: coreManager)
                        }
                        showVoiceBrowser = true
                    }
                } label: {
                    HStack {
                        Text("ElevenLabs Voice Browser")
                        Spacer()
                        Text(viewModel.selectedVoiceIds.isEmpty ? "None selected" : "\(viewModel.selectedVoiceIds.count) selected")
                            .foregroundStyle(.secondary)
                        Image(systemName: "chevron.right")
                            .font(.caption)
                            .foregroundStyle(.tertiary)
                    }
                }
                .disabled(!viewModel.hasElevenLabsKey)
            }

            Section {
                TextEditor(text: $viewModel.audioPrompt)
                    .frame(minHeight: 120)
                    .font(.callout)
                HStack {
                    Button("Save Prompt") {
                        Task { await viewModel.saveAudioPrompt(coreManager: coreManager) }
                    }
                    .buttonStyle(.borderedProminent)

                    Spacer()

                    Button("Reset to Default", role: .destructive) {
                        Task { await viewModel.resetAudioPrompt(coreManager: coreManager) }
                    }
                    .buttonStyle(.bordered)
                }
            } header: {
                Text("Audio Prompt")
            } footer: {
                Text("Instructions for how text should be transformed before speech synthesis.")
            }

            Section("Debug") {
                NavigationLink {
                    AudioNotificationsLogView()
                        .environment(coreManager)
                } label: {
                    Label("Audio Debug Log", systemImage: "list.bullet.rectangle")
                }
            }
        }
        #if os(macOS)
        .formStyle(.grouped)
        #endif
        .sheet(isPresented: $showVoiceBrowser) {
            VoiceBrowserSheet(viewModel: viewModel)
                .environment(coreManager)
                #if os(macOS)
                .frame(minWidth: 700, idealWidth: 780, minHeight: 480, idealHeight: 620)
                #endif
        }
    }
}

private struct ModelSelectorSheet: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.dismiss) private var dismiss
    @ObservedObject var viewModel: AppSettingsViewModel
    @State private var searchText = ""

    private var filteredModels: [ModelInfo] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if query.isEmpty { return viewModel.availableModels }
        return viewModel.availableModels.filter { model in
            let name = (model.name ?? model.id).lowercased()
            let id = model.id.lowercased()
            let description = (model.description ?? "").lowercased()
            return name.contains(query) || id.contains(query) || description.contains(query)
        }
    }

    var body: some View {
        NavigationStack {
            Group {
                if viewModel.isLoadingModels {
                    ProgressView("Loading models...")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if filteredModels.isEmpty {
                    ContentUnavailableView("No Models", systemImage: "cpu")
                } else {
                    List(filteredModels, id: \.id) { model in
                        Button {
                            Task {
                                await viewModel.toggleSelectedModel(
                                    coreManager: coreManager,
                                    modelId: model.id
                                )
                            }
                        } label: {
                            HStack(spacing: 10) {
                                let providerSlug = model.id.split(separator: "/").first.map(String.init) ?? ""
                                ProviderLogoView(providerSlug, size: 20)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(model.name ?? model.id)
                                        .foregroundStyle(.primary)
                                    Text(model.id)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                                Spacer()
                                if viewModel.isModelSelected(model.id) {
                                    Image(systemName: "checkmark.circle.fill")
                                        .foregroundStyle(Color.agentBrand)
                                } else {
                                    Image(systemName: "circle")
                                        .foregroundStyle(.tertiary)
                                }
                            }
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
            .searchable(text: $searchText, prompt: "Search models")
            .navigationTitle("OpenRouter Models")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .automatic) {
                    Button("Refresh") {
                        Task { await viewModel.fetchModels(coreManager: coreManager) }
                    }
                }
                ToolbarItem(placement: .automatic) {
                    if !viewModel.selectedModelIds.isEmpty {
                        Button("Clear") {
                            Task { await viewModel.clearSelectedModels(coreManager: coreManager) }
                        }
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }
}

private enum VoiceSortMode: String, CaseIterable, Identifiable {
    case nameAsc
    case nameDesc

    var id: String { rawValue }
    var title: String {
        switch self {
        case .nameAsc: return "Name A-Z"
        case .nameDesc: return "Name Z-A"
        }
    }
}

private struct VoiceBrowserSheet: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.dismiss) private var dismiss
    @ObservedObject var viewModel: AppSettingsViewModel
    @StateObject private var previewPlayer = VoicePreviewPlayer()
    @State private var searchText = ""
    @State private var selectedCategory = "All"
    @State private var sortMode: VoiceSortMode = .nameAsc

    private var categories: [String] {
        let all = viewModel.availableVoices.compactMap { $0.category }.filter { !$0.isEmpty }
        return ["All"] + Array(Set(all)).sorted()
    }

    private var filteredVoices: [VoiceInfo] {
        var voices = viewModel.availableVoices

        if selectedCategory != "All" {
            voices = voices.filter { $0.category == selectedCategory }
        }

        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        if !query.isEmpty {
            voices = voices.filter { voice in
                let name = voice.name.lowercased()
                let id = voice.voiceId.lowercased()
                let category = (voice.category ?? "").lowercased()
                let description = (voice.description ?? "").lowercased()
                return name.contains(query) || id.contains(query) || category.contains(query) || description.contains(query)
            }
        }

        switch sortMode {
        case .nameAsc:
            voices.sort { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
        case .nameDesc:
            voices.sort { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedDescending }
        }

        return voices
    }

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                controls
                    .padding(.horizontal)
                    .padding(.vertical, 10)

                Divider()

                Group {
                    if viewModel.isLoadingVoices {
                        ProgressView("Loading voices...")
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                    } else if filteredVoices.isEmpty {
                        ContentUnavailableView("No Voices", systemImage: "waveform")
                    } else {
                        List(filteredVoices, id: \.voiceId) { voice in
                            voiceRow(voice)
                        }
                    }
                }
            }
            .searchable(text: $searchText, prompt: "Search voices")
            .navigationTitle("ElevenLabs Voices")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .automatic) {
                    Button("Refresh") {
                        Task { await viewModel.fetchVoices(coreManager: coreManager) }
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") {
                        previewPlayer.stop()
                        dismiss()
                    }
                }
            }
            .safeAreaInset(edge: .bottom) {
                HStack {
                    Spacer()
                    Button("Done") {
                        previewPlayer.stop()
                        dismiss()
                    }
                    .buttonStyle(.borderedProminent)
                }
                .padding(.horizontal)
                .padding(.vertical, 8)
                .background(.ultraThinMaterial)
            }
        }
    }

    private var controls: some View {
        HStack {
            Picker("Category", selection: $selectedCategory) {
                ForEach(categories, id: \.self) { category in
                    Text(category).tag(category)
                }
            }
            .labelsHidden()

            Spacer(minLength: 12)

            Picker("Sort", selection: $sortMode) {
                ForEach(VoiceSortMode.allCases) { mode in
                    Text(mode.title).tag(mode)
                }
            }
            .labelsHidden()
            .pickerStyle(.menu)
        }
    }

    @ViewBuilder
    private func voiceRow(_ voice: VoiceInfo) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text(voice.name)
                        .font(.body)
                    Text(voice.voiceId)
                        .font(.caption)
                        .fontDesign(.monospaced)
                        .foregroundStyle(.secondary)
                }
                Spacer()

                Button {
                    Task {
                        await viewModel.toggleVoice(coreManager: coreManager, voiceId: voice.voiceId)
                    }
                } label: {
                    Label(
                        viewModel.selectedVoiceIds.contains(voice.voiceId) ? "Selected" : "Select",
                        systemImage: viewModel.selectedVoiceIds.contains(voice.voiceId) ? "checkmark.circle.fill" : "circle"
                    )
                }
                .buttonStyle(.bordered)
            }

            if let description = voice.description, !description.isEmpty {
                Text(description)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }

            HStack {
                if let category = voice.category, !category.isEmpty {
                    Text(category)
                        .font(.caption2)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(Color.systemGray6)
                        .clipShape(Capsule())
                }

                Spacer()

                if voice.previewUrl != nil {
                    Button {
                        previewPlayer.toggle(voiceId: voice.voiceId, previewUrl: voice.previewUrl)
                    } label: {
                        Label(
                            previewPlayer.playingVoiceId == voice.voiceId ? "Stop Preview" : "Play Preview",
                            systemImage: previewPlayer.playingVoiceId == voice.voiceId ? "stop.fill" : "play.fill"
                        )
                    }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
                } else {
                    Text("No preview available")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(.vertical, 6)
    }
}
