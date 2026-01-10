use nostrdb::Note;

/// Represents an operations status event (Nostr kind:24133)
/// Published by the backend to indicate which agents are working on which events.
///
/// Structure:
/// - e-tag: event ID being processed
/// - p-tags (lowercase): agent pubkeys currently working
/// - a-tag: project coordinate
#[derive(Debug, Clone)]
pub struct OperationsStatus {
    pub event_id: String,
    pub agent_pubkeys: Vec<String>,
    pub project_coordinate: String,
    pub created_at: u64,
}

impl OperationsStatus {
    /// Create from nostrdb::Note
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 24133 {
            return None;
        }

        let mut event_id: Option<String> = None;
        let mut agent_pubkeys: Vec<String> = Vec::new();
        let mut project_coordinate: Option<String> = None;

        for tag in note.tags() {
            if tag.count() < 2 {
                continue;
            }

            let tag_name = tag.get(0).and_then(|t| t.variant().str());

            match tag_name {
                Some("e") => {
                    // Event being processed - try string first, then id bytes
                    if event_id.is_none() {
                        if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                            event_id = Some(s.to_string());
                        } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                            event_id = Some(hex::encode(id_bytes));
                        }
                    }
                }
                Some("p") => {
                    // Agent pubkey (lowercase p) - try string first, then id bytes
                    if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                        agent_pubkeys.push(s.to_string());
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        agent_pubkeys.push(hex::encode(id_bytes));
                    }
                }
                Some("a") => {
                    // Project coordinate
                    if project_coordinate.is_none() {
                        if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                            project_coordinate = Some(s.to_string());
                        }
                    }
                }
                _ => {}
            }
        }

        let event_id = event_id?;
        let project_coordinate = project_coordinate.unwrap_or_default();

        Some(OperationsStatus {
            event_id,
            agent_pubkeys,
            project_coordinate,
            created_at: note.created_at(),
        })
    }

    /// Returns true if there are agents actively working
    pub fn is_active(&self) -> bool {
        !self.agent_pubkeys.is_empty()
    }
}
