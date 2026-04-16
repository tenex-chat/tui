import SwiftUI

// MARK: - Project Detail Content (Shared)

/// Shared content view used by both ProjectDetailSheet and ProjectDetailView
private struct ProjectDetailContent: View {
    let project: ProjectInfo

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                // Header
                VStack(alignment: .leading, spacing: 12) {
                    RoundedRectangle(cornerRadius: 16)
                        .fill(deterministicColor(for: project.id).gradient)
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

                Spacer()
            }
            .padding()
        }
        .navigationBarTitleDisplayMode(.inline)
    }
}

// MARK: - Project Detail Sheet (Modal)

/// Modal sheet presentation with its own NavigationStack and Done button
struct ProjectDetailSheet: View {
    let project: ProjectInfo
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ProjectDetailContent(project: project)
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

// MARK: - Project Detail View (Navigation Push)

/// Navigation destination view that inherits parent NavigationStack
struct ProjectDetailView: View {
    let project: ProjectInfo

    var body: some View {
        ProjectDetailContent(project: project)
            .navigationTitle(project.title)
    }
}

// MARK: - Previews

#Preview("Sheet") {
    ProjectDetailSheet(project: ProjectInfo(
        id: "test-project-id",
        title: "Test Project",
        description: "A test project for preview"
    ))
}

#Preview("Navigation Push") {
    NavigationStack {
        ProjectDetailView(project: ProjectInfo(
            id: "test-project-id",
            title: "Test Project",
            description: "A test project for preview"
        ))
    }
}
