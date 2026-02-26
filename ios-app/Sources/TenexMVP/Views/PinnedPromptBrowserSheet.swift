import SwiftUI

/// Sheet for applying and deleting saved pinned prompts.
struct PinnedPromptBrowserSheet: View {
    let onApply: (PinnedPrompt) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var searchText = ""
    @State private var pinnedPromptManager = PinnedPromptManager.shared

    private var filteredPrompts: [PinnedPrompt] {
        let prompts = pinnedPromptManager.all()
        guard !searchText.isEmpty else { return prompts }
        let query = searchText.lowercased()
        return prompts.filter {
            $0.title.lowercased().contains(query) || $0.text.lowercased().contains(query)
        }
    }

    var body: some View {
        NavigationStack {
            Group {
                if filteredPrompts.isEmpty {
                    emptyState
                } else {
                    promptList
                }
            }
            .navigationTitle("Pinned Prompts")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") {
                        dismiss()
                    }
                }
            }
            #if os(iOS)
            .searchable(text: $searchText, placement: .navigationBarDrawer(displayMode: .always))
            #else
            .searchable(text: $searchText)
            #endif
        }
        .tenexModalPresentation(detents: [.medium, .large])
    }

    private var promptList: some View {
        List {
            ForEach(filteredPrompts) { prompt in
                Button {
                    onApply(prompt)
                    dismiss()
                } label: {
                    PinnedPromptRow(prompt: prompt)
                }
                .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
            }
            .onDelete { indexSet in
                let ids = indexSet.map { filteredPrompts[$0].id }
                for id in ids {
                    Task {
                        await pinnedPromptManager.delete(id)
                    }
                }
            }
        }
        #if os(iOS)
        .listStyle(.plain)
        #else
        .listStyle(.inset)
        #endif
    }

    private var emptyState: some View {
        ContentUnavailableView {
            Label("No Pinned Prompts", systemImage: "pin")
        } description: {
            if searchText.isEmpty {
                Text("Pin a prompt from the composer to reuse it quickly.")
            } else {
                Text("No prompts matching \"\(searchText)\"")
            }
        }
    }
}

private struct PinnedPromptRow: View {
    let prompt: PinnedPrompt

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(prompt.title)
                    .font(.headline)
                    .foregroundStyle(.primary)
                    .lineLimit(1)

                Spacer()

                RelativeTimeText(date: prompt.lastUsedAt, style: .localizedAbbreviated)
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }

            Text(prompt.preview)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(2)
        }
    }
}
