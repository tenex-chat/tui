import SwiftUI
import WebKit

private enum detailLogger {
    static func info(_ s: String) { print("[HtmlReportDetailView] \(s)") }
    static func error(_ s: String) { print("[HtmlReportDetailView][ERROR] \(s)") }
}

// MARK: - HtmlReportSource

enum HtmlReportSource: Equatable {
    case html(content: String, baseURL: URL)
    case local(indexURL: URL, baseDirectory: URL)
}

// MARK: - HtmlReportDetailView

struct HtmlReportDetailView: View {
    let report: HtmlReport
    let versions: [HtmlReport]
    @Environment(TenexCoreManager.self) private var coreManager

    @State private var loadState: LoadState = .loading
    @State private var loadedSource: HtmlReportSource?
    @State private var resolvedConversation: ConversationFullInfo?
    @State private var showConversation = false
    @State private var selectedEventId: String?

    private enum LoadState: Equatable {
        case loading
        case loaded
        case failed(String)
    }

    init(report: HtmlReport, versions: [HtmlReport] = []) {
        self.report = report
        self.versions = versions
    }

    var body: some View {
        content
            .navigationTitle(activeReport.title.isEmpty ? "HTML Report" : activeReport.title)
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                if versionList.count > 1 {
                    ToolbarItem(placement: .primaryAction) {
                        versionMenu
                    }
                }
                if !activeReport.conversationId.isEmpty {
                    ToolbarItem(placement: .primaryAction) {
                        openConversationButton
                    }
                }
            }
            .task(id: activeReport.eventId) {
                await loadReport()
                await resolveConversation()
            }
            .navigationDestination(isPresented: $showConversation) {
                if let conversation = resolvedConversation {
                    ConversationWorkspaceView(conversation: conversation)
                        .environment(coreManager)
                } else {
                    Text("Conversation not available")
                        .foregroundStyle(.secondary)
                }
            }
    }

    private var versionList: [HtmlReport] {
        var seen = Set<String>()
        return ([report] + versions)
            .filter { seen.insert($0.eventId).inserted }
            .sorted {
                if $0.createdAt != $1.createdAt {
                    return $0.createdAt > $1.createdAt
                }
                return $0.eventId < $1.eventId
            }
    }

    private var activeReport: HtmlReport {
        let eventId = selectedEventId ?? report.eventId
        return versionList.first(where: { $0.eventId == eventId }) ?? report
    }

    @ViewBuilder
    private var content: some View {
        switch loadState {
        case .loading:
            ProgressView("Loading report…")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .accessibilityIdentifier("html_report_loading")
        case .loaded:
            if let source = loadedSource {
                HtmlReportWebView(source: source)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .accessibilityIdentifier("html_report_webview")
            } else {
                Text("Report unavailable")
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        case .failed(let message):
            VStack(spacing: 12) {
                Image(systemName: "exclamationmark.triangle")
                    .font(.system(size: 40))
                    .foregroundStyle(.secondary)
                Text("Couldn't load report")
                    .font(.headline)
                Text(message)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(.horizontal, 32)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .accessibilityIdentifier("html_report_error")
        }
    }

    private var openConversationButton: some View {
        Button {
            showConversation = true
        } label: {
            Label("Open Conversation", systemImage: "bubble.left.and.bubble.right")
        }
        .disabled(resolvedConversation == nil)
        .accessibilityIdentifier("html_report_open_conversation_button")
    }

    private var versionMenu: some View {
        Menu {
            ForEach(versionList, id: \.eventId) { version in
                Button {
                    selectVersion(version)
                } label: {
                    Label(
                        versionMenuTitle(for: version),
                        systemImage: version.eventId == activeReport.eventId ? "checkmark" : "doc.richtext"
                    )
                }
            }
        } label: {
            Label("Versions", systemImage: "clock.arrow.circlepath")
        }
        .accessibilityIdentifier("html_report_versions_button")
    }

    private func selectVersion(_ version: HtmlReport) {
        guard activeReport.eventId != version.eventId else { return }
        selectedEventId = version.eventId
        loadedSource = nil
        resolvedConversation = nil
        showConversation = false
        loadState = .loading
    }

    private func versionMenuTitle(for version: HtmlReport) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(version.createdAt))
            .formatted(date: .abbreviated, time: .shortened)
        if version.eventId == versionList.first?.eventId {
            return "Latest - \(date)"
        }
        return date
    }

    // MARK: - Loading

    private func loadReport() async {
        let target = activeReport
        detailLogger.info("loadReport start eventId=\(target.eventId) url=\(target.url)")
        let started = Date()
        do {
            let source = try await HtmlReportCache.shared.source(for: target)
            let ms = Int(Date().timeIntervalSince(started) * 1000)
            detailLogger.info("loadReport ok eventId=\(target.eventId) ms=\(ms)")
            await MainActor.run {
                guard activeReport.eventId == target.eventId else { return }
                loadedSource = source
                loadState = .loaded
            }
        } catch {
            let ms = Int(Date().timeIntervalSince(started) * 1000)
            detailLogger.error("loadReport failed eventId=\(target.eventId) ms=\(ms) error=\(error.localizedDescription)")
            await MainActor.run {
                guard activeReport.eventId == target.eventId else { return }
                loadState = .failed(error.localizedDescription)
            }
        }
    }

    private func resolveConversation() async {
        let target = activeReport
        let conversationId = target.conversationId
        guard !conversationId.isEmpty else {
            await MainActor.run {
                guard activeReport.eventId == target.eventId else { return }
                resolvedConversation = nil
            }
            return
        }
        let matches = await coreManager.core.getConversationsByIds(conversationIds: [conversationId])
        await MainActor.run {
            guard activeReport.eventId == target.eventId else { return }
            resolvedConversation = matches.first
        }
    }

}

// MARK: - WKWebView Bridge

private struct HtmlReportWebView {
    let source: HtmlReportSource
}

// MARK: iOS

#if os(iOS)
extension HtmlReportWebView: UIViewRepresentable {
    func makeUIView(context: Context) -> WKWebView {
        let webView = WKWebView(frame: .zero, configuration: makeConfiguration())
        webView.translatesAutoresizingMaskIntoConstraints = false
        webView.navigationDelegate = context.coordinator
        webView.allowsBackForwardNavigationGestures = true
        load(into: webView)
        return webView
    }

    func updateUIView(_ webView: WKWebView, context: Context) {
        if context.coordinator.lastSource != source {
            context.coordinator.lastSource = source
            load(into: webView)
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(source) }

    private func makeConfiguration() -> WKWebViewConfiguration {
        let config = WKWebViewConfiguration()
        config.defaultWebpagePreferences.allowsContentJavaScript = true
        return config
    }

    private func load(into webView: WKWebView) {
        switch source {
        case .html(let content, let baseURL):
            print("[HtmlReportWebView] loadHTMLString chars=\(content.count) baseURL=\(baseURL.absoluteString) firstChars=\(content.prefix(80))")
            webView.loadHTMLString(content, baseURL: baseURL)
        case .local(let indexURL, let baseDirectory):
            print("[HtmlReportWebView] loadFileURL indexURL=\(indexURL.path) baseDir=\(baseDirectory.path)")
            webView.loadFileURL(indexURL, allowingReadAccessTo: baseDirectory)
        }
    }
}

// MARK: macOS

#else
extension HtmlReportWebView: NSViewRepresentable {
    func makeNSView(context: Context) -> WKWebView {
        let webView = WKWebView(frame: .zero, configuration: makeConfiguration(context.coordinator.schemeHandler))
        webView.translatesAutoresizingMaskIntoConstraints = false
        webView.navigationDelegate = context.coordinator
        webView.allowsBackForwardNavigationGestures = true
        load(into: webView, coordinator: context.coordinator)
        return webView
    }

    func updateNSView(_ webView: WKWebView, context: Context) {
        if context.coordinator.lastSource != source {
            context.coordinator.lastSource = source
            load(into: webView, coordinator: context.coordinator)
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator(source) }

    private func makeConfiguration(_ handler: TenexBundleSchemeHandler) -> WKWebViewConfiguration {
        let config = WKWebViewConfiguration()
        config.defaultWebpagePreferences.allowsContentJavaScript = true
        config.setURLSchemeHandler(handler, forURLScheme: "tenex-file")
        return config
    }

    private func load(into webView: WKWebView, coordinator: Coordinator) {
        switch source {
        case .html(let content, let baseURL):
            webView.loadHTMLString(content, baseURL: baseURL)
        case .local(let indexURL, let baseDirectory):
            coordinator.schemeHandler.baseDirectory = baseDirectory
            webView.load(URLRequest(url: coordinator.schemeHandler.url(for: indexURL)))
        }
    }
}

final class TenexBundleSchemeHandler: NSObject, WKURLSchemeHandler {
    var baseDirectory: URL?

    private static let mimeTypes: [String: String] = [
        "html": "text/html", "htm": "text/html",
        "css": "text/css", "js": "application/javascript",
        "json": "application/json", "svg": "image/svg+xml",
        "png": "image/png", "jpg": "image/jpeg", "jpeg": "image/jpeg",
        "gif": "image/gif", "webp": "image/webp",
        "woff": "font/woff", "woff2": "font/woff2", "ttf": "font/ttf",
    ]

    func webView(_ webView: WKWebView, start urlSchemeTask: any WKURLSchemeTask) {
        guard let base = baseDirectory, let requestURL = urlSchemeTask.request.url else {
            urlSchemeTask.didFailWithError(URLError(.fileDoesNotExist))
            return
        }
        let relative = requestURL.path.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        let fileURL = (relative.isEmpty ? base.appendingPathComponent("index.html") : base.appendingPathComponent(relative))
            .standardizedFileURL
        let basePath = base.standardizedFileURL.path
        let readablePath = basePath.hasSuffix("/") ? basePath : basePath + "/"
        guard fileURL.path == basePath || fileURL.path.hasPrefix(readablePath) else {
            urlSchemeTask.didFailWithError(URLError(.noPermissionsToReadFile))
            return
        }
        do {
            let data = try Data(contentsOf: fileURL)
            let ext = fileURL.pathExtension.lowercased()
            let mime = Self.mimeTypes[ext] ?? "application/octet-stream"
            let response = URLResponse(url: requestURL, mimeType: mime, expectedContentLength: data.count, textEncodingName: "utf-8")
            urlSchemeTask.didReceive(response)
            urlSchemeTask.didReceive(data)
            urlSchemeTask.didFinish()
        } catch {
            urlSchemeTask.didFailWithError(error)
        }
    }

    func webView(_ webView: WKWebView, stop urlSchemeTask: any WKURLSchemeTask) {}

    func url(for fileURL: URL) -> URL {
        guard let base = baseDirectory?.standardizedFileURL else {
            return URL(string: "tenex-file://localhost/index.html")!
        }
        let standardizedFile = fileURL.standardizedFileURL
        let basePath = base.path.hasSuffix("/") ? base.path : base.path + "/"
        let relativePath = standardizedFile.path.hasPrefix(basePath)
            ? String(standardizedFile.path.dropFirst(basePath.count))
            : standardizedFile.lastPathComponent
        var components = URLComponents()
        components.scheme = "tenex-file"
        components.host = "localhost"
        components.path = "/" + relativePath
        return components.url ?? URL(string: "tenex-file://localhost/index.html")!
    }
}
#endif

// MARK: - Coordinator (shared)

extension HtmlReportWebView {
    final class Coordinator: NSObject, WKNavigationDelegate {
        var lastSource: HtmlReportSource
        #if os(macOS)
        let schemeHandler = TenexBundleSchemeHandler()
        #endif

        init(_ source: HtmlReportSource) {
            self.lastSource = source
        }

        func webView(_ webView: WKWebView, didStartProvisionalNavigation navigation: WKNavigation!) {
            print("[HtmlReportWebView] didStartProvisionalNavigation url=\(webView.url?.absoluteString ?? "nil")")
        }
        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            print("[HtmlReportWebView] didFinish url=\(webView.url?.absoluteString ?? "nil")")
        }
        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
            print("[HtmlReportWebView][ERROR] didFail error=\(error.localizedDescription)")
        }
        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
            print("[HtmlReportWebView][ERROR] didFailProvisionalNavigation error=\(error.localizedDescription)")
        }
    }
}
