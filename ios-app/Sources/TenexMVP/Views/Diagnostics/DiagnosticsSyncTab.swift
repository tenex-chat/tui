import SwiftUI

/// Sync tab showing negentropy sync status
struct DiagnosticsSyncTab: View {
    /// Sync data passed directly (already unwrapped from optional)
    let syncData: NegentropySyncDiagnostics
    let snapshotCapturedAt: Date

    var body: some View {
        VStack(spacing: 16) {
            // Status Card
            statusCard

            // Sync Details Section
            syncDetailsSection

            // Sync Statistics Section
            syncStatisticsSection

            // Recent Results Section
            recentResultsSection
        }
    }

    // MARK: - Status Card

    private var statusCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("NEGENTROPY SYNC")
                    .font(.caption)
                    .foregroundColor(.secondary)
                    .fontWeight(.medium)
                Spacer()
                Image(systemName: "arrow.triangle.2.circlepath")
                    .font(.title3)
                    .foregroundColor(syncData.enabled ? .green : .red)
            }

            HStack(alignment: .firstTextBaseline) {
                Text(syncData.enabled ? "Enabled" : "Disabled")
                    .font(.system(.title, design: .rounded))
                    .fontWeight(.bold)
                    .foregroundColor(syncData.enabled ? .green : .red)

                if syncData.syncInProgress {
                    ProgressView()
                        .scaleEffect(0.8)
                        .padding(.leading, 8)
                }
            }

            Text("Interval: \(syncData.currentIntervalSecs)s (\(syncData.currentIntervalSecs / 60) min)")
                .font(.subheadline)
                .foregroundColor(.secondary)
        }
        .padding()
        .frame(maxWidth: .infinity, alignment: .leading)
        .diagnosticCardStyle(withShadow: true)
    }

    // MARK: - Sync Details Section

    private var syncDetailsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            SectionHeader(title: "Sync Details")

            VStack(spacing: 0) {
                StatusRow(
                    label: "Status",
                    value: syncData.enabled ? "Enabled" : "Disabled",
                    valueColor: syncData.enabled ? .green : .red
                )

                Divider()

                StatusRow(
                    label: "Interval",
                    value: "\(syncData.currentIntervalSecs) seconds",
                    valueColor: .primary
                )

                Divider()

                StatusRow(
                    label: "Last Sync",
                    valueColor: .primary
                ) {
                    if let secondsSinceLastCycle = syncData.secondsSinceLastCycle {
                        RelativeTimeText(
                            ageSeconds: secondsSinceLastCycle,
                            referenceNow: snapshotCapturedAt,
                            style: .compact
                        )
                    } else {
                        Text("Never")
                    }
                }

                Divider()

                StatusRow(
                    label: "Next Sync",
                    value: nextSyncEstimate,
                    valueColor: .blue
                )

                Divider()

                StatusRow(
                    label: "In Progress",
                    value: syncData.syncInProgress ? "Yes" : "No",
                    valueColor: syncData.syncInProgress ? .blue : .secondary
                )
            }
            .diagnosticCardStyle()
        }
    }

    // MARK: - Sync Statistics Section

    private var syncStatisticsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            SectionHeader(title: "Statistics")

            VStack(spacing: 0) {
                StatusRow(
                    label: "Successful Syncs",
                    value: "\(syncData.successfulSyncs)",
                    valueColor: .green
                )

                Divider()

                StatusRow(
                    label: "Failed Syncs",
                    value: "\(syncData.failedSyncs)",
                    valueColor: syncData.failedSyncs > 0 ? .red : .secondary
                )

                Divider()

                StatusRow(
                    label: "Unsupported Relays",
                    value: "\(syncData.unsupportedSyncs)",
                    valueColor: syncData.unsupportedSyncs > 0 ? .orange : .secondary
                )

                Divider()

                StatusRow(
                    label: "Total Events Reconciled",
                    value: DiagnosticsFormatters.formatNumber(syncData.totalEventsReconciled),
                    valueColor: .primary
                )

                Divider()

                StatusRow(
                    label: "Success Rate",
                    value: String(format: "%.1f%%", syncData.successRate),
                    valueColor: syncData.successRateColor
                )
            }
            .diagnosticCardStyle()
        }
    }

    // MARK: - Recent Results Section

    private var recentResultsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            SectionHeader(title: "Recent Sync Results")

            if syncData.recentResults.isEmpty {
                Text("No sync results yet")
                    .font(.subheadline)
                    .foregroundColor(.secondary)
                    .frame(maxWidth: .infinity)
                    .padding()
                    .diagnosticCardStyle()
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(syncData.recentResults.prefix(10).enumerated()), id: \.offset) { index, result in
                        if index > 0 {
                            Divider()
                        }
                        recentResultRow(result)
                    }
                }
                .diagnosticCardStyle()
            }
        }
    }

    private func recentResultRow(_ result: SyncResultDiagnostic) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text("Kind \(result.kindLabel)")
                    .font(.subheadline)
                    .fontWeight(.medium)
                Spacer()
                Text(statusText(result.status))
                    .font(.caption)
                    .foregroundColor(statusColor(result.status))
            }

            HStack {
                if result.eventsReceived > 0 {
                    Text("+\(result.eventsReceived) events")
                        .font(.caption)
                        .foregroundColor(Color.healthGood)
                }
                Spacer()
                RelativeTimeText(
                    ageSeconds: result.secondsAgo,
                    referenceNow: snapshotCapturedAt,
                    style: .compact
                )
                    .font(.caption)
                    .foregroundColor(.secondary)
            }

            if let error = result.error {
                Text(error)
                    .font(.caption2)
                    .foregroundColor(Color.healthError)
                    .lineLimit(2)
            }
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 12)
    }

    private func statusText(_ status: String) -> String {
        switch status {
        case "ok": return "OK"
        case "unsupported": return "Unsupported"
        case "failed": return "Failed"
        default: return status
        }
    }

    private func statusColor(_ status: String) -> Color {
        switch status {
        case "ok": return .green
        case "unsupported": return .orange
        case "failed": return .red
        default: return .secondary
        }
    }

    // MARK: - Helpers

    private var nextSyncEstimate: String {
        guard let lastCycle = syncData.secondsSinceLastCycle else {
            return "Pending..."
        }

        let remaining = Int64(syncData.currentIntervalSecs) - Int64(lastCycle)
        if remaining <= 0 {
            return "Due now"
        } else if remaining < 60 {
            return "~\(remaining)s"
        } else {
            return "~\(remaining / 60)m \(remaining % 60)s"
        }
    }

}

#Preview {
    ScrollView {
        DiagnosticsSyncTab(
            syncData: NegentropySyncDiagnostics(
                enabled: true,
                currentIntervalSecs: 300,
                secondsSinceLastCycle: 120,
                syncInProgress: false,
                successfulSyncs: 42,
                failedSyncs: 2,
                unsupportedSyncs: 5,
                totalEventsReconciled: 1500,
                recentResults: [
                    SyncResultDiagnostic(kindLabel: "31933", eventsReceived: 3, status: "ok", error: nil, secondsAgo: 5),
                    SyncResultDiagnostic(kindLabel: "4199", eventsReceived: 0, status: "failed", error: "Connection timeout", secondsAgo: 10),
                    SyncResultDiagnostic(kindLabel: "513", eventsReceived: 0, status: "unsupported", error: nil, secondsAgo: 15),
                ]
            ),
            snapshotCapturedAt: Date()
        )
        .padding()
    }
    .background(Color.systemGroupedBackground)
}
