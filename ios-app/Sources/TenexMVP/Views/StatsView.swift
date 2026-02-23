import SwiftUI

/// Main Stats view with full TUI parity
/// Uses section navigation (sidebar/list) with metric cards + section detail content.
struct StatsView: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.dismiss) private var dismiss
    @StateObject private var viewModel = StatsViewModel()
    @State private var selectedSection: StatsTab?
    @State private var phonePath: [StatsTab] = []
    let defaultSection: StatsTab
    let isEmbedded: Bool

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    init(defaultSection: StatsTab = .rankings, isEmbedded: Bool = false) {
        self.defaultSection = defaultSection
        self.isEmbedded = isEmbedded
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
                embeddedStatsView
            } else if useSplitLayout {
                splitStatsView
            } else {
                phoneStatsView
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
            if let selectedSection {
                viewModel.selectedTab = selectedSection
            }
            await viewModel.loadStats()
        }
        .onChange(of: coreManager.statsVersion) { _, _ in
            Task { await viewModel.loadStats() }
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

    private var sectionListSelection: Binding<StatsTab?> {
        Binding(
            get: { selectedSection },
            set: { selectedSection = $0 }
        )
    }

    private var statsCategoryList: some View {
        List(StatsTab.allCases, selection: sectionListSelection) { section in
            NavigationLink(value: section) {
                Label(section.rawValue, systemImage: section.icon)
            }
        }
        #if os(macOS)
        .listStyle(.sidebar)
        #endif
    }

    @ViewBuilder
    private var statsDetailContent: some View {
        if let section = selectedSection {
            sectionContent(section)
        } else {
            ContentUnavailableView("Select a Section", systemImage: "clock")
        }
    }

    private var embeddedStatsView: some View {
        #if os(macOS)
        HSplitView {
            statsCategoryList
                .frame(minWidth: 160, idealWidth: 190, maxWidth: 240)

            statsDetailContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .onAppear {
            if selectedSection == nil {
                selectedSection = defaultSection
            }
        }
        #else
        HStack(spacing: 0) {
            statsCategoryList
                .frame(minWidth: 160, idealWidth: 190, maxWidth: 240)
            Divider()
            statsDetailContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .onAppear {
            if selectedSection == nil {
                selectedSection = defaultSection
            }
        }
        #endif
    }

    private var splitStatsView: some View {
        NavigationSplitView {
            statsCategoryList
        } detail: {
            statsDetailContent
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

    private var phoneStatsView: some View {
        NavigationStack(path: $phonePath) {
            List(StatsTab.allCases) { section in
                NavigationLink(value: section) {
                    Label(section.rawValue, systemImage: section.icon)
                }
            }
            .navigationDestination(for: StatsTab.self) { section in
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
    private func sectionContent(_ section: StatsTab) -> some View {
        ScrollView {
            if let snapshot = viewModel.snapshot {
                VStack(spacing: 0) {
                    if section == .rankings {
                        MetricCardsView(snapshot: snapshot)
                            .padding()

                        Divider()
                    }

                    TabContentView(
                        snapshot: snapshot,
                        selectedTab: section
                    )
                    .padding()
                }
            } else if let error = viewModel.error {
                ErrorView(error: error) {
                    Task { await viewModel.loadStats() }
                }
            } else if viewModel.isLoading {
                ProgressView("Loading stats...")
                    .frame(maxWidth: .infinity, minHeight: 220)
                    .padding()
            } else {
                EmptyStatsView {
                    Task { await viewModel.loadStats() }
                }
            }
        }
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
