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
                    runtimeText: coreManager.runtimeText
                )
                .environment(coreManager)
                .nowPlayingInset(coreManager: coreManager)
            } else {
                compactTabView
            }
        }
        .sheet(item: bunkerRequestBinding) { request in
            BunkerApprovalSheet(request: request) {
                coreManager.pendingBunkerRequests.removeAll { $0.requestId == request.requestId }
            }
            .environment(coreManager)
        }
        .sheet(isPresented: $showOnboarding) {
            OnboardingWizardSheet()
                .environment(coreManager)
        }
        .task {
            guard !hasCheckedOnboarding else { return }
            hasCheckedOnboarding = true
            // Short delay to let initial fetchData populate projects
            try? await Task.sleep(for: .seconds(2))
            if coreManager.projects.isEmpty {
                showOnboarding = true
            }
        }
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
}
