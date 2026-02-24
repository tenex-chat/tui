use std::collections::HashSet;

use nostrdb::Note;

/// BookmarkList - kind:14202 replaceable event for bookmarking nudges and skills.
///
/// Each `["e", "<event-id>"]` tag represents a bookmarked nudge or skill.
/// The event is replaceable: publishing a new one replaces the old one.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BookmarkList {
    /// Pubkey of the bookmark list owner (hex-encoded)
    pub pubkey: String,
    /// Set of bookmarked nudge/skill event IDs (hex-encoded)
    pub bookmarked_ids: HashSet<String>,
    /// Unix timestamp when this list was last updated
    pub last_updated: i64,
}

impl BookmarkList {
    /// Parse a BookmarkList from a kind:14202 nostrdb note.
    ///
    /// Each `["e", "<id>"]` tag is treated as a bookmarked item ID.
    /// Returns `None` if the note is not kind:14202.
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 14202 {
            return None;
        }

        let pubkey = hex::encode(note.pubkey());
        let last_updated = note.created_at() as i64;
        let mut bookmarked_ids = HashSet::new();

        for tag in note.tags() {
            if tag.count() < 2 {
                continue;
            }

            // Check if the tag name is "e"
            let is_e_tag = tag
                .get(0)
                .and_then(|t| t.variant().str())
                .map(|s| s == "e")
                .unwrap_or(false);

            if !is_e_tag {
                continue;
            }

            // Read the event ID - nostrdb stores 64-char hex IDs as binary (Id variant)
            if let Some(id_elem) = tag.get(1) {
                let id = match id_elem.variant() {
                    nostrdb::NdbStrVariant::Str(s) => s.to_string(),
                    nostrdb::NdbStrVariant::Id(bytes) => hex::encode(bytes),
                };
                if !id.is_empty() {
                    bookmarked_ids.insert(id);
                }
            }
        }

        Some(BookmarkList {
            pubkey,
            bookmarked_ids,
            last_updated,
        })
    }

    /// Check if a given item ID is in this bookmark list.
    pub fn contains(&self, item_id: &str) -> bool {
        self.bookmarked_ids.contains(item_id)
    }
}
