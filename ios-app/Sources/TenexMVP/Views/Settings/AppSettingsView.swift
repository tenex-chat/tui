import SwiftUI

struct AppSettingsView: View {
    @EnvironmentObject private var coreManager: TenexCoreManager
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
            if useSplitLayout {
                splitSettingsView
            } else {
                phoneSettingsView
            }
        }
        .task {
            await viewModel.load(coreManager: coreManager)
        }
        .onReceive(coreManager.diagnosticsVersionPublisher) { _ in
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

    private var splitSettingsView: some View {
        NavigationSplitView {
            List(SettingsSection.allCases, selection: $selectedSection) { section in
                NavigationLink(value: section) {
                    Label(section.title, systemImage: section.icon)
                }
            }
            #if os(macOS)
            .listStyle(.sidebar)
            #endif
            .navigationTitle("Settings")
        } detail: {
            Group {
                if let section = selectedSection {
                    sectionContent(section)
                } else {
                    ContentUnavailableView("Select a Section", systemImage: "gearshape")
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .navigationTitle(selectedSection?.title ?? "Settings")
            .toolbar {
                if !isEmbedded {
                    ToolbarItem(placement: .topBarTrailing) {
                        Button("Done") { dismiss() }
                    }
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
                if !isEmbedded {
                    ToolbarItem(placement: .topBarTrailing) {
                        Button("Done") { dismiss() }
                    }
                }
            }
            .onAppear {
                if !isEmbedded && phonePath.isEmpty {
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
                .environmentObject(coreManager)
        case .backends:
            BackendsSettingsSectionView(viewModel: viewModel)
                .environmentObject(coreManager)
        case .ai:
            AISettingsSectionView(viewModel: viewModel)
                .environmentObject(coreManager)
        case .audio:
            AudioSettingsSectionView(viewModel: viewModel)
                .environmentObject(coreManager)
        }
    }
}

private struct RelaysSettingsSectionView: View {
    @ObservedObject var viewModel: AppSettingsViewModel
    @EnvironmentObject private var coreManager: TenexCoreManager

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
    @EnvironmentObject private var coreManager: TenexCoreManager

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

private struct AISettingsSectionView: View {
    @EnvironmentObject private var coreManager: TenexCoreManager
    @ObservedObject var viewModel: AppSettingsViewModel

    @State private var elevenLabsKeyInput = ""
    @State private var openRouterKeyInput = ""
    @State private var isEditingElevenLabsKey = false
    @State private var isEditingOpenRouterKey = false
    @State private var showModelSelector = false

    var body: some View {
        Form {
            Section {
                apiKeyRow(
                    title: "ElevenLabs API Key",
                    description: "Required for audio synthesis",
                    hasKey: viewModel.hasElevenLabsKey,
                    isEditing: $isEditingElevenLabsKey,
                    keyInput: $elevenLabsKeyInput,
                    onSave: {
                        let key = elevenLabsKeyInput
                        Task {
                            await viewModel.saveElevenLabsKey(key)
                            elevenLabsKeyInput = ""
                            isEditingElevenLabsKey = false
                        }
                    },
                    onDelete: { Task { await viewModel.deleteElevenLabsKey() } }
                )

                apiKeyRow(
                    title: "OpenRouter API Key",
                    description: "Required for LLM text processing",
                    hasKey: viewModel.hasOpenRouterKey,
                    isEditing: $isEditingOpenRouterKey,
                    keyInput: $openRouterKeyInput,
                    onSave: {
                        let key = openRouterKeyInput
                        Task {
                            await viewModel.saveOpenRouterKey(key)
                            openRouterKeyInput = ""
                            isEditingOpenRouterKey = false
                        }
                    },
                    onDelete: { Task { await viewModel.deleteOpenRouterKey() } }
                )
            } header: {
                Text("API Keys")
            } footer: {
                #if os(macOS)
                Text("Keys are stored in local files on this Mac.")
                #else
                Text("Keys are stored in system Keychain.")
                #endif
            }

            Section("Model") {
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
                }
                .disabled(!viewModel.hasOpenRouterKey)
            }
        }
        #if os(macOS)
        .formStyle(.grouped)
        #endif
        .sheet(isPresented: $showModelSelector) {
            ModelSelectorSheet(viewModel: viewModel)
                .environmentObject(coreManager)
                #if os(macOS)
                .frame(minWidth: 500, idealWidth: 560, minHeight: 420, idealHeight: 560)
                #endif
        }
    }

    @ViewBuilder
    private func apiKeyRow(
        title: String,
        description: String,
        hasKey: Bool,
        isEditing: Binding<Bool>,
        keyInput: Binding<String>,
        onSave: @escaping () -> Void,
        onDelete: @escaping () -> Void
    ) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                VStack(alignment: .leading) {
                    Text(title)
                    Text(description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                if hasKey && !isEditing.wrappedValue {
                    HStack(spacing: 8) {
                        Text("••••••••")
                            .foregroundStyle(.secondary)
                        Button(role: .destructive, action: onDelete) {
                            Image(systemName: "trash")
                        }
                        .buttonStyle(.borderless)
                    }
                } else if !isEditing.wrappedValue {
                    Button("Set Key") { isEditing.wrappedValue = true }
                        .buttonStyle(.bordered)
                }
            }

            if isEditing.wrappedValue {
                HStack {
                    SecureField("Enter API key", text: keyInput)
                        .textFieldStyle(.roundedBorder)
                        .autocorrectionDisabled()
                        #if os(iOS)
                        .textInputAutocapitalization(.never)
                        #endif

                    Button("Save", action: onSave)
                        .buttonStyle(.borderedProminent)
                        .disabled(keyInput.wrappedValue.isEmpty || viewModel.isSavingApiKey)

                    Button("Cancel") {
                        keyInput.wrappedValue = ""
                        isEditing.wrappedValue = false
                    }
                    .buttonStyle(.bordered)
                }
            }
        }
        .padding(.vertical, 4)
    }
}

private struct AudioSettingsSectionView: View {
    @EnvironmentObject private var coreManager: TenexCoreManager
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
                        .environmentObject(coreManager)
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
                .environmentObject(coreManager)
                #if os(macOS)
                .frame(minWidth: 700, idealWidth: 780, minHeight: 480, idealHeight: 620)
                #endif
        }
    }
}

private struct ModelSelectorSheet: View {
    @EnvironmentObject private var coreManager: TenexCoreManager
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
                            HStack {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(model.name ?? model.id)
                                        .foregroundStyle(.primary)
                                    if let description = model.description, !description.isEmpty {
                                        Text(description)
                                            .font(.caption)
                                            .foregroundStyle(.secondary)
                                            .lineLimit(2)
                                    }
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
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Refresh") {
                        Task { await viewModel.fetchModels(coreManager: coreManager) }
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
                    if !viewModel.selectedModelIds.isEmpty {
                        Button("Clear") {
                            Task { await viewModel.clearSelectedModels(coreManager: coreManager) }
                        }
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
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
    @EnvironmentObject private var coreManager: TenexCoreManager
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
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Refresh") {
                        Task { await viewModel.fetchVoices(coreManager: coreManager) }
                    }
                }
                ToolbarItem(placement: .topBarTrailing) {
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
