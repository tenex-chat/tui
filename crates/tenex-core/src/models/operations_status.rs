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
    /// Thread ID (conversation root) - extracted from q-tag or e-tag with "root" marker
    pub thread_id: Option<String>,
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
        let mut thread_id: Option<String> = None;
        let mut root_e_tag: Option<String> = None;

        for tag in note.tags() {
            if tag.count() < 2 {
                continue;
            }

            let tag_name = tag.get(0).and_then(|t| t.variant().str());

            match tag_name {
                Some("e") => {
                    // Check for "root" marker in position 3
                    let marker = tag.get(3).and_then(|t| t.variant().str());
                    let id_value = if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                        Some(s.to_string())
                    } else {
                        tag.get(1).and_then(|t| t.variant().id()).map(hex::encode)
                    };

                    if let Some(id) = id_value {
                        if marker == Some("root") {
                            root_e_tag = Some(id.clone());
                        }
                        // First e-tag without "root" is the event being processed
                        if event_id.is_none() && marker != Some("root") {
                            event_id = Some(id);
                        } else if event_id.is_none() {
                            // If all e-tags are "root" marked, use the first one as event_id
                            event_id = Some(id);
                        }
                    }
                }
                Some("q") => {
                    // Quote tag - points to thread root (conversation)
                    if thread_id.is_none() {
                        if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                            thread_id = Some(s.to_string());
                        } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                            thread_id = Some(hex::encode(id_bytes));
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
        // Use q-tag for thread_id if available, otherwise fall back to e-tag with "root" marker
        let thread_id = thread_id.or(root_e_tag);

        Some(OperationsStatus {
            event_id,
            agent_pubkeys,
            project_coordinate,
            created_at: note.created_at(),
            thread_id,
        })
    }

    /// Returns true if there are agents actively working
    pub fn is_active(&self) -> bool {
        !self.agent_pubkeys.is_empty()
    }
}
