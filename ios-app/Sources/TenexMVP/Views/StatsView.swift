import SwiftUI

/// Main Stats view with full TUI parity
/// Shows metric cards, charts, rankings, and activity grid
struct StatsView: View {
    @StateObject private var viewModel: StatsViewModel

    init(coreManager: TenexCoreManager) {
        _viewModel = StateObject(wrappedValue: StatsViewModel(coreManager: coreManager))
    }

    var body: some View {
        ScrollView {
            VStack(spacing: 0) {
                // Metric Cards - shows data as it arrives
                if let snapshot = viewModel.snapshot {
                    MetricCardsView(snapshot: snapshot)
                        .padding()

                    Divider()
                }

                // Tab Navigation - always visible
                TabNavigationView(selectedTab: $viewModel.selectedTab)
                    .padding(.horizontal)
                    .padding(.top, 8)

                Divider()
                    .padding(.top, 8)

                // Tab Content - shows data as it arrives
                if let snapshot = viewModel.snapshot {
                    TabContentView(
                        snapshot: snapshot,
                        selectedTab: viewModel.selectedTab
                    )
                    .padding()
                }
            }
        }
        .navigationTitle("Stats")
        .navigationBarTitleDisplayMode(.large)
        .task {
            // Load stats on appear
            await viewModel.loadStats()
        }
    }
}

// MARK: - Tab Navigation View

struct TabNavigationView: View {
    @Binding var selectedTab: StatsTab
    @Environment(\.accessibilityReduceMotion) var reduceMotion

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            let pills = HStack(spacing: 12) {
                ForEach(StatsTab.allCases) { tab in
                    TabPillButton(
                        tab: tab,
                        isSelected: selectedTab == tab,
                        action: {
                            if reduceMotion {
                                selectedTab = tab
                            } else {
                                withAnimation(.easeInOut(duration: 0.2)) {
                                    selectedTab = tab
                                }
                            }
                        }
                    )
                }
            }
            if #available(iOS 26.0, macOS 26.0, *) {
                GlassEffectContainer {
                    pills
                }
            } else {
                pills
            }
        }
    }
}

struct TabPillButton: View {
    @Environment(\.accessibilityReduceTransparency) var reduceTransparency

    let tab: StatsTab
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
            .foregroundStyle(isSelected ? .blue : .primary)
        }
        .adaptiveGlassButtonStyle()
        .clipShape(Capsule())
        .accessibilityLabel("\(tab.rawValue) tab")
        .accessibilityAddTraits(isSelected ? [.isSelected] : [])
    }
}

// MARK: - Tab Content View

struct TabContentView: View {
    let snapshot: StatsSnapshot
    let selectedTab: StatsTab

    var body: some View {
        Group {
            switch selectedTab {
            case .chart:
                RuntimeChartView(snapshot: snapshot)
            case .rankings:
                RankingsView(snapshot: snapshot)
            case .messages:
                MessagesChartView(snapshot: snapshot)
            case .activity:
                ActivityGridView(snapshot: snapshot)
            }
        }
        .transition(.opacity)
    }
}

// MARK: - Error View

struct ErrorView: View {
    let error: Error
    let retry: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "exclamationmark.triangle.fill")
                .font(.system(.largeTitle))
                .foregroundColor(Color.healthWarning)

            Text("Failed to Load Stats")
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

// MARK: - Empty Stats View

struct EmptyStatsView: View {
    let onLoad: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "chart.bar.xaxis")
                .font(.system(.largeTitle))
                .foregroundColor(.secondary)

            Text("No Stats Available")
                .font(.title2)
                .fontWeight(.semibold)

            Text("Tap below to load stats data")
                .font(.subheadline)
                .foregroundColor(.secondary)

            Button(action: onLoad) {
                Label("Load Stats", systemImage: "arrow.clockwise")
            }
            .buttonStyle(.bordered)
            .padding(.top, 8)
        }
        .padding()
    }
}

#Preview {
    StatsView(coreManager: TenexCoreManager())
}
