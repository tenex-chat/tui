#if os(macOS)
import AppKit

enum MacWindowAuthSizing {
    private static let loginWindowSize = NSSize(width: 360, height: 460)
    private static let mainWindowSize = NSSize(width: 1200, height: 800)

    @MainActor
    static func updateMainWindowForAuthState(isLoggedIn: Bool) {
        guard let window = NSApp.keyWindow ?? NSApp.mainWindow ?? NSApp.windows.first else {
            return
        }

        if isLoggedIn {
            window.maxSize = NSSize(
                width: CGFloat.greatestFiniteMagnitude,
                height: CGFloat.greatestFiniteMagnitude
            )
            window.minSize = NSSize(width: 900, height: 620)

            if window.frame.size.width < 900 || window.frame.size.height < 620 {
                window.setContentSize(mainWindowSize)
                window.center()
            }
        } else {
            window.minSize = NSSize(width: 340, height: 430)
            window.maxSize = NSSize(width: 520, height: 680)
            window.setContentSize(loginWindowSize)
            window.center()
        }
    }
}
#endif
