import SwiftUI
import Kingfisher

struct TeamsHeroHeader: View {
    let totalCount: Int
    let featuredCount: Int

    var body: some View {
        ZStack(alignment: .leading) {
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .fill(Color.systemGray6.opacity(0.55))

            TeamsPolygonBackdrop()
                .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))

            VStack(alignment: .leading, spacing: 10) {
                Text("Teams")
                    .font(.system(size: 36, weight: .bold, design: .rounded))

                Text("Assemble and hire cross-functional agent squads into your projects.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)

                HStack(spacing: 10) {
                    Label("\(totalCount) total", systemImage: "person.2")
                    Label("\(featuredCount) featured", systemImage: "star")
                }
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
            }
            .padding(20)
        }
        .frame(height: 170)
    }
}

struct TeamsPolygonBackdrop: View {
    var body: some View {
        GeometryReader { proxy in
            Canvas { context, size in
                let width = size.width
                let height = size.height

                func polygon(_ points: [CGPoint], color: Color, stroke: Color) {
                    var path = Path()
                    if let first = points.first {
                        path.move(to: first)
                        for point in points.dropFirst() {
                            path.addLine(to: point)
                        }
                        path.closeSubpath()
                    }
                    context.fill(path, with: .color(color))
                    context.stroke(path, with: .color(stroke), lineWidth: 1)
                }

                polygon(
                    [
                        CGPoint(x: width * 0.52, y: height * 0.12),
                        CGPoint(x: width * 0.88, y: height * 0.06),
                        CGPoint(x: width * 0.95, y: height * 0.42),
                        CGPoint(x: width * 0.64, y: height * 0.46)
                    ],
                    color: .white.opacity(0.08),
                    stroke: .white.opacity(0.16)
                )

                polygon(
                    [
                        CGPoint(x: width * 0.58, y: height * 0.58),
                        CGPoint(x: width * 0.84, y: height * 0.52),
                        CGPoint(x: width * 0.98, y: height * 0.90),
                        CGPoint(x: width * 0.68, y: height * 0.94)
                    ],
                    color: .white.opacity(0.05),
                    stroke: .white.opacity(0.13)
                )

                polygon(
                    [
                        CGPoint(x: width * 0.73, y: height * 0.20),
                        CGPoint(x: width * 1.00, y: height * 0.18),
                        CGPoint(x: width * 1.00, y: height * 0.62),
                        CGPoint(x: width * 0.82, y: height * 0.62)
                    ],
                    color: .white.opacity(0.04),
                    stroke: .white.opacity(0.11)
                )
            }
            .frame(width: proxy.size.width, height: proxy.size.height)
        }
        .allowsHitTesting(false)
    }
}

struct TeamFeaturedCard: View {
    let item: TeamListItem

    var body: some View {
        TeamVisualCard(
            item: item,
            height: 320,
            titleFont: .title3.weight(.bold),
            showPrimaryCategory: true
        )
        .frame(width: 224)
    }
}

struct TeamGridCard: View {
    let item: TeamListItem

    var body: some View {
        TeamVisualCard(
            item: item,
            height: 244,
            titleFont: .headline,
            showPrimaryCategory: false
        )
    }
}

private struct TeamVisualCard: View {
    let item: TeamListItem
    let height: CGFloat
    let titleFont: Font
    let showPrimaryCategory: Bool

    private var shortDescription: String {
        item.team.description
            .replacingOccurrences(of: "\n", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var primaryCategory: String? {
        item.team.categories
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .first(where: { !$0.isEmpty })
    }

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            TeamCoverImage(imageURL: item.team.image, title: item.team.title)

            Rectangle()
                .fill(.black.opacity(0.4))

            VStack(alignment: .leading, spacing: 8) {
                if showPrimaryCategory, let primaryCategory {
                    Text(primaryCategory)
                        .font(.caption2.weight(.semibold))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(.black.opacity(0.35), in: Capsule())
                }

                Text(item.team.title.isEmpty ? "Untitled Team" : item.team.title)
                    .font(titleFont)
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
                        pubkey: item.team.pubkey,
                        fallbackPictureUrl: item.authorPictureURL,
                        size: 20,
                        showBorder: false
                    )
                    Text(item.authorDisplayName)
                        .font(.caption.weight(.medium))
                        .foregroundStyle(.white.opacity(0.95))
                        .lineLimit(1)
                }

                HStack(spacing: 12) {
                    Label("\(item.team.likeCount)", systemImage: "heart")
                    Label("\(item.team.commentCount)", systemImage: "bubble.right")
                }
                .font(.caption)
                .foregroundStyle(.white.opacity(0.9))
            }
            .padding(14)
        }
        .frame(maxWidth: .infinity)
        .frame(height: height)
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .stroke(.white.opacity(0.12), lineWidth: 1)
        )
        .shadow(color: .black.opacity(0.16), radius: 14, y: 8)
    }
}

struct TeamCoverImage: View {
    let imageURL: String?
    let title: String

    var body: some View {
        GeometryReader { proxy in
            if let imageURL,
               let url = URL(string: imageURL) {
                KFImage(url)
                    .placeholder {
                        TeamImagePlaceholder(title: title)
                    }
                    .retry(maxCount: 2, interval: .seconds(1))
                    .fade(duration: 0.15)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .frame(width: proxy.size.width, height: proxy.size.height)
                    .clipped()
            } else {
                TeamImagePlaceholder(title: title)
            }
        }
    }
}

private struct TeamImagePlaceholder: View {
    let title: String

    var body: some View {
        ZStack {
            Color.systemGray5.opacity(0.6)

            TeamsPolygonBackdrop()

            Text(initials)
                .font(.system(size: 36, weight: .black, design: .rounded))
                .foregroundStyle(.white.opacity(0.26))
        }
    }

    private var initials: String {
        let words = title
            .split(separator: " ")
            .map(String.init)
            .filter { !$0.isEmpty }

        if words.count >= 2 {
            let first = words[0].prefix(1)
            let second = words[1].prefix(1)
            return "\(first)\(second)".uppercased()
        }

        let compact = title.replacingOccurrences(of: " ", with: "")
        return String(compact.prefix(2)).uppercased()
    }
}

// MARK: - Agent Definition Visual Card

struct AgentDefinitionVisualCard: View {
    let item: AgentDefinitionListItem

    private var shortDescription: String {
        item.agent.description
            .replacingOccurrences(of: "\n", with: " ")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    var body: some View {
        ZStack(alignment: .bottomLeading) {
            AgentDefinitionCardBackground(
                pictureURL: item.agent.picture,
                name: item.agent.name
            )

            Rectangle()
                .fill(.black.opacity(0.4))

            VStack(alignment: .leading, spacing: 6) {
                if !item.agent.role.isEmpty {
                    Text(item.agent.role)
                        .font(.caption2.weight(.semibold))
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(.black.opacity(0.35), in: Capsule())
                }

                Text(item.agent.name.isEmpty ? "Unnamed Agent" : item.agent.name)
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
                        pubkey: item.agent.pubkey,
                        fallbackPictureUrl: item.authorPictureURL,
                        size: 20,
                        showBorder: false
                    )
                    Text(item.authorDisplayName)
                        .font(.caption.weight(.medium))
                        .foregroundStyle(.white.opacity(0.95))
                        .lineLimit(1)
                }

                HStack(spacing: 12) {
                    if let model = item.agent.model, !model.isEmpty {
                        Label(model, systemImage: "cpu")
                    }
                    if !item.agent.tools.isEmpty {
                        Label("\(item.agent.tools.count) tools", systemImage: "wrench")
                    }
                }
                .font(.caption)
                .foregroundStyle(.white.opacity(0.9))
            }
            .padding(14)
        }
        .frame(maxWidth: .infinity)
        .frame(height: 200)
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 18, style: .continuous)
                .stroke(.white.opacity(0.12), lineWidth: 1)
        )
        .shadow(color: .black.opacity(0.16), radius: 14, y: 8)
    }
}

private struct AgentDefinitionCardBackground: View {
    let pictureURL: String?
    let name: String

    var body: some View {
        GeometryReader { proxy in
            if let pictureURL,
               let url = URL(string: pictureURL) {
                KFImage(url)
                    .placeholder {
                        AgentDefinitionCardPlaceholder(name: name)
                    }
                    .retry(maxCount: 2, interval: .seconds(1))
                    .fade(duration: 0.15)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .frame(width: proxy.size.width, height: proxy.size.height)
                    .clipped()
            } else {
                AgentDefinitionCardPlaceholder(name: name)
            }
        }
    }
}

private struct AgentDefinitionCardPlaceholder: View {
    let name: String

    var body: some View {
        ZStack {
            Color.systemGray5.opacity(0.6)

            TeamsPolygonBackdrop()

            Text(initials)
                .font(.system(size: 36, weight: .black, design: .rounded))
                .foregroundStyle(.white.opacity(0.26))
        }
    }

    private var initials: String {
        let words = name
            .split(separator: " ")
            .map(String.init)
            .filter { !$0.isEmpty }

        if words.count >= 2 {
            let first = words[0].prefix(1)
            let second = words[1].prefix(1)
            return "\(first)\(second)".uppercased()
        }

        let compact = name.replacingOccurrences(of: " ", with: "")
        return String(compact.prefix(2)).uppercased()
    }
}

// MARK: - Agent Definitions Hero Header

struct AgentDefinitionsHeroHeader: View {
    let mineCount: Int
    let communityCount: Int

    var body: some View {
        ZStack(alignment: .leading) {
            RoundedRectangle(cornerRadius: 20, style: .continuous)
                .fill(Color.systemGray6.opacity(0.55))

            TeamsPolygonBackdrop()
                .clipShape(RoundedRectangle(cornerRadius: 20, style: .continuous))

            VStack(alignment: .leading, spacing: 10) {
                Text("Agent Definitions")
                    .font(.system(size: 36, weight: .bold, design: .rounded))

                Text("Reusable agent templates for your projects.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)

                HStack(spacing: 10) {
                    Label("\(mineCount) mine", systemImage: "person")
                    Label("\(communityCount) community", systemImage: "person.3")
                }
                .font(.caption.weight(.medium))
                .foregroundStyle(.secondary)
            }
            .padding(20)
        }
        .frame(height: 170)
    }
}
