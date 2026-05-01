import SwiftUI

private struct ProjectGeneralDraft: Equatable {
    var title: String = ""
    var description: String = ""
    var repoUrl: String = ""
    var pictureUrl: String = ""
}

struct ProjectSettingsView: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    let projectId: String
    @Binding var selectedProjectId: String?

    @State private var generalDraft = ProjectGeneralDraft()
    @State private var baselineGeneralDraft = ProjectGeneralDraft()
    @State private var pendingAgentPubkeys: [String] = []
    @State private var baselineAgentPubkeys: [String] = []

    @State private var installedAgents: [InstalledAgent] = []
    @State private var projectBackendPubkey: String?

    @State private var showAddAgentSheet = false
    @State private var showDeleteDialog = false

    @State private var agentSearch = ""
    @State private var manualPubkeyInput: String = ""
    @State private var manualPubkeyError: String? = nil

    @State private var isSavingGeneral = false
    @State private var isSavingAgents = false
    @State private var isBooting = false
    @State private var isDeleting = false

    @State private var errorMessage: String?
    @State private var showErrorAlert = false

    private var project: Project? {
        coreManager.projects.first { $0.id == projectId }
    }

    private var generalHasChanges: Bool {
        generalDraft != baselineGeneralDraft
    }

    private var agentsHaveChanges: Bool {
        pendingAgentPubkeys != baselineAgentPubkeys
    }

    private var isProjectOnline: Bool {
        coreManager.projectOnlineStatus[projectId] ?? false
    }

    private var onlineAgentCount: Int {
        coreManager.onlineAgents[projectId]?.count ?? 0
    }

    private var filteredAvailableAgents: [InstalledAgent] {
        let remaining = installedAgents.filter { !pendingAgentPubkeys.contains($0.pubkey) }
        guard !agentSearch.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return remaining
        }

        let query = agentSearch.lowercased()
        return remaining.filter { agent in
            agent.slug.lowercased().contains(query)
                || agent.pubkey.lowercased().contains(query)
        }
    }

    var body: some View {
        Form {
            if let project {
                headerSection(project: project)
                generalSection(project: project)
                agentsSection(project: project)
                advancedSection
                dangerSection(project: project)
            } else {
                ContentUnavailableView(
                    "Project Not Found",
                    systemImage: "folder.badge.questionmark",
                    description: Text("This project may have been deleted or is no longer available.")
                )
            }
        }
        #if os(macOS)
        .formStyle(.grouped)
        #endif
        .navigationTitle("Project Settings")
        .onAppear {
            syncDraftsFromProject()
            Task { await reloadSelectionData() }
        }
        .onChange(of: project?.createdAt ?? 0) { _, _ in
            syncDraftsFromProject()
        }
        .onChange(of: coreManager.projectOnlineStatus[projectId] ?? false) { _, _ in
            Task { await reloadSelectionData() }
        }
        .sheet(isPresented: $showAddAgentSheet) {
            addAgentSheet
        }
        .confirmationDialog(
            "Delete Project",
            isPresented: $showDeleteDialog,
            titleVisibility: .visible
        ) {
            Button("Delete Project", role: .destructive) {
                Task { await deleteProject() }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This republishes kind 31933 with a deleted tag and hides the project everywhere.")
        }
        .alert("Project Settings Error", isPresented: $showErrorAlert) {
            Button("OK") { errorMessage = nil }
        } message: {
            Text(errorMessage ?? "An unknown error occurred.")
        }
    }

    @ViewBuilder
    private func headerSection(project: Project) -> some View {
        Section {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .center, spacing: 12) {
                    Image(systemName: "folder.fill")
                        .font(.title2)
                        .foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 4) {
                        Text(project.title)
                            .font(.headline)
                            .lineLimit(1)
                        Text(project.id)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .textSelection(.enabled)
                    }
                }

                HStack(spacing: 8) {
                    Label(isProjectOnline ? "Online" : "Offline", systemImage: isProjectOnline ? "checkmark.circle.fill" : "xmark.circle")
                        .font(.subheadline)
                        .foregroundStyle(isProjectOnline ? Color.orange : .secondary)
                    if isProjectOnline {
                        Text("• \(onlineAgentCount) agent\(onlineAgentCount == 1 ? "" : "s") active")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                }

                if !isProjectOnline {
                    Button {
                        Task { await bootProject() }
                    } label: {
                        if isBooting {
                            ProgressView()
                                .frame(maxWidth: .infinity)
                        } else {
                            Label("Boot Project", systemImage: "power")
                                .frame(maxWidth: .infinity)
                        }
                    }
                    .disabled(isBooting || isDeleting)
                    .adaptiveGlassButtonStyle()
                }
            }
            .padding(.vertical, 6)
            .padding(.horizontal, 4)
            .modifier(AvailableGlassEffect(reduceTransparency: reduceTransparency))
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        }
    }

    @ViewBuilder
    private func generalSection(project: Project) -> some View {
        Section("General") {
            TextField("Title", text: $generalDraft.title)
#if os(iOS)
                .textInputAutocapitalization(.words)
#endif

            TextField("Description", text: $generalDraft.description, axis: .vertical)
                .lineLimit(3...8)

            TextField("Repository URL", text: $generalDraft.repoUrl)
#if os(iOS)
                .keyboardType(.URL)
                .textInputAutocapitalization(.never)
#endif
                .autocorrectionDisabled()

            TextField("Image URL", text: $generalDraft.pictureUrl)
#if os(iOS)
                .keyboardType(.URL)
                .textInputAutocapitalization(.never)
#endif
                .autocorrectionDisabled()

            if let imageUrl = URL(string: generalDraft.pictureUrl.trimmingCharacters(in: .whitespacesAndNewlines)),
               !generalDraft.pictureUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                AsyncImage(url: imageUrl) { phase in
                    switch phase {
                    case .empty:
                        HStack {
                            Spacer()
                            ProgressView()
                            Spacer()
                        }
                        .frame(height: 140)
                    case .success(let image):
                        image
                            .resizable()
                            .scaledToFill()
                            .frame(height: 140)
                            .frame(maxWidth: .infinity)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    case .failure:
                        Label("Unable to load image preview", systemImage: "photo.badge.exclamationmark")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    @unknown default:
                        EmptyView()
                    }
                }
            }

            LabeledContent("Created") {
                Text(TimestampTextFormatter.string(from: project.createdAt, style: .mediumDateShortTime))
                    .foregroundStyle(.secondary)
            }

            HStack(spacing: 12) {
                Button("Reset") {
                    generalDraft = baselineGeneralDraft
                }
                .disabled(!generalHasChanges || isSavingGeneral)

                Button {
                    Task { await saveGeneral(project: project) }
                } label: {
                    if isSavingGeneral {
                        ProgressView()
                    } else {
                        Text("Save")
                    }
                }
                .disabled(
                    !generalHasChanges
                        || generalDraft.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                        || isSavingGeneral
                        || isDeleting
                )
                .adaptiveGlassButtonStyle()
            }
        }
    }

    @ViewBuilder
    private func agentsSection(project: Project) -> some View {
        Section {
            if pendingAgentPubkeys.isEmpty {
                ContentUnavailableView(
                    "No Agents Assigned",
                    systemImage: "person.2",
                    description: Text(
                        projectBackendPubkey == nil
                            ? "Bring the project backend online before assigning agents."
                            : "Assign installed backend agents to this project."
                    )
                )
            } else {
                ForEach(Array(pendingAgentPubkeys.enumerated()), id: \.element) { index, agentPubkey in
                    HStack(spacing: 12) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(agentName(for: agentPubkey))
                                .font(.body.weight(.medium))
                                .lineLimit(1)
                            Text(shortPubkey(agentPubkey))
                                .font(.caption2.monospaced())
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }

                        Spacer()

                        if index == 0 {
                            Text("PM")
                                .font(.caption2.weight(.semibold))
                                .padding(.horizontal, 6)
                                .padding(.vertical, 3)
                                .background(.quaternary, in: Capsule())
                        }

                        Menu {
                            if index != 0 {
                                Button("Set as PM") {
                                    setProjectManager(agentPubkey: agentPubkey)
                                }
                            }
                            Button("Remove", role: .destructive) {
                                removeAgent(agentPubkey: agentPubkey)
                            }
                        } label: {
                            Image(systemName: "ellipsis.circle")
                                .font(.title3)
                        }
                    }
                }
            }

            Button {
                showAddAgentSheet = true
            } label: {
                Label("Add Agent", systemImage: "plus")
            }
            .adaptiveGlassButtonStyle()
            .disabled(isSavingAgents || isDeleting)

            HStack(spacing: 12) {
                Button("Cancel") {
                    pendingAgentPubkeys = baselineAgentPubkeys
                }
                .disabled(!agentsHaveChanges || isSavingAgents)

                Button {
                    Task { await saveAgents(project: project) }
                } label: {
                    if isSavingAgents {
                        ProgressView()
                    } else {
                        Text("Save")
                    }
                }
                .disabled(!agentsHaveChanges || isSavingAgents || isDeleting)
                .adaptiveGlassButtonStyle()
            }
        } header: {
            Text("Agents")
        } footer: {
            Text("The first agent pubkey is treated as the default PM. You can add agents by npub or hex pubkey even when the backend is offline.")
        }
    }

    private var advancedSection: some View {
        Section("Advanced") {
            Label("Coming soon", systemImage: "clock")
                .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private func dangerSection(project: Project) -> some View {
        Section("Danger Zone") {
            Button(role: .destructive) {
                showDeleteDialog = true
            } label: {
                if isDeleting {
                    ProgressView()
                        .frame(maxWidth: .infinity)
                } else {
                    Label("Delete Project", systemImage: "trash")
                        .frame(maxWidth: .infinity)
                }
            }
            .disabled(isDeleting || isSavingGeneral || isSavingAgents)

            Text("This publishes a tombstone kind 31933 event with a deleted tag for \(project.title).")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private var addAgentSheet: some View {
        NavigationStack {
            List {
                Section("Add by Key") {
                    TextField("npub or hex pubkey", text: $manualPubkeyInput)
                        .autocorrectionDisabled()
                        #if os(iOS)
                        .textInputAutocapitalization(.never)
                        .keyboardType(.asciiCapable)
                        #endif

                    if let error = manualPubkeyError {
                        Text(error)
                            .font(.caption)
                            .foregroundStyle(.red)
                    }

                    Button("Add") {
                        addManualAgent()
                    }
                    .disabled(manualPubkeyInput.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }

                Section("Installed Agents") {
                    if filteredAvailableAgents.isEmpty {
                        ContentUnavailableView(
                            projectBackendPubkey == nil ? "Backend Offline" : "No Installed Agents",
                            systemImage: "person.crop.circle.badge.exclamationmark",
                            description: Text(
                                projectBackendPubkey == nil
                                    ? "Wait for the project backend to come online before assigning agents."
                                    : "Install an agent into this backend before assigning it to the project."
                            )
                        )
                    } else {
                        ForEach(filteredAvailableAgents, id: \.pubkey) { agent in
                            Button {
                                addAgent(agentPubkey: agent.pubkey)
                                showAddAgentSheet = false
                            } label: {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(agent.slug)
                                        .font(.body.weight(.medium))
                                        .foregroundStyle(.primary)
                                    Text(shortPubkey(agent.pubkey))
                                        .font(.caption.monospaced())
                                        .foregroundStyle(.secondary)
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }
            .searchable(text: $agentSearch, prompt: "Search installed agents")
            .navigationTitle("Add Agents")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") {
                        showAddAgentSheet = false
                        agentSearch = ""
                        manualPubkeyInput = ""
                        manualPubkeyError = nil
                    }
                }
            }
        }
        .tenexModalPresentation(detents: [.medium, .large])
        #if os(macOS)
        .frame(minWidth: 480, idealWidth: 540, minHeight: 420, idealHeight: 520)
        #endif
    }

    private func syncDraftsFromProject() {
        guard let project else { return }

        let syncedGeneral = ProjectGeneralDraft(
            title: project.title,
            description: project.description ?? "",
            repoUrl: project.repoUrl ?? "",
            pictureUrl: project.pictureUrl ?? ""
        )
        generalDraft = syncedGeneral
        baselineGeneralDraft = syncedGeneral

        pendingAgentPubkeys = project.agentPubkeys
        baselineAgentPubkeys = project.agentPubkeys
    }

    private func reloadSelectionData() async {
        let backendPubkey = coreManager.safeCore.getProjectBackendPubkey(projectId: projectId)

        do {
            let installedAgents: [InstalledAgent]
            if let backendPubkey {
                installedAgents = try await coreManager.safeCore.getInstalledAgents(backendPubkey: backendPubkey)
            } else {
                installedAgents = []
            }

            await MainActor.run {
                projectBackendPubkey = backendPubkey
                self.installedAgents = installedAgents.sorted {
                    $0.slug.localizedCaseInsensitiveCompare($1.slug) == .orderedAscending
                }
            }
        } catch {
            presentError(error)
        }
    }

    private func bootProject() async {
        guard let project else { return }
        isBooting = true
        defer { isBooting = false }

        do {
            try await coreManager.safeCore.bootProject(projectId: project.id)
        } catch {
            presentError(error)
        }
    }

    private func saveGeneral(project: Project) async {
        isSavingGeneral = true
        defer { isSavingGeneral = false }

        do {
            try await coreManager.safeCore.updateProject(
                projectId: project.id,
                title: generalDraft.title.trimmingCharacters(in: .whitespacesAndNewlines),
                description: generalDraft.description,
                repoUrl: normalizedOptional(generalDraft.repoUrl),
                pictureUrl: normalizedOptional(generalDraft.pictureUrl),
                agentPubkeys: project.agentPubkeys,
                mcpToolIds: project.mcpToolIds
            )
            baselineGeneralDraft = generalDraft
        } catch {
            presentError(error)
        }
    }

    private func saveAgents(project: Project) async {
        isSavingAgents = true
        defer { isSavingAgents = false }

        do {
            try await coreManager.safeCore.updateProject(
                projectId: project.id,
                title: project.title,
                description: project.description ?? "",
                repoUrl: project.repoUrl,
                pictureUrl: project.pictureUrl,
                agentPubkeys: pendingAgentPubkeys,
                mcpToolIds: project.mcpToolIds
            )
            baselineAgentPubkeys = pendingAgentPubkeys
        } catch {
            presentError(error)
        }
    }

    private func deleteProject() async {
        guard let project else { return }
        isDeleting = true
        defer { isDeleting = false }

        do {
            try await coreManager.safeCore.deleteProject(projectId: project.id)
            selectedProjectId = nil
        } catch {
            presentError(error)
        }
    }

    private func addAgent(agentPubkey: String) {
        guard !pendingAgentPubkeys.contains(agentPubkey) else { return }
        pendingAgentPubkeys.append(agentPubkey)
    }

    private func resolveToHexPubkey(_ input: String) -> String? {
        let trimmed = input.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.lowercased().hasPrefix("npub1") {
            return Bech32.npubToHex(trimmed)
        }
        let lower = trimmed.lowercased()
        guard lower.count == 64, lower.allSatisfy({ $0.isHexDigit }) else { return nil }
        return lower
    }

    private func addManualAgent() {
        guard let hex = resolveToHexPubkey(manualPubkeyInput) else {
            manualPubkeyError = "Enter a valid npub or 64-character hex pubkey."
            return
        }
        manualPubkeyError = nil
        addAgent(agentPubkey: hex)
        manualPubkeyInput = ""
        showAddAgentSheet = false
    }

    private func removeAgent(agentPubkey: String) {
        pendingAgentPubkeys.removeAll { $0 == agentPubkey }
    }

    private func setProjectManager(agentPubkey: String) {
        guard let index = pendingAgentPubkeys.firstIndex(of: agentPubkey), index > 0 else { return }
        pendingAgentPubkeys.remove(at: index)
        pendingAgentPubkeys.insert(agentPubkey, at: 0)
    }

    private func agentName(for pubkey: String) -> String {
        if let slug = installedAgents.first(where: { $0.pubkey == pubkey })?.slug {
            return slug
        }
        let name = coreManager.displayName(for: pubkey)
        return name.isEmpty ? shortPubkey(pubkey) : name
    }

    private func shortPubkey(_ pubkey: String) -> String {
        guard pubkey.count > 16 else { return pubkey }
        return "\(pubkey.prefix(8))…\(pubkey.suffix(8))"
    }

    private func normalizedOptional(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    private func presentError(_ error: Error) {
        errorMessage = error.localizedDescription
        showErrorAlert = true
    }

}
