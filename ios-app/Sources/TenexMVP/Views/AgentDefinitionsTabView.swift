import SwiftUI

enum AgentDefinitionsLayoutMode {
    case adaptive
    case shellList
    case shellDetail
}

struct AgentDefinitionsTabView: View {
    @EnvironmentObject private var coreManager: TenexCoreManager
    let layoutMode: AgentDefinitionsLayoutMode
    private let selectedAgentBindingOverride: Binding<AgentInfo?>?

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    @StateObject private var viewModel = AgentDefinitionsViewModel()
    @State private var selectedAgentState: AgentInfo?
    @State private var hasConfiguredViewModel = false

    init(
        layoutMode: AgentDefinitionsLayoutMode = .adaptive,
        selectedAgent: Binding<AgentInfo?>? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedAgentBindingOverride = selectedAgent
    }

    private var selectedAgentBinding: Binding<AgentInfo?> {
        selectedAgentBindingOverride ?? $selectedAgentState
    }

    private var useSplitView: Bool {
        if layoutMode == .shellList || layoutMode == .shellDetail {
            return true
        }
        #if os(macOS)
        return true
        #else
        return horizontalSizeClass == .regular
        #endif
    }

    var body: some View {
        Group {
            switch layoutMode {
            case .shellList:
                shellListLayout
            case .shellDetail:
                shellDetailLayout
            case .adaptive:
                if useSplitView {
                    splitLayout
                } else {
                    shellListLayout
                }
            }
        }
        .task(id: layoutMode == .shellDetail) {
            if !hasConfiguredViewModel {
                viewModel.configure(with: coreManager)
                hasConfiguredViewModel = true
            }
            await viewModel.loadIfNeeded()
        }
        .onChange(of: coreManager.diagnosticsVersion) { _, _ in
            Task { await viewModel.refresh() }
        }
        .alert(
            "Unable to Load Agent Definitions",
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

    private var shellListLayout: some View {
        listContent
            .navigationTitle("Agent Definitions")
            .accessibilityIdentifier("section_list_column")
    }

    private var shellDetailLayout: some View {
        detailContent
            .accessibilityIdentifier("detail_column")
    }

    private var splitLayout: some View {
        #if os(macOS)
        return AnyView(
            HSplitView {
                listContent
                    .frame(minWidth: 340, idealWidth: 460, maxWidth: 560, maxHeight: .infinity)
                detailContent
                    .frame(minWidth: 560, maxWidth: .infinity, maxHeight: .infinity)
            }
        )
        #else
        return AnyView(
            NavigationSplitView {
                listContent
            } detail: {
                detailContent
            }
        )
        #endif
    }

    private var listContent: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                headerSection
                controlsSection

                if viewModel.filteredMine.isEmpty, viewModel.filteredCommunity.isEmpty {
                    emptyState
                } else {
                    if !viewModel.filteredMine.isEmpty {
                        sectionGrid(
                            title: "Mine",
                            subtitle: "Definitions you authored",
                            items: viewModel.filteredMine
                        )
                    }

                    if !viewModel.filteredCommunity.isEmpty {
                        sectionGrid(
                            title: "Community",
                            subtitle: "Definitions from other authors",
                            items: viewModel.filteredCommunity
                        )
                    }
                }
            }
            .padding()
        }
        #if os(iOS)
        .refreshable {
            await viewModel.refresh()
        }
        #endif
    }

    private var headerSection: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("Agent Definitions")
                .font(.largeTitle.weight(.bold))

            Text("Browse reusable agent templates (kind:4199)")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
    }

    private var controlsSection: some View {
        HStack(spacing: 10) {
            searchField

            Button {
                Task { await viewModel.refresh() }
            } label: {
                if viewModel.isLoading {
                    ProgressView()
                        .controlSize(.small)
                } else {
                    Label("Refresh", systemImage: "arrow.clockwise")
                }
            }
            .buttonStyle(.bordered)
            .disabled(viewModel.isLoading)
        }
    }

    private var searchField: some View {
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(.secondary)

            TextField("Search definitions", text: $viewModel.searchText)
                .textFieldStyle(.plain)

            if !viewModel.searchText.isEmpty {
                Button {
                    viewModel.searchText = ""
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .background(Color.systemGray6)
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }

    private func sectionGrid(
        title: String,
        subtitle: String,
        items: [AgentDefinitionListItem]
    ) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.headline)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            LazyVGrid(columns: [GridItem(.adaptive(minimum: 280, maximum: 380), spacing: 12)], spacing: 12) {
                ForEach(items) { item in
                    AgentDefinitionCardView(
                        item: item,
                        isSelected: selectedAgentBinding.wrappedValue?.id == item.id,
                        onSelect: { selectedAgentBinding.wrappedValue = item.agent }
                    )
                }
            }
        }
    }

    private var emptyState: some View {
        ContentUnavailableView(
            "No Agent Definitions",
            systemImage: "person.3.sequence",
            description: Text(viewModel.searchText.isEmpty ? "Definitions will appear here when discovered" : "Try adjusting your search query")
        )
        .frame(maxWidth: .infinity, minHeight: 280)
    }

    @ViewBuilder
    private var detailContent: some View {
        if let selectedAgent = selectedAgentBinding.wrappedValue,
           let item = viewModel.listItem(for: selectedAgent)
        {
            AgentDefinitionDetailView(item: item)
        } else {
            ContentUnavailableView(
                "Select an Agent Definition",
                systemImage: "person.3.sequence",
                description: Text("Choose a card to inspect details")
            )
        }
    }
}

private struct AgentDefinitionCardView: View {
    let item: AgentDefinitionListItem
    let isSelected: Bool
    let onSelect: () -> Void

    private var attachmentCount: Int {
        item.agent.fileIds.count
    }

    private var displayName: String {
        item.agent.name.isEmpty ? "Unnamed Agent" : item.agent.name
    }

    var body: some View {
        Button(action: onSelect) {
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .top, spacing: 10) {
                    AgentAvatarView(
                        agentName: displayName,
                        pubkey: item.agent.pubkey,
                        fallbackPictureUrl: item.agent.picture,
                        size: 36,
                        fontSize: 12,
                        showBorder: false,
                        isSelected: false
                    )

                    VStack(alignment: .leading, spacing: 4) {
                        Text(displayName)
                            .font(.headline)
                            .foregroundStyle(.primary)
                            .lineLimit(1)

                        HStack(spacing: 6) {
                            if !item.agent.role.isEmpty {
                                tag(item.agent.role)
                            }
                            if let model = item.agent.model, !model.isEmpty {
                                tag(model)
                            }
                        }
                    }

                    Spacer(minLength: 0)

                    if let version = item.agent.version, !version.isEmpty {
                        Text("v\(version)")
                            .font(.caption2.weight(.medium))
                            .foregroundStyle(.secondary)
                    }
                }

                Text(item.agent.description.isEmpty ? "No description provided" : item.agent.description)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)

                HStack(spacing: 10) {
                    Label(item.authorDisplayName, systemImage: "person")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)

                    if attachmentCount > 0 {
                        Label("\(attachmentCount) file\(attachmentCount == 1 ? "" : "s")", systemImage: "paperclip")
                            .font(.caption)
                            .foregroundStyle(Color.skillBrand)
                    }
                }
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color.systemBackground)
            .overlay {
                RoundedRectangle(cornerRadius: 12)
                    .stroke(isSelected ? Color.agentBrand : Color.systemGray5, lineWidth: isSelected ? 2 : 1)
            }
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
        .buttonStyle(.plain)
    }

    private func tag(_ text: String) -> some View {
        Text(text)
            .font(.caption2)
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(Color.systemGray6)
            .clipShape(Capsule())
    }
}

private struct AgentDefinitionDetailView: View {
    let item: AgentDefinitionListItem

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter
    }()

    private var displayName: String {
        item.agent.name.isEmpty ? "Unnamed Agent" : item.agent.name
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                header
                metadataCard
                descriptionCard
                instructionsCard

                if !item.agent.useCriteria.isEmpty {
                    useCriteriaCard
                }
                if !item.agent.tools.isEmpty {
                    toolsCard
                }
                if !item.agent.mcpServers.isEmpty {
                    mcpServersCard
                }
                if !item.agent.fileIds.isEmpty {
                    fileReferencesCard
                }
            }
            .padding()
        }
    }

    private var header: some View {
        HStack(alignment: .top, spacing: 12) {
            AgentAvatarView(
                agentName: displayName,
                pubkey: item.agent.pubkey,
                fallbackPictureUrl: item.agent.picture,
                size: 44,
                fontSize: 14,
                showBorder: false,
                isSelected: false
            )

            VStack(alignment: .leading, spacing: 4) {
                Text(displayName)
                    .font(.title2.weight(.bold))

                HStack(spacing: 8) {
                    if !item.agent.role.isEmpty {
                        chip(text: item.agent.role, foreground: .primary, background: Color.systemGray6)
                    }
                    if let model = item.agent.model, !model.isEmpty {
                        chip(text: model, foreground: Color.agentBrand, background: Color.agentBrand.opacity(0.15))
                    }
                    if let version = item.agent.version, !version.isEmpty {
                        chip(text: "v\(version)", foreground: .secondary, background: Color.systemGray6)
                    }
                }
            }

            Spacer(minLength: 0)
        }
    }

    private var metadataCard: some View {
        card(title: "Metadata") {
            VStack(alignment: .leading, spacing: 8) {
                metadataRow(title: "Author", value: item.authorDisplayName)
                metadataRow(title: "Author Pubkey", value: shortHex(item.agent.pubkey))
                metadataRow(title: "Created", value: formatDate(item.agent.createdAt))
                metadataRow(title: "Event ID", value: shortHex(item.agent.id))

                if !item.agent.dTag.isEmpty {
                    metadataRow(title: "d-tag", value: item.agent.dTag)
                }
            }
        }
    }

    private var descriptionCard: some View {
        card(title: "Description") {
            Text(item.agent.description.isEmpty ? "No description provided" : item.agent.description)
                .font(.body)
                .foregroundStyle(item.agent.description.isEmpty ? .secondary : .primary)
        }
    }

    private var instructionsCard: some View {
        card(title: "Instructions") {
            if item.agent.instructions.isEmpty {
                Text("No instructions provided")
                    .foregroundStyle(.secondary)
            } else {
                MarkdownView(content: item.agent.instructions)
            }
        }
    }

    private var useCriteriaCard: some View {
        card(title: "Use Criteria") {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(item.agent.useCriteria, id: \.self) { criteria in
                    HStack(alignment: .top, spacing: 6) {
                        Text("•")
                            .foregroundStyle(.secondary)
                        Text(criteria)
                            .foregroundStyle(.primary)
                    }
                }
            }
        }
    }

    private var toolsCard: some View {
        card(title: "Tools") {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 120), spacing: 6)], alignment: .leading, spacing: 6) {
                ForEach(item.agent.tools, id: \.self) { tool in
                    chip(text: tool, foreground: Color.skillBrand, background: Color.skillBrandBackground)
                }
            }
        }
    }

    private var mcpServersCard: some View {
        card(title: "MCP Servers") {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(item.agent.mcpServers, id: \.self) { serverId in
                    Text(serverId)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
        }
    }

    private var fileReferencesCard: some View {
        card(title: "File References (NIP-94 kind:1063)") {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(item.agent.fileIds, id: \.self) { fileId in
                    HStack(spacing: 8) {
                        Image(systemName: "paperclip")
                            .foregroundStyle(Color.skillBrand)
                        Text(fileId)
                            .font(.caption.monospaced())
                            .foregroundStyle(.secondary)
                            .textSelection(.enabled)
                    }
                }
            }
        }
    }

    private func card<Content: View>(title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title)
                .font(.headline)
            content()
        }
        .padding()
        .background(Color.systemBackground)
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .overlay {
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.systemGray5, lineWidth: 1)
        }
    }

    private func chip(text: String, foreground: Color, background: Color) -> some View {
        Text(text)
            .font(.caption)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(background)
            .foregroundStyle(foreground)
            .clipShape(Capsule())
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
