import SwiftUI

struct MainShellView: View {
    @Binding var userNpub: String
    @Binding var isLoggedIn: Bool
    let runtimeText: String

    @Environment(TenexCoreManager.self) private var coreManager

    @State private var selectedSection: AppSection? = .chats
    @State private var selectedConversation: ConversationFullInfo?
    @State private var selectedProjectId: String?
    @State private var showNewProject = false
    @State private var hasCheckedOnboarding = false
    @State private var selectedReport: Report?
    @State private var selectedInboxFilter: InboxFilter = .all
    @State private var selectedInboxItemId: String?
    @State private var activeInboxConversationId: String?
    @State private var selectedSearchConversation: ConversationFullInfo?
    @State private var selectedTeam: TeamInfo?
    @State private var selectedAgentInstance: AgentInstance?
    @State private var selectedAgentDefinition: AgentDefinition?
    @State private var selectedNudge: Nudge?
    @State private var newConversationProjectId: String?
    @State private var newConversationAgentPubkey: String?
    @State private var currentUserPubkey: String?
    @State private var currentUserDisplayName: String = "You"

    private var currentSection: AppSection {
        selectedSection ?? .chats
    }

    private var onlineProjectsCount: Int {
        coreManager.projects.reduce(into: 0) { count, project in
            if coreManager.projectOnlineStatus[project.id] ?? false {
                count += 1
            }
        }
    }

    private var activeConversationsCount: Int {
        ConversationActivityMetrics.activeConversationCount(conversations: coreManager.conversations)
    }

    private var totalOnlineAgentsCount: Int {
        coreManager.onlineAgents.values.reduce(0) { $0 + $1.count }
    }

    var body: some View {
        stableShell
        .onChange(of: coreManager.lastDeletedProjectId) { _, deletedProjectId in
            guard let deletedProjectId else { return }
            if selectedProjectId == deletedProjectId {
                selectedProjectId = nil
            }
        }
        .task(id: userNpub) {
            await refreshCurrentUserIdentity()
        }
        .task {
            guard !hasCheckedOnboarding else { return }
            hasCheckedOnboarding = true
            try? await Task.sleep(for: .seconds(2))
            if coreManager.projects.isEmpty {
                selectedSection = .projects
                showNewProject = true
            }
        }
    }

    private var appSidebar: some View {
        List {
            Section {
                ForEach(AppSection.allCases.filter { $0 != .teams && $0 != .agentDefinitions && $0 != .nudges && $0 != .settings && $0 != .diagnostics }) { section in
                    shellSidebarRowButton(for: section)
                }
            }

            Section("Browse") {
                shellSidebarRowButton(for: .agentDefinitions)
                shellSidebarRowButton(for: .nudges)
            }

            Section {
                shellSidebarRowButton(for: .settings)

                Menu {
                    if !userNpub.isEmpty {
                        Text(userNpub)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    Divider()

                    Button {
                        selectedSection = .diagnostics
                    } label: {
                        Label("Diagnostics", systemImage: "gauge.with.needle")
                    }

                    Divider()

                    Button(role: .destructive) {
                        Task {
                            _ = await coreManager.clearCredentials()
                            userNpub = ""
                            isLoggedIn = false
                        }
                    } label: {
                        Label("Log Out", systemImage: "rectangle.portrait.and.arrow.right")
                    }
                } label: {
                    HStack(spacing: 10) {
                        AgentAvatarView(
                            agentName: currentUserDisplayName,
                            pubkey: currentUserPubkey,
                            size: 20,
                            showBorder: false
                        )
                        Text(currentUserDisplayName)
                            .lineLimit(1)
                            .truncationMode(.tail)
                        Spacer(minLength: 6)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("sidebar_user_menu")

            }
        }
        .listStyle(.sidebar)
        .accessibilityIdentifier("app_sidebar")
        #if os(macOS)
        .navigationSplitViewColumnWidth(min: 210, ideal: 250, max: 300)
        #endif
    }

    @ViewBuilder
    private func shellSidebarRowButton(for section: AppSection) -> some View {
        Button {
            selectedSection = section
        } label: {
            shellSidebarRow(for: section)
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier(section.accessibilityRowID)
    }

    private var stableShell: some View {
        NavigationSplitView {
            appSidebar
        } detail: {
            sectionContentHost
                .background(Color.systemBackground.ignoresSafeArea())
                .accessibilityIdentifier("section_content_host")
        }
        .navigationSplitViewStyle(.balanced)
    }

    @ViewBuilder
    private func shellSidebarRow(for section: AppSection) -> some View {
        let unansweredAskCount = coreManager.unansweredAskCount
        let projectsOnlineCount = section == .projects ? onlineProjectsCount : 0
        let chatsActiveCount = section == .chats ? activeConversationsCount : 0
        let isSelected = currentSection == section
        let rowTint: Color = isSelected ? .accentColor : .primary

        HStack(spacing: 10) {
            HStack(spacing: 8) {
                Image(systemName: section.systemImage)
                    .symbolRenderingMode(.monochrome)
                    .foregroundStyle(rowTint)
                Text(section.title)
                    .foregroundStyle(rowTint)
                    .fontWeight(isSelected ? .semibold : .regular)
            }

            Spacer(minLength: 8)

            if section == .chats, chatsActiveCount > 0 {
                Text("\(chatsActiveCount)")
                    .font(.caption2.weight(.semibold))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.presenceOnline.opacity(0.16))
                    .foregroundStyle(Color.presenceOnline)
                    .clipShape(Capsule())
            } else if section == .inbox, unansweredAskCount > 0 {
                Text("\(unansweredAskCount)")
                    .font(.caption2.weight(.semibold))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.askBrandBackground)
                    .foregroundStyle(Color.askBrand)
                    .clipShape(Capsule())
            } else if section == .projects {
                Text("\(projectsOnlineCount)")
                    .font(.caption2.weight(.semibold))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background((projectsOnlineCount > 0 ? Color.presenceOnline : .secondary).opacity(0.16))
                    .foregroundStyle(projectsOnlineCount > 0 ? Color.presenceOnline : .secondary)
                    .clipShape(Capsule())
            } else if section == .agents, totalOnlineAgentsCount > 0 {
                Text("\(totalOnlineAgentsCount)")
                    .font(.caption2.weight(.semibold))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.agentBrand.opacity(0.16))
                    .foregroundStyle(Color.agentBrand)
                    .clipShape(Capsule())
            } else if section == .stats {
                Text(runtimeText)
                    .font(.caption2.weight(.semibold))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background((coreManager.hasActiveAgents ? Color.presenceOnline : .secondary).opacity(0.16))
                    .foregroundStyle(coreManager.hasActiveAgents ? Color.presenceOnline : .secondary)
                    .clipShape(Capsule())
            }
        }
    }

    private func refreshCurrentUserIdentity() async {
        let currentUser = await coreManager.safeCore.getCurrentUser()
        let resolvedPubkey = currentUser?.pubkey
            ?? Bech32.npubToHex(userNpub)

        let currentUserName = currentUser?.displayName
            .trimmingCharacters(in: .whitespacesAndNewlines)
        let hasCurrentUserName = (currentUserName?.isEmpty == false)

        let fallbackName: String = {
            if let currentUserName, !currentUserName.isEmpty {
                return currentUserName
            }
            if !userNpub.isEmpty {
                return shortUserDisplay(userNpub)
            }
            return "You"
        }()

        var resolvedName = fallbackName
        if !hasCurrentUserName, let pubkey = resolvedPubkey {
            let profileName = await coreManager.safeCore
                .getProfileName(pubkey: pubkey)
                .trimmingCharacters(in: .whitespacesAndNewlines)
            if !profileName.isEmpty, profileName != pubkey {
                resolvedName = profileName
            }
        }

        currentUserPubkey = resolvedPubkey
        currentUserDisplayName = resolvedName
    }

    private func shortUserDisplay(_ npub: String) -> String {
        guard npub.count > 20 else { return npub }
        return "\(npub.prefix(8))...\(npub.suffix(8))"
    }

    @ViewBuilder
    private var sectionContentHost: some View {
        switch currentSection {
        case .chats:
            #if os(macOS)
            ConversationsTabView(
                layoutMode: .shellComposite,
                selectedConversation: $selectedConversation,
                newConversationProjectId: $newConversationProjectId,
                newConversationAgentPubkey: $newConversationAgentPubkey,
                onShowDiagnosticsInApp: { selectedSection = .diagnostics }
            )
            .accessibilityIdentifier(AppSection.chats.accessibilityContentID)
            #else
            ConversationsTabView(
                layoutMode: .adaptive,
                selectedConversation: $selectedConversation,
                newConversationProjectId: $newConversationProjectId,
                newConversationAgentPubkey: $newConversationAgentPubkey
            )
            .accessibilityIdentifier(AppSection.chats.accessibilityContentID)
            #endif
        case .projects:
            ProjectsTabView(
                layoutMode: .shellComposite,
                selectedProjectId: $selectedProjectId,
                showNewProject: $showNewProject
            )
            .accessibilityIdentifier(AppSection.projects.accessibilityContentID)
        case .agents:
            AgentsTabView(
                layoutMode: .adaptive,
                selectedAgent: $selectedAgentInstance,
                onNavigateToChat: { projectId, agentPubkey in
                    selectedConversation = nil
                    newConversationProjectId = projectId
                    newConversationAgentPubkey = agentPubkey
                    selectedSection = .chats
                }
            )
            .accessibilityIdentifier(AppSection.agents.accessibilityContentID)
        case .reports:
            ReportsTabView(layoutMode: .adaptive, selectedReport: $selectedReport)
                .accessibilityIdentifier(AppSection.reports.accessibilityContentID)
        case .inbox:
            InboxView(
                layoutMode: .adaptive,
                selectedFilter: $selectedInboxFilter,
                selectedItemId: $selectedInboxItemId,
                activeConversationId: $activeInboxConversationId
            )
            .accessibilityIdentifier(AppSection.inbox.accessibilityContentID)
        case .search:
            SearchView(layoutMode: .adaptive, selectedConversation: $selectedSearchConversation)
                .accessibilityIdentifier(AppSection.search.accessibilityContentID)
        case .stats:
            StatsView(isEmbedded: true)
                .accessibilityIdentifier(AppSection.stats.accessibilityContentID)
        case .diagnostics:
            DiagnosticsView(coreManager: coreManager, isEmbedded: true)
                .environment(coreManager)
                .accessibilityIdentifier(AppSection.diagnostics.accessibilityContentID)
        case .teams:
            TeamsTabView(
                layoutMode: .adaptive,
                selectedTeam: $selectedTeam
            )
            .accessibilityIdentifier(AppSection.teams.accessibilityContentID)
        case .agentDefinitions:
            AgentDefinitionsTabView(
                layoutMode: .adaptive,
                selectedAgent: $selectedAgentDefinition,
                onShowAllTeams: {
                    selectedSection = .teams
                }
            )
            .accessibilityIdentifier(AppSection.agentDefinitions.accessibilityContentID)
        case .nudges:
            NudgesTabView(
                layoutMode: .adaptive,
                selectedNudge: $selectedNudge
            )
            .accessibilityIdentifier(AppSection.nudges.accessibilityContentID)
        case .settings:
            AppSettingsView(defaultSection: .relays, isEmbedded: true)
                .accessibilityIdentifier(AppSection.settings.accessibilityContentID)
        }
    }
}
