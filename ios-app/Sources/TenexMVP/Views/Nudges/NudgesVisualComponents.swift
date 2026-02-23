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

struct NudgeVisualCard: View {
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
        ZStack(alignment: .bottomLeading) {
            NudgeCardBackground()

            Rectangle()
                .fill(.black.opacity(0.38))

            VStack(alignment: .leading, spacing: 7) {
                if !item.nudge.hashtags.isEmpty {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 6) {
                            ForEach(item.nudge.hashtags.prefix(3), id: \.self) { tag in
                                Text("#\(tag)")
                                    .font(.caption2.weight(.semibold))
                                    .padding(.horizontal, 8)
                                    .padding(.vertical, 4)
                                    .background(.black.opacity(0.35), in: Capsule())
                            }
                        }
                    }
                }

                Text(item.nudge.title.isEmpty ? "Untitled Nudge" : item.nudge.title)
                    .font(.headline)
                    .foregroundStyle(.white)
                    .lineLimit(2)

                if !shortDescription.isEmpty {
                    Text(shortDescription)
                        .font(.subheadline)
                        .foregroundStyle(.white.opacity(0.9))
                        .lineLimit(2)
                }

                HStack(spacing: 8) {
                    AgentAvatarView(
                        agentName: item.authorDisplayName,
                        pubkey: item.nudge.pubkey,
                        fallbackPictureUrl: item.authorPictureURL,
                        size: 20,
                        showBorder: false
                    )
                    Text(item.authorDisplayName)
                        .font(.caption.weight(.medium))
                        .foregroundStyle(.white.opacity(0.95))
                        .lineLimit(1)
                }

                HStack(spacing: 8) {
                    Text(permissionLabel)
                        .font(.caption2.weight(.semibold))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(permissionColor.opacity(0.25), in: Capsule())
                    Spacer(minLength: 0)
                    Label(relativeTimeString(from: item.nudge.createdAt), systemImage: "clock")
                        .font(.caption2)
                        .foregroundStyle(.white.opacity(0.9))
                }
            }
            .padding(14)
        }
        .frame(maxWidth: .infinity)
        .frame(height: 220)
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .stroke(.white.opacity(0.12), lineWidth: 1)
        )
        .shadow(color: .black.opacity(0.16), radius: 14, y: 8)
    }

    private func relativeTimeString(from timestamp: UInt64) -> String {
        let now = UInt64(Date().timeIntervalSince1970)
        let diff = now > timestamp ? now - timestamp : 0

        if diff < 60 {
            return "now"
        }
        if diff < 3_600 {
            return "\(diff / 60)m"
        }
        if diff < 86_400 {
            return "\(diff / 3_600)h"
        }
        return "\(diff / 86_400)d"
    }
}

private struct NudgeCardBackground: View {
    var body: some View {
        ZStack {
            LinearGradient(
                colors: [
                    Color.agentBrand.opacity(0.64),
                    Color.askBrand.opacity(0.44),
                    Color.systemGray5.opacity(0.50)
                ],
                startPoint: .topLeading,
                endPoint: .bottomTrailing
            )
            TeamsPolygonBackdrop()
                .opacity(0.55)
        }
    }
}
