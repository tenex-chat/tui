use nostrdb::Note;

#[derive(Debug, Clone, PartialEq)]
pub enum AskQuestion {
    SingleSelect {
        title: String,
        question: String,
        suggestions: Vec<String>,
    },
    MultiSelect {
        title: String,
        question: String,
        options: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct AskEvent {
    pub title: Option<String>,
    pub context: String,
    pub questions: Vec<AskQuestion>,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub id: String,
    pub content: String,
    pub pubkey: String,
    pub thread_id: String,
    pub created_at: u64,
    /// Direct parent message ID (for threaded replies)
    /// None for messages replying directly to thread root
    pub reply_to: Option<String>,
    /// Whether this is a reasoning/thinking message (has "reasoning" tag)
    pub is_reasoning: bool,
    /// Ask event data if this message contains an ask
    pub ask_event: Option<AskEvent>,
    /// Q-tags pointing to delegated conversation IDs
    /// When an agent delegates work, the delegation message has q-tags pointing to child conversations
    pub q_tags: Vec<String>,
    /// P-tags (mentions) - pubkeys this message mentions
    /// Used for message grouping: p-tag breaks consecutive message groups
    pub p_tags: Vec<String>,
    /// Tool name from "tool" tag (e.g., "delegate", "fs_read", etc.)
    /// Used for grouping: delegation tools break groups and are never collapsible
    pub tool_name: Option<String>,
    /// Tool arguments from "tool-args" tag (JSON string)
    /// Used for extracting tool call parameters when stored in tags rather than content
    pub tool_args: Option<String>,
    /// LLM metadata tags (llm-prompt-tokens, llm-completion-tokens, llm-model, etc.)
    /// Key is the tag name without "llm-" prefix, value is the tag value
    pub llm_metadata: Vec<(String, String)>,
    /// Delegation tag value - parent conversation ID if this message has a delegation tag
    /// Format: ["delegation", "<parent-conversation-id>"]
    pub delegation_tag: Option<String>,
    /// Branch tag value - git branch associated with this message
    /// Format: ["branch", "<branch-name>"]
    pub branch: Option<String>,
}

impl Message {
    /// Create a Message from a kind:1 note with e-tag (NIP-10 "root" marker).
    /// Message detection: kind:1 + has e-tag with "root" marker
    ///
    /// NIP-10: ["e", <event-id>, <relay-url>, <marker>]
    /// - First e-tag with "root" marker = thread root reference
    /// - First e-tag with "reply" marker (or no marker for backwards compat) = direct parent
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 1 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let content = note.content().to_string();
        let created_at = note.created_at();

        let mut thread_id: Option<String> = None;
        let mut reply_to: Option<String> = None;
        let mut is_reasoning = false;
        let mut q_tags: Vec<String> = Vec::new();
        let mut p_tags: Vec<String> = Vec::new();
        let mut tool_name: Option<String> = None;
        let mut tool_args: Option<String> = None;
        let mut llm_metadata: Vec<(String, String)> = Vec::new();
        let mut delegation_tag: Option<String> = None;
        let mut branch: Option<String> = None;

        // Parse tags
        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());

            // Check for llm-* tags first
            if let Some(name) = tag_name {
                if name.starts_with("llm-") {
                    if let Some(value) = tag.get(1).and_then(|t| t.variant().str()) {
                        let key = name.strip_prefix("llm-").unwrap().to_string();
                        llm_metadata.push((key, value.to_string()));
                    }
                    continue;
                }
            }

            match tag_name {
                Some("p") => {
                    // P-tags are mentions (pubkeys)
                    if let Some(pubkey) = tag.get(1).and_then(|t| t.variant().str()) {
                        p_tags.push(pubkey.to_string());
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        p_tags.push(hex::encode(id_bytes));
                    }
                }
                Some("tool") => {
                    // Tool tag format: ["tool", "tool_name", ...]
                    tool_name = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                Some("tool-args") => {
                    // Tool args tag format: ["tool-args", "json_string"]
                    tool_args = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                Some("q") => {
                    // Q-tags point to delegated conversation IDs
                    if let Some(conv_id) = tag.get(1).and_then(|t| t.variant().str()) {
                        q_tags.push(conv_id.to_string());
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        q_tags.push(hex::encode(id_bytes));
                    }
                }
                Some("e") => {
                    // Extract event ID
                    let event_id = if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                        Some(s.to_string())
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        Some(hex::encode(id_bytes))
                    } else {
                        None
                    };

                    if let Some(eid) = event_id {
                        // Check marker (4th element in NIP-10: ["e", id, relay, marker])
                        let marker = tag.get(3).and_then(|t| t.variant().str());

                        match marker {
                            Some("root") => {
                                thread_id = Some(eid);
                            }
                            Some("reply") => {
                                reply_to = Some(eid);
                            }
                            None => {
                                // No marker: backwards compat - if we don't have root yet, use as root
                                if thread_id.is_none() {
                                    thread_id = Some(eid);
                                } else {
                                    // Second e-tag without marker = reply
                                    reply_to = Some(eid);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Some("reasoning") => {
                    is_reasoning = true;
                }
                Some("delegation") => {
                    // Delegation tag format: ["delegation", "<parent-conversation-id>"]
                    delegation_tag = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                Some("branch") => {
                    // Branch tag format: ["branch", "<branch-name>"]
                    branch = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                _ => {}
            }
        }

        // Must have at least one e-tag (for thread_id)
        let thread_id = thread_id?;

        // Parse ask event data if present
        let ask_event = Self::parse_ask_event(note);

        Some(Message {
            id,
            content,
            pubkey,
            thread_id,
            created_at,
            reply_to,
            is_reasoning,
            ask_event,
            q_tags,
            p_tags,
            tool_name,
            tool_args,
            llm_metadata,
            delegation_tag,
            branch,
        })
    }

    /// Create a Message from a kind:1 thread root note (the thread itself as first message).
    /// For displaying thread content as the first message in the conversation.
    pub fn from_thread_note(note: &Note) -> Option<Self> {
        if note.kind() != 1 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let content = note.content().to_string();
        let created_at = note.created_at();

        // Verify it's a thread (has a-tag, no e-tags) and collect tags
        let mut has_a_tag = false;
        let mut has_e_tag = false;
        let mut q_tags: Vec<String> = Vec::new();
        let mut p_tags: Vec<String> = Vec::new();
        let mut tool_name: Option<String> = None;
        let mut tool_args: Option<String> = None;
        let mut llm_metadata: Vec<(String, String)> = Vec::new();
        let mut branch: Option<String> = None;

        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());

            // Check for llm-* tags first
            if let Some(name) = tag_name {
                if name.starts_with("llm-") {
                    if let Some(value) = tag.get(1).and_then(|t| t.variant().str()) {
                        let key = name.strip_prefix("llm-").unwrap().to_string();
                        llm_metadata.push((key, value.to_string()));
                    }
                    continue;
                }
            }

            match tag_name {
                Some("a") => has_a_tag = true,
                Some("e") => has_e_tag = true,
                Some("p") => {
                    if let Some(pk) = tag.get(1).and_then(|t| t.variant().str()) {
                        p_tags.push(pk.to_string());
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        p_tags.push(hex::encode(id_bytes));
                    }
                }
                Some("tool") => {
                    tool_name = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                Some("tool-args") => {
                    tool_args = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                Some("q") => {
                    if let Some(conv_id) = tag.get(1).and_then(|t| t.variant().str()) {
                        q_tags.push(conv_id.to_string());
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        q_tags.push(hex::encode(id_bytes));
                    }
                }
                Some("branch") => {
                    branch = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                _ => {}
            }
        }

        if !has_a_tag || has_e_tag {
            return None;
        }

        // Parse ask event data if present
        let ask_event = Self::parse_ask_event(note);

        Some(Message {
            id: id.clone(),
            content,
            pubkey,
            thread_id: id,
            created_at,
            reply_to: None,
            is_reasoning: false,
            ask_event,
            q_tags,
            p_tags,
            tool_name,
            tool_args,
            llm_metadata,
            delegation_tag: None, // Thread root doesn't have delegation tag (use Thread.parent_conversation_id)
            branch,
        })
    }

    /// Check if message content contains markdown images
    pub fn has_images(&self) -> bool {
        self.content.contains("![")
    }

    /// Extract all image URLs from markdown content
    pub fn extract_image_urls(&self) -> Vec<String> {
        let mut urls = Vec::new();
        let mut chars = self.content.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '!' {
                if chars.peek() == Some(&'[') {
                    chars.next();

                    // Skip alt text
                    let mut depth = 1;
                    while let Some(ch) = chars.next() {
                        if ch == '[' {
                            depth += 1;
                        } else if ch == ']' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                    }

                    // Expect opening paren
                    if chars.peek() == Some(&'(') {
                        chars.next();

                        // Extract URL
                        let mut url = String::new();
                        while let Some(ch) = chars.peek() {
                            if *ch == ')' {
                                chars.next();
                                break;
                            }
                            url.push(*ch);
                            chars.next();
                        }

                        if !url.is_empty() {
                            urls.push(url.trim().to_string());
                        }
                    }
                }
            }
        }

        urls
    }

    /// Parse ask event from a note
    /// Returns Some(AskEvent) if this message contains ask tags
    pub fn parse_ask_event(note: &Note) -> Option<AskEvent> {
        let mut title: Option<String> = None;
        let mut questions: Vec<AskQuestion> = Vec::new();

        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());

            match tag_name {
                Some("title") => {
                    title = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                Some("question") => {
                    // ["question", title, question, ...suggestions]
                    let q_title = tag.get(1).and_then(|t| t.variant().str()).unwrap_or("").to_string();
                    let q_text = tag.get(2).and_then(|t| t.variant().str()).unwrap_or("").to_string();

                    let tag_count = tag.count();
                    let mut suggestions = Vec::new();
                    for idx in 3..tag_count {
                        if let Some(suggestion) = tag.get(idx).and_then(|t| t.variant().str()) {
                            suggestions.push(suggestion.to_string());
                        }
                    }

                    questions.push(AskQuestion::SingleSelect {
                        title: q_title,
                        question: q_text,
                        suggestions,
                    });
                }
                Some("multiselect") => {
                    // ["multiselect", title, question, ...options]
                    let q_title = tag.get(1).and_then(|t| t.variant().str()).unwrap_or("").to_string();
                    let q_text = tag.get(2).and_then(|t| t.variant().str()).unwrap_or("").to_string();

                    let tag_count = tag.count();
                    let mut options = Vec::new();
                    for idx in 3..tag_count {
                        if let Some(option) = tag.get(idx).and_then(|t| t.variant().str()) {
                            options.push(option.to_string());
                        }
                    }

                    questions.push(AskQuestion::MultiSelect {
                        title: q_title,
                        question: q_text,
                        options,
                    });
                }
                _ => {}
            }
        }

        if questions.is_empty() {
            None
        } else {
            Some(AskEvent {
                title,
                context: note.content().to_string(),
                questions,
            })
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{events::{ingest_events, wait_for_event_processing}, Database};
    use nostr_sdk::prelude::*;
    use nostrdb::{Filter, Transaction};
    use tempfile::tempdir;

    #[test]
    fn test_message_from_kind1_with_root_marker() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();
        let thread_id = "a".repeat(64);

        let event = EventBuilder::new(Kind::from(1), "Message content")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![thread_id.clone(), "".to_string(), "root".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(results.len() > 0, "Event should be indexed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let message = Message::from_note(&note);
        assert!(message.is_some(), "Should parse kind:1 with e-tag root marker");
        let message = message.unwrap();
        assert_eq!(message.thread_id, thread_id);
        assert_eq!(message.content, "Message content");
        assert!(message.reply_to.is_none());
    }

    #[test]
    fn test_message_with_root_and_reply_markers() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();
        let thread_id = "a".repeat(64);
        let parent_id = "b".repeat(64);

        let event = EventBuilder::new(Kind::from(1), "Reply content")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![thread_id.clone(), "".to_string(), "root".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![parent_id.clone(), "".to_string(), "reply".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(results.len() > 0, "Event should be indexed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let message = Message::from_note(&note);
        assert!(message.is_some(), "Should parse kind:1 with root and reply markers");
        let message = message.unwrap();
        assert_eq!(message.thread_id, thread_id);
        assert_eq!(message.reply_to, Some(parent_id));
    }

    #[test]
    fn test_message_backwards_compat_no_markers() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();
        let thread_id = "a".repeat(64);
        let parent_id = "b".repeat(64);

        // Old style: first e-tag = root, second = reply
        let event = EventBuilder::new(Kind::from(1), "Old style")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![thread_id.clone()],
            ))
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![parent_id.clone()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(results.len() > 0, "Event should be indexed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let message = Message::from_note(&note);
        assert!(message.is_some(), "Should parse backwards-compatible format");
        let message = message.unwrap();
        assert_eq!(message.thread_id, thread_id);
        assert_eq!(message.reply_to, Some(parent_id));
    }

    #[test]
    fn test_message_rejects_kind1_without_e_tag() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::from(1), "No e-tag")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec!["31933:pubkey:proj1".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(results.len() > 0, "Event should be indexed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let message = Message::from_note(&note);
        assert!(message.is_none(), "Should reject kind:1 without e-tag (it's a thread, not message)");
    }

    #[test]
    fn test_from_thread_note_creates_message() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::from(1), "Thread as message")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec!["31933:pubkey:proj1".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(results.len() > 0, "Event should be indexed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let message = Message::from_thread_note(&note);
        assert!(message.is_some(), "Should create message from thread note");
        let message = message.unwrap();
        assert_eq!(message.thread_id, message.id);
        assert!(message.reply_to.is_none());
    }

    #[test]
    fn test_has_images() {
        let msg = Message {
            id: "test".to_string(),
            content: "Here's an image: ![alt](https://example.com/image.png)".to_string(),
            pubkey: "pubkey".to_string(),
            thread_id: "thread".to_string(),
            created_at: 0,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec![],
            p_tags: vec![],
            tool_name: None,
            tool_args: None,
            llm_metadata: vec![],
            delegation_tag: None,
        };
        assert!(msg.has_images());

        let no_images = Message {
            id: "test".to_string(),
            content: "Just plain text".to_string(),
            pubkey: "pubkey".to_string(),
            thread_id: "thread".to_string(),
            created_at: 0,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec![],
            p_tags: vec![],
            tool_name: None,
            tool_args: None,
            llm_metadata: vec![],
            delegation_tag: None,
        };
        assert!(!no_images.has_images());
    }

    #[test]
    fn test_extract_image_urls_single() {
        let msg = Message {
            id: "test".to_string(),
            content: "Here's an image: ![alt text](https://example.com/image.png)".to_string(),
            pubkey: "pubkey".to_string(),
            thread_id: "thread".to_string(),
            created_at: 0,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec![],
            p_tags: vec![],
            tool_name: None,
            tool_args: None,
            llm_metadata: vec![],
            delegation_tag: None,
        };
        let urls = msg.extract_image_urls();
        assert_eq!(urls, vec!["https://example.com/image.png"]);
    }

    #[test]
    fn test_extract_image_urls_multiple() {
        let msg = Message {
            id: "test".to_string(),
            content: "![first](https://example.com/1.png) some text ![second](https://example.com/2.jpg)".to_string(),
            pubkey: "pubkey".to_string(),
            thread_id: "thread".to_string(),
            created_at: 0,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec![],
            p_tags: vec![],
            tool_name: None,
            tool_args: None,
            llm_metadata: vec![],
            delegation_tag: None,
        };
        let urls = msg.extract_image_urls();
        assert_eq!(urls, vec!["https://example.com/1.png", "https://example.com/2.jpg"]);
    }

    #[test]
    fn test_extract_image_urls_none() {
        let msg = Message {
            id: "test".to_string(),
            content: "Just plain text with no images".to_string(),
            pubkey: "pubkey".to_string(),
            thread_id: "thread".to_string(),
            created_at: 0,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec![],
            p_tags: vec![],
            tool_name: None,
            tool_args: None,
            llm_metadata: vec![],
            delegation_tag: None,
        };
        let urls = msg.extract_image_urls();
        assert!(urls.is_empty());
    }

    #[test]
    fn test_extract_image_urls_with_spaces() {
        let msg = Message {
            id: "test".to_string(),
            content: "![diagram]( https://example.com/image.png )".to_string(),
            pubkey: "pubkey".to_string(),
            thread_id: "thread".to_string(),
            created_at: 0,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec![],
            p_tags: vec![],
            tool_name: None,
            tool_args: None,
            llm_metadata: vec![],
            delegation_tag: None,
        };
        let urls = msg.extract_image_urls();
        assert_eq!(urls, vec!["https://example.com/image.png"]);
    }

    #[test]
    fn test_parse_ask_event_single_select() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::from(1), "Context for the question")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["What is the answer?"],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("question")),
                vec!["Q1", "Which option do you prefer?", "Option A", "Option B", "Option C"],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(results.len() > 0);
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let ask_event = Message::parse_ask_event(&note);
        assert!(ask_event.is_some());

        let ask = ask_event.unwrap();
        assert_eq!(ask.title, Some("What is the answer?".to_string()));
        assert_eq!(ask.context, "Context for the question");
        assert_eq!(ask.questions.len(), 1);

        match &ask.questions[0] {
            AskQuestion::SingleSelect { title, question, suggestions } => {
                assert_eq!(title, "Q1");
                assert_eq!(question, "Which option do you prefer?");
                assert_eq!(suggestions, &vec!["Option A", "Option B", "Option C"]);
            }
            _ => panic!("Expected SingleSelect question"),
        }
    }

    #[test]
    fn test_parse_ask_event_multiselect() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::from(1), "Pick your favorites")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Choose features"],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("multiselect")),
                vec!["Features", "Select all that apply", "Dark mode", "Notifications", "Analytics"],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(results.len() > 0);
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let ask_event = Message::parse_ask_event(&note);
        assert!(ask_event.is_some());

        let ask = ask_event.unwrap();
        assert_eq!(ask.title, Some("Choose features".to_string()));
        assert_eq!(ask.questions.len(), 1);

        match &ask.questions[0] {
            AskQuestion::MultiSelect { title, question, options } => {
                assert_eq!(title, "Features");
                assert_eq!(question, "Select all that apply");
                assert_eq!(options, &vec!["Dark mode", "Notifications", "Analytics"]);
            }
            _ => panic!("Expected MultiSelect question"),
        }
    }

    #[test]
    fn test_parse_ask_event_multiple_questions() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::from(1), "Please answer these questions")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Survey"],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("question")),
                vec!["Q1", "What is your name?"],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("multiselect")),
                vec!["Q2", "Select your interests", "Music", "Sports", "Art"],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(results.len() > 0);
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let ask_event = Message::parse_ask_event(&note);
        assert!(ask_event.is_some());

        let ask = ask_event.unwrap();
        assert_eq!(ask.questions.len(), 2);

        match &ask.questions[0] {
            AskQuestion::SingleSelect { title, question, suggestions } => {
                assert_eq!(title, "Q1");
                assert_eq!(question, "What is your name?");
                assert!(suggestions.is_empty());
            }
            _ => panic!("Expected SingleSelect question"),
        }

        match &ask.questions[1] {
            AskQuestion::MultiSelect { title, question, options } => {
                assert_eq!(title, "Q2");
                assert_eq!(question, "Select your interests");
                assert_eq!(options, &vec!["Music", "Sports", "Art"]);
            }
            _ => panic!("Expected MultiSelect question"),
        }
    }

    #[test]
    fn test_parse_ask_event_not_ask() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::from(1), "Just a regular message")
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(results.len() > 0);
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let ask_event = Message::parse_ask_event(&note);
        assert!(ask_event.is_none());
    }
}
