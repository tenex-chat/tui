import SwiftUI

/// Overview tab showing high-level system status
/// Handles optional sections gracefully with fallback displays
struct DiagnosticsOverviewTab: View {
    let snapshot: DiagnosticsSnapshot

    var body: some View {
        VStack(spacing: 16) {
            // Summary Cards
            summaryCards

            // System Status Section
            systemStatusSection

            // Quick Stats Section
            quickStatsSection
        }
    }

    // MARK: - Summary Cards

    private var summaryCards: some View {
        VStack(spacing: 12) {
            HStack(spacing: 12) {
                // Sync Status Card
                if let sync = snapshot.sync {
                    DiagnosticCard(
                        title: "Relay Sync",
                        value: sync.enabled ? "Enabled" : "Disabled",
                        subtitle: "Interval: \(sync.currentIntervalSecs)s",
                        color: sync.enabled ? .green : .red,
                        icon: "arrow.triangle.2.circlepath"
                    )
                } else {
                    DiagnosticCard(
                        title: "Relay Sync",
                        value: "—",
                        subtitle: "Unavailable",
                        color: .gray,
                        icon: "arrow.triangle.2.circlepath"
                    )
                }

                // Subscriptions Card
                if let subs = snapshot.subscriptions {
                    DiagnosticCard(
                        title: "Subscriptions",
                        value: "\(subs.count)",
                        subtitle: "\(snapshot.totalSubscriptionEvents) events",
                        color: .blue,
                        icon: "antenna.radiowaves.left.and.right"
                    )
                } else {
                    DiagnosticCard(
                        title: "Subscriptions",
                        value: "—",
                        subtitle: "Unavailable",
                        color: .gray,
                        icon: "antenna.radiowaves.left.and.right"
                    )
                }
            }

            HStack(spacing: 12) {
                // Uptime Card
                if let system = snapshot.system {
                    DiagnosticCard(
                        title: "Uptime",
                        value: DiagnosticsSnapshot.formatUptime(system.uptimeMs),
                        subtitle: "since init",
                        color: .purple,
                        icon: "clock.fill"
                    )
                } else {
                    DiagnosticCard(
                        title: "Uptime",
                        value: "—",
                        subtitle: "Unavailable",
                        color: .gray,
                        icon: "clock.fill"
                    )
                }

                // Total Events Card
                if let db = snapshot.database {
                    DiagnosticCard(
                        title: "Total Events",
                        value: formatNumber(db.totalEvents),
                        subtitle: "\(db.eventCountsByKind.count) kinds",
                        color: .orange,
                        icon: "doc.text.fill"
                    )
                } else {
                    DiagnosticCard(
                        title: "Total Events",
                        value: "—",
                        subtitle: "Not loaded",
                        color: .gray,
                        icon: "doc.text.fill"
                    )
                }
            }
        }
    }

    // MARK: - System Status Section

    private var systemStatusSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            SectionHeader(title: "System Status")

            if let system = snapshot.system {
                VStack(spacing: 0) {
                    StatusRow(
                        label: "Core Initialized",
                        value: system.isInitialized ? "Yes" : "No",
                        valueColor: system.isInitialized ? .green : .red
                    )

                    Divider()

                    StatusRow(
                        label: "Logged In",
                        value: system.isLoggedIn ? "Yes" : "No",
                        valueColor: system.isLoggedIn ? .green : .red
                    )

                    Divider()

                    StatusRow(
                        label: "Version",
                        value: system.version,
                        valueColor: .primary
                    )

                    Divider()

                    if let sync = snapshot.sync {
                        StatusRow(
                            label: "Sync In Progress",
                            value: sync.syncInProgress ? "Yes" : "No",
                            valueColor: sync.syncInProgress ? .blue : .secondary
                        )
                    } else {
                        StatusRow(
                            label: "Sync In Progress",
                            value: "—",
                            valueColor: .secondary
                        )
                    }
                }
                .background(Color(.systemBackground))
                .clipShape(RoundedRectangle(cornerRadius: 12))
                .overlay(
                    RoundedRectangle(cornerRadius: 12)
                        .stroke(Color(.systemGray5), lineWidth: 1)
                )
            } else {
                SectionUnavailablePlaceholder(message: "System information unavailable")
            }
        }
    }

    // MARK: - Quick Stats Section

    private var quickStatsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            SectionHeader(title: "Quick Stats")

            VStack(spacing: 0) {
                if let db = snapshot.database {
                    StatusRow(
                        label: "Database Size",
                        value: DiagnosticsSnapshot.formatBytes(db.dbSizeBytes),
                        valueColor: .primary
                    )
                } else {
                    StatusRow(
                        label: "Database Size",
                        value: "—",
                        valueColor: .secondary
                    )
                }

                Divider()

                if let sync = snapshot.sync {
                    StatusRow(
                        label: "Last Sync",
                        value: DiagnosticsSnapshot.formatTimeSince(sync.secondsSinceLastCycle),
                        valueColor: .primary
                    )

                    Divider()

                    StatusRow(
                        label: "Successful Syncs",
                        value: "\(sync.successfulSyncs)",
                        valueColor: .green
                    )

                    Divider()

                    StatusRow(
                        label: "Failed Syncs",
                        value: "\(sync.failedSyncs)",
                        valueColor: sync.failedSyncs > 0 ? .red : .secondary
                    )
                } else {
                    StatusRow(
                        label: "Last Sync",
                        value: "—",
                        valueColor: .secondary
                    )

                    Divider()

                    StatusRow(
                        label: "Successful Syncs",
                        value: "—",
                        valueColor: .secondary
                    )

                    Divider()

                    StatusRow(
                        label: "Failed Syncs",
                        value: "—",
                        valueColor: .secondary
                    )
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

    // MARK: - Helpers

    private func formatNumber(_ value: UInt64) -> String {
        if value >= 1_000_000 {
            return String(format: "%.1fM", Double(value) / 1_000_000)
        } else if value >= 1_000 {
            return String(format: "%.1fK", Double(value) / 1_000)
        } else {
            return "\(value)"
        }
    }
}

// MARK: - Section Unavailable Placeholder

struct SectionUnavailablePlaceholder: View {
    let message: String

    var body: some View {
        HStack {
            Image(systemName: "exclamationmark.triangle")
                .foregroundColor(.orange)
            Text(message)
                .font(.subheadline)
                .foregroundColor(.secondary)
        }
        .padding()
        .frame(maxWidth: .infinity)
        .background(Color(.systemBackground))
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color(.systemGray5), lineWidth: 1)
        )
    }
}

// MARK: - Diagnostic Card

struct DiagnosticCard: View {
    let title: String
    let value: String
    let subtitle: String
    let color: Color
    let icon: String

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(title.uppercased())
                    .font(.caption)
                    .foregroundColor(.secondary)
                    .fontWeight(.medium)
                Spacer()
                Image(systemName: icon)
                    .font(.title3)
                    .foregroundColor(color.opacity(0.7))
            }

            Text(value)
                .font(.system(.title2, design: .rounded))
                .fontWeight(.bold)
                .foregroundColor(color)
                .lineLimit(1)
                .minimumScaleFactor(0.7)

            Text(subtitle)
                .font(.caption2)
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
}

// MARK: - Section Header

struct SectionHeader: View {
    let title: String

    var body: some View {
        Text(title)
            .font(.headline)
            .foregroundColor(.primary)
    }
}

// MARK: - Status Row

struct StatusRow: View {
    let label: String
    let value: String
    let valueColor: Color

    var body: some View {
        HStack {
            Text(label)
                .font(.subheadline)
                .foregroundColor(.secondary)

            Spacer()

            Text(value)
                .font(.subheadline)
                .foregroundColor(valueColor)
                .fontWeight(.medium)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }
}

#Preview("All Data Available") {
    ScrollView {
        DiagnosticsOverviewTab(
            snapshot: DiagnosticsSnapshot(
                system: SystemDiagnostics(
                    logPath: "/var/mobile/.../logs/tenex.log",
                    uptimeMs: 7_200_000,
                    version: "0.1.0",
                    isInitialized: true,
                    isLoggedIn: true
                ),
                sync: NegentropySyncDiagnostics(
                    enabled: true,
                    currentIntervalSecs: 300,
                    secondsSinceLastCycle: 120,
                    syncInProgress: false,
                    successfulSyncs: 42,
                    failedSyncs: 2,
                    totalEventsReconciled: 1500
                ),
                subscriptions: [],
                totalSubscriptionEvents: 1678,
                database: DatabaseStats(
                    dbSizeBytes: 45_678_912,
                    eventCountsByKind: [],
                    totalEvents: 1963
                ),
                sectionErrors: []
            )
        )
        .padding()
    }
    .background(Color(.systemGroupedBackground))
}

#Preview("Partial Data (Some Sections Failed)") {
    ScrollView {
        DiagnosticsOverviewTab(
            snapshot: DiagnosticsSnapshot(
                system: SystemDiagnostics(
                    logPath: "/var/mobile/.../logs/tenex.log",
                    uptimeMs: 7_200_000,
                    version: "0.1.0",
                    isInitialized: true,
                    isLoggedIn: true
                ),
                sync: nil,  // Sync failed to load
                subscriptions: [],
                totalSubscriptionEvents: 0,
                database: nil,  // Database not loaded
                sectionErrors: ["Sync: Failed to acquire lock", "Database: Not loaded"]
            )
        )
        .padding()
    }
    .background(Color(.systemGroupedBackground))
}
