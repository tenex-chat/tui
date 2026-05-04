// crates/tenex-core/src/models/html_report.rs
use nostrdb::Note;

/// An HTML report (kind:1 with `["t", "html-report"]`).
///
/// Agents publish kind:1 events tagged `t:html-report` whose `url` tag points to
/// either a single HTML document or a `.zip` bundle hosted on Blossom. The note
/// content holds a human-readable description; an optional `title` tag overrides
/// the default title derived from the content.
#[derive(Debug, Clone, uniffi::Record)]
pub struct HtmlReport {
    /// Event ID (hex)
    pub event_id: String,
    /// Blossom URL of the report (single `.html` or `.zip` bundle)
    pub url: String,
    /// Display title (from `title` tag, or first 80 chars of content)
    pub title: String,
    /// Description (the kind:1 note content)
    pub description: String,
    /// Author pubkey (hex)
    pub author_pubkey: String,
    /// First `e` tag value referencing the source conversation; empty if none.
    pub conversation_id: String,
    /// First `a` tag value matching the project pattern (`31933:...`); empty if none.
    pub project_a_tag: String,
    /// Creation timestamp (unix seconds)
    pub created_at: u64,
    /// Whether the URL points to a `.zip` bundle
    pub is_zip: bool,
}

impl HtmlReport {
    /// Parse an `HtmlReport` from a nostrdb `Note`.
    ///
    /// Returns `None` if the note is not a kind:1 tagged with `t:html-report`,
    /// or if no `url` tag is present.
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 1 {
            return None;
        }

        let event_id = hex::encode(note.id());
        let author_pubkey = hex::encode(note.pubkey());
        let description = note.content().to_string();
        let created_at = note.created_at();

        let mut url = String::new();
        let mut title = String::new();
        let mut mime_type = String::new();
        let mut conversation_id = String::new();
        let mut project_a_tag = String::new();
        let mut has_html_report_tag = false;

        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());
            let tag_value = tag.get(1).and_then(|t| t.variant().str());

            match (tag_name, tag_value) {
                (Some("t"), Some(value)) if value == "html-report" => {
                    has_html_report_tag = true;
                }
                (Some("url"), Some(value)) if url.is_empty() => {
                    url = value.to_string();
                }
                (Some("title"), Some(value)) if title.is_empty() => {
                    title = value.to_string();
                }
                (Some("m"), Some(value)) if mime_type.is_empty() => {
                    mime_type = value.to_string();
                }
                (Some("e"), _) if conversation_id.is_empty() => {
                    if let Some(value) = tag_value {
                        conversation_id = value.to_string();
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        conversation_id = hex::encode(id_bytes);
                    }
                }
                (Some("a"), Some(value)) if project_a_tag.is_empty() => {
                    if value.starts_with("31933:") {
                        project_a_tag = value.to_string();
                    }
                }
                _ => {}
            }
        }

        if !has_html_report_tag || url.is_empty() {
            return None;
        }

        if title.trim().is_empty() {
            title = derive_title_from_content(&description);
        }

        let is_zip = mime_type.contains("zip") || url.to_ascii_lowercase().ends_with(".zip");

        Some(Self {
            event_id,
            url,
            title,
            description,
            author_pubkey,
            conversation_id,
            project_a_tag,
            created_at,
            is_zip,
        })
    }
}

fn derive_title_from_content(content: &str) -> String {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return "Untitled".to_string();
    }

    let first_line = trimmed
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or(trimmed);

    let snippet: String = first_line.chars().take(80).collect();
    if snippet.is_empty() {
        "Untitled".to_string()
    } else {
        snippet
    }
}

#[cfg(test)]
mod tests {
    use super::HtmlReport;
    use crate::store::{
        events::{ingest_events, wait_for_event_processing},
        Database,
    };
    use nostr_sdk::prelude::{EventBuilder, Keys, Kind, Tag, TagKind};
    use nostrdb::{Filter, Transaction};
    use tempfile::tempdir;

    fn custom_tag(name: &'static str, value: &str) -> Tag {
        Tag::custom(
            TagKind::Custom(std::borrow::Cow::Borrowed(name)),
            vec![value.to_string()],
        )
    }

    fn parse_html_report_from_builder(builder: EventBuilder) -> Option<HtmlReport> {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = builder.sign_with_keys(&keys).unwrap();
        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        HtmlReport::from_note(&note)
    }

    fn html_report_builder(content: &str) -> EventBuilder {
        EventBuilder::new(Kind::from(1), content)
            .tag(custom_tag("t", "html-report"))
            .tag(custom_tag("url", "https://blossom.example/report.html"))
    }

    #[test]
    fn parses_basic_html_report() {
        let report = parse_html_report_from_builder(html_report_builder("Quarterly summary"))
            .expect("should parse html report");

        assert_eq!(report.url, "https://blossom.example/report.html");
        assert_eq!(report.title, "Quarterly summary");
        assert_eq!(report.description, "Quarterly summary");
        assert!(!report.is_zip);
        assert!(report.conversation_id.is_empty());
        assert!(report.project_a_tag.is_empty());
    }

    #[test]
    fn rejects_kind1_without_html_report_tag() {
        let builder = EventBuilder::new(Kind::from(1), "Plain note")
            .tag(custom_tag("url", "https://blossom.example/report.html"));
        assert!(parse_html_report_from_builder(builder).is_none());
    }

    #[test]
    fn rejects_html_report_without_url() {
        let builder =
            EventBuilder::new(Kind::from(1), "Description").tag(custom_tag("t", "html-report"));
        assert!(parse_html_report_from_builder(builder).is_none());
    }

    #[test]
    fn title_tag_overrides_content_derived_title() {
        let report = parse_html_report_from_builder(
            html_report_builder("Some long description").tag(custom_tag("title", "Tagged Title")),
        )
        .unwrap();

        assert_eq!(report.title, "Tagged Title");
    }

    #[test]
    fn truncates_long_content_to_80_chars_for_title() {
        let long = "x".repeat(200);
        let report = parse_html_report_from_builder(
            EventBuilder::new(Kind::from(1), long.clone())
                .tag(custom_tag("t", "html-report"))
                .tag(custom_tag("url", "https://blossom.example/report.html")),
        )
        .unwrap();

        assert_eq!(report.title.chars().count(), 80);
    }

    #[test]
    fn detects_zip_url() {
        let report = parse_html_report_from_builder(
            EventBuilder::new(Kind::from(1), "Bundle")
                .tag(custom_tag("t", "html-report"))
                .tag(custom_tag("url", "https://blossom.example/bundle.zip")),
        )
        .unwrap();

        assert!(report.is_zip);
    }

    #[test]
    fn captures_first_e_and_project_a_tag() {
        let event_id = "a".repeat(64);
        let project_pubkey = "b".repeat(64);
        let report = parse_html_report_from_builder(
            html_report_builder("With references")
                .tag(custom_tag("e", &event_id))
                .tag(custom_tag(
                    "a",
                    &format!("31933:{}:project-slug", project_pubkey),
                ))
                .tag(custom_tag("a", "30023:other:doc")),
        )
        .unwrap();

        assert_eq!(report.conversation_id, event_id);
        assert_eq!(
            report.project_a_tag,
            format!("31933:{}:project-slug", project_pubkey)
        );
    }

    #[test]
    fn ignores_non_project_a_tags() {
        let report = parse_html_report_from_builder(
            html_report_builder("Doc reply").tag(custom_tag("a", "30023:author:doc-slug")),
        )
        .unwrap();

        assert!(report.project_a_tag.is_empty());
    }

    #[test]
    fn untitled_when_content_blank_and_no_title_tag() {
        let report = parse_html_report_from_builder(
            EventBuilder::new(Kind::from(1), "")
                .tag(custom_tag("t", "html-report"))
                .tag(custom_tag("url", "https://blossom.example/report.html")),
        )
        .unwrap();

        assert_eq!(report.title, "Untitled");
    }
}
