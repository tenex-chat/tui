use nostrdb::Note;

/// Agent Lesson - kind:4129 events containing learning insights
#[derive(Debug, Clone)]
pub struct Lesson {
    pub id: String,
    pub pubkey: String,
    pub title: String,
    pub content: String,
    pub detailed: Option<String>,
    pub reasoning: Option<String>,
    pub metacognition: Option<String>,
    pub reflection: Option<String>,
    pub category: Option<String>,
    pub created_at: u64,
}

impl Lesson {
    /// Parse a Lesson from a kind:4129 note
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 4129 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let content = note.content().to_string();
        let created_at = note.created_at();

        let mut title: Option<String> = None;
        let mut detailed: Option<String> = None;
        let mut reasoning: Option<String> = None;
        let mut metacognition: Option<String> = None;
        let mut reflection: Option<String> = None;
        let mut category: Option<String> = None;

        // Parse tags
        for tag in note.tags() {
            if tag.count() >= 2 {
                if let Some(tag_name) = tag.get(0).and_then(|t| t.variant().str()) {
                    if let Some(value) = tag.get(1).and_then(|t| t.variant().str()) {
                        match tag_name {
                            "title" => title = Some(value.to_string()),
                            "detailed" => detailed = Some(value.to_string()),
                            "reasoning" => reasoning = Some(value.to_string()),
                            "metacognition" => metacognition = Some(value.to_string()),
                            "reflection" => reflection = Some(value.to_string()),
                            "category" => category = Some(value.to_string()),
                            _ => {}
                        }
                    }
                }
            }
        }

        Some(Lesson {
            id,
            pubkey,
            title: title.unwrap_or_else(|| "Untitled Lesson".to_string()),
            content,
            detailed,
            reasoning,
            metacognition,
            reflection,
            category,
            created_at,
        })
    }

    /// Calculate estimated reading time (assumes 200 words per minute)
    pub fn reading_time(&self) -> String {
        let combined_text = vec![
            Some(&self.content),
            self.detailed.as_ref(),
            self.reasoning.as_ref(),
            self.metacognition.as_ref(),
            self.reflection.as_ref(),
        ]
        .into_iter()
        .flatten()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(" ");

        let words = combined_text.split_whitespace().count();
        let minutes = (words as f64 / 200.0).ceil() as usize;
        let minutes = minutes.max(1);

        format!("{} min read", minutes)
    }

    /// Get sections that have content (for pager navigation)
    pub fn sections(&self) -> Vec<(&str, &str)> {
        let mut sections = vec![("Summary", self.content.as_str())];

        if let Some(ref detailed) = self.detailed {
            sections.push(("Detailed", detailed.as_str()));
        }
        if let Some(ref reasoning) = self.reasoning {
            sections.push(("Reasoning", reasoning.as_str()));
        }
        if let Some(ref metacognition) = self.metacognition {
            sections.push(("Metacognition", metacognition.as_str()));
        }
        if let Some(ref reflection) = self.reflection {
            sections.push(("Reflection", reflection.as_str()));
        }

        sections
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reading_time() {
        let lesson = Lesson {
            id: "test".to_string(),
            pubkey: "test".to_string(),
            title: "Test".to_string(),
            content: "word ".repeat(200), // 200 words = 1 min
            detailed: None,
            reasoning: None,
            metacognition: None,
            reflection: None,
            category: None,
            created_at: 0,
        };

        assert_eq!(lesson.reading_time(), "1 min read");
    }

    #[test]
    fn test_sections() {
        let lesson = Lesson {
            id: "test".to_string(),
            pubkey: "test".to_string(),
            title: "Test".to_string(),
            content: "Summary content".to_string(),
            detailed: Some("Detailed content".to_string()),
            reasoning: None,
            metacognition: Some("Meta content".to_string()),
            reflection: None,
            category: None,
            created_at: 0,
        };

        let sections = lesson.sections();
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].0, "Summary");
        assert_eq!(sections[1].0, "Detailed");
        assert_eq!(sections[2].0, "Metacognition");
    }
}
