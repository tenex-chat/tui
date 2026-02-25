import XCTest
@testable import TenexMVP

final class AttachmentParserTests: XCTestCase {

    // MARK: - No Attachments (Fast Path)

    func testPlainTextReturnsNil() {
        let content = "Hello, this is a normal message."
        XCTAssertNil(AttachmentParser.parse(content))
    }

    func testEmptyStringReturnsNil() {
        XCTAssertNil(AttachmentParser.parse(""))
    }

    func testMarkdownWithoutAttachmentsReturnsNil() {
        let content = """
        # Heading

        Some **bold** text and `code`.

        - List item 1
        - List item 2
        """
        XCTAssertNil(AttachmentParser.parse(content))
    }

    func testInlineReferenceWithoutSeparatorReturnsNil() {
        // Has [Text Attachment 1] reference but no ---- separator with headers below
        let content = "[Text Attachment 1] some text but no attachment section"
        XCTAssertNil(AttachmentParser.parse(content))
    }

    // MARK: - Single Attachment

    func testSingleTextAttachment() {
        let content = """
        [Text Attachment 1] tell me more about the "Steinberger → OpenAI" thing

        ----

        -- Text Attachment 1 --

        This is the full attachment content that was previously displayed inline.
        """

        let result = AttachmentParser.parse(content)
        XCTAssertNotNil(result)

        guard let parsed = result else { return }

        // Should have 2 segments: attachment reference + text
        XCTAssertEqual(parsed.segments.count, 2)

        // First segment is the attachment reference
        if case .attachmentReference(let label) = parsed.segments[0] {
            XCTAssertEqual(label, "Text Attachment 1")
        } else {
            XCTFail("Expected attachment reference as first segment")
        }

        // Second segment is the text after the reference
        if case .text(let text) = parsed.segments[1] {
            XCTAssertTrue(text.contains("Steinberger"))
        } else {
            XCTFail("Expected text as second segment")
        }

        // Attachment content
        XCTAssertEqual(parsed.attachments.count, 1)
        XCTAssertEqual(
            parsed.attachments["Text Attachment 1"],
            "This is the full attachment content that was previously displayed inline."
        )
    }

    // MARK: - Multiple Attachments

    func testMultipleAttachments() {
        let content = """
        [Text Attachment 1] first question

        [Text Attachment 2] second question

        ----

        -- Text Attachment 1 --

        Content of the first attachment.

        -- Text Attachment 2 --

        Content of the second attachment.
        """

        let result = AttachmentParser.parse(content)
        XCTAssertNotNil(result)

        guard let parsed = result else { return }

        XCTAssertEqual(parsed.attachments.count, 2)
        XCTAssertEqual(parsed.attachments["Text Attachment 1"], "Content of the first attachment.")
        XCTAssertEqual(parsed.attachments["Text Attachment 2"], "Content of the second attachment.")

        // Should have segments for both references and text between them
        let refCount = parsed.segments.filter {
            if case .attachmentReference = $0 { return true }
            return false
        }.count
        XCTAssertEqual(refCount, 2)
    }

    // MARK: - Attachment Label Variants

    func testShortAttachmentLabel() {
        let content = """
        [Attachment 1] some text

        ----

        -- Attachment 1 --

        Attachment content here.
        """

        let result = AttachmentParser.parse(content)
        XCTAssertNotNil(result)

        guard let parsed = result else { return }

        XCTAssertEqual(parsed.attachments.count, 1)
        XCTAssertNotNil(parsed.attachments["Attachment 1"])
    }

    // MARK: - Separator Variants

    func testTripleDashSeparator() {
        let content = """
        [Text Attachment 1] question

        ---

        -- Text Attachment 1 --

        Content below triple dash.
        """

        let result = AttachmentParser.parse(content)
        XCTAssertNotNil(result)

        guard let parsed = result else { return }

        XCTAssertEqual(parsed.attachments["Text Attachment 1"], "Content below triple dash.")
    }

    func testLongDashSeparator() {
        let content = """
        [Text Attachment 1] question

        --------

        -- Text Attachment 1 --

        Content below long dash.
        """

        let result = AttachmentParser.parse(content)
        XCTAssertNotNil(result)

        guard let parsed = result else { return }

        XCTAssertEqual(parsed.attachments["Text Attachment 1"], "Content below long dash.")
    }

    // MARK: - Edge Cases

    func testAttachmentWithMarkdownContent() {
        let content = """
        [Text Attachment 1] describe this

        ----

        -- Text Attachment 1 --

        # Heading

        Some **bold** text and `code blocks`.

        - List item 1
        - List item 2
        """

        let result = AttachmentParser.parse(content)
        XCTAssertNotNil(result)

        guard let parsed = result else { return }

        let attachmentContent = parsed.attachments["Text Attachment 1"]!
        XCTAssertTrue(attachmentContent.contains("# Heading"))
        XCTAssertTrue(attachmentContent.contains("**bold**"))
        XCTAssertTrue(attachmentContent.contains("- List item 1"))
    }

    func testTextBeforeAndAfterReference() {
        let content = """
        Some preamble text.

        [Text Attachment 1] and a question about it.

        Some trailing text.

        ----

        -- Text Attachment 1 --

        Attachment content.
        """

        let result = AttachmentParser.parse(content)
        XCTAssertNotNil(result)

        guard let parsed = result else { return }

        // Should have text before, reference, and text after
        XCTAssertTrue(parsed.segments.count >= 3)

        // First segment should be the preamble
        if case .text(let text) = parsed.segments[0] {
            XCTAssertTrue(text.contains("preamble"))
        } else {
            XCTFail("Expected text segment for preamble")
        }
    }

    func testSeparatorWithoutAttachmentHeadersIsIgnored() {
        // Has an inline reference and a separator, but no attachment headers below
        let content = """
        [Text Attachment 1] question

        ----

        Just some regular text below the separator, no headers.
        """

        XCTAssertNil(AttachmentParser.parse(content))
    }
}
