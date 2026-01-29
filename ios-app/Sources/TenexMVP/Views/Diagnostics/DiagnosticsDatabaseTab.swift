import SwiftUI

/// Database tab showing LMDB database statistics
struct DiagnosticsDatabaseTab: View {
    /// Database data passed directly (already unwrapped from optional)
    let dbData: DatabaseStats

    var body: some View {
        VStack(spacing: 16) {
            // Summary Cards
            summaryCards

            // Event Breakdown Section
            eventBreakdownSection
        }
    }

    // MARK: - Summary Cards

    private var summaryCards: some View {
        HStack(spacing: 12) {
            // Database Size Card
            DiagnosticCard(
                title: "Database Size",
                value: DiagnosticsSnapshot.formatBytes(dbData.dbSizeBytes),
                subtitle: "LMDB file",
                color: .purple,
                icon: "cylinder"
            )

            // Total Events Card
            DiagnosticCard(
                title: "Total Events",
                value: DiagnosticsFormatters.formatNumber(dbData.totalEvents),
                subtitle: "\(dbData.eventCountsByKind.count) kinds",
                color: .blue,
                icon: "doc.text.fill"
            )
        }
    }

    // MARK: - Event Breakdown Section

    private var eventBreakdownSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            SectionHeader(title: "Events by Kind")

            if dbData.eventCountsByKind.isEmpty {
                emptyState
            } else {
                VStack(spacing: 0) {
                    ForEach(dbData.eventCountsByKind, id: \.kind) { kindCount in
                        EventKindRow(
                            kindCount: kindCount,
                            maxCount: dbData.eventCountsByKind.first?.count ?? 1
                        )

                        if kindCount.kind != dbData.eventCountsByKind.last?.kind {
                            Divider()
                        }
                    }
                }
                .background(Color(.systemBackground))
                .clipShape(RoundedRectangle(cornerRadius: 12))
                .overlay(
                    RoundedRectangle(cornerRadius: 12)
                        .stroke(Color(.systemGray5), lineWidth: 1)
                )
            }
        }
    }

    // MARK: - Empty State

    private var emptyState: some View {
        VStack(spacing: 16) {
            Image(systemName: "tray")
                .font(.system(size: 48))
                .foregroundColor(.secondary)

            Text("No Events in Database")
                .font(.headline)
                .foregroundColor(.secondary)

            Text("Events will appear here after syncing with relays")
                .font(.subheadline)
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 40)
    }

}

// MARK: - Event Kind Row

struct EventKindRow: View {
    let kindCount: KindEventCount
    let maxCount: UInt64

    private var percentage: Double {
        guard maxCount > 0 else { return 0 }
        return Double(kindCount.count) / Double(maxCount)
    }

    var body: some View {
        HStack(spacing: 12) {
            // Kind info
            VStack(alignment: .leading, spacing: 2) {
                Text(kindCount.name)
                    .font(.subheadline)
                    .fontWeight(.medium)
                    .foregroundColor(.primary)
                    .lineLimit(1)

                Text("kind \(kindCount.kind)")
                    .font(.caption)
                    .foregroundColor(.secondary)
            }

            Spacer()

            // Count and bar
            HStack(spacing: 12) {
                Text(DiagnosticsFormatters.formatNumber(kindCount.count))
                    .font(.subheadline)
                    .fontWeight(.semibold)
                    .foregroundColor(.primary)
                    .frame(minWidth: 50, alignment: .trailing)

                // Progress bar
                GeometryReader { geometry in
                    ZStack(alignment: .leading) {
                        RoundedRectangle(cornerRadius: 3)
                            .fill(Color(.systemGray5))

                        RoundedRectangle(cornerRadius: 3)
                            .fill(Color.blue)
                            .frame(width: geometry.size.width * percentage)
                    }
                }
                .frame(width: 80, height: 6)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

}

#Preview {
    ScrollView {
        DiagnosticsDatabaseTab(
            dbData: DatabaseStats(
                dbSizeBytes: 45_678_912,
                eventCountsByKind: [
                    KindEventCount(kind: 1, count: 1247, name: "Text Notes"),
                    KindEventCount(kind: 513, count: 89, name: "Conversations"),
                    KindEventCount(kind: 31933, count: 5, name: "Projects"),
                    KindEventCount(kind: 4199, count: 12, name: "Agent Definitions"),
                    KindEventCount(kind: 4201, count: 156, name: "Nudges"),
                    KindEventCount(kind: 24010, count: 89, name: "Project Status"),
                    KindEventCount(kind: 24133, count: 342, name: "Operations Status"),
                    KindEventCount(kind: 4129, count: 23, name: "Lessons")
                ],
                totalEvents: 1963
            )
        )
        .padding()
    }
    .background(Color(.systemGroupedBackground))
}
