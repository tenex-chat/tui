import SwiftUI

/// Main Diagnostics view showing system internals
/// Uses section navigation (sidebar/list) for Overview, Sync, Subscriptions, Database, and Bunker.
struct DiagnosticsView: View {
    @Environment(\.dismiss) private var dismiss
    @StateObject private var viewModel: DiagnosticsViewModel
    private let coreManager: TenexCoreManager
    @State private var selectedSection: DiagnosticsTab?
    @State private var phonePath: [DiagnosticsTab] = []
    let defaultSection: DiagnosticsTab
    let isEmbedded: Bool

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    init(
        coreManager: TenexCoreManager,
        defaultSection: DiagnosticsTab = .overview,
        isEmbedded: Bool = false
    ) {
        self.coreManager = coreManager
        self.defaultSection = defaultSection
        self.isEmbedded = isEmbedded
        _viewModel = StateObject(wrappedValue: DiagnosticsViewModel(coreManager: coreManager))
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
                embeddedDiagnosticsView
            } else if useSplitLayout {
                splitDiagnosticsView
            } else {
                phoneDiagnosticsView
            }
        }
        .navigationTitle("Diagnostics")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.large)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        .task {
            if let selectedSection {
                viewModel.selectedTab = selectedSection
            }
            await viewModel.loadDiagnostics(includeDatabaseStats: viewModel.selectedTab == .database)
        }
        .onDisappear {
            viewModel.cancelFetch()
        }
        .onChange(of: coreManager.diagnosticsVersion) { _, _ in
            Task { await viewModel.handleDiagnosticsVersionUpdate() }
        }
        .onChange(of: selectedSection) { _, section in
            guard let section, viewModel.selectedTab != section else { return }
            viewModel.selectedTab = section
        }
        .onChange(of: viewModel.selectedTab) { _, tab in
            if selectedSection != tab {
                selectedSection = tab
            }
        }
    }

    private var sectionListSelection: Binding<DiagnosticsTab?> {
        Binding(
            get: { selectedSection },
            set: { selectedSection = $0 }
        )
    }

    private var diagnosticsCategoryList: some View {
        List(DiagnosticsTab.allCases, selection: sectionListSelection) { section in
            NavigationLink(value: section) {
                Label(section.rawValue, systemImage: section.icon)
            }
        }
        #if os(macOS)
        .listStyle(.sidebar)
        #endif
    }

    @ViewBuilder
    private var diagnosticsDetailContent: some View {
        if let section = selectedSection {
            sectionContent(section)
        } else {
            ContentUnavailableView("Select a Section", systemImage: "gauge.with.needle")
        }
    }

    private var embeddedDiagnosticsView: some View {
        #if os(macOS)
        HSplitView {
            diagnosticsCategoryList
                .frame(minWidth: 160, idealWidth: 190, maxWidth: 240)

            diagnosticsDetailContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .onAppear {
            if selectedSection == nil {
                selectedSection = defaultSection
            }
        }
        #else
        HStack(spacing: 0) {
            diagnosticsCategoryList
                .frame(minWidth: 160, idealWidth: 190, maxWidth: 240)
            Divider()
            diagnosticsDetailContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .onAppear {
            if selectedSection == nil {
                selectedSection = defaultSection
            }
        }
        #endif
    }

    private var splitDiagnosticsView: some View {
        NavigationSplitView {
            diagnosticsCategoryList
        } detail: {
            diagnosticsDetailContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
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

    private var phoneDiagnosticsView: some View {
        NavigationStack(path: $phonePath) {
            List(DiagnosticsTab.allCases) { section in
                NavigationLink(value: section) {
                    Label(section.rawValue, systemImage: section.icon)
                }
            }
            .navigationDestination(for: DiagnosticsTab.self) { section in
                sectionContent(section)
                    .navigationTitle(section.rawValue)
                    .onAppear {
                        selectedSection = section
                    }
            }
            .onAppear {
                if phonePath.isEmpty {
                    phonePath = [defaultSection]
                }
            }
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }

    @ViewBuilder
    private func sectionContent(_ section: DiagnosticsTab) -> some View {
        switch section {
        case .bunker:
            ScrollView {
                DiagnosticsBunkerTab(auditEntries: viewModel.bunkerAuditLog)
                    .environment(coreManager)
                    .padding()
            }
        default:
            ScrollView {
                VStack(spacing: 0) {
                    if let snapshot = viewModel.snapshot,
                       !snapshot.sectionErrors.isEmpty {
                        DiagnosticsSectionErrorsBanner(errors: snapshot.sectionErrors)
                            .padding(.horizontal)
                            .padding(.top, 8)
                    }

                    if let snapshot = viewModel.snapshot {
                        DiagnosticsTabContent(
                            snapshot: snapshot,
                            selectedTab: section
                        )
                        .padding()
                    } else if let error = viewModel.error {
                        DiagnosticsErrorView(error: error) {
                            Task {
                                await viewModel.loadDiagnostics(includeDatabaseStats: section == .database)
                            }
                        }
                    } else if viewModel.isLoading {
                        ProgressView("Loading diagnostics...")
                            .frame(maxWidth: .infinity, minHeight: 220)
                            .padding()
                    } else {
                        DiagnosticsEmptyView {
                            Task {
                                await viewModel.loadDiagnostics(includeDatabaseStats: section == .database)
                            }
                        }
                    }
                }
            }
        }
    }
}

// MARK: - Tab Content View

struct DiagnosticsTabContent: View {
    let snapshot: DiagnosticsSnapshot
    let selectedTab: DiagnosticsTab

    var body: some View {
        Group {
            switch selectedTab {
            case .overview:
                DiagnosticsOverviewTab(snapshot: snapshot)
            case .sync:
                if let sync = snapshot.sync {
                    DiagnosticsSyncTab(syncData: sync)
                } else {
                    DiagnosticsSectionUnavailableView(
                        title: "Sync Data Unavailable",
                        message: "Unable to load negentropy sync statistics",
                        icon: "arrow.triangle.2.circlepath.slash"
                    )
                }
            case .subscriptions:
                if snapshot.subscriptions != nil {
                    DiagnosticsSubscriptionsTab(
                        subscriptions: snapshot.sortedSubscriptions,
                        totalEvents: snapshot.totalSubscriptionEvents
                    )
                } else {
                    DiagnosticsSectionUnavailableView(
                        title: "Subscription Data Unavailable",
                        message: "Unable to load subscription statistics",
                        icon: "antenna.radiowaves.left.and.right.slash"
                    )
                }
            case .database:
                if let database = snapshot.database {
                    DiagnosticsDatabaseTab(dbData: database)
                } else {
                    DiagnosticsSectionUnavailableView(
                        title: "Database Data Unavailable",
                        message: "Database stats load on demand and should appear shortly",
                        icon: "cylinder.split.1x2"
                    )
                }
            case .bunker:
                // Bunker tab is handled in parent view, this case should not be reached
                EmptyView()
            }
        }
        .transition(.opacity)
    }
}

// MARK: - Section Unavailable View

struct DiagnosticsSectionUnavailableView: View {
    let title: String
    let message: String
    let icon: String

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: icon)
                .font(.system(.largeTitle))
                .foregroundColor(Color.healthWarning)

            Text(title)
                .font(.headline)
                .foregroundColor(.primary)

            Text(message)
                .font(.subheadline)
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 40)
    }
}

// MARK: - Section Errors Banner

struct DiagnosticsSectionErrorsBanner: View {
    let errors: [String]
    @State private var isExpanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Button(action: { withAnimation { isExpanded.toggle() } }) {
                HStack {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundColor(Color.healthWarning)

                    Text("Some sections failed to load")
                        .font(.subheadline)
                        .fontWeight(.medium)
                        .foregroundColor(.primary)

                    Spacer()

                    Image(systemName: "chevron.down")
                        .font(.caption)
                        .foregroundColor(.secondary)
                        .rotationEffect(.degrees(isExpanded ? 180 : 0))
                }
                .padding(12)
            }
            .buttonStyle(.borderless)

            if isExpanded {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(errors, id: \.self) { error in
                        Text("• \(error)")
                            .font(.caption)
                            .foregroundColor(.secondary)
                    }
                }
                .padding(.horizontal, 12)
                .padding(.bottom, 12)
            }
        }
        .background(Color.healthWarning.opacity(0.1))
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color.healthWarning.opacity(0.3), lineWidth: 1)
        )
    }
}

// MARK: - Error View

struct DiagnosticsErrorView: View {
    let error: Error
    let retry: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(.largeTitle))
                .foregroundColor(Color.healthWarning)

            Text("Failed to Load Diagnostics")
                .font(.title2)
                .fontWeight(.semibold)

            Text(error.localizedDescription)
                .font(.subheadline)
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal)

            Button(action: retry) {
                Label("Retry", systemImage: "arrow.clockwise")
            }
            .buttonStyle(.bordered)
            .padding(.top, 8)
        }
        .padding()
    }
}

// MARK: - Empty View

struct DiagnosticsEmptyView: View {
    let onLoad: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "gauge.with.needle")
                .font(.system(.largeTitle))
                .foregroundColor(.secondary)

            Text("No Diagnostics Available")
                .font(.title2)
                .fontWeight(.semibold)

            Text("Tap below to load diagnostic data")
                .font(.subheadline)
                .foregroundColor(.secondary)

            Button(action: onLoad) {
                Label("Load Diagnostics", systemImage: "arrow.clockwise")
            }
            .buttonStyle(.bordered)
            .padding(.top, 8)
        }
        .padding()
    }
}

#Preview {
    DiagnosticsView(coreManager: TenexCoreManager())
}
