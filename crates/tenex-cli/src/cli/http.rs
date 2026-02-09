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
use std::convert::Infallible;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tower_http::cors::CorsLayer;

use crate::nostr::{DataChange, NostrCommand};
use crate::store::AppDataStore;
use tenex_core::runtime::CoreHandle;

/// OpenAI-compatible chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// OpenAI-compatible chat completion request
#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub model: Option<String>,
}

/// OpenAI-compatible chat completion response (non-streaming)
#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatCompletionChoice>,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionChoice {
    pub index: usize,
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
}

/// OpenAI-compatible streaming chunk
#[derive(Debug, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatCompletionChunkChoice>,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionChunkChoice {
    pub index: usize,
    pub delta: ChatCompletionDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Shared server state
#[derive(Clone)]
pub struct HTTPServerState {
    pub core_handle: CoreHandle,
    pub data_store: Arc<Mutex<AppDataStore>>,
    pub data_rx: Arc<Mutex<Receiver<DataChange>>>,
}

/// Start the OpenAI-compatible API server
pub async fn run_server(
    bind_addr: String,
    core_handle: CoreHandle,
    data_store: Arc<Mutex<AppDataStore>>,
    data_rx: Arc<Mutex<Receiver<DataChange>>>,
) -> Result<()> {
    let state = HTTPServerState {
        core_handle,
        data_store,
        data_rx,
    };

    let app = Router::new()
        .route("/:project_dtag/chat/completions", post(chat_completions))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    eprintln!("OpenAI-compatible API server listening on http://{}", bind_addr);
    eprintln!("Endpoint: http://{}/:project_dtag/chat/completions", bind_addr);

    axum::serve(listener, app).await?;

    Ok(())
}

/// Handler for POST /:project_dtag/chat/completions
async fn chat_completions(
    Path(project_dtag): Path<String>,
    State(state): State<HTTPServerState>,
    Json(request): Json<ChatCompletionRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Validate that we have messages
    if request.messages.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "messages array cannot be empty".to_string(),
        ));
    }

    // Get the last user message as the content to send
    let user_message = request
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "No user message found in messages array".to_string(),
            )
        })?;

    // Construct the project a-tag coordinate (format: kind:pubkey:identifier)
    // For now, we'll need to resolve this from the project_dtag
    // The project_dtag is the identifier part, we need to get the full coordinate
    let project_a_tag = resolve_project_coordinate(&state, &project_dtag)
        .map_err(|e| (StatusCode::NOT_FOUND, format!("Project not found: {}", e)))?;

    // Get the PM agent pubkey from the project status
    let agent_pubkey = get_pm_agent_pubkey(&state, &project_a_tag)
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, format!("Agent not available: {}", e)))?;

    // Create a title from the user message (first 50 chars)
    let title: String = if user_message.content.chars().count() > 50 {
        format!("{}...", user_message.content.chars().take(50).collect::<String>())
    } else {
        user_message.content.clone()
    };

    // Create response channel to get the thread ID (which is a valid 64-char hex event ID)
    let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<String>(1);

    // Publish a new thread using PublishThread command (not PublishMessage)
    // This creates a proper kind:1 event with no e-tags (thread root)
    let publish_result = state.core_handle.send(NostrCommand::PublishThread {
        project_a_tag: project_a_tag.clone(),
        title,
        content: user_message.content.clone(),
        agent_pubkey: Some(agent_pubkey.clone()),
        branch: None,
        nudge_ids: Vec::new(),
        reference_conversation_id: None,
        fork_message_id: None,
        response_tx: Some(response_tx),
    });

    if let Err(e) = publish_result {
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create thread: {}", e),
        ));
    }

    // Wait for the thread ID (with timeout)
    let thread_id = response_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Timeout waiting for thread creation".to_string(),
            )
        })?;

    // If streaming is requested, return SSE stream
    if request.stream {
        let stream = create_sse_stream(state.data_rx.clone(), thread_id, agent_pubkey);
        Ok(Sse::new(stream).keep_alive(KeepAlive::default()).into_response())
    } else {
        // Non-streaming: collect all chunks and return complete response
        // For simplicity, we'll still use streaming internally but collect it
        Err((
            StatusCode::NOT_IMPLEMENTED,
            "Non-streaming mode not yet implemented. Please use stream=true".to_string(),
        ))
    }
}

/// Create an SSE stream from the DataChange channel
fn create_sse_stream(
    data_rx: Arc<Mutex<Receiver<DataChange>>>,
    thread_id: String,
    agent_pubkey: String,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let stream = async_stream::stream! {
        // Send initial response with role
        let chunk = ChatCompletionChunk {
            id: thread_id.clone(),
            object: "chat.completion.chunk".to_string(),
            created: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            model: "tenex".to_string(),
            choices: vec![ChatCompletionChunkChoice {
                index: 0,
                delta: ChatCompletionDelta {
                    role: Some("assistant".to_string()),
                    content: None,
                },
                finish_reason: None,
            }],
        };

        if let Ok(json) = serde_json::to_string(&chunk) {
            yield Ok(Event::default().data(json));
        }

        // Poll for DataChange events
        loop {
            // Non-blocking receive with timeout
            let data_change = {
                let rx = data_rx.lock().unwrap();
                rx.recv_timeout(Duration::from_millis(100))
            };

            match data_change {
                Ok(DataChange::LocalStreamChunk {
                    agent_pubkey: chunk_agent,
                    conversation_id,
                    text_delta,
                    reasoning_delta: _,
                    is_finish,
                }) => {
                    // Only process chunks for our agent and thread
                    if chunk_agent != agent_pubkey || conversation_id != thread_id {
                        continue;
                    }

                    // Send text delta if present
                    if let Some(text) = text_delta {
                        let chunk = ChatCompletionChunk {
                            id: thread_id.clone(),
                            object: "chat.completion.chunk".to_string(),
                            created: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs(),
                            model: "tenex".to_string(),
                            choices: vec![ChatCompletionChunkChoice {
                                index: 0,
                                delta: ChatCompletionDelta {
                                    role: None,
                                    content: Some(text),
                                },
                                finish_reason: None,
                            }],
                        };

                        if let Ok(json) = serde_json::to_string(&chunk) {
                            yield Ok(Event::default().data(json));
                        }
                    }

                    // Send finish event if this is the last chunk
                    if is_finish {
                        let chunk = ChatCompletionChunk {
                            id: thread_id.clone(),
                            object: "chat.completion.chunk".to_string(),
                            created: std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_secs(),
                            model: "tenex".to_string(),
                            choices: vec![ChatCompletionChunkChoice {
                                index: 0,
                                delta: ChatCompletionDelta {
                                    role: None,
                                    content: None,
                                },
                                finish_reason: Some("stop".to_string()),
                            }],
                        };

                        if let Ok(json) = serde_json::to_string(&chunk) {
                            yield Ok(Event::default().data(json));
                        }

                        // Send [DONE] marker
                        yield Ok(Event::default().data("[DONE]"));
                        break;
                    }
                }
                Ok(_) => {
                    // Ignore other DataChange variants
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // Continue polling
                    continue;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    // Channel closed, end stream
                    break;
                }
            }
        }
    };

    stream
}

/// Default timeout for waiting for project to appear (10 seconds)
const WAIT_FOR_PROJECT_TIMEOUT_SECS: u64 = 10;

/// Resolve project dtag to full coordinate, waiting for project to appear if needed.
/// This addresses timing issues where HTTP requests arrive before projects are synced.
fn resolve_project_coordinate(state: &HTTPServerState, project_dtag: &str) -> Result<String> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(WAIT_FOR_PROJECT_TIMEOUT_SECS);

    loop {
        // Check for timeout
        if start.elapsed() > timeout {
            return Err(anyhow::anyhow!(
                "Timeout waiting for project '{}' to appear. Project may not exist or is not synced yet.",
                project_dtag
            ));
        }

        // Try to find the project
        {
            let store = state.data_store.lock().unwrap();
            let projects = store.get_projects();

            for project in projects {
                // Check if the project id (d-tag) matches
                if project.id == project_dtag {
                    return Ok(project.a_tag());
                }
            }
        }

        // Project not found yet, wait a bit before retrying
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Default timeout for waiting for project status (30 seconds)
const WAIT_FOR_STATUS_TIMEOUT_SECS: u64 = 30;

/// Get the PM agent pubkey from project status, waiting for status to appear if needed.
/// This addresses timing issues where HTTP requests arrive before project status is synced.
fn get_pm_agent_pubkey(state: &HTTPServerState, project_a_tag: &str) -> Result<String> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(WAIT_FOR_STATUS_TIMEOUT_SECS);

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

        // Status not found yet, wait a bit before retrying
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}
