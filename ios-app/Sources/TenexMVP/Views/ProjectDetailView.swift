import SwiftUI

// MARK: - Feed Item

private enum ProjectFeedItem: Identifiable {
    case conversation(ConversationFullInfo)
    case htmlReport(HtmlReportVersionEntry)
    case markdownReport(Report)

    var id: String {
        switch self {
        case .conversation(let c): return "conv:\(c.thread.id)"
        case .htmlReport(let e): return "html:\(e.id)"
        case .markdownReport(let r): return "md:\(r.id)"
        }
    }

    var timestamp: UInt64 {
        switch self {
        case .conversation(let c): return c.thread.effectiveLastActivity
        case .htmlReport(let e): return e.latest.createdAt
        case .markdownReport(let r): return r.createdAt
        }
    }
}

// MARK: - ProjectDetailView

struct ProjectDetailView: View {
    @Environment(TenexCoreManager.self) private var coreManager

    let projectId: String
    @Binding var selectedProjectId: String?

    @State private var conversationHierarchy = ConversationFullHierarchy(conversations: [])
    @State private var showNewConversation = false
    @State private var isBooting = false
    @State private var showBootError = false
    @State private var bootError: String?
    private var audioPlayer: AudioNotificationPlayer { AudioNotificationPlayer.shared }

    private var project: Project? {
        coreManager.projects.first { $0.id == projectId }
    }

    private var isOnline: Bool {
        coreManager.projectOnlineStatus[projectId] ?? false
    }

    private var projectConversations: [ConversationFullInfo] {
        coreManager.conversations.filter { conv in
            TenexCoreManager.projectId(fromATag: conv.projectATag) == projectId && !conv.isArchived
        }
    }

    private var projectReports: [Report] {
        coreManager.reports.filter { report in
            TenexCoreManager.projectId(fromATag: report.projectATag) == projectId
        }
    }

    private var projectHtmlReports: [HtmlReport] {
        coreManager.htmlReports.filter { report in
            TenexCoreManager.projectId(fromATag: report.projectATag) == projectId
        }
    }

    private var feedItems: [ProjectFeedItem] {
        var items: [ProjectFeedItem] = []

        for conv in projectConversations {
            items.append(.conversation(conv))
        }
        for entry in HtmlReportVersionEntry.grouped(from: projectHtmlReports) {
            items.append(.htmlReport(entry))
        }
        for report in projectReports {
            items.append(.markdownReport(report))
        }

        return items.sorted { $0.timestamp > $1.timestamp }
    }

    var body: some View {
        Group {
            if feedItems.isEmpty {
                emptyStateView
            } else {
                List {
                    ForEach(feedItems) { item in
                        itemRow(for: item)
                    }
                }
                #if os(iOS)
                .listStyle(.plain)
                #else
                .listStyle(.inset)
                #endif
                .refreshable {
                    await coreManager.manualRefresh()
                }
            }
        }
        .navigationTitle(project?.title ?? "Project")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        .task(id: projectConversations.map(\.thread.id)) {
            conversationHierarchy = ConversationFullHierarchy(conversations: projectConversations)
            await coreManager.hierarchyCache.preloadForConversations(projectConversations)
        }
        .navigationDestination(isPresented: $showNewConversation) {
            if let project {
                ConversationWorkspaceView(
                    source: .newThread(
                        project: project,
                        agentPubkey: UserDefaults.standard.string(forKey: "tenex.lastAgent.\(projectId)")
                    )
                )
                .environment(coreManager)
                #if os(iOS)
                .toolbar(.hidden, for: .tabBar)
                #endif
            }
        }
        .alert("Boot Failed", isPresented: $showBootError) {
            Button("OK") { bootError = nil }
        } message: {
            if let bootError { Text(bootError) }
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                NavigationLink {
                    ProjectSettingsView(
                        projectId: projectId,
                        selectedProjectId: $selectedProjectId
                    )
                } label: {
                    Image(systemName: "gearshape")
                }
            }
            ToolbarItem(placement: .primaryAction) {
                Button {
                    showNewConversation = true
                } label: {
                    Image(systemName: "square.and.pencil")
                }
            }
            if !isOnline {
                ToolbarItem(placement: .primaryAction) {
                    Button(action: bootProject) {
                        if isBooting {
                            ProgressView()
                                .scaleEffect(0.8)
                        } else {
                            Image(systemName: "power")
                                .foregroundStyle(Color.agentBrand)
                        }
                    }
                    .disabled(isBooting)
                }
            }
        }
    }

    private func bootProject() {
        isBooting = true
        bootError = nil
        Task {
            do {
                try await coreManager.core.bootProject(projectId: projectId)
            } catch {
                await MainActor.run {
                    bootError = error.localizedDescription
                    showBootError = true
                }
            }
            await MainActor.run { isBooting = false }
        }
    }

    @ViewBuilder
    private func itemRow(for item: ProjectFeedItem) -> some View {
        switch item {
        case .conversation(let conversation):
            let hierarchy = coreManager.hierarchyCache.getHierarchy(for: conversation.thread.id)
            NavigationLink {
                ConversationAdaptiveDetailView(conversation: conversation)
                    .environment(coreManager)
                    #if os(iOS)
                    .toolbar(.hidden, for: .tabBar)
                    #endif
            } label: {
                ConversationRowFull(
                    conversation: conversation,
                    projectTitle: nil,
                    isHierarchicallyActive: conversationHierarchy.isHierarchicallyActive(conversation.id),
                    pTaggedRecipientInfo: hierarchy?.pTaggedRecipientInfo,
                    delegationAgentInfos: hierarchy?.delegationAgentInfos ?? [],
                    isPlayingAudio: audioPlayer.playbackState != .idle && audioPlayer.currentConversationId == conversation.thread.id,
                    isAudioPlaying: audioPlayer.isPlaying,
                    showsChevron: false,
                    onSelect: nil,
                    onToggleArchive: nil
                )
                .equatable()
            }

        case .htmlReport(let entry):
            NavigationLink {
                HtmlReportDetailView(report: entry.latest, versions: entry.versions)
                    .environment(coreManager)
            } label: {
                HtmlReportRowView(report: entry.latest, project: nil, versionCount: entry.versions.count)
            }

        case .markdownReport(let report):
            NavigationLink {
                ReportDetailView(report: report)
                    .environment(coreManager)
            } label: {
                ReportRowView(report: report, project: nil)
            }
        }
    }

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Image(systemName: "tray")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("Nothing Here Yet")
                .font(.title2)
                .fontWeight(.semibold)
            Text("Chats and reports for this project will appear here.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

