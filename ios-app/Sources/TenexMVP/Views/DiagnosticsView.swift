import SwiftUI

/// Main Diagnostics view showing system internals
/// 5 tabs: Overview, Sync, Subscriptions, Database, Settings
struct DiagnosticsView: View {
    @StateObject private var viewModel: DiagnosticsViewModel
    private let coreManager: TenexCoreManager

    init(coreManager: TenexCoreManager) {
        self.coreManager = coreManager
        _viewModel = StateObject(wrappedValue: DiagnosticsViewModel(coreManager: coreManager))
    }

    var body: some View {
        VStack(spacing: 0) {
            // Show section errors banner if any sections failed (not for Settings/Bunker tabs)
            if viewModel.selectedTab != .settings,
               viewModel.selectedTab != .bunker,
               let snapshot = viewModel.snapshot,
               !snapshot.sectionErrors.isEmpty {
                DiagnosticsSectionErrorsBanner(errors: snapshot.sectionErrors)
                    .padding(.horizontal)
                    .padding(.top, 8)
            }

            // Tab Navigation - always visible
            DiagnosticsTabNavigation(selectedTab: $viewModel.selectedTab)
                .padding(.horizontal)
                .padding(.top, 8)

            Divider()
                .padding(.top, 8)

            // Tab Content
            if viewModel.selectedTab == .settings {
                AppSettingsView(defaultSection: .audio, isEmbedded: true)
                    .environment(coreManager)
            } else if viewModel.selectedTab == .bunker {
                ScrollView {
                    DiagnosticsBunkerTab(auditEntries: viewModel.bunkerAuditLog)
                        .environment(coreManager)
                        .padding()
                }
            } else {
                ScrollView {
                    if let snapshot = viewModel.snapshot {
                        DiagnosticsTabContent(
                            snapshot: snapshot,
                            selectedTab: viewModel.selectedTab
                        )
                        .padding()
                    }
                }
            }
        }
        .navigationTitle("Diagnostics")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.large)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        .task {
            await viewModel.loadDiagnostics()
        }
        .onDisappear {
            viewModel.cancelFetch()
        }
        .onChange(of: coreManager.diagnosticsVersion) { _, _ in
            Task { await viewModel.loadDiagnostics() }
        }
    }
}

// MARK: - Tab Navigation View

struct DiagnosticsTabNavigation: View {
    @Binding var selectedTab: DiagnosticsTab

    var body: some View {
        MailStyleCategoryPicker(
            cases: DiagnosticsTab.allCases,
            selection: $selectedTab,
            icon: \.icon,
            label: \.rawValue
        )
    }
}

struct DiagnosticsTabPill: View {
    let tab: DiagnosticsTab
    let isSelected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                Image(systemName: tab.icon)
                    .font(.caption)
                Text(tab.rawValue)
                    .font(.subheadline)
                    .fontWeight(isSelected ? .semibold : .regular)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)
            .background(
                isSelected
                    ? Color.agentBrand
                    : Color.systemGray5
            )
            .foregroundColor(isSelected ? .white : .primary)
            .clipShape(Capsule())
        }
        .accessibilityLabel("\(tab.rawValue) tab")
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
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
            case .settings:
                // Settings tab is handled in parent view, this case should not be reached
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
