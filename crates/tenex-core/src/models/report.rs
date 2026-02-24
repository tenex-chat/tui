// crates/tenex-core/src/models/report.rs
use nostrdb::Note;

/// A report/document (kind:30023 - Article)
#[derive(Debug, Clone, uniffi::Record, serde::Serialize, serde::Deserialize)]
pub struct Report {
    /// Event ID (hex)
    pub id: String,
    /// d-tag slug (for version tracking - same slug = same document, different versions)
    pub slug: String,
    /// Project a-tag this report belongs to
    pub project_a_tag: String,
    /// Author pubkey (hex)
    pub author: String,
    /// Document title (from title tag)
    pub title: String,
    /// Summary (from summary tag, or first 160 chars of content)
    pub summary: String,
    /// Full markdown content
    pub content: String,
    /// Hashtags (t-tags)
    pub hashtags: Vec<String>,
    /// Creation timestamp
    pub created_at: u64,
    /// Calculated reading time in minutes (content length / 200 words per minute)
    pub reading_time_mins: u8,
}

impl Report {
    /// Parse a Report from a nostrdb Note (kind:30023)
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 30023 {
            return None;
        }

        let id = hex::encode(note.id());
        let author = hex::encode(note.pubkey());
        let content = note.content().to_string();
        let created_at = note.created_at();

        let mut slug = String::new();
        let mut project_a_tag = String::new();
        let mut title = String::new();
        let mut summary = String::new();
        let mut hashtags = Vec::new();

        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());
            match tag_name {
                Some("d") => {
                    if let Some(val) = tag.get(1).and_then(|t| t.variant().str()) {
                        slug = val.to_string();
                    }
                }
                Some("a") => {
                    if let Some(val) = tag.get(1).and_then(|t| t.variant().str()) {
                        project_a_tag = val.to_string();
                    }
                }
                Some("title") => {
                    if let Some(val) = tag.get(1).and_then(|t| t.variant().str()) {
                        title = val.to_string();
                    }
                }
                Some("summary") => {
                    if let Some(val) = tag.get(1).and_then(|t| t.variant().str()) {
                        summary = val.to_string();
                    }
                }
                Some("t") => {
                    if let Some(val) = tag.get(1).and_then(|t| t.variant().str()) {
                        hashtags.push(val.to_string());
                    }
                }
                _ => {}
            }
        }

        // Require slug and project_a_tag
        if slug.is_empty() || project_a_tag.is_empty() {
            return None;
        }

        // Default title from first line of content
        if title.is_empty() {
            title = content.lines().next().unwrap_or("Untitled").to_string();
        }

        // Default summary from content
        if summary.is_empty() {
            summary = content.chars().take(160).collect();
        }

        // Calculate reading time (average 200 words per minute)
        let word_count = content.split_whitespace().count();
        let reading_time_mins = ((word_count as f32 / 200.0).ceil() as u8).max(1);

        Some(Self {
            id,
            slug,
            project_a_tag,
            author,
            title,
            summary,
            content,
            hashtags,
            created_at,
            reading_time_mins,
        })
    }

    /// Get the a-tag for this report (for thread references)
    pub fn a_tag(&self) -> String {
        format!("30023:{}:{}", self.author, self.slug)
    }
}
