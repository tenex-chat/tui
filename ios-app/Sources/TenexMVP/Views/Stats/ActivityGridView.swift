import SwiftUI

/// GitHub-style activity grid showing LLM activity by hour
/// Matches TUI activity grid with proper color scale and layout
struct ActivityGridView: View {
    let snapshot: StatsSnapshot

    @State private var visualizationMode: ActivityVisualizationMode = .tokens

    private let hoursPerDay = 24
    private let daysToShow = 30

    // Performance: Cached date formatter as static
    private static let dayFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d"
        formatter.timeZone = TimeZone(identifier: "UTC")
        return formatter
    }()

    var body: some View {
        // Performance: Compute lookup dictionary once per render (O(n) once instead of O(n) per cell)
        let activityLookup = Dictionary(uniqueKeysWithValues: snapshot.activityByHour.map { ($0.hourStart, $0) })

        // Compute todayStart once per render (not cached as static)
        let todayStart = Self.computeTodayStart()
        VStack(alignment: .leading, spacing: 16) {
            // Header with toggle
            HStack {
                Text("LLM Activity (Last 30 Days)")
                    .font(.headline)
                    .accessibilityAddTraits(.isHeader)

                Spacer()

                Picker("Visualization", selection: $visualizationMode) {
                    ForEach(ActivityVisualizationMode.allCases) { mode in
                        Text(mode.rawValue).tag(mode)
                    }
                }
                .pickerStyle(.segmented)
                .frame(width: 200)
            }

            if snapshot.activityByHour.isEmpty {
                EmptyChartView(message: "No activity data available")
            } else {
                VStack(alignment: .leading, spacing: 12) {
                    // Legend
                    HStack(spacing: 12) {
                        ForEach(ActivityIntensityLevel.allCases) { level in
                            LegendCell(level: level)
                        }
                        Text("Less")
                            .font(.caption2)
                            .foregroundColor(.secondary)

                        Text("More")
                            .font(.caption2)
                            .foregroundColor(.secondary)
                    }

                    ScrollView(.vertical, showsIndicators: false) {
                        VStack(alignment: .leading, spacing: 8) {
                            // Hour labels header row (columns are hours)
                            HStack(spacing: 2) {
                                // Empty space for day labels column
                                Color.clear
                                    .frame(width: 60)

                                // Hour labels (every 3rd hour to avoid clutter)
                                ForEach(0..<hoursPerDay, id: \.self) { hour in
                                    if hour % 3 == 0 {
                                        Text(String(format: "%02d", hour))
                                            .font(.system(size: 10))
                                            .foregroundColor(.secondary)
                                            .frame(width: 12, alignment: .center)
                                    } else {
                                        Color.clear
                                            .frame(width: 12)
                                    }
                                }
                            }

                            // Grid: each row is a day, each column is an hour
                            ForEach((0..<daysToShow).reversed(), id: \.self) { dayOffset in
                                HStack(spacing: 2) {
                                    // Day label
                                    Text(dayLabel(for: dayOffset, todayStart: todayStart))
                                        .font(.system(size: 10))
                                        .foregroundColor(.secondary)
                                        .frame(width: 50, alignment: .trailing)
                                        .padding(.trailing, 8)

                                    // Hour cells for this day (columns are hours 0-23)
                                    ForEach(0..<hoursPerDay, id: \.self) { hour in
                                        ActivityCell(
                                            activity: Self.activityForHour(hour: hour, daysAgo: dayOffset, todayStart: todayStart, lookup: activityLookup),
                                            mode: visualizationMode
                                        )
                                    }
                                }
                            }
                        }
                        .padding()
                    }
                    .background(Color.systemBackground)
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                    .overlay(
                        RoundedRectangle(cornerRadius: 8)
                            .stroke(Color.systemGray4, lineWidth: 1)
                    )
                }
            }
        }
        .padding()
        .background(
            RoundedRectangle(cornerRadius: 12)
                .fill(Color.systemBackground)
                .shadow(color: Color.primary.opacity(0.05), radius: 8, x: 0, y: 2)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color.systemGray5, lineWidth: 1)
        )
    }

    private static func computeTodayStart() -> UInt64 {
        let now = UInt64(Date().timeIntervalSince1970)
        let secondsPerDay: UInt64 = 86400
        return (now / secondsPerDay) * secondsPerDay
    }

    private static func activityForHour(hour: Int, daysAgo: Int, todayStart: UInt64, lookup: [UInt64: HourActivity]) -> HourActivity? {
        // Calculate the hour_start timestamp for this cell
        let secondsPerDay: UInt64 = 86400
        let secondsPerHour: UInt64 = 3600
        let dayStart = todayStart - (UInt64(daysAgo) * secondsPerDay)
        let hourStart = dayStart + (UInt64(hour) * secondsPerHour)

        // O(1) lookup instead of O(n) linear search
        return lookup[hourStart]
    }

    private func dayLabel(for dayOffset: Int, todayStart: UInt64) -> String {
        let secondsPerDay: UInt64 = 86400
        let dayStart = todayStart - (UInt64(dayOffset) * secondsPerDay)

        let date = Date(timeIntervalSince1970: TimeInterval(dayStart))
        // Use cached formatter
        return Self.dayFormatter.string(from: date)
    }
}

// MARK: - Activity Cell

struct ActivityCell: View {
    let activity: HourActivity?
    let mode: ActivityVisualizationMode

    private let cellSize: CGFloat = 10

    var body: some View {
        Rectangle()
            .fill(color)
            .frame(width: cellSize, height: cellSize)
            .cornerRadius(2)
            .overlay(
                Rectangle()
                    .stroke(Color.systemGray6, lineWidth: 0.5)
                    .cornerRadius(2)
            )
            .help(tooltipText)
            .accessibilityLabel(accessibilityText)
    }

    private var color: Color {
        guard let activity = activity else {
            return Color.systemGray6
        }

        let value: UInt64 = mode == .tokens ? activity.tokens : activity.messages
        if value == 0 {
            return Color.systemGray6
        }

        // Use correct pre-computed intensity from Rust based on mode (0-255)
        let intensityValue = mode == .tokens ? activity.tokenIntensity : activity.messageIntensity
        let intensity = Double(intensityValue) / 255.0

        if intensity >= 0.75 {
            return Color.activityHigh
        } else if intensity >= 0.5 {
            return Color.activityMediumHigh
        } else if intensity >= 0.25 {
            return Color.activityMedium
        } else {
            return Color.activityLow
        }
    }

    private var tooltipText: String {
        guard let activity = activity else {
            return "No data"
        }

        let value = mode == .tokens ? activity.tokens : activity.messages
        let label = mode == .tokens ? "tokens" : "messages"

        return "\(value) \(label)"
    }

    private var accessibilityText: String {
        tooltipText
    }
}

// MARK: - Legend Cell

struct LegendCell: View {
    let level: ActivityIntensityLevel

    var body: some View {
        Rectangle()
            .fill(level.color)
            .frame(width: 12, height: 12)
            .cornerRadius(2)
    }
}

// MARK: - Supporting Types

enum ActivityVisualizationMode: String, CaseIterable, Identifiable {
    case tokens = "Tokens"
    case messages = "Messages"

    var id: String { rawValue }
}

enum ActivityIntensityLevel: CaseIterable, Identifiable {
    case none
    case low
    case medium
    case high

    var id: Int {
        switch self {
        case .none: return 0
        case .low: return 1
        case .medium: return 2
        case .high: return 3
        }
    }

    var color: Color {
        switch self {
        case .none:   return Color.activityNone
        case .low:    return Color.activityLow
        case .medium: return Color.activityMediumHigh
        case .high:   return Color.activityHigh
        }
    }
}

#Preview {
    let sampleData = (0..<(30*24)).map { offset in
        let secondsPerHour: UInt64 = 3600
        let now = UInt64(Date().timeIntervalSince1970)
        let currentHour = (now / secondsPerHour) * secondsPerHour
        let hourStart = currentHour - (UInt64(offset) * secondsPerHour)

        let tokens = UInt64.random(in: 0...10000)
        let messages = UInt64.random(in: 0...50)
        let tokenIntensity = UInt8((Double(tokens) / 10000.0 * 255.0).rounded())
        let messageIntensity = UInt8((Double(messages) / 50.0 * 255.0).rounded())

        return HourActivity(
            hourStart: hourStart,
            tokens: tokens,
            messages: messages,
            tokenIntensity: tokenIntensity,
            messageIntensity: messageIntensity
        )
    }

    return ActivityGridView(
        snapshot: StatsSnapshot(
            totalCost14Days: 0,
            todayRuntimeMs: 0,
            avgDailyRuntimeMs: 0,
            activeDaysCount: 0,
            runtimeByDay: [],
            costByProject: [],
            topConversations: [],
            messagesByDay: [],
            activityByHour: sampleData,
            maxTokens: 10000,
            maxMessages: 50
        )
    )
    .padding()
    .background(Color.systemGroupedBackground)
}
