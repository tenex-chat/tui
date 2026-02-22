import SwiftUI

/// Main Stats view with full TUI parity
/// Shows metric cards, charts, rankings, and activity grid
struct StatsView: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @StateObject private var viewModel = StatsViewModel()

    var body: some View {
        ScrollView {
            VStack(spacing: 0) {
                if let snapshot = viewModel.snapshot {
                    MetricCardsView(snapshot: snapshot)
                        .padding()

                    Divider()
                }

                StatsTabNavigationView(selectedTab: $viewModel.selectedTab)
                    .padding(.horizontal)
                    .padding(.top, 8)

                Divider()
                    .padding(.top, 8)

                if let snapshot = viewModel.snapshot {
                    TabContentView(
                        snapshot: snapshot,
                        selectedTab: viewModel.selectedTab
                    )
                    .padding()
                }
            }
        }
        .navigationTitle("LLM Runtime")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.large)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        .task {
            viewModel.configure(with: coreManager)
            await viewModel.loadStats()
        }
        .onChange(of: coreManager.statsVersion) { _, _ in
            Task { await viewModel.loadStats() }
        }
    }
}

// MARK: - Mail.app-style Tab Navigation

struct StatsTabNavigationView: View {
    @Binding var selectedTab: StatsTab
    @Environment(\.accessibilityReduceMotion) var reduceMotion

    var body: some View {
        MailStyleCategoryPicker(
            cases: StatsTab.allCases,
            selection: $selectedTab,
            icon: \.icon,
            label: \.rawValue
        )
    }
}

/// Reusable Mail.app-style category picker.
/// Unselected items show icon-only in subtle circular pills.
/// Selected item shows icon + text in a prominent capsule.
struct MailStyleCategoryPicker<T: Identifiable & Hashable>: View {
    let cases: [T]
    @Binding var selection: T
    let icon: KeyPath<T, String>
    let label: KeyPath<T, String>
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            let pills = HStack(spacing: 8) {
                ForEach(cases) { item in
                    let isSelected = selection.id == item.id
                    Button {
                        if reduceMotion {
                            selection = item
                        } else {
                            withAnimation(.easeInOut(duration: 0.2)) {
                                selection = item
                            }
                        }
                    } label: {
                        HStack(spacing: 6) {
                            Image(systemName: item[keyPath: icon])
                                .font(.body)

                            if isSelected {
                                Text(item[keyPath: label])
                                    .font(.subheadline)
                                    .fontWeight(.semibold)
                            }
                        }
                        .padding(.horizontal, isSelected ? 14 : 10)
                        .padding(.vertical, 8)
                        .foregroundStyle(isSelected ? Color.accentColor : .secondary)
                        .background(
                            isSelected
                                ? Color.accentColor.opacity(0.15)
                                : Color.secondary.opacity(0.08),
                            in: Capsule()
                        )
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("\(item[keyPath: label]) tab")
                    .accessibilityAddTraits(isSelected ? [.isSelected] : [])
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

// MARK: - Tab Content View

struct TabContentView: View {
    let snapshot: StatsSnapshot
    let selectedTab: StatsTab

    var body: some View {
        Group {
            switch selectedTab {
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
    StatsView()
        .environment(TenexCoreManager())
}
