#[derive(Debug, Clone, PartialEq)]
pub enum InboxEventType {
    Mention,
    Reply,
    ThreadReply,
}

#[derive(Debug, Clone)]
pub struct InboxItem {
    pub id: String,
    pub event_type: InboxEventType,
    pub title: String,
    pub preview: String,
    pub project_a_tag: String,
    pub author_pubkey: String,
    pub created_at: u64,
    pub is_read: bool,
    pub thread_id: Option<String>,
}

/// Agent chatter - kind:1111 events that a-tag one of our projects
#[derive(Debug, Clone)]
pub struct AgentChatter {
    pub id: String,
    pub content: String,
    pub project_a_tag: String,
    pub author_pubkey: String,
    pub created_at: u64,
    pub thread_id: String,
}
