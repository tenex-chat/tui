import SwiftUI

struct BackendApprovalSheet: View {
    @Environment(TenexCoreManager.self) private var coreManager
    let request: BackendApprovalRequest
    let onDismiss: () -> Void

    @State private var isSubmitting = false

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            header
            Divider()
            projectsSection
            Spacer(minLength: 0)
            actionButtons
        }
        .padding(24)
        .frame(minWidth: 360, idealWidth: 500, minHeight: 420)
    }

    private var header: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: "server.rack")
                .font(.system(size: 28))
                .foregroundStyle(.orange)
                .frame(width: 40, height: 40)

            VStack(alignment: .leading, spacing: 6) {
                Text("Approve Backend?")
                    .font(.title3.bold())

                Text("This backend has published status for \(projectCountText).")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)

                Text(request.backendPubkey)
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
                    .textSelection(.enabled)
                    .lineLimit(2)
                    .truncationMode(.middle)
            }

            Spacer(minLength: 0)
        }
    }

    private var projectsSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Label("Seen Projects", systemImage: "folder")
                    .font(.headline)
                Spacer()
                Text("\(request.projects.count)")
                    .font(.caption.monospacedDigit())
                    .foregroundStyle(.secondary)
            }

            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    ForEach(request.projects) { project in
                        projectRow(project)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(maxHeight: 230)
            .padding(10)
            .background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
        }
    }

    private func projectRow(_ project: BackendApprovalProject) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "folder.fill")
                .foregroundStyle(.secondary)
                .frame(width: 20)

            VStack(alignment: .leading, spacing: 4) {
                Text(project.title)
                    .font(.subheadline.weight(.semibold))
                    .lineLimit(2)

                if !project.projectId.isEmpty {
                    Text(project.projectId)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }

                Text(project.aTag)
                    .font(.caption2.monospaced())
                    .foregroundStyle(.tertiary)
                    .textSelection(.enabled)
                    .lineLimit(2)
                    .truncationMode(.middle)
            }

            Spacer(minLength: 0)
        }
        .padding(.vertical, 2)
    }

    private var actionButtons: some View {
        VStack(spacing: 10) {
            Button {
                isSubmitting = true
                Task {
                    await coreManager.approvePendingBackend(backendPubkey: request.backendPubkey)
                    isSubmitting = false
                }
            } label: {
                Label("Approve", systemImage: "checkmark")
                    .frame(maxWidth: .infinity)
            }
            .adaptiveProminentGlassButtonStyle()
            .controlSize(.large)
            .disabled(isSubmitting)

            HStack(spacing: 12) {
                Button {
                    onDismiss()
                } label: {
                    Label("Not Now", systemImage: "clock")
                        .frame(maxWidth: .infinity)
                }
                .adaptiveGlassButtonStyle()
                .controlSize(.large)
                .disabled(isSubmitting)

                Button(role: .destructive) {
                    isSubmitting = true
                    Task {
                        await coreManager.blockPendingBackend(backendPubkey: request.backendPubkey)
                        isSubmitting = false
                    }
                } label: {
                    Label("Block", systemImage: "hand.raised")
                        .frame(maxWidth: .infinity)
                }
                .adaptiveGlassButtonStyle()
                .controlSize(.large)
                .disabled(isSubmitting)
            }
        }
    }

    private var projectCountText: String {
        let count = request.projects.count
        return "\(count) project\(count == 1 ? "" : "s")"
    }
}
