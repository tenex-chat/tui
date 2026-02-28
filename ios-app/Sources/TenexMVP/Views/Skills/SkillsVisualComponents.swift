import Foundation
import SwiftUI
import Kingfisher

struct SkillsHeroHeader: View {
    var body: some View {
        ZStack(alignment: .leading) {
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

            VStack(alignment: .leading, spacing: 10) {
                Text("Skills")
                    .font(.system(size: 56, weight: .regular, design: .rounded))
                    .foregroundStyle(.primary)

                Text("Give Codex superpowers.")
                    .font(.system(size: 48, weight: .regular, design: .rounded))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .minimumScaleFactor(0.7)
            }
            .padding(.horizontal, 26)
            .padding(.vertical, 24)
        }
        .frame(maxWidth: .infinity, minHeight: 200, alignment: .leading)
    }
}

struct SkillCatalogCard: View {
    let item: SkillListItem
    let isBookmarked: Bool
    let isTogglingBookmark: Bool
    let onToggleBookmark: () -> Void

    private var skill: Skill { item.skill }

    private var title: String {
        let value = skill.title.trimmingCharacters(in: .whitespacesAndNewlines)
        return value.isEmpty ? "Untitled Skill" : value
    }

    private var descriptionText: String {
        let base = skill.description.trimmingCharacters(in: .whitespacesAndNewlines)
        if !base.isEmpty {
            return base.replacingOccurrences(of: "\n", with: " ")
        }

        let preview = skill.content.trimmingCharacters(in: .whitespacesAndNewlines)
        if preview.isEmpty {
            return "No description"
        }

        return preview.replacingOccurrences(of: "\n", with: " ")
    }

    private var iconURL: URL? {
        let image = skill.image?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        guard !image.isEmpty else { return nil }
        return URL(string: image)
    }

    var body: some View {
        HStack(spacing: 14) {
            iconView

            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.title3.weight(.semibold))
                    .lineLimit(1)

                Text(descriptionText)
                    .font(.title3.weight(.regular))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)

                HStack(spacing: 8) {
                    HStack(spacing: 6) {
                        AgentAvatarView(
                            agentName: item.authorDisplayName,
                            pubkey: skill.pubkey,
                            fallbackPictureUrl: item.authorPictureURL,
                            size: 16,
                            showBorder: false
                        )
                        Text(item.authorDisplayName)
                            .font(.caption.weight(.medium))
                            .foregroundStyle(.tertiary)
                            .lineLimit(1)
                    }

                    if !skill.fileIds.isEmpty {
                        Label("\(skill.fileIds.count)", systemImage: "paperclip")
                            .font(.caption.weight(.medium))
                            .foregroundStyle(Color.skillBrand)
                    }
                }
            }

            Spacer(minLength: 8)

            Button(action: onToggleBookmark) {
                if isTogglingBookmark {
                    ProgressView()
                        .controlSize(.small)
                        .frame(width: 24, height: 24)
                } else {
                    Image(systemName: isBookmarked ? "minus" : "plus")
                        .font(.system(size: 22, weight: .regular))
                        .foregroundStyle(isBookmarked ? Color.skillBrand : .secondary)
                        .frame(width: 24, height: 24)
                }
            }
            .buttonStyle(.plain)
            .accessibilityLabel(isBookmarked ? "Remove bookmark" : "Add bookmark")
        }
        .padding(18)
        .background(
            RoundedRectangle(cornerRadius: 22, style: .continuous)
                .fill(Color.systemGray6.opacity(0.22))
        )
        .overlay(
            RoundedRectangle(cornerRadius: 22, style: .continuous)
                .stroke(Color.systemGray5.opacity(0.25), lineWidth: 1)
        )
        .contentShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
    }

    @ViewBuilder
    private var iconView: some View {
        let shape = RoundedRectangle(cornerRadius: 14, style: .continuous)

        if let iconURL {
            KFImage(iconURL)
                .placeholder {
                    placeholderIcon
                }
                .cancelOnDisappear(true)
                .resizable()
                .scaledToFill()
                .frame(width: 52, height: 52)
                .clipShape(shape)
                .overlay(
                    shape.stroke(Color.systemGray5.opacity(0.3), lineWidth: 0.5)
                )
        } else {
            placeholderIcon
        }
    }

    private var placeholderIcon: some View {
        RoundedRectangle(cornerRadius: 14, style: .continuous)
            .fill(Color.skillBrandBackground)
            .frame(width: 52, height: 52)
            .overlay(
                Text(String(title.prefix(1)).uppercased())
                    .font(.title3.weight(.bold))
                    .foregroundStyle(Color.skillBrand)
            )
    }
}
