import SwiftUI

enum TeamsLayoutMode {
    case adaptive
    case shellList
    case shellDetail
}

struct TeamsTabView: View {
    @Environment(TenexCoreManager.self) private var coreManager

    let layoutMode: TeamsLayoutMode
    private let selectedTeamBindingOverride: Binding<TeamInfo?>?

    @StateObject private var viewModel = TeamsViewModel()
    @State private var selectedTeamState: TeamInfo?
    @State private var hasConfiguredViewModel = false
    @State private var navigationPath: [TeamListItem] = []
    @State private var searchText = ""

    init(
        layoutMode: TeamsLayoutMode = .adaptive,
        selectedTeam: Binding<TeamInfo?>? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedTeamBindingOverride = selectedTeam
    }

    private var selectedTeamBinding: Binding<TeamInfo?> {
        selectedTeamBindingOverride ?? $selectedTeamState
    }

    private var query: String {
        searchText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
    }

    private var filteredFeaturedTeams: [TeamListItem] {
        viewModel.featuredTeams.filter(matchesQuery)
    }

    private var filteredCategorySections: [TeamCategorySection] {
        viewModel.categorySections.compactMap { section in
            let filtered = section.teams.filter(matchesQuery)
            guard !filtered.isEmpty else { return nil }
            return TeamCategorySection(title: section.title, teams: filtered)
        }
    }

    var body: some View {
        Group {
            switch layoutMode {
            case .shellList, .adaptive:
                navigationListLayout
            case .shellDetail:
                shellDetailLayout
            }
        }
        .task {
            if !hasConfiguredViewModel {
                viewModel.configure(with: coreManager)
                hasConfiguredViewModel = true
            }
            await viewModel.loadIfNeeded()
        }
        .onChange(of: coreManager.teamsVersion) { _, _ in
            Task { await viewModel.refresh() }
        }
        .alert(
            "Unable to Load Teams",
            isPresented: Binding(
                get: { viewModel.errorMessage != nil },
                set: { isPresented in
                    if !isPresented {
                        viewModel.errorMessage = nil
                    }
                }
            )
        ) {
            Button("OK", role: .cancel) {
                viewModel.errorMessage = nil
            }
        } message: {
            Text(viewModel.errorMessage ?? "Unknown error")
        }
    }

    private var navigationListLayout: some View {
        NavigationStack(path: $navigationPath) {
            listContent
                .navigationTitle("Teams")
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #else
                .toolbarTitleDisplayMode(.inline)
                #endif
                .navigationDestination(for: TeamListItem.self) { item in
                    TeamDetailView(teamId: item.id, viewModel: viewModel)
                }
                .searchable(text: $searchText, placement: .toolbar, prompt: "Search teams")
                .toolbar {
                    ToolbarItem(placement: .automatic) {
                        if viewModel.isLoading {
                            ProgressView()
                                .controlSize(.small)
                        }
                    }

                    ToolbarItem(placement: .automatic) {
                        Button {
                            Task { await viewModel.refresh() }
                        } label: {
                            Label("Refresh", systemImage: "arrow.clockwise")
                        }
                        .disabled(viewModel.isLoading)
                    }
                }
        }
        .accessibilityIdentifier("section_list_column")
    }

    private var shellDetailLayout: some View {
        ContentUnavailableView(
            "Teams",
            systemImage: "person.2",
            description: Text("Select a team from Browse to open details.")
        )
        .accessibilityIdentifier("detail_column")
    }

    private var listContent: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 26) {
                TeamsHeroHeader(
                    totalCount: viewModel.allTeams.count,
                    featuredCount: viewModel.featuredTeams.count
                )

                if viewModel.allTeams.isEmpty, !viewModel.isLoading {
                    ContentUnavailableView(
                        "No Teams Yet",
                        systemImage: "person.2.slash",
                        description: Text("Teams from kind:34199 events will appear here.")
                    )
                    .frame(maxWidth: .infinity, minHeight: 260)
                } else {
                    if !filteredFeaturedTeams.isEmpty {
                        featuredRail
                    }

                    if filteredCategorySections.isEmpty,
                       !query.isEmpty,
                       !viewModel.isLoading {
                        ContentUnavailableView(
                            "No Matching Teams",
                            systemImage: "magnifyingglass",
                            description: Text("Try a different search term.")
                        )
                        .frame(maxWidth: .infinity, minHeight: 180)
                    } else {
                        ForEach(filteredCategorySections) { section in
                            categorySection(section)
                        }
                    }
                }
            }
            .frame(maxWidth: 960, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .center)
            .padding(.horizontal, 18)
            .padding(.vertical, 20)
        }
        .background(Color.systemBackground.ignoresSafeArea())
        #if os(iOS)
        .refreshable {
            await viewModel.refresh()
        }
        #endif
    }

    private var featuredRail: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("Featured Teams")
                    .font(.title3.weight(.semibold))
                Spacer()
                Text("Top by likes")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 14) {
                    ForEach(filteredFeaturedTeams) { item in
                        Button {
                            open(item)
                        } label: {
                            TeamFeaturedCard(item: item)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.vertical, 2)
            }
        }
    }

    private func categorySection(_ section: TeamCategorySection) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(section.title)
                .font(.title3.weight(.semibold))
                .lineLimit(1)

            LazyVGrid(
                columns: [GridItem(.adaptive(minimum: 290), spacing: 14, alignment: .top)],
                alignment: .leading,
                spacing: 14
            ) {
                ForEach(section.teams) { item in
                    Button {
                        open(item)
                    } label: {
                        TeamGridCard(item: item)
                    }
                    .buttonStyle(.plain)
                }
            }
        }
    }

    private func open(_ item: TeamListItem) {
        selectedTeamBinding.wrappedValue = item.team
        navigationPath.append(item)
    }

    private func matchesQuery(_ item: TeamListItem) -> Bool {
        guard !query.isEmpty else { return true }

        let haystacks = [
            item.team.title,
            item.team.description,
            item.authorDisplayName,
            item.team.categories.joined(separator: " "),
            item.team.tags.joined(separator: " ")
        ]

        return haystacks.contains { $0.lowercased().contains(query) }
    }
}
