import SwiftUI

extension MessageComposerView {
    func projectChipView(_ project: Project) -> some View {
        HStack(spacing: 8) {
            Menu {
                projectSelectionMenuContent()
            } label: {
                projectMenuChipLabel(project)
            }
            .menuIndicator(.hidden)
            .menuStyle(.borderlessButton)
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(.bar)
    }

    func projectMenuChipLabel(_ project: Project) -> some View {
        HStack(spacing: 6) {
            RoundedRectangle(cornerRadius: 4)
                .fill(deterministicColor(for: project.id).gradient)
                .frame(width: 24, height: 24)
                .overlay {
                    Image(systemName: "folder.fill")
                        .font(.caption2)
                        .foregroundStyle(.white)
                }

            Text(project.title)
                .font(.subheadline)
                .fontWeight(.medium)
                .foregroundStyle(.primary)

            Image(systemName: "chevron.down")
                .font(.caption2)
                .foregroundStyle(.tertiary)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(
            Capsule()
                .fill(Color.systemBackground)
                .shadow(color: .black.opacity(0.1), radius: 2, x: 0, y: 1)
        )
    }

    var projectPromptView: some View {
        Menu {
            projectSelectionMenuContent()
        } label: {
            HStack(spacing: 12) {
                Image(systemName: "folder")
                    .foregroundStyle(Color.composerAction)
                Text("Select a project to start")
                    .foregroundStyle(.secondary)
                Spacer()
                Image(systemName: "chevron.down")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .background(.bar)
        }
        .menuIndicator(.hidden)
        .menuStyle(.borderlessButton)
    }

    var agentPromptView: some View {
        Button(action: { openAgentSelector() }) {
            HStack(spacing: 12) {
                Image(systemName: "person")
                    .foregroundStyle(Color.composerAction)
                Text("Select an agent (optional)")
                    .foregroundStyle(.secondary)
                Spacer()
                Image(systemName: "chevron.right")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .background(.bar)
        }
        .buttonStyle(.borderless)
    }

    /// Compact project prompt button for horizontal layout
    var projectPromptButton: some View {
        Menu {
            projectSelectionMenuContent()
        } label: {
            HStack(spacing: 6) {
                Image(systemName: "folder")
                    .font(.caption)
                    .foregroundStyle(Color.composerAction)
                Text("Select project")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                Image(systemName: "chevron.down")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(
                Capsule()
                    .strokeBorder(Color.secondary.opacity(0.3), lineWidth: 1)
            )
        }
        .menuIndicator(.hidden)
        .menuStyle(.borderlessButton)
    }

    /// Compact agent prompt button for horizontal layout
    var agentPromptButton: some View {
        Button(action: { openAgentSelector() }) {
            HStack(spacing: 6) {
                Image(systemName: "person")
                    .font(.caption)
                    .foregroundStyle(Color.composerAction)
                Text("Select agent")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(
                Capsule()
                    .strokeBorder(Color.secondary.opacity(0.3), lineWidth: 1)
            )
        }
        .buttonStyle(.borderless)
    }

    func agentChipView(_ agent: ProjectAgent) -> some View {
        HStack(spacing: 8) {
            OnlineAgentChipView(agent: agent) {
                openAgentSelector()
            }
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(.bar)
    }

    /// Shows the reply target agent (used when replying and the agent isn't in online agents list)
    func replyTargetChipView(name: String, pubkey: String, onChange: @escaping () -> Void) -> some View {
        HStack(spacing: 8) {
            Button(action: onChange) {
                HStack(spacing: 6) {
                    AgentAvatarView(
                        agentName: name,
                        pubkey: pubkey,
                        size: 24,
                        showBorder: false
                    )
                    .environment(coreManager)

                    Text("@\(name)")
                        .font(.subheadline)
                        .fontWeight(.medium)
                        .foregroundStyle(.primary)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .background(
                    Capsule()
                        .fill(Color.systemBackground)
                        .shadow(color: .black.opacity(0.1), radius: 2, x: 0, y: 1)
                )
            }
            .buttonStyle(.borderless)
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(.bar)
    }

    var nudgeChipsView: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(selectedNudges, id: \.id) { nudge in
                    NudgeChipView(nudge: nudge) {
                        isDirty = true
                        draft.removeNudge(nudge.id)
                        persistSelectedNudgeIds()
                    }
                }

            }
            .padding(.horizontal, 16)
        }
        .padding(.vertical, 12)
        .background(.bar)
    }

    var skillChipsView: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 8) {
                ForEach(selectedSkills, id: \.id) { skill in
                    SkillChipView(skill: skill) {
                        isDirty = true
                        draft.removeSkill(skill.id)
                        persistSelectedSkillIds()
                    }
                }
            }
            .padding(.horizontal, 16)
        }
        .padding(.vertical, 12)
        .background(.bar)
    }

    var workspaceInlineControlRow: some View {
        HStack(spacing: 12) {
            workspaceAccessoryButton

            if dictationManager.state.isRecording {
                DictationRecordingBar(
                    audioLevelSamples: dictationManager.audioLevelSamples,
                    recordingStartDate: dictationManager.recordingStartDate,
                    error: dictationManager.error,
                    onStop: {
                        Task {
                            await dictationManager.stopRecording()
                        }
                    }
                )
            } else {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 16) {
                        if let project = selectedProject {
                            if isNewConversation {
                                projectSelectionInlineToken(text: workspaceProjectLabel(project.title))
                            } else {
                                Text(workspaceProjectLabel(project.title))
                                    .font(workspaceTokenFont)
                                    .lineLimit(1)
                                    .foregroundStyle(workspaceTokenTextColor)
                                    .padding(.horizontal, 2)
                                    .padding(.vertical, 2)
                                    .frame(height: workspaceContextRowHeight)
                            }
                        } else if isNewConversation {
                            projectSelectionInlineToken(text: "Select project")
                        }

                        if selectedProject != nil {
                            agentPopoverToken
                        }

                        if let agent = selectedAgent, let model = agent.model, !model.isEmpty {
                            inlineContextToken(text: model, showChevron: false) {
                                workspaceAgentToConfig = agent
                            }
                        }

                        nudgeSkillToken
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)

                workspaceMicGlyph
                #if os(macOS)
                if shouldShowWorkspaceReferenceConversationButton {
                    workspaceReferenceConversationButton
                }
                #endif
                workspacePinnedPromptsControl
            }

            Button(action: sendMessage) {
                Image(systemName: "arrow.up")
                    .font(.system(size: workspaceIconSize, weight: .semibold))
                    .foregroundStyle(canSend ? workspaceComposerShellColor : Color.secondary.opacity(0.9))
                    .frame(width: workspaceSendButtonSize, height: workspaceSendButtonSize)
                    .background(
                        Circle()
                            .fill(canSend ? Color.white.opacity(0.78) : Color.white.opacity(0.14))
                    )
            }
            .buttonStyle(.borderless)
            .disabled(!canSend)
            .help("Send")
        }
        .frame(height: max(workspaceContextRowHeight, workspaceBottomRowHeight))
        .padding(.horizontal, 18)
    }

    var workspaceAccessoryButton: some View {
        Button {
            #if os(iOS)
            showImagePicker = true
            #else
            openMacFilePicker()
            #endif
        } label: {
            Image(systemName: "plus")
                .font(.system(size: workspaceIconSize, weight: .medium))
                .foregroundStyle(.secondary)
                .frame(width: workspaceAccessoryButtonSize, height: workspaceAccessoryButtonSize)
        }
        .buttonStyle(.borderless)
        .disabled(selectedProject == nil || isUploadingImage)
        .help("Attach image")
    }

    var workspaceMicGlyph: some View {
        Button {
            Task {
                preDictationText = localText
                try? await dictationManager.startRecording()
            }
        } label: {
            Image(systemName: dictationManager.state.isRecording ? "mic.fill" : "mic")
                .font(.system(size: workspaceIconSize, weight: .medium))
                .foregroundStyle(dictationManager.state.isRecording ? Color.recordingActive : .secondary.opacity(0.88))
                .frame(width: workspaceAccessoryButtonSize, height: workspaceAccessoryButtonSize)
        }
        .buttonStyle(.borderless)
        .disabled(!dictationManager.state.isIdle || selectedProject == nil)
        .help("Voice dictation")
    }

    #if os(macOS)
    var shouldShowWorkspaceReferenceConversationButton: Bool {
        usesWorkspaceInlineLayout && !isNewConversation && conversationId != nil && selectedProject != nil
    }

    var workspaceReferenceConversationButton: some View {
        Button {
            triggerReferenceConversationLaunch()
        } label: {
            Image(systemName: "arrow.triangle.branch")
                .font(.system(size: workspaceIconSize, weight: .medium))
                .foregroundStyle(.secondary.opacity(0.88))
                .frame(width: workspaceAccessoryButtonSize, height: workspaceAccessoryButtonSize)
        }
        .buttonStyle(.borderless)
        .disabled(onReferenceConversationRequested == nil)
        .help("Reference this conversation")
    }

    func triggerReferenceConversationLaunch() {
        guard let onReferenceConversationRequested,
              let project = selectedProject,
              let conversationId = conversationId
        else {
            return
        }

        let messages = coreManager.messagesByConversation[conversationId] ?? []
        let contextMessage = ConversationFormatters.generateContextMessage(
            conversationId: conversationId,
            messages: messages
        )
        let seed = NewThreadComposerSeed(
            projectId: project.id,
            agentPubkey: draft.agentPubkey,
            initialContent: "[Text Attachment 1]",
            textAttachments: [TextAttachment(id: 1, content: contextMessage)],
            referenceConversationId: conversationId,
            referenceReportATag: nil
        )

        onReferenceConversationRequested(
            ReferenceConversationLaunchPayload(seed: seed)
        )
    }
    #endif

    @ViewBuilder
    var workspacePinnedPromptsControl: some View {
        switch pinControlMode {
        case .hidden:
            EmptyView()
        case .menu:
            workspacePinnedPromptsMenu
        case .pinAction:
            workspacePinPromptButton
        }
    }

    var workspacePinnedPromptsMenu: some View {
        Menu {
            pinnedPromptsMenuContent()
        } label: {
            Image(systemName: recentPinnedPrompts.isEmpty ? "pin" : "pin.fill")
                .font(.system(size: workspaceIconSize, weight: .medium))
                .foregroundStyle(.secondary.opacity(0.88))
                .frame(width: workspaceAccessoryButtonSize, height: workspaceAccessoryButtonSize)
        }
        .menuStyle(.borderlessButton)
        .help("Pinned prompts")
    }

    var workspacePinPromptButton: some View {
        Button {
            pinCurrentPrompt()
        } label: {
            Image(systemName: "pin")
                .font(.system(size: workspaceIconSize, weight: .medium))
                .foregroundStyle(.secondary.opacity(0.88))
                .frame(width: workspaceAccessoryButtonSize, height: workspaceAccessoryButtonSize)
        }
        .buttonStyle(.borderless)
        .accessibilityLabel("Pin this prompt")
        .help("Pin this prompt")
    }

    var workspaceTokenFont: Font {
        .subheadline
    }

    var workspaceTokenTextColor: Color {
        .secondary.opacity(0.92)
    }

    var workspaceTokenIconColor: Color {
        .secondary.opacity(0.86)
    }

    var agentPopoverToken: some View {
        Button {
            showWorkspaceAgentPopover.toggle()
        } label: {
            HStack(spacing: 4) {
                Text(selectedAgent.map { workspaceAgentLabel($0.name) } ?? "Agent")
                    .font(workspaceTokenFont)
                    .lineLimit(1)
                    .foregroundStyle(workspaceTokenTextColor)
                Image(systemName: "chevron.down")
                    .font(.system(size: max(9, workspaceIconSize - 4), weight: .semibold))
                    .foregroundStyle(workspaceTokenIconColor)
            }
            .padding(.horizontal, 2)
            .padding(.vertical, 2)
            .frame(height: workspaceContextRowHeight)
            .contentShape(Rectangle())
        }
        .buttonStyle(.borderless)
        .popover(isPresented: $showWorkspaceAgentPopover) {
            WorkspaceAgentPopoverContent(
                agents: availableAgents,
                selectedPubkey: draft.agentPubkey,
                onSelect: { pubkey in
                    draft.agentPubkey = pubkey
                    isDirty = true
                    if let projectId = selectedProject?.id {
                        Task {
                            await draftManager.updateAgent(pubkey, conversationId: conversationId, projectId: projectId)
                        }
                    }
                    showWorkspaceAgentPopover = false
                },
                onClear: {
                    draft.agentPubkey = nil
                    isDirty = true
                    if let projectId = selectedProject?.id {
                        Task {
                            await draftManager.updateAgent(nil, conversationId: conversationId, projectId: projectId)
                        }
                    }
                    showWorkspaceAgentPopover = false
                },
                onConfig: { agent in
                    showWorkspaceAgentPopover = false
                    workspaceAgentToConfig = agent
                }
            )
            .environment(coreManager)
        }
    }

    var nudgeSkillToken: some View {
        Button {
            openNudgeSkillSelector(mode: .all)
        } label: {
            HStack(spacing: 4) {
                Text("/")
                    .font(workspaceTokenFont.monospaced())
                    .foregroundStyle(workspaceTokenTextColor)
                if (selectedNudges.count + selectedSkills.count) > 0 {
                    Text("\(selectedNudges.count + selectedSkills.count)")
                        .font(workspaceTokenFont)
                        .foregroundStyle(workspaceTokenTextColor)
                }
            }
            .padding(.horizontal, 2)
            .padding(.vertical, 2)
            .frame(height: workspaceContextRowHeight)
            .contentShape(Rectangle())
        }
        .buttonStyle(.borderless)
        .help("Shortcuts")
    }

    func inlineContextToken(text: String, showChevron: Bool = true, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 4) {
                Text(text)
                    .font(workspaceTokenFont)
                    .lineLimit(1)
                    .foregroundStyle(workspaceTokenTextColor)
                if showChevron {
                    Image(systemName: "chevron.down")
                        .font(.system(size: max(9, workspaceIconSize - 4), weight: .semibold))
                        .foregroundStyle(workspaceTokenIconColor)
                }
            }
            .padding(.horizontal, 2)
            .padding(.vertical, 2)
            .frame(height: workspaceContextRowHeight)
            .contentShape(Rectangle())
        }
        .buttonStyle(.borderless)
    }

    func projectSelectionInlineToken(text: String) -> some View {
        Menu {
            projectSelectionMenuContent()
        } label: {
            Text(text)
                .font(workspaceTokenFont)
                .lineLimit(1)
                .foregroundStyle(workspaceTokenTextColor)
            .padding(.horizontal, 2)
            .padding(.vertical, 2)
            .frame(height: workspaceContextRowHeight)
            .contentShape(Rectangle())
        }
        .menuIndicator(.hidden)
        .menuStyle(.borderlessButton)
    }

    @ViewBuilder
    private func projectSelectionMenuContent() -> some View {
        if projectMenuState.booted.isEmpty && projectMenuState.unbooted.isEmpty {
            Text("No projects available")
        } else {
            if !projectMenuState.booted.isEmpty {
                Section("Booted Projects") {
                    ForEach(projectMenuState.booted, id: \.id) { project in
                        projectSelectionButton(for: project)
                    }
                }
            }

            if !projectMenuState.unbooted.isEmpty {
                Menu("Unbooted Projects") {
                    ForEach(projectMenuState.unbooted, id: \.id) { project in
                        projectSelectionButton(for: project)
                    }
                }
            }
        }
    }

    private func projectSelectionButton(for project: Project) -> some View {
        Button {
            selectProjectForComposer(project)
        } label: {
            if selectedProject?.id == project.id {
                Label(project.title, systemImage: "checkmark")
            } else {
                Text(project.title)
            }
        }
    }

    private var projectMenuState: ComposerProjectMenuState {
        let sortedProjects = coreManager.projects.sorted { lhs, rhs in
            let lhsOnline = coreManager.projectOnlineStatus[lhs.id] ?? false
            let rhsOnline = coreManager.projectOnlineStatus[rhs.id] ?? false
            if lhsOnline != rhsOnline { return lhsOnline }
            return lhs.title.localizedCaseInsensitiveCompare(rhs.title) == .orderedAscending
        }

        let booted = sortedProjects.filter { coreManager.projectOnlineStatus[$0.id] ?? false }
        let unbooted = sortedProjects.filter { !(coreManager.projectOnlineStatus[$0.id] ?? false) }
        return ComposerProjectMenuState(booted: booted, unbooted: unbooted)
    }

    private func selectProjectForComposer(_ project: Project) {
        guard selectedProject?.id != project.id else { return }
        selectedProject = project
        projectChanged()
    }

    func workspaceProjectLabel(_ name: String) -> String {
        truncatedWorkspaceToken(name)
    }

    func workspaceAgentLabel(_ name: String) -> String {
        truncatedWorkspaceToken(name)
    }

    func truncatedWorkspaceToken(_ value: String, limit: Int = 24) -> String {
        guard value.count > limit else { return value }
        let end = value.index(value.startIndex, offsetBy: limit)
        return "\(value[..<end])..."
    }

}

private struct ComposerProjectMenuState {
    var booted: [Project] = []
    var unbooted: [Project] = []
}

struct ProjectChipView: View {
    let project: Project
    let onChange: () -> Void

    var body: some View {
        Button(action: onChange) {
            HStack(spacing: 6) {
                RoundedRectangle(cornerRadius: 4)
                    .fill(projectColor.gradient)
                    .frame(width: 24, height: 24)
                    .overlay {
                        Image(systemName: "folder.fill")
                            .font(.caption2)
                            .foregroundStyle(.white)
                    }

                Text(project.title)
                    .font(.subheadline)
                    .fontWeight(.medium)
                    .foregroundStyle(.primary)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(
                Capsule()
                    .fill(Color.systemBackground)
                    .shadow(color: .black.opacity(0.1), radius: 2, x: 0, y: 1)
            )
        }
        .buttonStyle(.borderless)
        .contentShape(Capsule())
    }

    private var projectColor: Color {
        deterministicColor(for: project.id)
    }
}

struct OnlineAgentChipView: View {
    @Environment(TenexCoreManager.self) var coreManager
    let agent: ProjectAgent
    let onChange: () -> Void

    var body: some View {
        Button(action: onChange) {
            HStack(spacing: 6) {
                AgentAvatarView(
                    agentName: agent.name,
                    pubkey: agent.pubkey,
                    size: 24,
                    showBorder: false
                )
                .environment(coreManager)

                Text("@\(agent.name)")
                    .font(.subheadline)
                    .fontWeight(.medium)
                    .foregroundStyle(.primary)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(
                Capsule()
                    .fill(Color.systemBackground)
                    .shadow(color: .black.opacity(0.1), radius: 2, x: 0, y: 1)
            )
        }
        .buttonStyle(.borderless)
        .contentShape(Capsule())
    }
}

struct WorkspaceAgentPopoverContent: View {
    @Environment(TenexCoreManager.self) var coreManager
    let agents: [ProjectAgent]
    let selectedPubkey: String?
    let onSelect: (String) -> Void
    let onClear: () -> Void
    let onConfig: (ProjectAgent) -> Void

    @State private var searchText = ""

    private var filteredAgents: [ProjectAgent] {
        let list = searchText.isEmpty
            ? agents
            : agents.filter { $0.name.localizedCaseInsensitiveContains(searchText) }
        return list.sorted { a, b in
            if a.isPm != b.isPm { return a.isPm }
            return a.name.localizedCaseInsensitiveCompare(b.name) == .orderedAscending
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            // Search field
            HStack(spacing: 8) {
                Image(systemName: "magnifyingglass")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                TextField("Search agents", text: $searchText)
                    .textFieldStyle(.plain)
                    .font(.subheadline)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)

            Divider()

            if filteredAgents.isEmpty {
                Text(searchText.isEmpty ? "No agents online" : "No results")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 20)
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(filteredAgents, id: \.pubkey) { agent in
                            agentRow(agent)
                        }
                    }
                }
                .frame(maxHeight: 280)
            }

            if selectedPubkey != nil {
                Divider()
                Button(role: .destructive) {
                    onClear()
                } label: {
                    HStack {
                        Image(systemName: "xmark")
                            .font(.caption)
                        Text("Clear agent")
                            .font(.subheadline)
                    }
                    .foregroundStyle(.red)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
            }
        }
        .frame(width: 280)
    }

    private func agentRow(_ agent: ProjectAgent) -> some View {
        Button {
            onSelect(agent.pubkey)
        } label: {
            HStack(spacing: 8) {
                AgentAvatarView(
                    agentName: agent.name,
                    pubkey: agent.pubkey,
                    size: 28,
                    showBorder: false,
                    isSelected: selectedPubkey == agent.pubkey
                )
                .environment(coreManager)

                VStack(alignment: .leading, spacing: 1) {
                    HStack(spacing: 4) {
                        Text(agent.name)
                            .font(.subheadline)
                            .fontWeight(.medium)
                            .foregroundStyle(.primary)
                        if agent.isPm {
                            Text("PM")
                                .font(.system(size: 9, weight: .semibold))
                                .foregroundStyle(.white)
                                .padding(.horizontal, 4)
                                .padding(.vertical, 1)
                                .background(Capsule().fill(Color.agentBrand))
                        }
                    }
                    if let model = agent.model, !model.isEmpty {
                        Text(model)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }

                Spacer()

                if selectedPubkey == agent.pubkey {
                    Image(systemName: "checkmark")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(Color.accentColor)
                }

                Button {
                    onConfig(agent)
                } label: {
                    Image(systemName: "gearshape")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .frame(width: 28, height: 28)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .contentShape(Rectangle())
        }
        .buttonStyle(.borderless)
    }
}
