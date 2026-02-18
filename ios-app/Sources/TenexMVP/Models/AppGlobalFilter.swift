import Foundation

/// App-level time window for global filtering across chats/reports/inbox/search.
enum AppTimeWindow: String, CaseIterable, Codable {
    case hours4
    case hours12
    case hours24
    case days7
    case all

    var label: String {
        switch self {
        case .hours4:
            return "4h"
        case .hours12:
            return "12h"
        case .hours24:
            return "24h"
        case .days7:
            return "7d"
        case .all:
            return "All"
        }
    }

    var coreTimeFilter: TimeFilterOption {
        switch self {
        case .hours4, .hours12, .hours24:
            return .today
        case .days7:
            return .thisWeek
        case .all:
            return .all
        }
    }

    var cutoffSeconds: UInt64? {
        switch self {
        case .hours4:
            return 4 * 60 * 60
        case .hours12:
            return 12 * 60 * 60
        case .hours24:
            return 24 * 60 * 60
        case .days7:
            return 7 * 24 * 60 * 60
        case .all:
            return nil
        }
    }

    func cutoffTimestamp(now: UInt64) -> UInt64? {
        guard let cutoffSeconds else { return nil }
        return now > cutoffSeconds ? now - cutoffSeconds : 0
    }

    func includes(timestamp: UInt64, now: UInt64) -> Bool {
        guard let cutoff = cutoffTimestamp(now: now) else { return true }
        return timestamp >= cutoff
    }
}

extension AppTimeWindow {
    static let defaultValue: AppTimeWindow = .hours24
}

struct AppGlobalFilterSnapshot: Equatable {
    let projectIds: Set<String>
    let timeWindow: AppTimeWindow

    var isDefault: Bool {
        projectIds.isEmpty && timeWindow == .defaultValue
    }

    func includes(projectId: String?, timestamp: UInt64, now: UInt64) -> Bool {
        let matchesProject: Bool
        if projectIds.isEmpty {
            matchesProject = true
        } else if let projectId {
            matchesProject = projectIds.contains(projectId)
        } else {
            matchesProject = false
        }

        return matchesProject && timeWindow.includes(timestamp: timestamp, now: now)
    }
}
