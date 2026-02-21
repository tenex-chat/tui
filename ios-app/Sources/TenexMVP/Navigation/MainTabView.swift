import SwiftUI

enum AppSection: String, CaseIterable, Identifiable {
    case chats
    case projects
    case reports
    case inbox
    case search
    case teams
    case agentDefinitions

    var id: String { rawValue }

    var title: String {
        switch self {
        case .chats: return "Chats"
        case .projects: return "Projects"
        case .reports: return "Reports"
        case .inbox: return "Inbox"
        case .search: return "Search"
        case .teams: return "Teams"
        case .agentDefinitions: return "Agent Definitions"
        }
    }

    var systemImage: String {
        switch self {
        case .chats: return "bubble.left.and.bubble.right"
        case .projects: return "folder"
        case .reports: return "doc.richtext"
        case .inbox: return "tray"
        case .search: return "magnifyingglass"
        case .teams: return "person.2"
        case .agentDefinitions: return "person.3.sequence"
        }
    }

    var accessibilityRowID: String {
        "section_row_\(rawValue)"
    }

    var accessibilityContentID: String {
        "section_content_\(rawValue)"
    }
}

struct MainTabView: View {
    @Binding var userNpub: String
    @Binding var isLoggedIn: Bool
    @Environment(TenexCoreManager.self) var coreManager

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    @State private var selectedTab = 0
    @State private var showAISettings = false
    @State private var showDiagnostics = false
    @State private var showStats = false

    private var useMailShellLayout: Bool {
        #if os(macOS)
        true
        #else
        horizontalSizeClass == .regular
        #endif
    }

    var body: some View {
        Group {
            if useMailShellLayout {
                MainShellView(
                    userNpub: $userNpub,
                    isLoggedIn: $isLoggedIn,
                    runtimeText: coreManager.runtimeText,
                    onShowSettings: { showAISettings = true },
                    onShowDiagnostics: { showDiagnostics = true },
                    onShowStats: { showStats = true }
                )
                .environment(coreManager)
                .nowPlayingInset(coreManager: coreManager)
            } else {
                compactTabView
            }
        }
        .sheet(isPresented: $showAISettings) {
            AppSettingsView(defaultSection: .audio)
                .tenexModalPresentation(detents: [.large])
                #if os(macOS)
                .frame(minWidth: 500, idealWidth: 520, minHeight: 500, idealHeight: 600)
                #endif
        }
        .sheet(isPresented: $showDiagnostics) {
            NavigationStack {
                DiagnosticsView(coreManager: coreManager)
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) {
                            Button("Done") { showDiagnostics = false }
                        }
                    }
            }
            .tenexModalPresentation(detents: [.large])
        }
        .sheet(isPresented: $showStats) {
            NavigationStack {
                StatsView(coreManager: coreManager)
                    .toolbar {
                        ToolbarItem(placement: .cancellationAction) {
                            Button("Done") { showStats = false }
                        }
                    }
            }
            .tenexModalPresentation(detents: [.large])
        }
        .ignoresSafeArea(.keyboard)
    }

    private var compactTabView: some View {
        TabView(selection: $selectedTab) {
            Tab("Chats", systemImage: "bubble.left.and.bubble.right", value: 0) {
                ConversationsTabView()
                    .environment(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }

            Tab("Projects", systemImage: "folder", value: 1) {
                ProjectsTabView()
                    .environment(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }

            Tab("Reports", systemImage: "doc.richtext", value: 4) {
                ReportsTabView()
                    .environment(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }

            Tab("Inbox", systemImage: "tray", value: 3) {
                InboxView()
                    .environment(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }
            .badge(coreManager.unansweredAskCount)

            Tab(value: 10, role: .search) {
                SearchView()
                    .environment(coreManager)
                .nowPlayingInset(coreManager: coreManager)
            } label: {
                Label("Search", systemImage: "magnifyingglass")
            }
        }
        #if os(iOS)
        .tabBarMinimizeBehavior(.onScrollDown)
        #endif
    }
}
