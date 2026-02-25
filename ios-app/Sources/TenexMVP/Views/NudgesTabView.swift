import SwiftUI

enum NudgesLayoutMode {
    case adaptive
    case shellList
    case shellDetail
}

struct NudgesTabView: View {
    @Environment(TenexCoreManager.self) private var coreManager

    let layoutMode: NudgesLayoutMode
    private let selectedNudgeBindingOverride: Binding<Nudge?>?

    @StateObject private var viewModel = NudgesViewModel()
    @State private var selectedNudgeState: Nudge?
    @State private var hasConfiguredViewModel = false
    @State private var navigationPath: [NudgeListItem] = []
    @State private var showNewNudgeSheet = false
    @State private var sourceNudgeForDraft: Nudge?

    init(
        layoutMode: NudgesLayoutMode = .adaptive,
        selectedNudge: Binding<Nudge?>? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedNudgeBindingOverride = selectedNudge
    }

    private var selectedNudgeBinding: Binding<Nudge?> {
        selectedNudgeBindingOverride ?? $selectedNudgeState
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
        .onChange(of: coreManager.diagnosticsVersion) { _, _ in
            Task { await viewModel.refresh() }
        }
        .sheet(isPresented: $showNewNudgeSheet) {
            NewNudgeSheet(
                sourceNudge: sourceNudgeForDraft,
                availableTools: viewModel.availableTools
            ) { submission in
                let created = await viewModel.createNudge(
                    title: submission.title,
                    description: submission.description,
                    content: submission.content,
                    hashtags: submission.hashtags,
                    allowTools: submission.allowTools,
                    denyTools: submission.denyTools,
                    onlyTools: submission.onlyTools
                )

                if created, let newestMine = viewModel.mine.first {
                    selectedNudgeBinding.wrappedValue = newestMine.nudge
                    navigationPath = [newestMine]
                }

                return created
            }
            .environment(coreManager)
        }
        .alert(
            "Unable to Load Nudges",
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
                .navigationTitle("Nudges")
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #else
                .toolbarTitleDisplayMode(.inline)
                #endif
                .navigationDestination(for: NudgeListItem.self) { item in
                    NudgeDetailView(
                        item: item,
                        canDelete: viewModel.canDelete(item),
                        onFork: {
                            presentNewNudgeSheet(source: item.nudge)
                        },
                        onDelete: {
                            await delete(item)
                        }
                    )
                }
                .searchable(text: $viewModel.searchText, placement: .toolbar, prompt: "Search nudges")
                .toolbar {
                    ToolbarItem(placement: .automatic) {
                        if viewModel.isLoading {
                            ProgressView()
                                .controlSize(.small)
                        }
                    }

                    ToolbarItem(placement: .automatic) {
                        Button {
                            presentNewNudgeSheet(source: nil)
                        } label: {
                            Label("New", systemImage: "plus")
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
            "Nudges",
            systemImage: "forward.circle",
            description: Text("Select a nudge from Browse to open details.")
        )
        .accessibilityIdentifier("detail_column")
    }

    private var listContent: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                NudgesHeroHeader(
                    mineCount: viewModel.filteredMine.count,
                    communityCount: viewModel.filteredCommunity.count
                )

                if viewModel.filteredMine.isEmpty, viewModel.filteredCommunity.isEmpty {
                    emptyState
                } else {
                    if !viewModel.filteredMine.isEmpty {
                        tableSection(
                            title: "Mine",
                            subtitle: "Nudges authored by you",
                            items: viewModel.filteredMine
                        )
                    }

                    if !viewModel.filteredCommunity.isEmpty {
                        tableSection(
                            title: "Community",
                            subtitle: "Nudges shared by other authors",
                            items: viewModel.filteredCommunity
                        )
                    }
                }
            }
            .frame(maxWidth: 960, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .center)
            .padding(.horizontal, 20)
            .padding(.vertical, 24)
        }
        .background(Color.systemBackground.ignoresSafeArea())
        #if os(iOS)
        .refreshable {
            await viewModel.refresh()
        }
        #endif
    }

    private func tableSection(
        title: String,
        subtitle: String,
        items: [NudgeListItem]
    ) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.headline)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .padding(.bottom, 4)

            NudgeTableHeader()

            Divider()

            LazyVStack(spacing: 0) {
                ForEach(Array(items.enumerated()), id: \.element.id) { index, item in
                    Button {
                        open(item)
                    } label: {
                        NudgeTableRow(item: item)
                            .background(index.isMultiple(of: 2) ? Color.clear : Color.systemGray6.opacity(0.4))
                    }
                    .buttonStyle(.plain)
                    .contextMenu {
                        Button {
                            presentNewNudgeSheet(source: item.nudge)
                        } label: {
                            Label("Fork", systemImage: "square.on.square")
                        }

                        if viewModel.canDelete(item) {
                            Button(role: .destructive) {
                                Task {
                                    _ = await delete(item)
                                }
                            } label: {
                                Label("Delete", systemImage: "trash")
                            }
                        }
                    }
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 8, style: .continuous)
                    .stroke(Color.systemGray5, lineWidth: 0.5)
            )
        }
    }

    private var emptyState: some View {
        ContentUnavailableView(
            "No Nudges",
            systemImage: "forward.circle",
            description: Text(viewModel.searchText.isEmpty ? "Create a nudge or refresh to discover community nudges." : "Try adjusting your search query")
        )
        .frame(maxWidth: .infinity, minHeight: 280)
    }

    private func open(_ item: NudgeListItem) {
        selectedNudgeBinding.wrappedValue = item.nudge
        navigationPath.append(item)
    }

    private func presentNewNudgeSheet(source: Nudge?) {
        sourceNudgeForDraft = source

        if showNewNudgeSheet {
            showNewNudgeSheet = false
            DispatchQueue.main.async {
                showNewNudgeSheet = true
            }
            return
        }

        showNewNudgeSheet = true
    }

    @discardableResult
    private func delete(_ item: NudgeListItem) async -> Bool {
        let deleted = await viewModel.deleteNudge(id: item.id)
        if deleted {
            selectedNudgeBinding.wrappedValue = nil
            navigationPath.removeAll { $0.id == item.id }
        }
        return deleted
    }
}

private struct NudgeDetailView: View {
    let item: NudgeListItem
    let canDelete: Bool
    let onFork: () -> Void
    let onDelete: () async -> Bool

    @State private var showDeleteConfirmation = false
    @State private var isDeleting = false

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter
    }()

    private var nudge: Nudge {
        item.nudge
    }

    private var nudgeTitle: String {
        nudge.title.isEmpty ? "Untitled Nudge" : nudge.title
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                header
                metadataSection
                hashtagsSection
                toolPermissionsSection
                contentSection
            }
            .frame(maxWidth: 920, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .center)
            .padding(.horizontal, 20)
            .padding(.top, 16)
            .padding(.bottom, 24)
        }
        .background(Color.systemBackground.ignoresSafeArea())
        .navigationTitle(nudgeTitle)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        .confirmationDialog(
            "Delete Nudge",
            isPresented: $showDeleteConfirmation,
            titleVisibility: .visible
        ) {
            Button("Delete", role: .destructive) {
                Task {
                    isDeleting = true
                    _ = await onDelete()
                    isDeleting = false
                }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This publishes a NIP-09 kind:5 deletion for this nudge event.")
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 12) {
                VStack(alignment: .leading, spacing: 8) {
                    Text(nudgeTitle)
                        .font(.title2.weight(.bold))

                    if !nudge.description.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        Text(nudge.description)
                            .font(.body)
                            .foregroundStyle(.secondary)
                    }
                }

                Spacer(minLength: 0)

                Button(action: onFork) {
                    Label("Fork", systemImage: "square.on.square")
                }
                .adaptiveGlassButtonStyle()
                .accessibilityLabel("Fork Nudge")

                if canDelete {
                    Button(role: .destructive) {
                        showDeleteConfirmation = true
                    } label: {
                        if isDeleting {
                            ProgressView()
                        } else {
                            Label("Delete", systemImage: "trash")
                        }
                    }
                    .adaptiveGlassButtonStyle()
                    .disabled(isDeleting)
                }
            }

            Divider()
        }
    }

    private var metadataSection: some View {
        section(title: "Metadata") {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .center, spacing: 10) {
                    Text("Author")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .frame(width: 110, alignment: .leading)

                    HStack(spacing: 8) {
                        AgentAvatarView(
                            agentName: item.authorDisplayName,
                            pubkey: nudge.pubkey,
                            fallbackPictureUrl: item.authorPictureURL,
                            size: 20,
                            showBorder: false
                        )
                        Text(item.authorDisplayName)
                            .font(.caption)
                    }

                    Spacer(minLength: 0)
                }

                metadataRow(title: "Pubkey", value: shortHex(nudge.pubkey))
                metadataRow(title: "Created", value: formatDate(nudge.createdAt))
                metadataRow(title: "Event ID", value: shortHex(nudge.id))

                if let supersedes = nudge.supersedes, !supersedes.isEmpty {
                    metadataRow(title: "Supersedes", value: shortHex(supersedes))
                }
            }
        }
    }

    private var hashtagsSection: some View {
        section(title: "Hashtags") {
            if nudge.hashtags.isEmpty {
                Text("No hashtags")
                    .foregroundStyle(.secondary)
            } else {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 90), spacing: 8)], alignment: .leading, spacing: 8) {
                    ForEach(nudge.hashtags, id: \.self) { tag in
                        chip(
                            text: "#\(tag)",
                            foreground: Color.askBrand,
                            background: Color.askBrand.opacity(0.15)
                        )
                    }
                }
            }
        }
    }

    private var toolPermissionsSection: some View {
        section(title: "Tool Permissions") {
            VStack(alignment: .leading, spacing: 10) {
                permissionModeSummary
                toolLists
            }
        }
    }

    @ViewBuilder
    private var permissionModeSummary: some View {
        HStack(spacing: 8) {
            if !nudge.onlyTools.isEmpty {
                chip(
                    text: "Exclusive Mode",
                    foreground: Color.askBrand,
                    background: Color.askBrand.opacity(0.18)
                )
                chip(
                    text: "Only \(nudge.onlyTools.count)",
                    foreground: Color.askBrand,
                    background: Color.askBrand.opacity(0.18)
                )
            } else {
                chip(
                    text: "Additive Mode",
                    foreground: Color.agentBrand,
                    background: Color.agentBrand.opacity(0.18)
                )
                chip(
                    text: "Allow \(nudge.allowedTools.count)",
                    foreground: Color.presenceOnline,
                    background: Color.presenceOnline.opacity(0.18)
                )
                chip(
                    text: "Deny \(nudge.deniedTools.count)",
                    foreground: Color.askBrand,
                    background: Color.askBrand.opacity(0.18)
                )
            }
        }
    }

    @ViewBuilder
    private var toolLists: some View {
        if !nudge.onlyTools.isEmpty {
            toolGrid(
                title: "Only Tools",
                tools: nudge.onlyTools,
                tint: Color.askBrand
            )
        } else if nudge.allowedTools.isEmpty && nudge.deniedTools.isEmpty {
            Text("No tool modifiers configured.")
                .font(.caption)
                .foregroundStyle(.secondary)
        } else {
            if !nudge.allowedTools.isEmpty {
                toolGrid(
                    title: "Allow",
                    tools: nudge.allowedTools,
                    tint: Color.presenceOnline
                )
            }

            if !nudge.deniedTools.isEmpty {
                toolGrid(
                    title: "Deny",
                    tools: nudge.deniedTools,
                    tint: Color.askBrand
                )
            }
        }
    }

    private func toolGrid(title: String, tools: [String], tint: Color) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)

            LazyVGrid(columns: [GridItem(.adaptive(minimum: 110), spacing: 6)], alignment: .leading, spacing: 6) {
                ForEach(tools, id: \.self) { tool in
                    chip(
                        text: tool,
                        foreground: tint,
                        background: tint.opacity(0.15)
                    )
                }
            }
        }
    }

    private var contentSection: some View {
        section(title: "Content") {
            if nudge.content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                Text("No content provided")
                    .foregroundStyle(.secondary)
            } else {
                MarkdownView(content: nudge.content)
            }
        }
    }

    private func section<Content: View>(title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title)
                .font(.headline)
            content()
        }
        .padding(.bottom, 2)
    }

    private func chip(text: String, foreground: Color, background: Color) -> some View {
        Text(text)
            .font(.caption)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(background, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
            .foregroundStyle(foreground)
    }

    private func metadataRow(title: String, value: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(title)
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(width: 110, alignment: .leading)
            Text(value)
                .font(.caption)
                .foregroundStyle(.primary)
                .textSelection(.enabled)
            Spacer(minLength: 0)
        }
    }

    private func formatDate(_ timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return Self.dateFormatter.string(from: date)
    }

    private func shortHex(_ value: String) -> String {
        guard value.count > 16 else { return value }
        return "\(value.prefix(8))...\(value.suffix(8))"
    }
}
