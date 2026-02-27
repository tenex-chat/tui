import SwiftUI

/// GitHub-style activity heatmap.
/// X-axis = days (oldest → newest, left → right).
/// Y-axis = 2-hour chunks (12 rows), mirroring how GitHub uses day-of-week on Y.
struct ActivityGridView: View {
    let snapshot: StatsSnapshot

    @State private var visualizationMode: ActivityVisualizationMode = .tokens
    @Environment(\.colorScheme) private var colorScheme

    private static let cellSize: CGFloat = 10
    private static let cellGap: CGFloat = 3
    private static let labelColumnWidth: CGFloat = 24
    private static let hoursPerRow = 3       // group size
    private static let rowCount = 24 / hoursPerRow  // 8 rows

    private let daysToShow = 30

    private static let dayFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "MMM d"
        f.timeZone = TimeZone(identifier: "UTC")
        return f
    }()

    private static let utcCalendar: Calendar = {
        var cal = Calendar(identifier: .gregorian)
        cal.timeZone = TimeZone(identifier: "UTC")!
        return cal
    }()

    var body: some View {
        let activityLookup = Dictionary(
            uniqueKeysWithValues: snapshot.activityByHour.map { ($0.hourStart, $0) }
        )
        let todayStart = Self.computeTodayStart()

        VStack(alignment: .leading, spacing: 8) {
            if snapshot.activityByHour.isEmpty {
                EmptyChartView(message: "No activity data available")
            } else {
                HStack {
                    Spacer()
                    Picker("", selection: $visualizationMode) {
                        ForEach(ActivityVisualizationMode.allCases) { mode in
                            Text(mode.rawValue).tag(mode)
                        }
                    }
                    .pickerStyle(.segmented)
                    .frame(maxWidth: 200)
                }

                ScrollView(.horizontal, showsIndicators: false) {
                    VStack(alignment: .leading, spacing: 0) {
                        dateLabelsRow(todayStart: todayStart)
                            .padding(.bottom, 6)

                        VStack(spacing: Self.cellGap) {
                            ForEach(0..<Self.rowCount, id: \.self) { rowIndex in
                                let startHour = rowIndex * Self.hoursPerRow
                                HStack(spacing: Self.cellGap) {
                                    // Label at rows 0, 3, 6, 9 → hours 00, 06, 12, 18
                                    if startHour % 6 == 0 {
                                        Text(String(format: "%02d", startHour))
                                            .font(.system(size: 9))
                                            .foregroundStyle(.secondary)
                                            .frame(width: Self.labelColumnWidth, alignment: .trailing)
                                    } else {
                                        Color.clear.frame(width: Self.labelColumnWidth)
                                    }

                                    // One cell per day, oldest (col 0) → newest (col 29)
                                    ForEach(0..<daysToShow, id: \.self) { col in
                                        let dayOffset = daysToShow - 1 - col
                                        ActivityCell(
                                            activity: Self.chunkActivity(
                                                startHour: startHour,
                                                daysAgo: dayOffset,
                                                todayStart: todayStart,
                                                lookup: activityLookup
                                            ),
                                            mode: visualizationMode,
                                            colorScheme: colorScheme
                                        )
                                    }
                                }
                            }
                        }

                        // Legend — bottom right, matching GitHub's "Less ○●●●● More"
                        HStack(spacing: 4) {
                            Spacer()
                            Text("Less")
                                .font(.system(size: 10))
                                .foregroundStyle(.secondary)
                            ForEach(ActivityIntensityLevel.allCases) { level in
                                LegendCell(level: level, colorScheme: colorScheme)
                            }
                            Text("More")
                                .font(.system(size: 10))
                                .foregroundStyle(.secondary)
                        }
                        .padding(.top, 8)
                    }
                    .padding(12)
                    .padding(.trailing, 36) // extra room so the rightmost date label isn't clipped
                }
                .defaultScrollAnchor(.trailing) // show most-recent (right) end on first load
                .background(Color.activityGridBackground(colorScheme: colorScheme))
                .clipShape(RoundedRectangle(cornerRadius: 6))
                .overlay(
                    RoundedRectangle(cornerRadius: 6)
                        .stroke(Color.activityGridBorder(colorScheme: colorScheme), lineWidth: 1)
                )
            }
        }
    }

    // MARK: - Date Labels Row

    @ViewBuilder
    private func dateLabelsRow(todayStart: UInt64) -> some View {
        HStack(spacing: Self.cellGap) {
            // Blank spacer aligning with the hour-label column
            Color.clear.frame(width: Self.labelColumnWidth)

            ForEach(0..<daysToShow, id: \.self) { col in
                let dayOffset = daysToShow - 1 - col
                // Last column: right-align so the label extends left (stays in-bounds).
                // All others: left-align so the label extends right over adjacent empty cells.
                let isLast = col == daysToShow - 1
                let showLabel = col == 0 || isLast || isMonthBoundary(col: col, todayStart: todayStart)
                Color.clear
                    .frame(width: Self.cellSize, height: 14)
                    .overlay(alignment: isLast ? .trailing : .leading) {
                        if showLabel {
                            Text(Self.dateLabel(daysAgo: dayOffset, todayStart: todayStart))
                                .font(.system(size: 10))
                                .foregroundStyle(.secondary)
                                .fixedSize(horizontal: true, vertical: false)
                        }
                    }
            }
        }
    }

    private func isMonthBoundary(col: Int, todayStart: UInt64) -> Bool {
        guard col > 0 else { return false }
        let secondsPerDay: UInt64 = 86400
        let dayOffset = UInt64(daysToShow - 1 - col)
        let thisDate = Date(timeIntervalSince1970: TimeInterval(todayStart - dayOffset * secondsPerDay))
        let prevDate = Date(timeIntervalSince1970: TimeInterval(todayStart - (dayOffset + 1) * secondsPerDay))
        return Self.utcCalendar.component(.month, from: thisDate)
            != Self.utcCalendar.component(.month, from: prevDate)
    }

    // MARK: - Helpers

    private static func dateLabel(daysAgo: Int, todayStart: UInt64) -> String {
        let secondsPerDay: UInt64 = 86400
        let dayStart = todayStart - (UInt64(daysAgo) * secondsPerDay)
        return dayFormatter.string(from: Date(timeIntervalSince1970: TimeInterval(dayStart)))
    }

    private static func computeTodayStart() -> UInt64 {
        let now = UInt64(Date().timeIntervalSince1970)
        let secondsPerDay: UInt64 = 86400
        return (now / secondsPerDay) * secondsPerDay
    }

    /// Aggregates `hoursPerRow` consecutive hours into one synthetic HourActivity.
    /// Tokens/messages are summed; intensities use the per-hour max.
    private static func chunkActivity(
        startHour: Int, daysAgo: Int, todayStart: UInt64, lookup: [UInt64: HourActivity]
    ) -> HourActivity? {
        let secondsPerDay: UInt64 = 86400
        let secondsPerHour: UInt64 = 3600
        let dayStart = todayStart - UInt64(daysAgo) * secondsPerDay

        var tokens: UInt64 = 0
        var messages: UInt64 = 0
        var tokenIntensity: UInt8 = 0
        var messageIntensity: UInt8 = 0
        var anyData = false

        for offset in 0..<hoursPerRow {
            guard let h = lookup[dayStart + UInt64(startHour + offset) * secondsPerHour] else { continue }
            anyData = true
            tokens += h.tokens
            messages += h.messages
            tokenIntensity = max(tokenIntensity, h.tokenIntensity)
            messageIntensity = max(messageIntensity, h.messageIntensity)
        }

        guard anyData else { return nil }
        return HourActivity(
            hourStart: dayStart + UInt64(startHour) * secondsPerHour,
            tokens: tokens,
            messages: messages,
            tokenIntensity: tokenIntensity,
            messageIntensity: messageIntensity
        )
    }
}

// MARK: - Activity Cell

struct ActivityCell: View {
    let activity: HourActivity?
    let mode: ActivityVisualizationMode
    let colorScheme: ColorScheme

    var body: some View {
        RoundedRectangle(cornerRadius: 2)
            .fill(cellColor)
            .frame(width: 10, height: 10)
            .help(tooltipText)
            .accessibilityLabel(tooltipText)
    }

    private var cellColor: Color {
        guard let activity, activity.tokens > 0 || activity.messages > 0 else {
            return Color.activityColor(intensity: 0, colorScheme: colorScheme)
        }
        let intensityByte = mode == .tokens ? activity.tokenIntensity : activity.messageIntensity
        return Color.activityColor(intensity: Double(intensityByte) / 255.0, colorScheme: colorScheme)
    }

    private var tooltipText: String {
        guard let activity else { return "No data" }
        let value = mode == .tokens ? activity.tokens : activity.messages
        let label = mode == .tokens ? "tokens" : "messages"
        return "\(value) \(label)"
    }
}

// MARK: - Legend Cell

struct LegendCell: View {
    let level: ActivityIntensityLevel
    let colorScheme: ColorScheme

    var body: some View {
        RoundedRectangle(cornerRadius: 2)
            .fill(level.color(colorScheme: colorScheme))
            .frame(width: 10, height: 10)
    }
}

// MARK: - Supporting Types

enum ActivityVisualizationMode: String, CaseIterable, Identifiable {
    case tokens = "Tokens"
    case messages = "Messages"

    var id: String { rawValue }
}

enum ActivityIntensityLevel: CaseIterable, Identifiable {
    case none, low, medium, mediumHigh, high

    var id: Int {
        switch self {
        case .none:       return 0
        case .low:        return 1
        case .medium:     return 2
        case .mediumHigh: return 3
        case .high:       return 4
        }
    }

    func color(colorScheme: ColorScheme) -> Color {
        switch self {
        case .none:       return Color.activityColor(intensity: 0,    colorScheme: colorScheme)
        case .low:        return Color.activityColor(intensity: 0.1,  colorScheme: colorScheme)
        case .medium:     return Color.activityColor(intensity: 0.35, colorScheme: colorScheme)
        case .mediumHigh: return Color.activityColor(intensity: 0.6,  colorScheme: colorScheme)
        case .high:       return Color.activityColor(intensity: 0.9,  colorScheme: colorScheme)
        }
    }
}

#Preview {
    let sampleData = (0..<(30 * 24)).map { offset in
        let secondsPerHour: UInt64 = 3600
        let now = UInt64(Date().timeIntervalSince1970)
        let currentHour = (now / secondsPerHour) * secondsPerHour
        let hourStart = currentHour - UInt64(offset) * secondsPerHour
        let tokens = UInt64.random(in: 0...10000)
        let messages = UInt64.random(in: 0...50)
        return HourActivity(
            hourStart: hourStart,
            tokens: tokens,
            messages: messages,
            tokenIntensity: UInt8((Double(tokens) / 10000.0 * 255.0).rounded()),
            messageIntensity: UInt8((Double(messages) / 50.0 * 255.0).rounded())
        )
    }
    ActivityGridView(
        snapshot: StatsSnapshot(
            totalCost14Days: 0,
            costByProject: [],
            messagesByDay: [],
            runtimeByDay: [],
            activityByHour: sampleData,
            maxTokens: 10000,
            maxMessages: 50
        )
    )
    .padding()
    .background(Color.systemGroupedBackground)
}
