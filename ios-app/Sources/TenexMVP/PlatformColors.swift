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

extension View {
    /// Applies glass button style, or falls back to bordered style when
    /// accessibility's reduce transparency is enabled.
    func adaptiveGlassButtonStyle() -> some View {
        modifier(AdaptiveButtonStyle())
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
    static let systemBackground = Color(.windowBackgroundColor)
    static let systemGroupedBackground = Color(.windowBackgroundColor)
    static let systemGray4 = Color(.separatorColor)
    static let systemGray5 = Color(.quaternaryLabelColor)
    static let systemGray6 = Color(.controlBackgroundColor)

    // MARK: - macOS Conversation Workspace Surfaces
    static let conversationWorkspaceBackdropMac = Color(
        red: 17.0 / 255.0,
        green: 20.0 / 255.0,
        blue: 24.0 / 255.0
    )
    static let conversationWorkspaceSurfaceMac = Color(
        red: 27.0 / 255.0,
        green: 32.0 / 255.0,
        blue: 38.0 / 255.0
    )
    static let conversationWorkspaceBorderMac = Color(
        red: 56.0 / 255.0,
        green: 63.0 / 255.0,
        blue: 72.0 / 255.0
    )
    static let conversationComposerShellMac = Color(
        red: 20.0 / 255.0,
        green: 24.0 / 255.0,
        blue: 29.0 / 255.0
    )
    static let conversationComposerFooterMac = Color(
        red: 26.0 / 255.0,
        green: 31.0 / 255.0,
        blue: 37.0 / 255.0
    )
    static let conversationComposerStrokeMac = Color(
        red: 46.0 / 255.0,
        green: 54.0 / 255.0,
        blue: 64.0 / 255.0
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

    // MARK: - Activity Heatmap (Tailwind green scale)
    static let activityHigh: Color = Color(red: 34/255, green: 197/255, blue: 94/255)
    static let activityMediumHigh: Color = Color(red: 74/255, green: 222/255, blue: 128/255)
    static let activityMedium: Color = Color(red: 134/255, green: 239/255, blue: 172/255)
    static let activityLow: Color = Color(red: 187/255, green: 247/255, blue: 208/255)
    static let activityNone: Color = .systemGray6

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

// MARK: - macOS SwiftUI Compatibility

#if os(macOS)

// .navigationBarTitleDisplayMode is iOS-only; no-op on macOS
enum NavigationBarTitleDisplayMode {
    case inline, large, automatic
}

extension View {
    func navigationBarTitleDisplayMode(_ displayMode: NavigationBarTitleDisplayMode) -> some View {
        self
    }
}

// iOS-only toolbar placements mapped to native macOS placements
extension ToolbarItemPlacement {
    static var topBarTrailing: ToolbarItemPlacement { .primaryAction }
    static var topBarLeading: ToolbarItemPlacement { .navigation }
}

#endif
