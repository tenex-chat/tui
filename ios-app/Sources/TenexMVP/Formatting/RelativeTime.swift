import Foundation
import SwiftUI

enum RelativeTimeStyle {
    case localizedAbbreviated
    case compact
}

enum RelativeTime {
    private static let localizedAbbreviatedFormatter: RelativeDateTimeFormatter = {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter
    }()

    static func string(date: Date, now: Date, style: RelativeTimeStyle) -> String {
        let elapsed = now.timeIntervalSince(date)
        if elapsed >= 0 && elapsed < 60 {
            return "just now"
        }

        switch style {
        case .localizedAbbreviated:
            return localizedAbbreviatedFormatter.localizedString(for: date, relativeTo: now)
        case .compact:
            return compactString(interval: elapsed)
        }
    }

    static func string(timestamp: UInt64, now: Date, style: RelativeTimeStyle) -> String {
        string(date: Date(timeIntervalSince1970: TimeInterval(timestamp)), now: now, style: style)
    }

    static func date(referenceNow: Date, ageSeconds: UInt64) -> Date {
        referenceNow.addingTimeInterval(-TimeInterval(ageSeconds))
    }

    private static func compactString(interval: TimeInterval) -> String {
        let isPast = interval >= 0
        let seconds = max(0, Int(abs(interval)))

        let quantity: String
        if seconds < 60 {
            quantity = "<1m"
        } else if seconds < 3_600 {
            quantity = "\(seconds / 60)m"
        } else if seconds < 86_400 {
            quantity = "\(seconds / 3_600)h"
        } else if seconds < 604_800 {
            quantity = "\(seconds / 86_400)d"
        } else if seconds < 2_592_000 {
            quantity = "\(seconds / 604_800)w"
        } else if seconds < 31_536_000 {
            quantity = "\(seconds / 2_592_000)mo"
        } else {
            quantity = "\(seconds / 31_536_000)y"
        }

        return isPast ? "\(quantity) ago" : "in \(quantity)"
    }
}

struct RelativeTimeSchedule: TimelineSchedule {
    let referenceDate: Date

    func entries(from startDate: Date, mode: Mode) -> Entries {
        Entries(
            referenceDate: referenceDate,
            nextDate: Self.nextUpdateDate(after: startDate, referenceDate: referenceDate)
        )
    }

    struct Entries: Sequence, IteratorProtocol {
        let referenceDate: Date
        var nextDate: Date

        mutating func next() -> Date? {
            let date = nextDate
            nextDate = RelativeTimeSchedule.nextUpdateDate(after: date, referenceDate: referenceDate)
            return date
        }
    }

    private static func nextUpdateDate(after current: Date, referenceDate: Date) -> Date {
        let elapsed = current.timeIntervalSince(referenceDate)
        if elapsed >= 0 && elapsed < 60 {
            return current.addingTimeInterval(1)
        }

        let timestamp = current.timeIntervalSince1970
        let nextMinuteBoundary = (floor(timestamp / 60) + 1) * 60
        return Date(timeIntervalSince1970: nextMinuteBoundary)
    }
}

struct RelativeTimeText: View {
    private let date: Date
    private let style: RelativeTimeStyle

    init(date: Date, style: RelativeTimeStyle = .localizedAbbreviated) {
        self.date = date
        self.style = style
    }

    init(timestamp: UInt64, style: RelativeTimeStyle = .localizedAbbreviated) {
        self.init(date: Date(timeIntervalSince1970: TimeInterval(timestamp)), style: style)
    }

    init(ageSeconds: UInt64, referenceNow: Date, style: RelativeTimeStyle = .localizedAbbreviated) {
        self.init(date: RelativeTime.date(referenceNow: referenceNow, ageSeconds: ageSeconds), style: style)
    }

    var body: some View {
        TimelineView(RelativeTimeSchedule(referenceDate: date)) { context in
            Text(RelativeTime.string(date: date, now: context.date, style: style))
        }
    }
}
