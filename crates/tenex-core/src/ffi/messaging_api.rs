use super::*;

#[uniffi::export]
impl TenexCore {
    /// Send a new conversation (thread) to a project.
    ///
    /// Creates a new kind:1 event with title tag and project a-tag.
    /// Returns the event ID on success.
    pub fn send_thread(
        &self,
        project_id: String,
        title: String,
        content: String,
        agent_pubkey: Option<String>,
        nudge_ids: Vec<String>,
        skill_ids: Vec<String>,
        reference_conversation_id: Option<String>,
    ) -> Result<SendMessageResult, TenexError> {
        let project_a_tag = get_project_a_tag(&self.store, &project_id)?;
        let core_handle = get_core_handle(&self.core_handle)?;

        // Create a channel to receive the event ID
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<String>(1);

        // Send the publish thread command
        core_handle
            .send(NostrCommand::PublishThread {
                project_a_tag,
                title,
                content,
                agent_pubkey,
                nudge_ids,
                skill_ids,
                reference_conversation_id,
                fork_message_id: None,
                response_tx: Some(response_tx),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send publish thread command: {}", e),
            })?;

        // Wait for the event ID with timeout
        match response_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(event_id) => Ok(SendMessageResult {
                event_id,
                success: true,
            }),
            Err(_) => Err(TenexError::Internal {
                message: "Timed out waiting for thread publish confirmation".to_string(),
            }),
        }
    }

    /// Send a message to an existing conversation.
    ///
    /// Creates a new kind:1 event with e-tag pointing to the thread root.
    /// Returns the event ID on success.
    pub fn send_message(
        &self,
        conversation_id: String,
        project_id: String,
        content: String,
        agent_pubkey: Option<String>,
        nudge_ids: Vec<String>,
        skill_ids: Vec<String>,
    ) -> Result<SendMessageResult, TenexError> {
        let project_a_tag = get_project_a_tag(&self.store, &project_id)?;
        let core_handle = get_core_handle(&self.core_handle)?;

        // Create a channel to receive the event ID
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<String>(1);

        // Send the publish message command
        core_handle
            .send(NostrCommand::PublishMessage {
                thread_id: conversation_id,
                project_a_tag,
                content,
                agent_pubkey,
                reply_to: None,

                nudge_ids,
                skill_ids,
                ask_author_pubkey: None,
                response_tx: Some(response_tx),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send publish message command: {}", e),
            })?;

        // Wait for the event ID with timeout
        match response_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(event_id) => Ok(SendMessageResult {
                event_id,
                success: true,
            }),
            Err(_) => Err(TenexError::Internal {
                message: "Timed out waiting for message publish confirmation".to_string(),
            }),
        }
    }

    /// Answer an ask event by sending a formatted response.
    ///
    /// The response is formatted as markdown with each question's title and answer,
    /// and published as a kind:1 reply to the ask event.
    pub fn answer_ask(
        &self,
        ask_event_id: String,
        ask_author_pubkey: String,
        conversation_id: String,
        project_id: String,
        answers: Vec<AskAnswer>,
    ) -> Result<SendMessageResult, TenexError> {
        let project_a_tag = get_project_a_tag(&self.store, &project_id)?;
        let core_handle = get_core_handle(&self.core_handle)?;

        // Format answers as markdown (matching TUI format)
        let content = format_ask_answers(&answers);

        // Create a channel to receive the event ID
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<String>(1);

        // Send the publish message command with reply_to pointing to the ask event
        core_handle
            .send(NostrCommand::PublishMessage {
                thread_id: conversation_id,
                project_a_tag,
                content,
                agent_pubkey: None,
                reply_to: Some(ask_event_id),
                nudge_ids: Vec::new(),
                skill_ids: Vec::new(),
                ask_author_pubkey: Some(ask_author_pubkey),
                response_tx: Some(response_tx),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send ask answer command: {}", e),
            })?;

        // Wait for the event ID with timeout
        match response_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(event_id) => Ok(SendMessageResult {
                event_id,
                success: true,
            }),
            Err(_) => Err(TenexError::Internal {
                message: "Timed out waiting for ask answer publish confirmation".to_string(),
            }),
        }
    }
}
