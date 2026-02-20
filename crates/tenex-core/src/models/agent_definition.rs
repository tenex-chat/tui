use nostrdb::Note;

use crate::constants::{DEFAULT_AGENT_NAME, DEFAULT_AGENT_ROLE};

/// Agent Definition - kind:4199 events describing AI agents
#[derive(Debug, Clone, uniffi::Record)]
pub struct AgentDefinition {
    pub id: String,
    pub pubkey: String,
    pub d_tag: String,
    pub name: String,
    pub description: String,
    pub role: String,
    pub instructions: String,
    pub picture: Option<String>,
    pub version: Option<String>,
    pub model: Option<String>,
    pub tools: Vec<String>,
    pub mcp_servers: Vec<String>,
    pub use_criteria: Vec<String>,
    /// File attachment event IDs (e-tags referencing NIP-94 kind:1063 events)
    pub file_ids: Vec<String>,
    pub created_at: u64,
}

impl AgentDefinition {
    /// Parse an AgentDefinition from a kind:4199 note
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 4199 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let content_instructions = note.content().to_string();
        let created_at = note.created_at();

        let mut d_tag: Option<String> = None;
        let mut name: Option<String> = None;
        let mut description: Option<String> = None;
        let mut role: Option<String> = None;
        let mut instructions_tag: Option<String> = None;
        let mut picture: Option<String> = None;
        let mut version: Option<String> = None;
        let mut model: Option<String> = None;
        let mut tools: Vec<String> = Vec::new();
        let mut mcp_servers: Vec<String> = Vec::new();
        let mut use_criteria: Vec<String> = Vec::new();
        let mut file_ids: Vec<String> = Vec::new();

        for tag in note.tags() {
            if tag.count() >= 2 {
                if let Some(tag_name) = tag.get(0).and_then(|t| t.variant().str()) {
                    if tag_name == "e" {
                        if let Some(event_id) = tag.get(1).map(|t| match t.variant() {
                            nostrdb::NdbStrVariant::Str(s) => s.to_string(),
                            nostrdb::NdbStrVariant::Id(bytes) => hex::encode(bytes),
                        }) {
                            file_ids.push(event_id);
                        }
                        continue;
                    }

                    if let Some(value) = tag.get(1).and_then(|t| t.variant().str()) {
                        match tag_name {
                            "d" => d_tag = Some(value.to_string()),
                            "title" => name = Some(value.to_string()),
                            "description" => description = Some(value.to_string()),
                            "role" => role = Some(value.to_string()),
                            "instructions" => instructions_tag = Some(value.to_string()),
                            "picture" | "image" => picture = Some(value.to_string()),
                            "ver" => version = Some(value.to_string()),
                            "version" => {
                                if version.is_none() {
                                    version = Some(value.to_string());
                                }
                            }
                            "model" => model = Some(value.to_string()),
                            "tool" => tools.push(value.to_string()),
                            "mcp" => mcp_servers.push(value.to_string()),
                            "use-criteria" => use_criteria.push(value.to_string()),
                            _ => {}
                        }
                    }
                }
            }
        }

        Some(AgentDefinition {
            id,
            pubkey,
            d_tag: d_tag.unwrap_or_default(),
            name: name.unwrap_or_else(|| DEFAULT_AGENT_NAME.to_string()),
            description: description.unwrap_or_default(),
            role: role.unwrap_or_else(|| DEFAULT_AGENT_ROLE.to_string()),
            instructions: instructions_tag.unwrap_or(content_instructions),
            picture,
            version,
            model,
            tools,
            mcp_servers,
            use_criteria,
            file_ids,
            created_at,
        })
    }

    /// Get a short preview of the description
    pub fn description_preview(&self, max_chars: usize) -> String {
        if self.description.len() <= max_chars {
            self.description.clone()
        } else {
            format!("{}...", &self.description[..max_chars.saturating_sub(3)])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{
        events::{ingest_events, wait_for_event_processing},
        Database,
    };
    use nostr_sdk::prelude::*;
    use nostrdb::{Filter, Transaction};
    use tempfile::tempdir;

    fn parse_agent_from_builder(builder: EventBuilder) -> AgentDefinition {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = builder.sign_with_keys(&keys).unwrap();
        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = Filter::new().kinds([4199]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        AgentDefinition::from_note(&note).expect("should parse agent definition")
    }

    #[test]
    fn parses_ver_tag_with_priority_over_legacy_version() {
        let agent = parse_agent_from_builder(
            EventBuilder::new(Kind::Custom(4199), "content instructions")
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                    vec!["Agent".to_string()],
                ))
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("version")),
                    vec!["1".to_string()],
                ))
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("ver")),
                    vec!["2".to_string()],
                )),
        );

        assert_eq!(agent.version.as_deref(), Some("2"));
    }

    #[test]
    fn parses_legacy_version_tag() {
        let agent = parse_agent_from_builder(
            EventBuilder::new(Kind::Custom(4199), "content instructions")
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                    vec!["Agent".to_string()],
                ))
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("version")),
                    vec!["7".to_string()],
                )),
        );

        assert_eq!(agent.version.as_deref(), Some("7"));
    }

    #[test]
    fn parses_instructions_tag_and_falls_back_to_content() {
        let from_tag = parse_agent_from_builder(
            EventBuilder::new(Kind::Custom(4199), "legacy content")
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                    vec!["Agent".to_string()],
                ))
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("instructions")),
                    vec!["tag instructions".to_string()],
                )),
        );
        assert_eq!(from_tag.instructions, "tag instructions");

        let from_content = parse_agent_from_builder(
            EventBuilder::new(Kind::Custom(4199), "content fallback").tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Agent".to_string()],
            )),
        );
        assert_eq!(from_content.instructions, "content fallback");
    }

    #[test]
    fn parses_e_tags_into_file_ids() {
        let file_event_id = "abc123def456789012345678901234567890123456789012345678901234abcd";
        let agent = parse_agent_from_builder(
            EventBuilder::new(Kind::Custom(4199), "instructions")
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                    vec!["Agent".to_string()],
                ))
                .tag(Tag::custom(
                    TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                    vec![file_event_id.to_string()],
                )),
        );

        assert_eq!(agent.file_ids.len(), 1);
        assert_eq!(agent.file_ids[0], file_event_id);
    }
}
