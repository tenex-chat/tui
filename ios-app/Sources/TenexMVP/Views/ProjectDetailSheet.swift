import SwiftUI

// MARK: - Project Detail Sheet

struct ProjectDetailSheet: View {
    let project: ProjectInfo
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    // Header
                    VStack(alignment: .leading, spacing: 12) {
                        RoundedRectangle(cornerRadius: 16)
                            .fill(Color.blue.gradient)
                            .frame(width: 80, height: 80)
                            .overlay {
                                Image(systemName: "folder.fill")
                                    .foregroundStyle(.white)
                                    .font(.system(.title))
                            }

                        Text(project.title)
                            .font(.largeTitle)
                            .fontWeight(.bold)

                        Text(project.id)
                            .font(.system(.subheadline, design: .monospaced))
                            .foregroundStyle(.secondary)
                    }

                    Divider()

                    // Description
                    if let description = project.description {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Description")
                                .font(.headline)

                            Text(description)
                                .font(.body)
                                .foregroundStyle(.secondary)
                        }
                    }

                    Divider()

                    // Coming Soon
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Conversations")
                            .font(.headline)

                        HStack {
                            Image(systemName: "bubble.left.and.bubble.right")
                                .font(.title2)
                                .foregroundStyle(.secondary)

                            Text("Conversations coming soon...")
                                .foregroundStyle(.secondary)
                        }
                        .padding()
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Color.systemGray6)
                        .clipShape(RoundedRectangle(cornerRadius: 12))
                    }

                    Spacer()
                }
                .padding()
            }
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        dismiss()
                    }
                }
            }
        }
    }
}

#Preview {
    ProjectDetailSheet(project: ProjectInfo(
        id: "test-project-id",
        title: "Test Project",
        description: "A test project for preview"
    ))
}
