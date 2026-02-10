import SwiftUI

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
    #endif
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

// iOS-only toolbar placements mapped to .automatic on macOS
extension ToolbarItemPlacement {
    static var topBarTrailing: ToolbarItemPlacement { .automatic }
    static var topBarLeading: ToolbarItemPlacement { .automatic }
}

#endif
