import SwiftUI
import CryptoKit

/// A sheet for selecting a project to a-tag in a new conversation.
/// Supports search and single-select with proper cancel semantics.
struct ProjectSelectorSheet: View {
    // MARK: - Properties

    /// Available projects to choose from
    let projects: [ProjectInfo]

    /// Currently selected project (binding for single-select)
    @Binding var selectedProject: ProjectInfo?

    /// Callback when selection is confirmed
    var onDone: (() -> Void)?

    // MARK: - Environment

    @Environment(\.dismiss) private var dismiss

    // MARK: - State

    /// Local copy of selection - only committed on Done, discarded on Cancel
    @State private var localSelectedProject: ProjectInfo?
    @State private var searchText = ""

    // MARK: - Computed

    private var filteredProjects: [ProjectInfo] {
        if searchText.isEmpty {
            return projects
        }

        let lowercasedSearch = searchText.lowercased()
        return projects.filter { project in
            project.title.lowercased().contains(lowercasedSearch) ||
            project.id.lowercased().contains(lowercasedSearch) ||
            (project.description?.lowercased().contains(lowercasedSearch) ?? false)
        }
    }

    // MARK: - Body

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // Selected project bar (if any)
                if let project = localSelectedProject {
                    selectedProjectBar(project)
                }

                // Search and project list
                List {
                    if filteredProjects.isEmpty {
                        emptyStateView
                    } else {
                        ForEach(filteredProjects, id: \.id) { project in
                            ProjectRowSelectView(
                                project: project,
                                isSelected: localSelectedProject?.id == project.id
                            ) {
                                selectProject(project)
                            }
                        }
                    }
                }
                .listStyle(.plain)
            }
            .searchable(text: $searchText, prompt: "Search projects...")
            .navigationTitle("Select Project")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        // Discard local changes
                        dismiss()
                    }
                }

                ToolbarItem(placement: .primaryAction) {
                    Button("Done") {
                        // Commit local changes to parent binding
                        selectedProject = localSelectedProject
                        onDone?()
                        dismiss()
                    }
                    .fontWeight(.semibold)
                }
            }
            .onAppear {
                // Initialize local state from parent binding
                localSelectedProject = selectedProject
            }
        }
    }

    // MARK: - Subviews

    private func selectedProjectBar(_ project: ProjectInfo) -> some View {
        HStack(spacing: 8) {
            Text(project.title)
                .font(.subheadline)
                .fontWeight(.medium)

            Button(action: { localSelectedProject = nil }) {
                Image(systemName: "xmark.circle.fill")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)

            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(Color(.systemGray6))
        .foregroundStyle(.blue)
    }

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Image(systemName: searchText.isEmpty ? "folder.badge.questionmark" : "magnifyingglass")
                .font(.system(size: 40))
                .foregroundStyle(.secondary)

            if searchText.isEmpty {
                Text("No Projects Available")
                    .font(.headline)
                Text("You don't have access to any projects.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            } else {
                Text("No Results")
                    .font(.headline)
                Text("No projects match \"\(searchText)\"")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 60)
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    // MARK: - Actions

    private func selectProject(_ project: ProjectInfo) {
        // Toggle selection (single-select)
        if localSelectedProject?.id == project.id {
            localSelectedProject = nil
        } else {
            localSelectedProject = project
        }
    }
}

// MARK: - Project Row Select View

struct ProjectRowSelectView: View {
    let project: ProjectInfo
    let isSelected: Bool
    let onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            HStack(spacing: 12) {
                // Project icon
                projectIcon

                // Project info
                VStack(alignment: .leading, spacing: 4) {
                    Text(project.title)
                        .font(.headline)
                        .foregroundStyle(.primary)

                    if let description = project.description, !description.isEmpty {
                        Text(description)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                    }

                    Text(project.id)
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                }

                Spacer()

                // Selection indicator
                Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                    .font(.title2)
                    .foregroundStyle(isSelected ? .blue : .secondary)
            }
            .padding(.vertical, 8)
        }
        .buttonStyle(.plain)
    }

    private var projectIcon: some View {
        RoundedRectangle(cornerRadius: 8)
            .fill(projectColor.gradient)
            .frame(width: 48, height: 48)
            .overlay {
                Image(systemName: "folder.fill")
                    .foregroundStyle(.white)
                    .font(.title3)
            }
            .overlay(
                RoundedRectangle(cornerRadius: 8)
                    .strokeBorder(isSelected ? Color.blue : Color.clear, lineWidth: 2)
            )
    }

    /// Deterministic color using SHA-256 hash (stable across app launches)
    private var projectColor: Color {
        let colors: [Color] = [.blue, .purple, .orange, .green, .pink, .indigo, .teal, .cyan]
        let data = Data(project.id.utf8)
        let hash = SHA256.hash(data: data)
        let firstByte = hash.withUnsafeBytes { $0[0] }
        return colors[Int(firstByte) % colors.count]
    }
}

// MARK: - Preview

#Preview {
    ProjectSelectorSheet(
        projects: [
            ProjectInfo(
                id: "tenex-tui",
                title: "TENEX TUI Client",
                description: "Terminal UI client for TENEX"
            ),
            ProjectInfo(
                id: "nostr-sdk",
                title: "Nostr SDK",
                description: "Swift SDK for Nostr protocol"
            ),
            ProjectInfo(
                id: "mobile-app",
                title: "Mobile App",
                description: nil
            )
        ],
        selectedProject: .constant(
            ProjectInfo(
                id: "tenex-tui",
                title: "TENEX TUI Client",
                description: "Terminal UI client for TENEX"
            )
        )
    )
}
