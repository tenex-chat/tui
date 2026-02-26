#if os(macOS)
import SwiftUI
import AppKit

enum WorkspacePasteBehavior {
    /// TUI parity: large paste becomes text attachment when >5 lines OR >500 chars.
    static func shouldBeAttachment(_ text: String) -> Bool {
        let lineCount = rustLineCount(text)
        let charCount = text.utf8.count
        return lineCount > 5 || charCount > 500
    }

    /// TUI parity smart paste: wrap known JSON/code blocks in markdown fences.
    static func smartFormatPaste(_ text: String) -> String {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)

        if trimmed.hasPrefix("```") {
            return text
        }

        if !trimmed.contains("\n") && trimmed.count < 50 {
            return text
        }

        if looksLikeJson(trimmed) {
            return "```json\n\(trimmed)\n```"
        }

        if let language = detectCodeLanguage(trimmed) {
            return "```\(language)\n\(trimmed)\n```"
        }

        return text
    }

    private static func looksLikeJson(_ text: String) -> Bool {
        (text.hasPrefix("{") && text.hasSuffix("}")) || (text.hasPrefix("[") && text.hasSuffix("]"))
    }

    /// Match Rust `str::lines().count()` behavior used by the TUI threshold check.
    private static func rustLineCount(_ text: String) -> Int {
        guard !text.isEmpty else { return 0 }
        let normalized = text.replacingOccurrences(of: "\r\n", with: "\n")
        var lineCount = normalized.split(separator: "\n", omittingEmptySubsequences: false).count
        if normalized.last == "\n" {
            lineCount -= 1
        }
        return max(lineCount, 0)
    }

    private static func detectCodeLanguage(_ text: String) -> String? {
        if (text.contains("fn ") && text.contains("->"))
            || text.contains("impl ")
            || text.contains("pub struct ")
            || text.contains("use std::")
            || text.contains("#[derive(") {
            return "rust"
        }

        if (text.contains("import ") && text.contains(" from "))
            || text.contains("export ")
            || (text.contains("const ") && text.contains(" = "))
            || text.contains("function ")
            || text.contains("=> {") {
            if text.contains(": string")
                || text.contains(": number")
                || text.contains(": boolean")
                || text.contains("interface ")
                || text.contains("<T>") {
                return "typescript"
            }
            return "javascript"
        }

        if (text.contains("def ") && text.contains(":"))
            || (text.contains("import ") && !text.contains(" from \"") && !text.contains(" from '"))
            || (text.contains("class ") && text.contains(":"))
            || text.contains("if __name__") {
            return "python"
        }

        if (text.contains("func ") && text.contains("package "))
            || (text.contains("type ") && text.contains(" struct {")) {
            return "go"
        }

        if text.hasPrefix("#!/bin/")
            || text.hasPrefix("$ ")
            || (text.contains("echo ") && text.contains("&&")) {
            return "bash"
        }

        if text.contains("<!DOCTYPE") || text.contains("<html") || text.contains("<div") {
            return "html"
        }

        if text.contains("{") && (text.contains("color:") || text.contains("display:")) {
            return "css"
        }

        let uppercase = text.uppercased()
        if uppercase.contains("SELECT ") && (uppercase.contains(" FROM ") || uppercase.contains(" WHERE ")) {
            return "sql"
        }

        return nil
    }
}

struct WorkspaceComposerTextView: NSViewRepresentable {
    @Binding var text: String
    @Binding var isFocused: Bool

    let isEnabled: Bool
    let onSubmit: () -> Void
    let onHistoryPrevious: () -> Bool
    let onHistoryNext: () -> Bool
    let transformPaste: (String) -> String

    func makeCoordinator() -> Coordinator {
        Coordinator(self)
    }

    func makeNSView(context: Context) -> NSScrollView {
        let textView = WorkspaceTextView()
        textView.delegate = context.coordinator

        textView.isRichText = false
        textView.importsGraphics = false
        textView.allowsImageEditing = false
        textView.isEditable = isEnabled
        textView.isSelectable = true
        textView.drawsBackground = false
        textView.font = NSFont.systemFont(ofSize: 19)
        textView.textColor = NSColor.labelColor.withAlphaComponent(0.94)
        textView.string = text
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.maxSize = NSSize(
            width: CGFloat.greatestFiniteMagnitude,
            height: CGFloat.greatestFiniteMagnitude
        )
        textView.textContainerInset = NSSize(width: 0, height: 0)
        textView.textContainer?.lineFragmentPadding = 0
        textView.textContainer?.widthTracksTextView = true
        textView.textContainer?.heightTracksTextView = false
        textView.isAutomaticDashSubstitutionEnabled = false
        textView.isAutomaticQuoteSubstitutionEnabled = false
        textView.isAutomaticDataDetectionEnabled = false
        textView.isAutomaticLinkDetectionEnabled = false
        textView.isAutomaticTextReplacementEnabled = false
        textView.isAutomaticSpellingCorrectionEnabled = false

        textView.onSubmit = onSubmit
        textView.onHistoryPrevious = onHistoryPrevious
        textView.onHistoryNext = onHistoryNext
        textView.transformPaste = transformPaste

        let scrollView = NSScrollView()
        scrollView.drawsBackground = false
        scrollView.borderType = .noBorder
        scrollView.hasVerticalScroller = false
        scrollView.hasHorizontalScroller = false
        scrollView.autohidesScrollers = true
        scrollView.documentView = textView

        return scrollView
    }

    func updateNSView(_ nsView: NSScrollView, context: Context) {
        guard let textView = nsView.documentView as? WorkspaceTextView else { return }

        textView.onSubmit = onSubmit
        textView.onHistoryPrevious = onHistoryPrevious
        textView.onHistoryNext = onHistoryNext
        textView.transformPaste = transformPaste
        textView.isEditable = isEnabled

        if textView.string != text {
            context.coordinator.isProgrammaticTextUpdate = true
            textView.string = text
            let end = (text as NSString).length
            textView.setSelectedRange(NSRange(location: end, length: 0))
            context.coordinator.isProgrammaticTextUpdate = false
        }

        guard let window = nsView.window else { return }
        let isFirstResponder = window.firstResponder === textView

        if isFocused && !isFirstResponder {
            window.makeFirstResponder(textView)
        } else if !isFocused && isFirstResponder {
            window.makeFirstResponder(nil)
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: WorkspaceComposerTextView
        var isProgrammaticTextUpdate = false

        init(_ parent: WorkspaceComposerTextView) {
            self.parent = parent
        }

        func textDidBeginEditing(_ notification: Notification) {
            if !parent.isFocused {
                parent.isFocused = true
            }
        }

        func textDidEndEditing(_ notification: Notification) {
            if parent.isFocused {
                parent.isFocused = false
            }
        }

        func textDidChange(_ notification: Notification) {
            guard !isProgrammaticTextUpdate,
                  let textView = notification.object as? NSTextView
            else {
                return
            }
            parent.text = textView.string
        }
    }
}

private final class WorkspaceTextView: NSTextView {
    var onSubmit: (() -> Void)?
    var onHistoryPrevious: (() -> Bool)?
    var onHistoryNext: (() -> Bool)?
    var transformPaste: ((String) -> String)?

    override func keyDown(with event: NSEvent) {
        let modifiers = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
        let hasNonShiftModifier = modifiers.contains(.command) || modifiers.contains(.option) || modifiers.contains(.control)

        switch event.keyCode {
        case 36, 76:
            if !hasNonShiftModifier {
                if modifiers.contains(.shift) {
                    insertNewline(nil)
                } else {
                    onSubmit?()
                }
                return
            }
        case 126:
            if onHistoryPrevious?() == true {
                return
            }
        case 125:
            if onHistoryNext?() == true {
                return
            }
        default:
            break
        }

        super.keyDown(with: event)
    }

    override func paste(_ sender: Any?) {
        guard isEditable else {
            super.paste(sender)
            return
        }

        if let pastedText = NSPasteboard.general.string(forType: .string) {
            let replacement = transformPaste?(pastedText) ?? pastedText
            insertText(replacement, replacementRange: selectedRange())
            return
        }

        super.paste(sender)
    }
}
#endif
