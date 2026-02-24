use nostrdb::Note;

/// Team Pack - kind:34199 events grouping agent definitions into a hireable team.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TeamPack {
    pub id: String,
    pub pubkey: String,
    pub d_tag: String,
    pub title: String,
    pub description: String,
    pub image: Option<String>,
    /// Agent definition event IDs (kind:4199) from repeated `e` tags.
    pub agent_definition_ids: Vec<String>,
    /// Team categories from repeated `c` tags.
    pub categories: Vec<String>,
    /// Hashtags from repeated `t` tags.
    pub tags: Vec<String>,
    pub created_at: u64,
}

impl TeamPack {
    /// Parse a TeamPack from a kind:34199 note.
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 34199 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let created_at = note.created_at();
        let description = note.content().to_string();

        let mut d_tag: Option<String> = None;
        let mut title: Option<String> = None;
        let mut image: Option<String> = None;
        let mut agent_definition_ids: Vec<String> = Vec::new();
        let mut categories: Vec<String> = Vec::new();
        let mut tags: Vec<String> = Vec::new();

        for tag in note.tags() {
            if tag.count() < 2 {
                continue;
            }

            let Some(tag_name) = tag.get(0).and_then(|t| t.variant().str()) else {
                continue;
            };

            // Handle e-tags first because nostrdb may store 64-char ids as Id variant.
            if tag_name == "e" {
                if let Some(agent_id) = tag.get(1).map(|t| match t.variant() {
                    nostrdb::NdbStrVariant::Str(s) => s.to_string(),
                    nostrdb::NdbStrVariant::Id(bytes) => hex::encode(bytes),
                }) {
                    agent_definition_ids.push(agent_id);
                }
                continue;
            }

            let Some(value) = tag.get(1).and_then(|t| t.variant().str()) else {
                continue;
            };

            match tag_name {
                "d" => d_tag = Some(value.to_string()),
                "title" => title = Some(value.to_string()),
                "image" | "picture" => image = Some(value.to_string()),
                "c" => categories.push(value.to_string()),
                "t" => tags.push(value.to_string()),
                _ => {}
            }
        }

        Some(Self {
            id,
            pubkey,
            d_tag: d_tag.unwrap_or_default(),
            title: title.unwrap_or_else(|| "Untitled Team".to_string()),
            description,
            image,
            agent_definition_ids,
            categories,
            tags,
            created_at,
        })
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

    fn parse_team_from_builder(builder: EventBuilder) -> TeamPack {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = builder.sign_with_keys(&keys).unwrap();
        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = Filter::new().kinds([34199]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        TeamPack::from_note(&note).expect("should parse team pack")
    }

    #[test]
    fn parses_basic_team_pack() {
        let team = parse_team_from_builder(
            EventBuilder::new(Kind::Custom(34199), "Team description markdown")
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("d")),
                    vec!["team-default".to_string()],
                ))
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                    vec!["Default Team".to_string()],
                ))
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("image")),
                    vec!["https://example.com/team.png".to_string()],
                ))
                .tag(Tag::custom(
                    TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                    vec![
                        "abc123def456789012345678901234567890123456789012345678901234abcd"
                            .to_string(),
                    ],
                ))
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("c")),
                    vec!["Marketing".to_string()],
                ))
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("t")),
                    vec!["growth".to_string()],
                )),
        );

        assert_eq!(team.d_tag, "team-default");
        assert_eq!(team.title, "Default Team");
        assert_eq!(team.description, "Team description markdown");
        assert_eq!(team.image.as_deref(), Some("https://example.com/team.png"));
        assert_eq!(team.agent_definition_ids.len(), 1);
        assert_eq!(team.categories, vec!["Marketing"]);
        assert_eq!(team.tags, vec!["growth"]);
    }

    #[test]
    fn defaults_missing_fields() {
        let team = parse_team_from_builder(EventBuilder::new(Kind::Custom(34199), ""));
        assert_eq!(team.d_tag, "");
        assert_eq!(team.title, "Untitled Team");
        assert!(team.image.is_none());
        assert!(team.agent_definition_ids.is_empty());
        assert!(team.categories.is_empty());
    }
}
