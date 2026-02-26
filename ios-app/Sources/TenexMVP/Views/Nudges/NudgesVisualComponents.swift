import SwiftUI

struct NudgesHeroHeader: View {
    let mineCount: Int
    let communityCount: Int

    var body: some View {
        ZStack(alignment: .leading) {
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .fill(Color.systemGray6.opacity(0.55))

            TeamsPolygonBackdrop()
                .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))

            VStack(alignment: .leading, spacing: 10) {
                Text("Nudges")
                    .font(.system(size: 36, weight: .bold, design: .rounded))

                Text("Reusable prompts with tool constraints you can attach to conversations.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)

                HStack(spacing: 10) {
                    Label("\(mineCount) mine", systemImage: "person")
                    Label("\(communityCount) community", systemImage: "person.2")
                }
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
            }
            .padding(20)
        }
        .frame(height: 170)
    }
}

struct NudgeTableHeader: View {
    var body: some View {
        HStack(spacing: 0) {
            Text("Name")
                .frame(minWidth: 140, alignment: .leading)
            Text("Description")
                .frame(maxWidth: .infinity, alignment: .leading)
            Text("Tags")
                .frame(width: 140, alignment: .leading)
            Text("Permissions")
                .frame(width: 120, alignment: .leading)
            Text("Author")
                .frame(width: 120, alignment: .leading)
            Text("Age")
                .frame(width: 50, alignment: .trailing)
        }
        .font(.caption.weight(.medium))
        .foregroundStyle(.secondary)
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
    }
}

struct NudgeTableRow: View {
    let item: NudgeListItem

    private var shortDescription: String {
        item.nudge.description
            .replacingOccurrences(of: "\n", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var permissionLabel: String {
        if !item.nudge.onlyTools.isEmpty {
            return "Only: \(item.nudge.onlyTools.count)"
        }

        let allowCount = item.nudge.allowedTools.count
        let denyCount = item.nudge.deniedTools.count
        return "Allow \(allowCount) · Deny \(denyCount)"
    }

    private var permissionColor: Color {
        item.nudge.onlyTools.isEmpty ? Color.agentBrand : Color.askBrand
    }

    var body: some View {
        HStack(spacing: 0) {
            // Name
            Text(item.nudge.title.isEmpty ? "Untitled Nudge" : item.nudge.title)
                .font(.body.weight(.medium))
                .lineLimit(1)
                .frame(minWidth: 140, alignment: .leading)

            // Description
            Text(shortDescription)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .frame(maxWidth: .infinity, alignment: .leading)

            // Tags
            HStack(spacing: 4) {
                ForEach(item.nudge.hashtags.prefix(2), id: \.self) { tag in
                    Text("#\(tag)")
                        .font(.caption2.weight(.medium))
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Color.askBrand.opacity(0.15), in: Capsule())
                        .foregroundStyle(Color.askBrand)
                }
                if item.nudge.hashtags.count > 2 {
                    Text("+\(item.nudge.hashtags.count - 2)")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
            }
            .frame(width: 140, alignment: .leading)

            // Permissions
            Text(permissionLabel)
                .font(.caption.weight(.medium))
                .padding(.horizontal, 8)
                .padding(.vertical, 3)
                .background(permissionColor.opacity(0.15), in: Capsule())
                .foregroundStyle(permissionColor)
                .frame(width: 120, alignment: .leading)

            // Author
            HStack(spacing: 6) {
                AgentAvatarView(
                    agentName: item.authorDisplayName,
                    pubkey: item.nudge.pubkey,
                    fallbackPictureUrl: item.authorPictureURL,
                    size: 18,
                    showBorder: false
                )
                Text(item.authorDisplayName)
                    .font(.caption)
                    .lineLimit(1)
            }
            .frame(width: 120, alignment: .leading)

            // Age
            RelativeTimeText(timestamp: item.nudge.createdAt, style: .compact)
                .font(.caption)
                .foregroundStyle(.tertiary)
                .frame(width: 50, alignment: .trailing)
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .contentShape(Rectangle())
    }
}
