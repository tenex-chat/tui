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
    /// Create from JSON string (for ephemeral events received via DataChange channel)
    ///
    /// Kind:24133 event structure:
    /// - e-tag: conversation_id (event being processed)
    /// - p-tags (lowercase): agent pubkeys currently working
    /// - P-tag (uppercase): user pubkey (ignored for now)
    /// - a-tag: project coordinate
    pub fn from_json(json: &str) -> Option<Self> {
        let event: serde_json::Value = serde_json::from_str(json).ok()?;

        let kind = event.get("kind")?.as_u64()?;
        if kind != 24133 {
            return None;
        }

        let created_at = event.get("created_at")?.as_u64()?;

        let tags_value = event.get("tags")?.as_array()?;

        // Collect all e-tags, tracking root vs non-root separately
        // Use filter_map to be tolerant of malformed tags
        let mut non_root_e_tags: Vec<String> = Vec::new();
        let mut root_e_tags: Vec<String> = Vec::new();
        let mut agent_pubkeys: Vec<String> = Vec::new();
        let mut project_coordinate: Option<String> = None;
        let mut thread_id: Option<String> = None;

        for tag_value in tags_value {
            // Skip malformed tags gracefully (tolerant parsing like ProjectStatus)
            let Some(tag_arr) = tag_value.as_array() else { continue };
            if tag_arr.is_empty() {
                continue;
            }

            let Some(tag_name) = tag_arr.first().and_then(|v| v.as_str()) else { continue };

            match tag_name {
                "e" => {
                    // e-tag: ["e", "<event_id>", "<relay>", "<marker>"]
                    // Check for "root" marker in position 3
                    let marker = tag_arr.get(3).and_then(|v| v.as_str());
                    if let Some(id) = tag_arr.get(1).and_then(|v| v.as_str()) {
                        if marker == Some("root") {
                            root_e_tags.push(id.to_string());
                        } else {
                            non_root_e_tags.push(id.to_string());
                        }
                    }
                }
                "q" => {
                    // Quote tag - points to thread root (conversation)
                    if thread_id.is_none() {
                        if let Some(s) = tag_arr.get(1).and_then(|v| v.as_str()) {
                            thread_id = Some(s.to_string());
                        }
                    }
                }
                "p" => {
                    // Agent pubkey (lowercase p)
                    if let Some(s) = tag_arr.get(1).and_then(|v| v.as_str()) {
                        agent_pubkeys.push(s.to_string());
                    }
                }
                "a" => {
                    // Project coordinate: ["a", "31933:<user_pubkey>:<project_id>", "<relay>", ""]
                    if project_coordinate.is_none() {
                        if let Some(s) = tag_arr.get(1).and_then(|v| v.as_str()) {
                            project_coordinate = Some(s.to_string());
                        }
                    }
                }
                // "P" (uppercase) is user pubkey - ignored for OperationsStatus
                _ => {}
            }
        }

        // After loop: prefer first non-root e-tag, fallback to first root e-tag
        let event_id = non_root_e_tags.first()
            .or(root_e_tags.first())
            .cloned()?;
        let project_coordinate = project_coordinate.unwrap_or_default();
        // Use q-tag for thread_id if available, otherwise fall back to first root e-tag
        let thread_id = thread_id.or_else(|| root_e_tags.first().cloned());

        Some(OperationsStatus {
            event_id,
            agent_pubkeys,
            project_coordinate,
            created_at,
            thread_id,
        })
    }

    /// Create from nostrdb::Note
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 24133 {
            return None;
        }

        // Collect all e-tags, tracking root vs non-root separately
        // Tolerant parsing - skip malformed tags gracefully
        let mut non_root_e_tags: Vec<String> = Vec::new();
        let mut root_e_tags: Vec<String> = Vec::new();
        let mut agent_pubkeys: Vec<String> = Vec::new();
        let mut project_coordinate: Option<String> = None;
        let mut thread_id: Option<String> = None;

        for tag in note.tags() {
            // Skip malformed tags gracefully
            if tag.count() < 2 {
                continue;
            }

            let Some(tag_name) = tag.get(0).and_then(|t| t.variant().str()) else { continue };

            match tag_name {
                "e" => {
                    // Check for "root" marker in position 3
                    let marker = tag.get(3).and_then(|t| t.variant().str());
                    let id_value = if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                        Some(s.to_string())
                    } else {
                        tag.get(1).and_then(|t| t.variant().id()).map(hex::encode)
                    };

                    if let Some(id) = id_value {
                        if marker == Some("root") {
                            root_e_tags.push(id);
                        } else {
                            non_root_e_tags.push(id);
                        }
                    }
                }
                "q" => {
                    // Quote tag - points to thread root (conversation)
                    if thread_id.is_none() {
                        if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                            thread_id = Some(s.to_string());
                        } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                            thread_id = Some(hex::encode(id_bytes));
                        }
                    }
                }
                "p" => {
                    // Agent pubkey (lowercase p) - try string first, then id bytes
                    if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                        agent_pubkeys.push(s.to_string());
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        agent_pubkeys.push(hex::encode(id_bytes));
                    }
                }
                "a" => {
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

        // After loop: prefer first non-root e-tag, fallback to first root e-tag
        let event_id = non_root_e_tags.first()
            .or(root_e_tags.first())
            .cloned()?;
        let project_coordinate = project_coordinate.unwrap_or_default();
        // Use q-tag for thread_id if available, otherwise fall back to first root e-tag
        let thread_id = thread_id.or_else(|| root_e_tags.first().cloned());

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
