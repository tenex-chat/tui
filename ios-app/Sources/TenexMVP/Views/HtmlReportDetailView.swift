import SwiftUI
import WebKit
#if os(iOS)
import ZIPFoundation
#endif

// MARK: - HtmlReportSource

enum HtmlReportSource: Equatable {
    case html(content: String, baseURL: URL)
    case local(indexURL: URL, baseDirectory: URL)
}

// MARK: - HtmlReportDetailView

struct HtmlReportDetailView: View {
    let report: HtmlReport
    @Environment(TenexCoreManager.self) private var coreManager

    @State private var loadState: LoadState = .loading
    @State private var loadedSource: HtmlReportSource?
    @State private var resolvedConversation: ConversationFullInfo?
    @State private var showConversation = false

    private enum LoadState: Equatable {
        case loading
        case loaded
        case failed(String)
    }

    var body: some View {
        content
            .navigationTitle(report.title.isEmpty ? "HTML Report" : report.title)
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                if !report.conversationId.isEmpty {
                    ToolbarItem(placement: .primaryAction) {
                        openConversationButton
                    }
                }
            }
            .task {
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

    // MARK: - Loading

    private func loadReport() async {
        guard let url = URL(string: report.url) else {
            await MainActor.run { loadState = .failed("Invalid URL: \(report.url)") }
            return
        }

        do {
            let (data, response) = try await URLSession.shared.data(from: url)
            if let http = response as? HTTPURLResponse, !(200..<300).contains(http.statusCode) {
                throw HtmlReportError.httpStatus(http.statusCode)
            }
            let contentType = (response as? HTTPURLResponse)?.value(forHTTPHeaderField: "Content-Type") ?? ""
            let isZip = contentType.contains("zip") || report.isZip

            if isZip {
                let source = try await extractZip(data: data, eventId: report.eventId)
                await MainActor.run { loadedSource = source; loadState = .loaded }
            } else {
                let htmlString = String(data: data, encoding: .utf8) ?? String(data: data, encoding: .isoLatin1) ?? ""
                await MainActor.run {
                    loadedSource = .html(content: htmlString, baseURL: url)
                    loadState = .loaded
                }
            }
        } catch {
            await MainActor.run { loadState = .failed(error.localizedDescription) }
        }
    }

    private func resolveConversation() async {
        let conversationId = report.conversationId
        guard !conversationId.isEmpty else { return }
        let matches = await coreManager.core.getConversationsByIds(conversationIds: [conversationId])
        await MainActor.run { resolvedConversation = matches.first }
    }

    // MARK: - Zip extraction

    private func extractZip(data: Data, eventId: String) async throws -> HtmlReportSource {
        let fileManager = FileManager.default
        let baseDir = fileManager.temporaryDirectory
            .appendingPathComponent("tenex-html-reports", isDirectory: true)
            .appendingPathComponent(eventId, isDirectory: true)

        if fileManager.fileExists(atPath: baseDir.path) {
            try? fileManager.removeItem(at: baseDir)
        }
        try fileManager.createDirectory(at: baseDir, withIntermediateDirectories: true)

        let zipURL = baseDir.appendingPathComponent("bundle.zip")
        try data.write(to: zipURL, options: .atomic)

        #if os(macOS)
        try runUnzip(zipURL: zipURL, into: baseDir)
        #else
        try fileManager.unzipItem(at: zipURL, to: baseDir)
        #endif
        try? fileManager.removeItem(at: zipURL)

        guard let indexURL = locateIndexHTML(in: baseDir) else {
            throw HtmlReportError.missingIndex
        }

        return .local(indexURL: indexURL, baseDirectory: baseDir)
    }

    #if os(macOS)
    private func runUnzip(zipURL: URL, into directory: URL) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/unzip")
        process.arguments = ["-o", "-q", zipURL.path, "-d", directory.path]
        let stderr = Pipe()
        process.standardError = stderr
        process.standardOutput = Pipe()
        try process.run()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else {
            let errorData = stderr.fileHandleForReading.readDataToEndOfFile()
            let message = String(data: errorData, encoding: .utf8) ?? "unzip exited \(process.terminationStatus)"
            throw HtmlReportError.unzipFailed(message)
        }
    }
    #endif

    private func locateIndexHTML(in directory: URL) -> URL? {
        let fileManager = FileManager.default
        guard let enumerator = fileManager.enumerator(
            at: directory,
            includingPropertiesForKeys: [.isRegularFileKey],
            options: [.skipsHiddenFiles]
        ) else { return nil }

        var firstHtml: URL?
        var bestIndex: URL?
        var bestIndexDepth = Int.max

        for case let fileURL as URL in enumerator {
            guard (try? fileURL.resourceValues(forKeys: [.isRegularFileKey]))?.isRegularFile == true else { continue }
            let name = fileURL.lastPathComponent.lowercased()
            let depth = fileURL.pathComponents.count
            if name == "index.html" {
                if depth < bestIndexDepth { bestIndex = fileURL; bestIndexDepth = depth }
            } else if (name.hasSuffix(".html") || name.hasSuffix(".htm")) && firstHtml == nil {
                firstHtml = fileURL
            }
        }
        return bestIndex ?? firstHtml
    }
}

// MARK: - Errors

private enum HtmlReportError: LocalizedError {
    case httpStatus(Int)
    case unzipFailed(String)
    case missingIndex

    var errorDescription: String? {
        switch self {
        case .httpStatus(let code): return "Server returned HTTP \(code)"
        case .unzipFailed(let message): return "Failed to unzip bundle: \(message)"
        case .missingIndex: return "Bundle does not contain an index.html"
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
            webView.loadHTMLString(content, baseURL: baseURL)
        case .local(let indexURL, let baseDirectory):
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
        case .local(_, let baseDirectory):
            coordinator.schemeHandler.baseDirectory = baseDirectory
            webView.load(URLRequest(url: URL(string: "tenex-file://localhost/index.html")!))
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
        let fileURL = relative.isEmpty ? base.appendingPathComponent("index.html") : base.appendingPathComponent(relative)
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
    }
}
