import SwiftUI

extension Nudge: Identifiable {}
extension Skill: Identifiable {}

// MARK: - Nudge Chip View (for display in composer)

struct NudgeChipView: View {
    let nudge: Nudge
    let onRemove: () -> Void

    var body: some View {
        HStack(spacing: 6) {
            // Slash icon
            Text("/")
                .font(.subheadline)
                .fontWeight(.bold)
                .foregroundStyle(Color.projectBrand)

            // Nudge title
            Text(nudge.title)
                .font(.subheadline)
                .fontWeight(.medium)
                .foregroundStyle(.primary)

            // Remove button
            Button(action: onRemove) {
                Image(systemName: "xmark.circle.fill")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.borderless)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(
            Capsule()
                .fill(Color.systemBackground)
                .shadow(color: .black.opacity(0.1), radius: 2, x: 0, y: 1)
        )
    }
}

// MARK: - Skill Chip View (for display in composer)

struct SkillChipView: View {
    let skill: Skill
    let onRemove: () -> Void

    /// Check if skill has file attachments
    private var hasFiles: Bool {
        !skill.content.isEmpty
    }

    var body: some View {
        HStack(spacing: 6) {
            // Bolt icon
            Image(systemName: "bolt.fill")
                .font(.subheadline)
                .foregroundStyle(Color.skillBrand)

            // Skill title
            Text(skill.title)
                .font(.subheadline)
                .fontWeight(.medium)
                .foregroundStyle(.primary)

            // File indicator
            if hasFiles {
                Image(systemName: "doc.fill")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }

            // Remove button
            Button(action: onRemove) {
                Image(systemName: "xmark.circle.fill")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.borderless)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(
            Capsule()
                .fill(Color.systemBackground)
                .shadow(color: .black.opacity(0.1), radius: 2, x: 0, y: 1)
        )
    }
}
