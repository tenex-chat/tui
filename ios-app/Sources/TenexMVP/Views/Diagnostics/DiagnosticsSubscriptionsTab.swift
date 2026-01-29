import SwiftUI

/// Subscriptions tab showing active relay subscriptions
struct DiagnosticsSubscriptionsTab: View {
    /// Subscriptions data passed directly (already unwrapped from optional)
    let subscriptions: [SubscriptionDiagnostics]
    let totalEvents: UInt64

    var body: some View {
        VStack(spacing: 16) {
            // Summary Card
            summaryCard

            // Subscriptions List
            if subscriptions.isEmpty {
                emptyState
            } else {
                subscriptionsList
            }
        }
    }

    // MARK: - Summary Card

    private var summaryCard: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text("TOTAL SUBSCRIPTIONS")
                    .font(.caption)
                    .foregroundColor(.secondary)
                    .fontWeight(.medium)
                Spacer()
                Image(systemName: "antenna.radiowaves.left.and.right")
                    .font(.title3)
                    .foregroundColor(.blue)
            }

            Text("\(subscriptions.count)")
                .font(.system(.title, design: .rounded))
                .fontWeight(.bold)
                .foregroundColor(.primary)

            Text("\(DiagnosticsFormatters.formatNumber(totalEvents)) events received")
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

    // MARK: - Subscriptions List

    private var subscriptionsList: some View {
        VStack(alignment: .leading, spacing: 12) {
            SectionHeader(title: "Active Subscriptions")

            VStack(spacing: 8) {
                ForEach(subscriptions.sorted(by: { $0.eventsReceived > $1.eventsReceived }), id: \.subId) { sub in
                    SubscriptionRow(subscription: sub)
                }
            }
        }
    }

    // MARK: - Empty State

    private var emptyState: some View {
        VStack(spacing: 16) {
            Image(systemName: "antenna.radiowaves.left.and.right.slash")
                .font(.system(size: 48))
                .foregroundColor(.secondary)

            Text("No Active Subscriptions")
                .font(.headline)
                .foregroundColor(.secondary)

            Text("Subscriptions will appear here when connected to relays")
                .font(.subheadline)
                .foregroundColor(.secondary)
                .multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 40)
    }

}

// MARK: - Subscription Row

struct SubscriptionRow: View {
    let subscription: SubscriptionDiagnostics
    @State private var isExpanded = false

    var body: some View {
        VStack(spacing: 0) {
            // Header (always visible)
            Button(action: { withAnimation { isExpanded.toggle() } }) {
                HStack {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(subscription.description)
                            .font(.subheadline)
                            .fontWeight(.medium)
                            .foregroundColor(.primary)
                            .lineLimit(1)

                        Text("Created \(formatAge(subscription.ageSecs)) ago")
                            .font(.caption)
                            .foregroundColor(.secondary)

                        // Kind chips
                        ScrollView(.horizontal, showsIndicators: false) {
                            HStack(spacing: 6) {
                                ForEach(subscription.kinds, id: \.self) { kind in
                                    KindChip(kind: kind)
                                }
                            }
                        }
                    }

                    Spacer()

                    VStack(alignment: .trailing, spacing: 4) {
                        Text("\(subscription.eventsReceived)")
                            .font(.headline)
                            .foregroundColor(.blue)

                        Image(systemName: "chevron.down")
                            .font(.caption)
                            .foregroundColor(.secondary)
                            .rotationEffect(.degrees(isExpanded ? 180 : 0))
                    }
                }
                .padding()
            }
            .buttonStyle(.plain)

            // Expanded Details
            if isExpanded {
                VStack(spacing: 0) {
                    Divider()

                    DetailRow(label: "Subscription ID", value: String(subscription.subId.prefix(16)) + "...")

                    Divider()

                    DetailRow(label: "Kinds", value: subscription.kinds.map { String($0) }.joined(separator: ", "))

                    Divider()

                    DetailRow(label: "Events Received", value: "\(subscription.eventsReceived)")

                    Divider()

                    DetailRow(label: "Age", value: formatAge(subscription.ageSecs))
                }
                .background(Color(.systemGray6))
            }
        }
        .background(Color(.systemBackground))
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .overlay(
            RoundedRectangle(cornerRadius: 12)
                .stroke(Color(.systemGray5), lineWidth: 1)
        )
    }

    private func formatAge(_ seconds: UInt64) -> String {
        if seconds < 60 {
            return "\(seconds)s"
        } else if seconds < 3600 {
            return "\(seconds / 60)m"
        } else {
            return "\(seconds / 3600)h \((seconds % 3600) / 60)m"
        }
    }
}

// MARK: - Kind Chip

struct KindChip: View {
    let kind: UInt16

    var body: some View {
        Text("kind:\(kind)")
            .font(.caption2)
            .fontWeight(.medium)
            .foregroundColor(.blue)
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(Color.blue.opacity(0.1))
            .clipShape(Capsule())
    }
}

// MARK: - Detail Row

struct DetailRow: View {
    let label: String
    let value: String

    var body: some View {
        HStack {
            Text(label)
                .font(.caption)
                .foregroundColor(.secondary)

            Spacer()

            Text(value)
                .font(.caption)
                .foregroundColor(.primary)
                .fontDesign(.monospaced)
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
    }
}

#Preview {
    ScrollView {
        DiagnosticsSubscriptionsTab(
            subscriptions: [
                SubscriptionDiagnostics(
                    subId: "abc123def456",
                    description: "Text notes and contact lists",
                    kinds: [1, 3, 4],
                    eventsReceived: 1247,
                    ageSecs: 3600
                ),
                SubscriptionDiagnostics(
                    subId: "xyz789ghi012",
                    description: "User metadata",
                    kinds: [0],
                    eventsReceived: 342,
                    ageSecs: 3600
                ),
                SubscriptionDiagnostics(
                    subId: "jkl345mno678",
                    description: "Project status updates",
                    kinds: [24010],
                    eventsReceived: 89,
                    ageSecs: 1800
                )
            ],
            totalEvents: 1678
        )
        .padding()
    }
    .background(Color(.systemGroupedBackground))
}
