import Foundation

enum TimestampTextStyle {
    case mediumDate
    case mediumDateShortTime
    case longDateShortTime
    case shortTime
}

enum TimestampTextFormatter {
    static func string(from timestamp: UInt64, style: TimestampTextStyle) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return formatter(for: style).string(from: date)
    }

    private static let mediumDateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .none
        return formatter
    }()

    private static let mediumDateShortTimeFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter
    }()

    private static let longDateShortTimeFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .long
        formatter.timeStyle = .short
        return formatter
    }()

    private static let shortTimeFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .none
        formatter.timeStyle = .short
        return formatter
    }()

    private static func formatter(for style: TimestampTextStyle) -> DateFormatter {
        switch style {
        case .mediumDate:
            return mediumDateFormatter
        case .mediumDateShortTime:
            return mediumDateShortTimeFormatter
        case .longDateShortTime:
            return longDateShortTimeFormatter
        case .shortTime:
            return shortTimeFormatter
        }
    }
}
