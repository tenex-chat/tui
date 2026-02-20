import SwiftUI

extension MessageComposerView {
    func projectChipView(_ project: Project) -> some View {
        HStack(spacing: 8) {
            ProjectChipView(project: project) {
                showProjectSelector = true
            }
            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(.bar)
    }

    var projectPromptView: some View {
        Button(action: { showProjectSelector = true }) {
            HStack(spacing: 12) {
                Image(systemName: "folder")
                    .foregroundStyle(Color.composerAction)
                Text("Select a project to start")
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
        Button(action: { showProjectSelector = true }) {
            HStack(spacing: 6) {
                Image(systemName: "folder")
                    .font(.caption)
                    .foregroundStyle(Color.composerAction)
                Text("Select project")
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
                    .environmentObject(coreManager)

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

                Button(action: { openNudgeSkillSelector(mode: .nudges) }) {
                    HStack(spacing: 4) {
                        Image(systemName: "plus")
                            .font(.caption)
                        Text("Add Nudge")
                            .font(.caption)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background(
                        Capsule()
                            .strokeBorder(Color.secondary.opacity(0.3), lineWidth: 1)
                    )
                    .foregroundStyle(.secondary)
                }
                .buttonStyle(.borderless)
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

                Button(action: { openNudgeSkillSelector(mode: .skills) }) {
                    HStack(spacing: 4) {
                        Image(systemName: "plus")
                            .font(.caption)
                        Text("Add Skill")
                            .font(.caption)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background(
                        Capsule()
                            .strokeBorder(Color.secondary.opacity(0.3), lineWidth: 1)
                    )
                    .foregroundStyle(.secondary)
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 16)
        }
        .padding(.vertical, 12)
        .background(.bar)
    }

    var workspaceInlineControlRow: some View {
        HStack(spacing: 12) {
            workspaceAccessoryButton

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 16) {
                    if let project = selectedProject {
                        inlineContextToken(text: workspaceProjectLabel(project.title)) {
                            if isNewConversation {
                                showProjectSelector = true
                            }
                        }
                    } else if isNewConversation {
                        inlineContextToken(text: "Select project") {
                            showProjectSelector = true
                        }
                    }

                    if let agent = selectedAgent {
                        inlineContextToken(text: workspaceAgentLabel(agentContextSummary(agent: agent))) {
                            openAgentSelector()
                        }
                    } else if let targetPubkey = initialAgentPubkey, let targetName = replyTargetAgentName {
                        inlineContextToken(text: workspaceAgentLabel(targetName)) {
                            draft.setAgent(targetPubkey)
                            openAgentSelector()
                        }
                    } else if selectedProject != nil {
                        inlineContextToken(text: "Agent") {
                            openAgentSelector()
                        }
                    }

                    inlineContextToken(text: nudgeSkillContextSummary) {
                        openNudgeSkillSelector(mode: .all)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)

            workspaceMicGlyph

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
            #if os(macOS)
            .keyboardShortcut(.return, modifiers: [.command])
            #endif
            .help("Send")
        }
        .frame(height: max(workspaceContextRowHeight, workspaceBottomRowHeight))
        .padding(.horizontal, 18)
        .overlay(alignment: .top) {
            Rectangle()
                .fill(workspaceComposerStrokeColor.opacity(0.72))
                .frame(height: 1)
        }
        .background(workspaceComposerFooterColor)
    }

    var workspaceAccessoryButton: some View {
        Button {
            #if os(iOS)
            showImagePicker = true
            #else
            openNudgeSkillSelector(mode: .all)
            #endif
        } label: {
            Image(systemName: "plus")
                .font(.system(size: workspaceIconSize, weight: .medium))
                .foregroundStyle(.secondary)
                .frame(width: workspaceAccessoryButtonSize, height: workspaceAccessoryButtonSize)
        }
        .buttonStyle(.borderless)
        #if os(iOS)
        .disabled(selectedProject == nil || isUploadingImage)
        #else
        .disabled(selectedProject == nil)
        #endif
    }

    var workspaceMicGlyph: some View {
        Image(systemName: "mic")
            .font(.system(size: workspaceIconSize, weight: .medium))
            .foregroundStyle(.secondary.opacity(0.88))
            .frame(width: workspaceAccessoryButtonSize, height: workspaceAccessoryButtonSize)
    }

    func inlineContextToken(text: String, showChevron: Bool = true, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 4) {
                Text(text)
                    .font(.subheadline)
                    .lineLimit(1)
                    .foregroundStyle(.secondary.opacity(0.95))
                if showChevron {
                    Image(systemName: "chevron.down")
                        .font(.system(size: max(9, workspaceIconSize - 4), weight: .semibold))
                        .foregroundStyle(.secondary.opacity(0.88))
                }
            }
            .padding(.horizontal, 2)
            .padding(.vertical, 2)
            .frame(height: workspaceContextRowHeight)
            .contentShape(Rectangle())
        }
        .buttonStyle(.borderless)
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

    func agentContextSummary(agent: ProjectAgent) -> String {
        if !usesWorkspaceInlineLayout, let model = agent.model, !model.isEmpty {
            return "\(agent.name) (\(model))"
        }
        return agent.name
    }

    var nudgeSkillContextSummary: String {
        let selectedCount = selectedNudges.count + selectedSkills.count
        guard selectedCount > 0 else { return "Shortcuts" }
        return selectedCount == 1 ? "1 selected" : "\(selectedCount) selected"
    }
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
    @EnvironmentObject var coreManager: TenexCoreManager
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
                .environmentObject(coreManager)

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
