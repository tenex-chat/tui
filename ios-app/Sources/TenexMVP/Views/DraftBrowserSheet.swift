import SwiftUI

/// Sheet for browsing and restoring named drafts.
/// Presented from the composer; shows drafts for the current project.
struct DraftBrowserSheet: View {
    let projectId: String
    let onRestore: (NamedDraft) -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var searchText = ""
    @State private var namedDraftManager = NamedDraftManager.shared

    private var filteredDrafts: [NamedDraft] {
        let projectDrafts = namedDraftManager.draftsForProject(projectId)
        guard !searchText.isEmpty else { return projectDrafts }
        let query = searchText.lowercased()
        return projectDrafts.filter {
            $0.name.lowercased().contains(query) || $0.text.lowercased().contains(query)
        }
    }

    var body: some View {
        NavigationStack {
            Group {
                if filteredDrafts.isEmpty {
                    emptyState
                } else {
                    draftList
                }
            }
            .navigationTitle("Saved Drafts")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
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

    private var draftList: some View {
        List {
            ForEach(filteredDrafts) { draft in
                Button {
                    onRestore(draft)
                    dismiss()
                } label: {
                    DraftRow(draft: draft)
                }
                .listRowInsets(EdgeInsets(top: 8, leading: 16, bottom: 8, trailing: 16))
            }
            .onDelete { indexSet in
                let ids = indexSet.map { filteredDrafts[$0].id }
                for id in ids {
                    Task {
                        await namedDraftManager.delete(id)
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
            Label("No Saved Drafts", systemImage: "doc.text")
        } description: {
            if searchText.isEmpty {
                Text("Save a draft from the composer using the bookmark button to reuse it later.")
            } else {
                Text("No drafts matching \"\(searchText)\"")
            }
        }
    }
}

private struct DraftRow: View {
    let draft: NamedDraft

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(draft.name)
                    .font(.headline)
                    .foregroundStyle(.primary)
                    .lineLimit(1)

                Spacer()

                Text(draft.lastModified, style: .relative)
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }

            Text(draft.preview)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(2)
        }
    }
}
