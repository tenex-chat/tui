import SwiftUI
#if os(iOS)
import UIKit
#elseif os(macOS)
import AppKit
#endif

enum SkillsLayoutMode {
    case adaptive
    case shellList
    case shellDetail
}

struct SkillsTabView: View {
    @Environment(TenexCoreManager.self) private var coreManager

    let layoutMode: SkillsLayoutMode
    private let selectedSkillBindingOverride: Binding<Skill?>?

    @StateObject private var viewModel = SkillsViewModel()
    @State private var selectedSkillState: Skill?
    @State private var hasConfiguredViewModel = false
    @State private var navigationPath: [SkillListItem] = []
    @State private var detailItem: SkillListItem?

    init(
        layoutMode: SkillsLayoutMode = .adaptive,
        selectedSkill: Binding<Skill?>? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedSkillBindingOverride = selectedSkill
    }

    private var selectedSkillBinding: Binding<Skill?> {
        selectedSkillBindingOverride ?? $selectedSkillState
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
        .onChange(of: coreManager.contentCatalogVersion) { _, _ in
            Task { await viewModel.refresh() }
        }
        .sheet(item: $detailItem) { item in
            SkillDetailView(item: item)
            #if os(macOS)
            .frame(minWidth: 980, minHeight: 620)
            #endif
            .environment(coreManager)
        }
        .alert(
            "Unable to Load Skills",
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
                .navigationTitle("Skills")
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #else
                .toolbarTitleDisplayMode(.inline)
                #endif
                #if os(iOS)
                .navigationDestination(for: SkillListItem.self) { item in
                    SkillDetailView(item: item)
                }
                #endif
                .searchable(text: $viewModel.searchText, placement: .toolbar, prompt: "Search skills")
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
            "Skills",
            systemImage: "bolt.fill",
            description: Text("Select a skill from Browse to open details.")
        )
        .accessibilityIdentifier("detail_column")
    }

    private var listContent: some View {
        let items = viewModel.filteredMine + viewModel.filteredCommunity

        return ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                SkillsHeroHeader()

                if items.isEmpty {
                    emptyState
                } else {
                    LazyVGrid(columns: listColumns, spacing: 16) {
                        ForEach(items) { item in
                            SkillCatalogCard(item: item)
                                .onTapGesture {
                                    open(item)
                                }
                        }
                    }
                }
            }
            .frame(maxWidth: 1200, alignment: .leading)
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

    private var listColumns: [GridItem] {
        #if os(macOS)
        return [
            GridItem(.flexible(minimum: 280), spacing: 16),
            GridItem(.flexible(minimum: 280), spacing: 16)
        ]
        #else
        return [GridItem(.flexible(), spacing: 12)]
        #endif
    }

    private var emptyState: some View {
        ContentUnavailableView(
            "No Skills",
            systemImage: "bolt.fill",
            description: Text(viewModel.searchText.isEmpty ? "Refresh to discover community skills." : "Try adjusting your search query")
        )
        .frame(maxWidth: .infinity, minHeight: 280)
    }

    private func open(_ item: SkillListItem) {
        selectedSkillBinding.wrappedValue = item.skill
        #if os(macOS)
        detailItem = item
        #else
        navigationPath.append(item)
        #endif
    }
}

private struct SkillDetailView: View {
    let item: SkillListItem

    @State private var hasCopiedNevent = false

    private var skill: Skill {
        item.skill
    }

    private var skillTitle: String {
        skill.title.isEmpty ? "Untitled Skill" : skill.title
    }

    private var skillNevent: String? {
        Bech32.hexEventIdToNevent(skill.id)
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                header
                metadataSection
                hashtagsSection
                fileAttachmentsSection
                contentSection
            }
            .frame(maxWidth: 920, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .center)
            .padding(.horizontal, 20)
            .padding(.top, 16)
            .padding(.bottom, 24)
        }
        .background(Color.systemBackground.ignoresSafeArea())
        .navigationTitle(skillTitle)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(skillTitle)
                .font(.title2.weight(.bold))

            if !skill.description.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                Text(skill.description)
                    .font(.body)
                    .foregroundStyle(.secondary)
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
                            pubkey: skill.pubkey,
                            fallbackPictureUrl: item.authorPictureURL,
                            size: 20,
                            showBorder: false
                        )
                        Text(item.authorDisplayName)
                            .font(.caption)
                    }

                    Spacer(minLength: 0)
                }

                metadataRow(
                    title: "Created",
                    value: TimestampTextFormatter.string(from: skill.createdAt, style: .mediumDateShortTime)
                )

                if let nevent = skillNevent {
                    HStack(alignment: .center, spacing: 10) {
                        Text("Event")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .frame(width: 110, alignment: .leading)

                        Button {
                            copyNevent(nevent)
                        } label: {
                            Label(hasCopiedNevent ? "Copied nevent1" : "Copy nevent1", systemImage: hasCopiedNevent ? "checkmark" : "doc.on.doc")
                                .font(.caption.weight(.semibold))
                        }
                        .buttonStyle(.borderless)

                        Spacer(minLength: 0)
                    }
                }
            }
        }
    }

    private var hashtagsSection: some View {
        section(title: "Hashtags") {
            if skill.hashtags.isEmpty {
                Text("No hashtags")
                    .foregroundStyle(.secondary)
            } else {
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 90), spacing: 8)], alignment: .leading, spacing: 8) {
                    ForEach(skill.hashtags, id: \.self) { tag in
                        chip(
                            text: "#\(tag)",
                            foreground: Color.skillBrand,
                            background: Color.skillBrandBackground
                        )
                    }
                }
            }
        }
    }

    private var fileAttachmentsSection: some View {
        section(title: "File Attachments") {
            if skill.fileIds.isEmpty {
                Text("No file attachments")
                    .foregroundStyle(.secondary)
            } else {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(skill.fileIds, id: \.self) { fileId in
                        metadataRow(title: "File", value: shortHex(fileId))
                    }
                }
            }
        }
    }

    private var contentSection: some View {
        section(title: "Content") {
            if skill.content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                Text("No content provided")
                    .foregroundStyle(.secondary)
            } else {
                MarkdownView(content: skill.content)
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

    private func shortHex(_ value: String) -> String {
        guard value.count > 16 else { return value }
        return "\(value.prefix(8))...\(value.suffix(8))"
    }

    private func copyNevent(_ nevent: String) {
        #if os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(nevent, forType: .string)
        #else
        UIPasteboard.general.string = nevent
        #endif

        hasCopiedNevent = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) {
            hasCopiedNevent = false
        }
    }
}
