import SwiftUI

// MARK: - Adaptive Button Style

/// A ViewModifier that applies glass button style or bordered button style
/// based on accessibility settings, working around Swift's type inference limitations
/// with ternary operators on different ButtonStyle types.
struct AdaptiveButtonStyle: ViewModifier {
    @Environment(\.accessibilityReduceTransparency) var reduceTransparency

    func body(content: Content) -> some View {
        if reduceTransparency {
            content.buttonStyle(.bordered)
        } else if #available(iOS 26.0, macOS 26.0, *) {
            content.buttonStyle(.glass)
        } else {
            content.buttonStyle(.bordered)
        }
    }
}

/// A ViewModifier that applies a prominent glass button style or prominent bordered
/// style based on accessibility settings and OS availability.
struct AdaptiveProminentButtonStyle: ViewModifier {
    @Environment(\.accessibilityReduceTransparency) var reduceTransparency

    func body(content: Content) -> some View {
        if reduceTransparency {
            content.buttonStyle(.borderedProminent)
        } else if #available(iOS 26.0, macOS 26.0, *) {
            content.buttonStyle(.glassProminent)
        } else {
            content.buttonStyle(.borderedProminent)
        }
    }
}

extension View {
    /// Applies glass button style, or falls back to bordered style when
    /// accessibility's reduce transparency is enabled.
    func adaptiveGlassButtonStyle() -> some View {
        modifier(AdaptiveButtonStyle())
    }

    /// Applies a prominent glass button style, or falls back to prominent bordered style
    /// when accessibility's reduce transparency is enabled.
    func adaptiveProminentGlassButtonStyle() -> some View {
        modifier(AdaptiveProminentButtonStyle())
    }

    /// Applies consistent iOS modal sizing so sheets don't collapse to fitted content.
    /// On non-iOS platforms this is a no-op.
    @ViewBuilder
    func tenexModalPresentation(detents: [PresentationDetent] = [.medium, .large]) -> some View {
        #if os(iOS)
        if #available(iOS 26.0, *) {
            self
                .presentationSizing(.page)
                .presentationDetents(Set(detents))
                .presentationDragIndicator(.visible)
        } else {
            self
                .presentationDetents(Set(detents))
                .presentationDragIndicator(.visible)
        }
        #else
        self
        #endif
    }

    /// Applies a unified list surface in macOS so list columns use the grouped
    /// list tone instead of inheriting the full content background.
    @ViewBuilder
    func tenexListSurfaceBackground() -> some View {
        #if os(macOS)
        self
            .scrollContentBackground(.hidden)
            .background(Color.systemGroupedBackground)
        #else
        self
        #endif
    }
}

// MARK: - Availability-Guarded Glass Effect

/// ViewModifier that applies glassEffect on iOS/macOS 26+, falls back to regularMaterial.
struct AvailableGlassEffect: ViewModifier {
    var reduceTransparency: Bool

    func body(content: Content) -> some View {
        if reduceTransparency {
            content.background(.regularMaterial)
        } else if #available(iOS 26.0, macOS 26.0, *) {
            content.glassEffect(.regular)
        } else {
            content.background(.regularMaterial)
        }
    }
}

// MARK: - Platform Color Extensions

extension Color {
    #if os(iOS)
    static let systemBackground = Color(.systemBackground)
    static let systemGroupedBackground = Color(.systemGroupedBackground)
    static let systemGray4 = Color(.systemGray4)
    static let systemGray5 = Color(.systemGray5)
    static let systemGray6 = Color(.systemGray6)
    #elseif os(macOS)
    static let systemBackground = Color(
        red: 17.0 / 255.0,
        green: 17.0 / 255.0,
        blue: 17.0 / 255.0
    )
    static let systemGroupedBackground = Color(
        red: 30.0 / 255.0,
        green: 31.0 / 255.0,
        blue: 31.0 / 255.0
    )
    static let systemGray4 = Color(.separatorColor)
    static let systemGray5 = Color(.quaternaryLabelColor)
    static let systemGray6 = Color(.controlBackgroundColor)

    // MARK: - macOS Conversation Workspace Surfaces
    static let conversationWorkspaceBackdropMac = Color(
        red: 17.0 / 255.0,
        green: 17.0 / 255.0,
        blue: 17.0 / 255.0
    )
    static let conversationWorkspaceSurfaceMac = Color(
        red: 22.0 / 255.0,
        green: 22.0 / 255.0,
        blue: 22.0 / 255.0
    )
    static let conversationWorkspaceBorderMac = Color(
        red: 47.0 / 255.0,
        green: 47.0 / 255.0,
        blue: 47.0 / 255.0
    )
    static let conversationComposerShellMac = Color(
        red: 20.0 / 255.0,
        green: 20.0 / 255.0,
        blue: 20.0 / 255.0
    )
    static let conversationComposerFooterMac = Color(
        red: 24.0 / 255.0,
        green: 24.0 / 255.0,
        blue: 24.0 / 255.0
    )
    static let conversationComposerStrokeMac = Color(
        red: 52.0 / 255.0,
        green: 52.0 / 255.0,
        blue: 52.0 / 255.0
    )
    #endif

    // MARK: - Conversation Status
    static let statusActive: Color = .green
    static let statusActiveBackground: Color = .green.opacity(0.15)
    static let statusWaiting: Color = .orange
    static let statusWaitingBackground: Color = .orange.opacity(0.15)
    static let statusCompleted: Color = .gray
    static let statusCompletedBackground: Color = .gray.opacity(0.15)
    static let statusDefault: Color = .blue
    static let statusDefaultBackground: Color = .blue.opacity(0.15)

    // MARK: - Feature Brands
    static let skillBrand: Color = .orange
    static let skillBrandBackground: Color = .orange.opacity(0.15)
    static let agentBrand: Color = .blue
    static let projectBrand: Color = .purple
    static let projectBrandBackground: Color = .purple.opacity(0.15)

    // MARK: - Presence
    static let presenceOnline: Color = .green
    static let presenceOnlineBackground: Color = .green.opacity(0.15)
    static let presenceOffline: Color = .gray
    static let presenceOfflineBackground: Color = .gray.opacity(0.15)

    // MARK: - Ask / Question Events
    static let askBrand: Color = .orange
    static let askBrandSubtleBackground: Color = .orange.opacity(0.05)
    static let askBrandBackground: Color = .orange.opacity(0.15)
    static let askBrandBorder: Color = .orange.opacity(0.3)

    // MARK: - Message Bubbles
    static let messageBubbleUserBackground: Color = .blue.opacity(0.15)
    static let messageBubbleAgent: Color = .systemGray6
    static let messageUserAvatarColor: Color = .green

    // MARK: - Todo Items
    static let todoDone: Color = .green
    static let todoDoneBackground: Color = .green.opacity(0.15)
    static let todoInProgress: Color = .blue
    static let todoSkipped: Color = .gray

    // MARK: - Activity Heatmap (GitHub color scale, adaptive)

    /// Returns a GitHub-style heatmap color for the given intensity (0.0 = none, 1.0 = max).
    static func activityColor(intensity: Double, colorScheme: ColorScheme) -> Color {
        if colorScheme == .dark {
            if intensity == 0 { return Color(red: 22/255, green: 27/255, blue: 34/255) }   // #161b22
            if intensity < 0.25 { return Color(red: 14/255, green: 68/255, blue: 41/255) } // #0e4429
            if intensity < 0.5  { return Color(red: 0,      green: 109/255, blue: 50/255) } // #006d32
            if intensity < 0.75 { return Color(red: 38/255, green: 166/255, blue: 65/255) } // #26a641
            return Color(red: 57/255, green: 211/255, blue: 83/255)                         // #39d353
        } else {
            if intensity == 0 { return Color(red: 235/255, green: 237/255, blue: 240/255) } // #ebedf0
            if intensity < 0.25 { return Color(red: 155/255, green: 233/255, blue: 168/255) } // #9be9a8
            if intensity < 0.5  { return Color(red: 64/255,  green: 196/255, blue: 99/255) }  // #40c463
            if intensity < 0.75 { return Color(red: 48/255,  green: 161/255, blue: 78/255) }  // #30a14e
            return Color(red: 33/255, green: 110/255, blue: 57/255)                           // #216e39
        }
    }

    static func activityGridBackground(colorScheme: ColorScheme) -> Color {
        colorScheme == .dark
            ? Color(red: 13/255, green: 17/255, blue: 23/255)    // #0d1117
            : .white
    }

    static func activityGridBorder(colorScheme: ColorScheme) -> Color {
        colorScheme == .dark
            ? Color(red: 48/255, green: 54/255, blue: 61/255)     // #30363d
            : Color(red: 208/255, green: 215/255, blue: 222/255)  // #d0d7de
    }

    // MARK: - Recording
    static let recordingActive: Color = .red
    static let recordingActiveBackground: Color = .red.opacity(0.3)

    // MARK: - Diagnostics / Health
    static let healthGood: Color = .green
    static let healthWarning: Color = .orange
    static let healthError: Color = .red

    // MARK: - Inbox
    static let unreadIndicator: Color = .blue

    // MARK: - Composer
    static let composerAction: Color = .blue
    static let composerDestructive: Color = .red
    static let composerWarning: Color = .orange

    // MARK: - Stats
    static let statCost: Color = .green
    static let statRuntime: Color = .blue
    static let statAverage: Color = .purple
    static let statUserMessages: Color = .blue
    static let statAllMessages: Color = .purple

    // MARK: - Status Helper Methods
    static func conversationStatus(for status: String?, isActive: Bool = false) -> Color {
        if isActive { return .statusActive }
        switch (status ?? "").lowercased() {
        case "active", "in progress": return .statusActive
        case "waiting", "blocked":   return .statusWaiting
        case "completed", "done":    return .statusCompleted
        default:                     return .statusDefault
        }
    }

    static func conversationStatusBackground(for status: String?, isActive: Bool = false) -> Color {
        if isActive { return .statusActiveBackground }
        switch (status ?? "").lowercased() {
        case "active", "in progress": return .statusActiveBackground
        case "waiting", "blocked":   return .statusWaitingBackground
        case "completed", "done":    return .statusCompletedBackground
        default:                     return .statusDefaultBackground
        }
    }
}

// Note: macOS compatibility shims (NavigationBarTitleDisplayMode, toolbar placements, etc.)
// are provided by the main project's PlatformCompat.swift to avoid duplicate definitions
