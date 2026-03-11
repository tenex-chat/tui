import SwiftUI

struct NudgesHeroHeader: View {
    var body: some View {
        ZStack(alignment: .leading) {
#if !os(macOS)
            RoundedRectangle(cornerRadius: 22, style: .continuous)
                .fill(
                    LinearGradient(
                        colors: [
                            Color.black.opacity(0.38),
                            Color.black.opacity(0.22)
                        ],
                        startPoint: .topLeading,
                        endPoint: .bottomTrailing
                    )
                )
#endif

            VStack(alignment: .leading, spacing: 10) {
                Text("Nudges")
                    .font(titleFont)
                    .fontDesign(.rounded)
                    .foregroundStyle(.primary)

                Text("Guide your agents with reusable prompts")
                    .font(subtitleFont)
                    .fontDesign(.rounded)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .minimumScaleFactor(0.7)
            }
            .padding(.horizontal, 26)
            .padding(.vertical, 24)
        }
        .frame(maxWidth: .infinity, minHeight: 200, alignment: .leading)
    }

    private var titleFont: Font {
#if os(macOS)
        return .largeTitle.weight(.regular)
#else
        return .system(size: 56, weight: .regular, design: .rounded)
#endif
    }

    private var subtitleFont: Font {
#if os(macOS)
        return .title3.weight(.regular)
#else
        return .system(size: 48, weight: .regular, design: .rounded)
#endif
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
