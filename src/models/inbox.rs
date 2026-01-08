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

/// Agent chatter - kind:1 messages or kind:4129 lessons that reference our projects
#[derive(Debug, Clone)]
pub enum AgentChatter {
    Message {
        id: String,
        content: String,
        project_a_tag: String,
        author_pubkey: String,
        created_at: u64,
        thread_id: String,
    },
    Lesson {
        id: String,
        title: String,
        content: String,
        author_pubkey: String,
        created_at: u64,
        category: Option<String>,
    },
}

impl AgentChatter {
    pub fn id(&self) -> &str {
        match self {
            AgentChatter::Message { id, .. } => id,
            AgentChatter::Lesson { id, .. } => id,
        }
    }

    pub fn created_at(&self) -> u64 {
        match self {
            AgentChatter::Message { created_at, .. } => *created_at,
            AgentChatter::Lesson { created_at, .. } => *created_at,
        }
    }

    pub fn author_pubkey(&self) -> &str {
        match self {
            AgentChatter::Message { author_pubkey, .. } => author_pubkey,
            AgentChatter::Lesson { author_pubkey, .. } => author_pubkey,
        }
    }
}
