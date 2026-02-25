import SwiftUI

struct PinPromptTitleSheet: View {
    @Environment(\.dismiss) private var dismiss

    @Binding var title: String
    let promptText: String
    let onSave: (String) -> Void

    private var trimmedTitle: String {
        title.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Title") {
                    TextField("Release note template", text: $title)
                }

                Section("Prompt") {
                    Text(promptText)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(4)
                }
            }
            .navigationTitle("Pin Prompt")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                }

                ToolbarItem(placement: .primaryAction) {
                    Button("Save") {
                        onSave(trimmedTitle)
                    }
                    .fontWeight(.semibold)
                    .disabled(trimmedTitle.isEmpty)
                }
            }
        }
        #if os(iOS)
        .tenexModalPresentation(detents: [.medium])
        #else
        .frame(minWidth: 420, idealWidth: 480, minHeight: 280, idealHeight: 320)
        #endif
    }
}
