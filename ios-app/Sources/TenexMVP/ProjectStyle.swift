import SwiftUI
import CryptoKit

func deterministicProjectColor(for projectDTag: String) -> Color {
    let normalizedProjectDTag = projectDTag.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !normalizedProjectDTag.isEmpty else { return .accentColor }

    let hash = Array(SHA256.hash(data: Data(normalizedProjectDTag.utf8)))
    let hueSeed = (UInt16(hash[0]) << 8) | UInt16(hash[1])
    let saturationSeed = Double(hash[2]) / 255.0
    let lightnessSeed = Double(hash[3]) / 255.0

    let hue = Double(hueSeed) / Double(UInt16.max)
    let saturation = 0.60 + (0.18 * saturationSeed)
    let lightness = 0.52 + (0.10 * lightnessSeed)
    let rgb = hslToRGB(hue: hue, saturation: saturation, lightness: lightness)

    return Color(red: rgb.red, green: rgb.green, blue: rgb.blue)
}

private func hslToRGB(hue: Double, saturation: Double, lightness: Double) -> (red: Double, green: Double, blue: Double) {
    guard saturation > 0 else {
        return (lightness, lightness, lightness)
    }

    let q = lightness < 0.5
        ? lightness * (1 + saturation)
        : lightness + saturation - (lightness * saturation)
    let p = (2 * lightness) - q

    return (
        hueToChannel(p: p, q: q, t: hue + (1.0 / 3.0)),
        hueToChannel(p: p, q: q, t: hue),
        hueToChannel(p: p, q: q, t: hue - (1.0 / 3.0))
    )
}

private func hueToChannel(p: Double, q: Double, t: Double) -> Double {
    let wrappedT: Double
    if t < 0 {
        wrappedT = t + 1
    } else if t > 1 {
        wrappedT = t - 1
    } else {
        wrappedT = t
    }

    if wrappedT < (1.0 / 6.0) {
        return p + ((q - p) * 6 * wrappedT)
    }
    if wrappedT < 0.5 {
        return q
    }
    if wrappedT < (2.0 / 3.0) {
        return p + ((q - p) * ((2.0 / 3.0) - wrappedT) * 6)
    }
    return p
}

struct ProjectColorSwatch: View {
    let projectId: String
    var size: CGFloat
    var cornerRadius: CGFloat? = nil

    private var color: Color {
        deterministicProjectColor(for: projectId)
    }

    var body: some View {
        RoundedRectangle(
            cornerRadius: cornerRadius ?? max(4, size * 0.3),
            style: .continuous
        )
        .fill(color.gradient)
        .frame(width: size, height: size)
    }
}

struct ProjectPill: View {
    let projectTitle: String
    let projectId: String
    var font: Font = .caption
    var swatchSize: CGFloat = 9
    var horizontalPadding: CGFloat = 10
    var verticalPadding: CGFloat = 4

    private var color: Color {
        deterministicProjectColor(for: projectId)
    }

    var body: some View {
        HStack(spacing: 6) {
            ProjectColorSwatch(projectId: projectId, size: swatchSize)
            Text(projectTitle)
                .font(font)
                .fontWeight(.medium)
                .lineLimit(1)
        }
        .padding(.horizontal, horizontalPadding)
        .padding(.vertical, verticalPadding)
        .background(color.opacity(0.14))
        .overlay {
            Capsule(style: .continuous)
                .strokeBorder(color.opacity(0.24), lineWidth: 1)
        }
        .foregroundStyle(color)
        .clipShape(Capsule(style: .continuous))
    }
}

struct ProjectColorDot: View {
    let projectId: String
    var size: CGFloat = 18

    private var color: Color {
        deterministicProjectColor(for: projectId)
    }

    var body: some View {
        Circle()
            .fill(color.opacity(0.18))
            .overlay {
                Circle()
                    .strokeBorder(color.opacity(0.30), lineWidth: 1)
            }
            .overlay {
                Circle()
                    .fill(color.gradient)
                    .padding(max(2, size * 0.17))
            }
            .frame(width: size, height: size)
    }
}

struct ProjectInlineIndicator: View {
    let project: Project
    var showsChevron: Bool = false

    var body: some View {
        HStack(spacing: showsChevron ? 5 : 0) {
            ProjectColorDot(projectId: project.id, size: 18)
            if showsChevron {
                Image(systemName: "chevron.down")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(.secondary.opacity(0.86))
            }
        }
        .fixedSize(horizontal: true, vertical: false)
        .frame(minHeight: 22)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Project \(project.title)")
        .help(project.title)
    }
}
