import SwiftUI

/// Sync tab showing negentropy sync status
struct DiagnosticsSyncTab: View {
    /// Sync data passed directly (already unwrapped from optional)
    let syncData: NegentropySyncDiagnostics

    var body: some View {
        VStack(spacing: 16) {
            // Status Card
            statusCard

            // Sync Details Section
            syncDetailsSection

            // Sync Statistics Section
            syncStatisticsSection
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
        .background(
            RoundedRectangle(cornerRadius: 12)
                .fill(Color(.systemBackground))
                .shadow(color: Color.black.opacity(0.05), radius: 8, x: 0, y: 2)
        )
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color(.systemGray5), lineWidth: 1)
        )
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
                    value: DiagnosticsSnapshot.formatTimeSince(syncData.secondsSinceLastCycle),
                    valueColor: .primary
                )

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
            .background(Color(.systemBackground))
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .stroke(Color(.systemGray5), lineWidth: 1)
            )
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
                    label: "Total Events Reconciled",
                    value: DiagnosticsFormatters.formatNumber(syncData.totalEventsReconciled),
                    valueColor: .primary
                )

                Divider()

                let successRate = syncData.successfulSyncs + syncData.failedSyncs > 0
                    ? Double(syncData.successfulSyncs) / Double(syncData.successfulSyncs + syncData.failedSyncs) * 100
                    : 100.0
                StatusRow(
                    label: "Success Rate",
                    value: String(format: "%.1f%%", successRate),
                    valueColor: successRate >= 90 ? .green : (successRate >= 70 ? .orange : .red)
                )
            }
            .background(Color(.systemBackground))
            .clipShape(RoundedRectangle(cornerRadius: 12))
            .overlay(
                RoundedRectangle(cornerRadius: 12)
                    .stroke(Color(.systemGray5), lineWidth: 1)
            )
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
                totalEventsReconciled: 1500
            )
        )
        .padding()
    }
    .background(Color(.systemGroupedBackground))
}
