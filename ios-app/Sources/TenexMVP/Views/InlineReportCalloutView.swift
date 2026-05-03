import SwiftUI

// MARK: - Inline Report Callout View

/// Tappable callout card shown inside the chat for a report published by an agent
/// (via `html_publish` or `report_publish`). Tapping the card opens the published
/// report in a sheet appropriate for its kind.
struct InlineReportCalloutView: View {
    enum ReportKind: Equatable {
        case html(HtmlReport)
        case markdown(Report)
    }

    let kind: ReportKind

    @Environment(TenexCoreManager.self) private var coreManager
    @State private var showReport = false

    // MARK: - Derived Values

    private var iconSystemName: String {
        switch kind {
        case .html: return "doc.richtext.fill"
        case .markdown: return "doc.text"
        }
    }

    private var iconColor: Color {
        switch kind {
        case .html: return .accentColor
        case .markdown: return .secondary
        }
    }

    private var title: String {
        switch kind {
        case .html(let report):
            return report.title.isEmpty ? "Untitled HTML Report" : report.title
        case .markdown(let report):
            return report.title.isEmpty ? "Untitled Report" : report.title
        }
    }

    private var subtitle: String {
        switch kind {
        case .html(let report):
            let trimmed = report.description.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !trimmed.isEmpty, trimmed != report.title else { return "" }
            return trimmed
        case .markdown(let report):
            return report.summary.trimmingCharacters(in: .whitespacesAndNewlines)
        }
    }

    private var kindLabel: String {
        switch kind {
        case .html(let report): return report.isZip ? "HTML Bundle" : "HTML Report"
        case .markdown: return "Markdown Report"
        }
    }

    private var accessibilityIdentifier: String {
        switch kind {
        case .html(let report): return "inline_report_callout_html_\(report.eventId)"
        case .markdown(let report): return "inline_report_callout_markdown_\(report.id)"
        }
    }

    // MARK: - Body

    var body: some View {
        Button {
            showReport = true
        } label: {
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: iconSystemName)
                    .font(.title3)
                    .foregroundStyle(iconColor)
                    .frame(width: 24)

                VStack(alignment: .leading, spacing: 4) {
                    Text(kindLabel)
                        .font(.caption2)
                        .fontWeight(.semibold)
                        .foregroundStyle(.tertiary)
                        .textCase(.uppercase)

                    Text(title)
                        .font(.subheadline)
                        .fontWeight(.semibold)
                        .foregroundStyle(.primary)
                        .lineLimit(2)
                        .multilineTextAlignment(.leading)

                    if !subtitle.isEmpty {
                        Text(subtitle)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(2)
                            .multilineTextAlignment(.leading)
                    }
                }

                Spacer(minLength: 8)

                VStack(spacing: 2) {
                    Image(systemName: "arrow.up.right.square")
                        .font(.callout)
                        .foregroundStyle(.tint)
                    Text("Open")
                        .font(.caption2)
                        .foregroundStyle(.tint)
                }
            }
            .padding(12)
            .background(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .fill(.quaternary)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .strokeBorder(.tertiary, lineWidth: 0.5)
            )
        }
        .buttonStyle(.borderless)
        .accessibilityIdentifier(accessibilityIdentifier)
        .sheet(isPresented: $showReport) {
            reportSheet
        }
    }

    // MARK: - Sheet Content

    @ViewBuilder
    private var reportSheet: some View {
        switch kind {
        case .html(let htmlReport):
            NavigationStack {
                HtmlReportDetailView(report: htmlReport)
                    .environment(coreManager)
                    .toolbar {
                        ToolbarItem(placement: .confirmationAction) {
                            Button("Done") { showReport = false }
                        }
                    }
            }
        case .markdown(let report):
            NavigationStack {
                ReportDetailView(report: report)
                    .environment(coreManager)
                    .toolbar {
                        ToolbarItem(placement: .confirmationAction) {
                            Button("Done") { showReport = false }
                        }
                    }
            }
            .tenexModalPresentation(detents: [.large])
        }
    }
}
