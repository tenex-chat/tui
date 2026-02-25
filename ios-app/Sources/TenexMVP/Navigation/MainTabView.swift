import SwiftUI

enum AppSection: String, CaseIterable, Identifiable {
    case chats
    case projects
    case reports
    case inbox
    case search
    case stats
    case diagnostics
    case teams
    case agentDefinitions
    case nudges
    case settings

    var id: String { rawValue }

    var title: String {
        switch self {
        case .chats: return "Chats"
        case .projects: return "Projects"
        case .reports: return "Reports"
        case .inbox: return "Inbox"
        case .search: return "Search"
        case .stats: return "LLM Runtime"
        case .diagnostics: return "Diagnostics"
        case .teams: return "Teams"
        case .agentDefinitions: return "Agent Definitions"
        case .nudges: return "Nudges"
        case .settings: return "Settings"
        }
    }

    var systemImage: String {
        switch self {
        case .chats: return "bubble.left.and.bubble.right"
        case .projects: return "folder"
        case .reports: return "doc.richtext"
        case .inbox: return "tray"
        case .search: return "magnifyingglass"
        case .stats: return "clock"
        case .diagnostics: return "gauge.with.needle"
        case .teams: return "person.2"
        case .agentDefinitions: return "person.3.sequence"
        case .nudges: return "forward.circle"
        case .settings: return "gearshape"
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
    @State private var showOnboarding = false
    @State private var hasCheckedOnboarding = false

    #if os(iOS)
    private var useSidebarAdaptableTabs: Bool {
        if #available(iOS 26.0, *) {
            return horizontalSizeClass == .regular
        } else {
            return false
        }
    }
    #endif

    private var useMailShellLayout: Bool {
        #if os(macOS)
        true
        #else
        horizontalSizeClass == .regular
        #endif
    }

    var body: some View {
        Group {
            #if os(macOS)
            MainShellView(
                userNpub: $userNpub,
                isLoggedIn: $isLoggedIn,
                runtimeText: coreManager.runtimeText
            )
            .environment(coreManager)
            .nowPlayingInset(coreManager: coreManager)
            #else
            if useSidebarAdaptableTabs {
                if #available(iOS 26.0, *) {
                    sidebarAdaptableTabView
                } else {
                    compactTabView
                }
            } else if useMailShellLayout {
                MainShellView(
                    userNpub: $userNpub,
                    isLoggedIn: $isLoggedIn,
                    runtimeText: coreManager.runtimeText
                )
                .environment(coreManager)
                .nowPlayingInset(coreManager: coreManager)
            } else {
                compactTabView
            }
            #endif
        }
        .sheet(item: bunkerRequestBinding) { request in
            BunkerApprovalSheet(request: request) {
                coreManager.pendingBunkerRequests.removeAll { $0.requestId == request.requestId }
            }
            .environment(coreManager)
        }
        #if os(iOS)
        .sheet(isPresented: $showOnboarding) {
            NavigationStack {
                CreateProjectView(onComplete: { showOnboarding = false })
            }
            .environment(coreManager)
        }
        .task {
            guard !hasCheckedOnboarding else { return }
            hasCheckedOnboarding = true
            // Only auto-show on compact iPhone; shell layouts handle it inline
            guard !useMailShellLayout && !useSidebarAdaptableTabs else { return }
            try? await Task.sleep(for: .seconds(2))
            if coreManager.projects.isEmpty {
                showOnboarding = true
            }
        }
        #endif
        .ignoresSafeArea(.keyboard)
    }

    private var bunkerRequestBinding: Binding<FfiBunkerSignRequest?> {
        Binding(
            get: { coreManager.pendingBunkerRequests.first },
            set: { newValue in
                if newValue == nil, let first = coreManager.pendingBunkerRequests.first {
                    coreManager.rejectBunkerRequest(requestId: first.requestId)
                }
            }
        )
    }

    private var compactTabView: some View {
        TabView(selection: $selectedTab) {
            Tab("Chats", systemImage: "bubble.left.and.bubble.right", value: 0) {
                ConversationsTabView()
                    .environment(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }

            Tab("Projects", systemImage: "folder", value: 1) {
                ProjectsTabView(layoutMode: .adaptive)
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

    #if os(iOS)
    @available(iOS 26.0, *)
    private var sidebarAdaptableTabView: some View {
        TabView(selection: $selectedTab) {
            Tab("Chats", systemImage: "bubble.left.and.bubble.right", value: 0) {
                ConversationsTabView(layoutMode: .adaptive)
                    .environment(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }

            Tab("Projects", systemImage: "folder", value: 1) {
                ProjectsTabView(layoutMode: .adaptive)
                    .environment(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }

            Tab("Reports", systemImage: "doc.richtext", value: 4) {
                ReportsTabView(layoutMode: .adaptive)
                    .environment(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }

            Tab("Inbox", systemImage: "tray", value: 3) {
                InboxView(layoutMode: .adaptive)
                    .environment(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }
            .badge(coreManager.unansweredAskCount)

            Tab("LLM Runtime", systemImage: "clock", value: 2) {
                StatsView(isEmbedded: false)
                    .environment(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }

            Tab("Teams", systemImage: "person.2", value: 5) {
                TeamsTabView(layoutMode: .adaptive)
                    .environment(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }

            Tab("Agent Definitions", systemImage: "person.3.sequence", value: 6) {
                AgentDefinitionsTabView(layoutMode: .adaptive)
                    .environment(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }

            Tab("Nudges", systemImage: "forward.circle", value: 7) {
                NudgesTabView(layoutMode: .adaptive)
                    .environment(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }

            TabSection("More") {
                Tab("Settings", systemImage: "gearshape", value: 8) {
                    AppSettingsView(defaultSection: .relays, isEmbedded: false)
                        .environment(coreManager)
                        .nowPlayingInset(coreManager: coreManager)
                }

                Tab("Diagnostics", systemImage: "gauge.with.needle", value: 9) {
                    DiagnosticsView(coreManager: coreManager, isEmbedded: false)
                        .environment(coreManager)
                        .nowPlayingInset(coreManager: coreManager)
                }
            }

            Tab(value: 10, role: .search) {
                SearchView(layoutMode: .adaptive)
                    .environment(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            } label: {
                Label("Search", systemImage: "magnifyingglass")
            }
        }
        .tabViewStyle(.sidebarAdaptable)
        .tabBarMinimizeBehavior(.onScrollDown)
    }
    #endif
}
