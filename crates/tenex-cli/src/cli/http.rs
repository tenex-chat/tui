use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive},
        IntoResponse, Sse,
    },
    routing::post,
    Json, Router,
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;

use crate::nostr::{DataChange, NostrCommand};
use crate::store::AppDataStore;
use tenex_core::runtime::CoreHandle;

// ============================================================================
// OpenAI Responses API Types
// ============================================================================

/// Input content item for rich content (text, image, file)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum InputContentItem {
    #[serde(rename = "input_text")]
    InputText { text: String },
    #[serde(rename = "input_image")]
    InputImage { image_url: String },
    #[serde(rename = "input_file")]
    InputFile { file_id: String },
}

/// A message in the input array
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputMessage {
    pub role: String,
    #[serde(default)]
    pub content: InputMessageContent,
}

/// Content of an input message - either a simple string or array of content items
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InputMessageContent {
    Text(String),
    Array(Vec<InputContentItem>),
}

impl Default for InputMessageContent {
    fn default() -> Self {
        InputMessageContent::Text(String::new())
    }
}

/// The input field can be a simple string or an array of messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseInput {
    Text(String),
    Messages(Vec<InputMessage>),
}

/// OpenAI Responses API request body
#[derive(Debug, Deserialize)]
pub struct ResponsesRequest {
    /// The input - either a simple string or array of messages
    pub input: ResponseInput,
    /// Model to use (optional, we use our own)
    #[serde(default)]
    pub model: Option<String>,
    /// Whether to stream the response
    #[serde(default)]
    pub stream: bool,
    /// Previous response ID for conversation chaining
    #[serde(default)]
    pub previous_response_id: Option<String>,
    /// Instructions for the model
    #[serde(default)]
    pub instructions: Option<String>,
    /// Whether to store the response
    #[serde(default)]
    pub store: Option<bool>,
    /// Optional metadata
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    /// Optional user identifier
    #[serde(default)]
    pub user: Option<String>,
}

/// Output content item
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OutputContentItem {
    #[serde(rename = "output_text")]
    OutputText {
        text: String,
        #[serde(default)]
        annotations: Vec<serde_json::Value>,
    },
}

/// Output message in the response
#[derive(Debug, Clone, Serialize)]
pub struct OutputMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub content: Vec<OutputContentItem>,
}

/// Usage information
#[derive(Debug, Clone, Serialize)]
pub struct ResponseUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

/// OpenAI Responses API response object
#[derive(Debug, Clone, Serialize)]
pub struct ResponsesResponse {
    pub id: String,
    /// Unix timestamp in seconds (integer, not float)
    pub created_at: u64,
    pub status: String,
    pub model: String,
    pub object: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<serde_json::Value>,
    pub output: Vec<OutputMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ResponseUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_response_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

// ============================================================================
// Streaming Event Types
// ============================================================================

/// Event types for streaming responses
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    #[serde(rename = "response.created")]
    ResponseCreated { response: ResponsesResponse },
    #[serde(rename = "response.in_progress")]
    ResponseInProgress { response: ResponsesResponse },
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded {
        output_index: usize,
        item: OutputMessage,
    },
    #[serde(rename = "response.content_part.added")]
    ContentPartAdded {
        output_index: usize,
        content_index: usize,
        part: OutputContentItem,
    },
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta {
        output_index: usize,
        content_index: usize,
        delta: String,
    },
    #[serde(rename = "response.output_text.done")]
    OutputTextDone {
        output_index: usize,
        content_index: usize,
        text: String,
    },
    #[serde(rename = "response.output_item.done")]
    OutputItemDone {
        output_index: usize,
        item: OutputMessage,
    },
    #[serde(rename = "response.completed")]
    ResponseCompleted { response: ResponsesResponse },
}

// ============================================================================
// Server State and Handlers
// ============================================================================

/// OpenAI-style error response
#[derive(Debug, Clone, Serialize)]
pub struct OpenAIError {
    pub error: OpenAIErrorBody,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenAIErrorBody {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl OpenAIError {
    pub fn new(message: impl Into<String>, error_type: impl Into<String>) -> Self {
        Self {
            error: OpenAIErrorBody {
                message: message.into(),
                error_type: error_type.into(),
                code: None,
            },
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(message, "invalid_request_error")
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(message, "not_found_error")
    }

    pub fn server_error(message: impl Into<String>) -> Self {
        Self::new(message, "server_error")
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(message, "timeout_error")
    }
}

/// Shared server state
#[derive(Clone)]
pub struct HTTPServerState {
    pub core_handle: CoreHandle,
    /// Data store shared with daemon - uses std::sync::Mutex for compatibility with sync code paths
    pub data_store: Arc<Mutex<AppDataStore>>,
    /// Broadcast channel for DataChange events - each SSE stream subscribes to get its own receiver
    pub data_tx: broadcast::Sender<DataChange>,
    /// Maps OpenAI response IDs (resp_xxx) to Nostr event IDs for conversation chaining
    /// Uses tokio::sync::Mutex since it's only accessed in async handlers
    pub response_id_map: Arc<tokio::sync::Mutex<HashMap<String, String>>>,
}

/// Start the OpenAI Responses API server
pub async fn run_server(
    bind_addr: String,
    core_handle: CoreHandle,
    data_store: Arc<Mutex<AppDataStore>>,
    data_tx: broadcast::Sender<DataChange>,
) -> Result<()> {
    let state = HTTPServerState {
        core_handle,
        data_store,
        data_tx,
        // response_id_map uses tokio::sync::Mutex since it's accessed in async handlers
        response_id_map: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/:project_dtag/responses", post(responses_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    eprintln!("OpenAI Responses API server listening on http://{}", bind_addr);
    eprintln!("Endpoint: http://{}/:project_dtag/responses", bind_addr);

    axum::serve(listener, app).await?;

    Ok(())
}

/// Helper to create an OpenAI error JSON response
fn openai_error_response(
    status: StatusCode,
    error: OpenAIError,
) -> (StatusCode, axum::Json<OpenAIError>) {
    (status, axum::Json(error))
}

/// Safely extract a prefix of a string (handles multi-byte UTF-8)
fn safe_string_prefix(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Handler for POST /:project_dtag/responses
async fn responses_handler(
    Path(project_dtag): Path<String>,
    State(state): State<HTTPServerState>,
    Json(request): Json<ResponsesRequest>,
) -> Result<impl IntoResponse, (StatusCode, axum::Json<OpenAIError>)> {
    // Validate message roles if using messages array
    if let ResponseInput::Messages(ref messages) = request.input {
        for msg in messages {
            if !["user", "assistant", "system"].contains(&msg.role.as_str()) {
                return Err(openai_error_response(
                    StatusCode::BAD_REQUEST,
                    OpenAIError::bad_request(format!(
                        "Invalid message role '{}'. Must be one of: user, assistant, system",
                        msg.role
                    )),
                ));
            }
        }
    }

    // Extract the user message content from the input
    let user_content = extract_user_content(&request.input).map_err(|e| {
        openai_error_response(StatusCode::BAD_REQUEST, OpenAIError::bad_request(e.to_string()))
    })?;

    // Construct the project a-tag coordinate (format: kind:pubkey:identifier)
    let project_a_tag = resolve_project_coordinate(&state, &project_dtag).await.map_err(|e| {
        openai_error_response(
            StatusCode::NOT_FOUND,
            OpenAIError::not_found(format!("Project not found: {}", e)),
        )
    })?;

    // Get the PM agent pubkey from the project status
    let agent_pubkey = get_pm_agent_pubkey(&state, &project_a_tag).await.map_err(|e| {
        openai_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            OpenAIError::server_error(format!("Agent not available: {}", e)),
        )
    })?;

    // Create a title from the user message (first 50 chars, safely handling UTF-8)
    let title: String = if user_content.chars().count() > 50 {
        format!("{}...", safe_string_prefix(&user_content, 50))
    } else {
        user_content.clone()
    };

    // Resolve previous_response_id to actual Nostr event ID if provided
    let reference_conversation_id = if let Some(ref prev_id) = request.previous_response_id {
        // Strip "resp_" prefix and look up the actual Nostr event ID
        if prev_id.starts_with("resp_") {
            let lookup_key = prev_id.clone();
            // Using tokio::sync::Mutex for async-friendly locking
            let map = state.response_id_map.lock().await;
            map.get(&lookup_key).cloned()
        } else {
            // Assume it's already a Nostr event ID
            Some(prev_id.clone())
        }
    } else {
        None
    };

    // Create response channel to get the thread ID (which is a valid 64-char hex event ID)
    // Using std::sync::mpsc::sync_channel to match what NostrCommand::PublishThread expects
    let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<String>(1);

    // Publish a new thread using PublishThread command
    // This creates a proper kind:1 event with no e-tags (thread root)
    let publish_result = state.core_handle.send(NostrCommand::PublishThread {
        project_a_tag: project_a_tag.clone(),
        title,
        content: user_content.clone(),
        agent_pubkey: Some(agent_pubkey.clone()),
        nudge_ids: Vec::new(),
        reference_conversation_id,
        fork_message_id: None,
        response_tx: Some(response_tx),
    });

    if let Err(e) = publish_result {
        return Err(openai_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            OpenAIError::server_error(format!("Failed to create thread: {}", e)),
        ));
    }

    // Wait for the thread ID (with timeout)
    // Using tokio::task::spawn_blocking to avoid blocking the async runtime
    let thread_id = tokio::time::timeout(
        tokio::time::Duration::from_secs(5),
        tokio::task::spawn_blocking(move || response_rx.recv()),
    )
    .await
    .map_err(|_| {
        openai_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            OpenAIError::timeout("Timeout waiting for thread creation"),
        )
    })?
    .map_err(|_| {
        openai_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            OpenAIError::server_error("Thread creation task panicked"),
        )
    })?
    .map_err(|_| {
        openai_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            OpenAIError::server_error("Thread creation failed - channel closed"),
        )
    })?;

    // Validate thread_id length before slicing
    if thread_id.len() < 32 {
        return Err(openai_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            OpenAIError::server_error(format!(
                "Invalid thread ID length: expected at least 32 chars, got {}",
                thread_id.len()
            )),
        ));
    }

    // Generate response ID in OpenAI format (safely handle UTF-8)
    let response_id = format!("resp_{}", safe_string_prefix(&thread_id, 32));

    // Store the mapping from response_id to actual Nostr event ID
    {
        // Using tokio::sync::Mutex for async-friendly locking
        let mut map = state.response_id_map.lock().await;
        map.insert(response_id.clone(), thread_id.clone());
    }

    // Use Unix timestamp in seconds as integer (not float)
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // If streaming is requested, return SSE stream
    if request.stream {
        // Subscribe to the broadcast channel - each stream gets its own receiver
        let data_rx = state.data_tx.subscribe();

        let stream = create_responses_sse_stream(
            data_rx,
            thread_id,
            agent_pubkey,
            response_id,
            created_at,
            request.previous_response_id,
            request.metadata,
        );
        Ok(Sse::new(stream).keep_alive(KeepAlive::default()).into_response())
    } else {
        // Non-streaming: collect all chunks and return complete response
        Err(openai_error_response(
            StatusCode::NOT_IMPLEMENTED,
            OpenAIError::bad_request("Non-streaming mode not yet implemented. Please use stream=true"),
        ))
    }
}

/// Extract the user content from the input field
fn extract_user_content(input: &ResponseInput) -> Result<String> {
    match input {
        ResponseInput::Text(text) => {
            if text.is_empty() {
                return Err(anyhow::anyhow!("input cannot be empty"));
            }
            Ok(text.clone())
        }
        ResponseInput::Messages(messages) => {
            if messages.is_empty() {
                return Err(anyhow::anyhow!("input messages array cannot be empty"));
            }

            // Find the last user message
            let user_message = messages
                .iter()
                .rev()
                .find(|m| m.role == "user")
                .ok_or_else(|| anyhow::anyhow!("No user message found in input array"))?;

            // Extract text content
            match &user_message.content {
                InputMessageContent::Text(text) => {
                    if text.is_empty() {
                        return Err(anyhow::anyhow!("user message content cannot be empty"));
                    }
                    Ok(text.clone())
                }
                InputMessageContent::Array(items) => {
                    // Concatenate all text items
                    let text: String = items
                        .iter()
                        .filter_map(|item| match item {
                            InputContentItem::InputText { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    if text.is_empty() {
                        return Err(anyhow::anyhow!("No text content found in user message"));
                    }
                    Ok(text)
                }
            }
        }
    }
}

/// StreamEvent for response.failed
#[derive(Debug, Clone, Serialize)]
pub struct ResponseFailedEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub response: ResponsesResponse,
}

/// Timeout for waiting for agent response (5 minutes)
const AGENT_RESPONSE_TIMEOUT_SECS: u64 = 300;

/// Create an SSE stream for the Responses API format
fn create_responses_sse_stream(
    mut data_rx: broadcast::Receiver<DataChange>,
    thread_id: String,
    agent_pubkey: String,
    response_id: String,
    created_at: u64,
    previous_response_id: Option<String>,
    metadata: Option<serde_json::Value>,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let stream = async_stream::stream! {
        // Safely generate msg_id (handle potential short thread_id)
        let msg_id = format!("msg_{}", safe_string_prefix(&thread_id, 24));
        let mut accumulated_text = String::new();
        let mut received_any_chunk = false;
        let stream_start = tokio::time::Instant::now();

        // Create initial response object
        let initial_response = ResponsesResponse {
            id: response_id.clone(),
            created_at,
            status: "in_progress".to_string(),
            model: "tenex".to_string(),
            object: "response".to_string(),
            error: None,
            output: vec![],
            output_text: None,
            usage: None,
            previous_response_id: previous_response_id.clone(),
            metadata: metadata.clone(),
        };

        // Send response.created event
        let created_event = StreamEvent::ResponseCreated {
            response: initial_response.clone(),
        };
        if let Ok(json) = serde_json::to_string(&created_event) {
            yield Ok(Event::default().event("response.created").data(json));
        }

        // Send response.in_progress event
        let in_progress_event = StreamEvent::ResponseInProgress {
            response: initial_response.clone(),
        };
        if let Ok(json) = serde_json::to_string(&in_progress_event) {
            yield Ok(Event::default().event("response.in_progress").data(json));
        }

        // Send output_item.added for the assistant message
        let output_item = OutputMessage {
            id: msg_id.clone(),
            message_type: "message".to_string(),
            role: "assistant".to_string(),
            status: Some("in_progress".to_string()),
            content: vec![],
        };
        let item_added_event = StreamEvent::OutputItemAdded {
            output_index: 0,
            item: output_item.clone(),
        };
        if let Ok(json) = serde_json::to_string(&item_added_event) {
            yield Ok(Event::default().event("response.output_item.added").data(json));
        }

        // Send content_part.added for the text content
        let content_part = OutputContentItem::OutputText {
            text: String::new(),
            annotations: vec![],
        };
        let part_added_event = StreamEvent::ContentPartAdded {
            output_index: 0,
            content_index: 0,
            part: content_part,
        };
        if let Ok(json) = serde_json::to_string(&part_added_event) {
            yield Ok(Event::default().event("response.content_part.added").data(json));
        }

        // Poll for DataChange events using async recv with timeout
        loop {
            // Check for overall timeout
            if stream_start.elapsed() > tokio::time::Duration::from_secs(AGENT_RESPONSE_TIMEOUT_SECS) {
                // Emit response.failed event
                let failed_response = ResponsesResponse {
                    id: response_id.clone(),
                    created_at,
                    status: "failed".to_string(),
                    model: "tenex".to_string(),
                    object: "response".to_string(),
                    error: Some(serde_json::json!({
                        "message": "Agent response timeout",
                        "type": "timeout_error"
                    })),
                    output: vec![],
                    output_text: None,
                    usage: None,
                    previous_response_id: previous_response_id.clone(),
                    metadata: metadata.clone(),
                };
                let failed_event = ResponseFailedEvent {
                    event_type: "response.failed".to_string(),
                    response: failed_response,
                };
                if let Ok(json) = serde_json::to_string(&failed_event) {
                    yield Ok(Event::default().event("response.failed").data(json));
                }
                break;
            }

            // Use async timeout for receiving
            let recv_result = tokio::time::timeout(
                tokio::time::Duration::from_millis(100),
                data_rx.recv(),
            )
            .await;

            match recv_result {
                Ok(Ok(DataChange::LocalStreamChunk {
                    agent_pubkey: chunk_agent,
                    conversation_id,
                    text_delta,
                    reasoning_delta: _,
                    is_finish,
                })) => {
                    // Only process chunks for our agent and thread
                    if chunk_agent != agent_pubkey || conversation_id != thread_id {
                        continue;
                    }

                    received_any_chunk = true;

                    // Send text delta if present
                    if let Some(text) = text_delta {
                        accumulated_text.push_str(&text);

                        let delta_event = StreamEvent::OutputTextDelta {
                            output_index: 0,
                            content_index: 0,
                            delta: text,
                        };

                        if let Ok(json) = serde_json::to_string(&delta_event) {
                            yield Ok(Event::default().event("response.output_text.delta").data(json));
                        }
                    }

                    // Send completion events if this is the last chunk
                    if is_finish {
                        // Send output_text.done
                        let text_done_event = StreamEvent::OutputTextDone {
                            output_index: 0,
                            content_index: 0,
                            text: accumulated_text.clone(),
                        };
                        if let Ok(json) = serde_json::to_string(&text_done_event) {
                            yield Ok(Event::default().event("response.output_text.done").data(json));
                        }

                        // Send output_item.done with completed message
                        let completed_item = OutputMessage {
                            id: msg_id.clone(),
                            message_type: "message".to_string(),
                            role: "assistant".to_string(),
                            status: Some("completed".to_string()),
                            content: vec![OutputContentItem::OutputText {
                                text: accumulated_text.clone(),
                                annotations: vec![],
                            }],
                        };
                        let item_done_event = StreamEvent::OutputItemDone {
                            output_index: 0,
                            item: completed_item.clone(),
                        };
                        if let Ok(json) = serde_json::to_string(&item_done_event) {
                            yield Ok(Event::default().event("response.output_item.done").data(json));
                        }

                        // Send response.completed (without usage since we don't track tokens)
                        let completed_response = ResponsesResponse {
                            id: response_id.clone(),
                            created_at,
                            status: "completed".to_string(),
                            model: "tenex".to_string(),
                            object: "response".to_string(),
                            error: None,
                            output: vec![completed_item],
                            output_text: Some(accumulated_text.clone()),
                            usage: None, // Don't include fake token counts
                            previous_response_id: previous_response_id.clone(),
                            metadata: metadata.clone(),
                        };
                        let completed_event = StreamEvent::ResponseCompleted {
                            response: completed_response,
                        };
                        if let Ok(json) = serde_json::to_string(&completed_event) {
                            yield Ok(Event::default().event("response.completed").data(json));
                        }

                        break;
                    }
                }
                Ok(Ok(_)) => {
                    // Ignore other DataChange variants
                    continue;
                }
                Ok(Err(broadcast::error::RecvError::Lagged(n))) => {
                    // We missed some messages due to slow processing
                    // Log this but continue - the stream should still work
                    eprintln!("SSE stream lagged, missed {} messages", n);
                    continue;
                }
                Ok(Err(broadcast::error::RecvError::Closed)) => {
                    // Channel closed, end stream with failure if we haven't completed
                    if !received_any_chunk {
                        let failed_response = ResponsesResponse {
                            id: response_id.clone(),
                            created_at,
                            status: "failed".to_string(),
                            model: "tenex".to_string(),
                            object: "response".to_string(),
                            error: Some(serde_json::json!({
                                "message": "Data channel closed unexpectedly",
                                "type": "server_error"
                            })),
                            output: vec![],
                            output_text: None,
                            usage: None,
                            previous_response_id: previous_response_id.clone(),
                            metadata: metadata.clone(),
                        };
                        let failed_event = ResponseFailedEvent {
                            event_type: "response.failed".to_string(),
                            response: failed_response,
                        };
                        if let Ok(json) = serde_json::to_string(&failed_event) {
                            yield Ok(Event::default().event("response.failed").data(json));
                        }
                    }
                    break;
                }
                Err(_) => {
                    // Timeout - continue polling
                    continue;
                }
            }
        }
    };

    stream
}

/// Resolve project dtag to full coordinate.
/// Queries nostrdb directly to get fresh data, bypassing the in-memory cache
/// which doesn't receive project discovery events in the HTTP server context.
async fn resolve_project_coordinate(state: &HTTPServerState, project_dtag: &str) -> Result<String> {
    let store = state.data_store.lock().unwrap();
    store.find_project_a_tag_by_dtag_from_ndb(project_dtag)
        .ok_or_else(|| anyhow::anyhow!("Project with dtag '{}' not found", project_dtag))
}

/// Default timeout for waiting for project status (30 seconds)
const WAIT_FOR_STATUS_TIMEOUT_SECS: u64 = 30;

/// Get the PM agent pubkey from project status, waiting for status to appear if needed.
/// This addresses timing issues where HTTP requests arrive before project status is synced.
async fn get_pm_agent_pubkey(state: &HTTPServerState, project_a_tag: &str) -> Result<String> {
    let start = tokio::time::Instant::now();
    let timeout = tokio::time::Duration::from_secs(WAIT_FOR_STATUS_TIMEOUT_SECS);

    loop {
        // Check for timeout
        if start.elapsed() > timeout {
            return Err(anyhow::anyhow!(
                "Timeout waiting for project status. Project may not be booted."
            ));
        }

        // Try to get the status
        {
            let store = state.data_store.lock().unwrap();

            if let Some(status) = store.get_project_status(project_a_tag) {
                if let Some(pm_agent) = status.pm_agent() {
                    return Ok(pm_agent.pubkey.clone());
                }
                // Status exists but no PM agent - this is a real error, don't wait
                return Err(anyhow::anyhow!("No PM agent found in project status"));
            }
        }

        // Status not found yet, wait a bit before retrying (async sleep)
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test safe string slicing with various inputs
    #[test]
    fn test_safe_string_prefix_normal() {
        // Normal ASCII string
        assert_eq!(safe_string_prefix("abcdefghij", 5), "abcde");
        assert_eq!(safe_string_prefix("abcdefghij", 10), "abcdefghij");
        assert_eq!(safe_string_prefix("abcdefghij", 15), "abcdefghij"); // Longer than string
    }

    #[test]
    fn test_safe_string_prefix_utf8() {
        // Multi-byte UTF-8 characters (e.g., emoji, CJK)
        let emoji_str = "🔥hello";
        assert_eq!(safe_string_prefix(emoji_str, 1), "🔥"); // One char
        assert_eq!(safe_string_prefix(emoji_str, 3), "🔥he");

        let cjk_str = "你好世界"; // "Hello world" in Chinese
        assert_eq!(safe_string_prefix(cjk_str, 2), "你好");
        assert_eq!(safe_string_prefix(cjk_str, 5), "你好世界"); // Full string
    }

    #[test]
    fn test_safe_string_prefix_empty() {
        assert_eq!(safe_string_prefix("", 5), "");
        assert_eq!(safe_string_prefix("abc", 0), "");
    }

    #[test]
    fn test_safe_string_prefix_hex_id() {
        // Real-world case: 64-char hex Nostr event ID
        let event_id = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(event_id.len(), 64);

        // Taking 32 chars for response ID
        let prefix32 = safe_string_prefix(event_id, 32);
        assert_eq!(prefix32, "0123456789abcdef0123456789abcdef");
        assert_eq!(prefix32.len(), 32);

        // Taking 24 chars for message ID
        let prefix24 = safe_string_prefix(event_id, 24);
        assert_eq!(prefix24, "0123456789abcdef01234567");
        assert_eq!(prefix24.len(), 24);
    }

    /// Test OpenAI error types
    #[test]
    fn test_openai_error_serialization() {
        let error = OpenAIError::bad_request("Invalid input");
        let json = serde_json::to_string(&error).unwrap();

        // Verify JSON structure
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["error"]["message"], "Invalid input");
        assert_eq!(parsed["error"]["type"], "invalid_request_error");
    }

    #[test]
    fn test_openai_error_types() {
        assert_eq!(OpenAIError::bad_request("x").error.error_type, "invalid_request_error");
        assert_eq!(OpenAIError::not_found("x").error.error_type, "not_found_error");
        assert_eq!(OpenAIError::server_error("x").error.error_type, "server_error");
        assert_eq!(OpenAIError::timeout("x").error.error_type, "timeout_error");
    }

    /// Test response ID format generation
    #[test]
    fn test_response_id_format() {
        // Valid 64-char hex ID
        let thread_id = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        let response_id = format!("resp_{}", safe_string_prefix(thread_id, 32));

        assert!(response_id.starts_with("resp_"));
        assert_eq!(response_id.len(), 37); // "resp_" (5) + 32 = 37
        assert_eq!(response_id, "resp_abcdef0123456789abcdef0123456789");
    }

    #[test]
    fn test_message_id_format() {
        let thread_id = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
        let msg_id = format!("msg_{}", safe_string_prefix(thread_id, 24));

        assert!(msg_id.starts_with("msg_"));
        assert_eq!(msg_id.len(), 28); // "msg_" (4) + 24 = 28
    }

    /// Test input content extraction
    #[test]
    fn test_extract_user_content_text() {
        let input = ResponseInput::Text("Hello world".to_string());
        let result = extract_user_content(&input).unwrap();
        assert_eq!(result, "Hello world");
    }

    #[test]
    fn test_extract_user_content_empty_text() {
        let input = ResponseInput::Text("".to_string());
        let result = extract_user_content(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot be empty"));
    }

    #[test]
    fn test_extract_user_content_messages() {
        let input = ResponseInput::Messages(vec![
            InputMessage {
                role: "system".to_string(),
                content: InputMessageContent::Text("You are helpful".to_string()),
            },
            InputMessage {
                role: "user".to_string(),
                content: InputMessageContent::Text("Hello!".to_string()),
            },
        ]);
        let result = extract_user_content(&input).unwrap();
        assert_eq!(result, "Hello!");
    }

    #[test]
    fn test_extract_user_content_no_user_message() {
        let input = ResponseInput::Messages(vec![
            InputMessage {
                role: "system".to_string(),
                content: InputMessageContent::Text("You are helpful".to_string()),
            },
        ]);
        let result = extract_user_content(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("No user message"));
    }

    #[test]
    fn test_extract_user_content_empty_messages() {
        let input = ResponseInput::Messages(vec![]);
        let result = extract_user_content(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    /// Test that ResponsesResponse uses u64 for created_at (not f64)
    #[test]
    fn test_responses_response_timestamp_type() {
        let response = ResponsesResponse {
            id: "resp_test".to_string(),
            created_at: 1234567890u64, // Should be u64
            status: "completed".to_string(),
            model: "tenex".to_string(),
            object: "response".to_string(),
            error: None,
            output: vec![],
            output_text: None,
            usage: None,
            previous_response_id: None,
            metadata: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // created_at should be serialized as an integer, not a float
        assert!(parsed["created_at"].is_number());
        assert_eq!(parsed["created_at"].as_u64().unwrap(), 1234567890);
    }

    /// Test that usage is omitted when None
    #[test]
    fn test_responses_response_no_usage() {
        let response = ResponsesResponse {
            id: "resp_test".to_string(),
            created_at: 1234567890u64,
            status: "completed".to_string(),
            model: "tenex".to_string(),
            object: "response".to_string(),
            error: None,
            output: vec![],
            output_text: None,
            usage: None, // Should be omitted from JSON
            previous_response_id: None,
            metadata: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // usage field should not be present
        assert!(parsed.get("usage").is_none());
    }

    /// Test StreamEvent serialization
    #[test]
    fn test_stream_event_delta() {
        let event = StreamEvent::OutputTextDelta {
            output_index: 0,
            content_index: 0,
            delta: "Hello".to_string(),
        };

        let json = serde_json::to_string(&event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "response.output_text.delta");
        assert_eq!(parsed["delta"], "Hello");
    }

    #[test]
    fn test_response_failed_event() {
        let failed_event = ResponseFailedEvent {
            event_type: "response.failed".to_string(),
            response: ResponsesResponse {
                id: "resp_test".to_string(),
                created_at: 1234567890,
                status: "failed".to_string(),
                model: "tenex".to_string(),
                object: "response".to_string(),
                error: Some(serde_json::json!({
                    "message": "Timeout",
                    "type": "timeout_error"
                })),
                output: vec![],
                output_text: None,
                usage: None,
                previous_response_id: None,
                metadata: None,
            },
        };

        let json = serde_json::to_string(&failed_event).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "response.failed");
        assert_eq!(parsed["response"]["status"], "failed");
        assert_eq!(parsed["response"]["error"]["type"], "timeout_error");
    }
}
