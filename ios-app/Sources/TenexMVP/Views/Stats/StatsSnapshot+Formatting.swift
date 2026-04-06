import Foundation

// MARK: - StatsSnapshot Formatting Helpers

extension StatsSnapshot {

    /// Format a runtime value in **milliseconds** to a compact human-readable string.
    ///
    /// - 0 → "0s"
    /// - 1...999 → "\(ms)ms"
    /// - seconds only → "\(s)s"
    /// - minutes + optional seconds → "Xm" or "Xm Ys"
    /// - hours + optional minutes → "Xh" or "Xh Ym" (seconds are dropped)
    static func formatRuntime(_ ms: UInt64) -> String {
        if ms == 0 { return "0s" }

        let totalSeconds = ms / 1000
        let remainingMs = ms % 1000

        // Sub-second: show as milliseconds
        if totalSeconds == 0 {
            return "\(ms)ms"
        }

        let hours = totalSeconds / 3600
        let minutes = (totalSeconds % 3600) / 60
        let seconds = totalSeconds % 60

        if hours > 0 {
            // Hour range: drop seconds entirely
            if minutes > 0 {
                return "\(hours)h \(minutes)m"
            }
            return "\(hours)h"
        }

        if minutes > 0 {
            if seconds > 0 {
                return "\(minutes)m \(seconds)s"
            }
            return "\(minutes)m"
        }

        return "\(seconds)s"
    }

    // MARK: - Day Label

    /// Shared date formatter for "MMM d" style labels (e.g. "Jan 15").
    private static let dayFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d"
        formatter.timeZone = TimeZone(identifier: "UTC")
        return formatter
    }()

    /// Format a day-start timestamp into a label relative to `todayStart`.
    ///
    /// - Same day → "Today"
    /// - Previous day → "Yest."
    /// - Older → "MMM d" (e.g. "Jan 15")
    static func formatDayLabel(_ dayStart: UInt64, todayStart: UInt64) -> String {
        if dayStart == todayStart {
            return "Today"
        }

        let secondsPerDay: UInt64 = 86400
        if todayStart >= secondsPerDay, dayStart == todayStart - secondsPerDay {
            return "Yest."
        }

        return dayFormatter.string(from: Date(timeIntervalSince1970: TimeInterval(dayStart)))
    }
}
