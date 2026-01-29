import SwiftUI

/// Main Diagnostics view showing system internals
/// 4 tabs: Overview, Sync, Subscriptions, Database
struct DiagnosticsView: View {
    @StateObject private var viewModel: DiagnosticsViewModel

    init(coreManager: TenexCoreManager) {
        _viewModel = StateObject(wrappedValue: DiagnosticsViewModel(coreManager: coreManager))
    }

    var body: some View {
        NavigationStack {
            ZStack {
                if viewModel.isLoading && viewModel.snapshot == nil {
                    // Initial loading state
                    ProgressView("Loading diagnostics...")
                } else if let error = viewModel.error, viewModel.snapshot == nil {
                    // Complete failure error state (no snapshot at all)
                    DiagnosticsErrorView(error: error) {
                        Task {
                            await viewModel.loadDiagnostics()
                        }
                    }
                } else if let snapshot = viewModel.snapshot {
                    // Main content (may have partial data with section errors)
                    ScrollView {
                        VStack(spacing: 0) {
                            // Show section errors banner if any sections failed
                            if !viewModel.sectionErrors.isEmpty {
                                DiagnosticsSectionErrorsBanner(errors: viewModel.sectionErrors)
                                    .padding(.horizontal)
                                    .padding(.top, 8)
                            }

                            // Tab Navigation
                            DiagnosticsTabNavigation(selectedTab: $viewModel.selectedTab)
                                .padding(.horizontal)
                                .padding(.top, 8)

                            Divider()
                                .padding(.top, 8)

                            // Tab Content
                            DiagnosticsTabContent(
                                snapshot: snapshot,
                                selectedTab: viewModel.selectedTab,
                                isLoading: viewModel.isLoading
                            )
                            .padding()
                        }
                    }
                    .refreshable {
                        await viewModel.refresh()
                    }
                } else {
                    // Empty state
                    DiagnosticsEmptyView {
                        Task {
                            await viewModel.loadDiagnostics()
                        }
                    }
                }
            }
            .navigationTitle("Diagnostics")
            .navigationBarTitleDisplayMode(.large)
            .task {
                await viewModel.loadDiagnostics()
            }
            .onDisappear {
                viewModel.cancelFetch()
            }
        }
    }
}

// MARK: - Tab Navigation View

struct DiagnosticsTabNavigation: View {
    @Binding var selectedTab: DiagnosticsTab

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 12) {
                ForEach(DiagnosticsTab.allCases) { tab in
                    DiagnosticsTabPill(
                        tab: tab,
                        isSelected: selectedTab == tab,
                        action: {
                            withAnimation(.easeInOut(duration: 0.2)) {
                                selectedTab = tab
                            }
                        }
                    )
                }
            }
        }
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
                    ? Color.blue
                    : Color(.systemGray5)
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
    let isLoading: Bool

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
                if let subscriptions = snapshot.subscriptions {
                    DiagnosticsSubscriptionsTab(
                        subscriptions: subscriptions,
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
                } else if isLoading {
                    // Database stats are being loaded (lazy loading)
                    VStack(spacing: 16) {
                        ProgressView()
                        Text("Loading database statistics...")
                            .font(.subheadline)
                            .foregroundColor(.secondary)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 40)
                } else {
                    DiagnosticsSectionUnavailableView(
                        title: "Database Data Unavailable",
                        message: "Unable to load database statistics",
                        icon: "cylinder.split.1x2"
                    )
                }
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
                .font(.system(size: 48))
                .foregroundColor(.orange)

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
                        .foregroundColor(.orange)

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
            .buttonStyle(.plain)

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
        .background(Color.orange.opacity(0.1))
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color.orange.opacity(0.3), lineWidth: 1)
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
                .font(.system(size: 60))
                .foregroundColor(.orange)

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
                .font(.system(size: 60))
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
