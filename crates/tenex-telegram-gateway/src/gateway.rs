use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use tenex_core::events::CoreEvent;
use tenex_core::models::{AskQuestion, Message};
use tenex_core::nostr::NostrCommand;
use tenex_core::store::AppDataStore;

use crate::config::GatewayConfig;
use crate::ngrok::NgrokTunnel;
use crate::state::{
    BindingAgentOption, BindingProjectOption, ChatBinding, GatewayStateStore, MessageLink,
    MessageSource, PendingBindingSession, TriggerMode,
};
use crate::telegram::{
    AnswerCallbackQueryRequest, EditMessageTextRequest, InlineKeyboardButton, InlineKeyboardMarkup,
    SendMessageRequest, TelegramBotIdentity, TelegramCallbackQuery, TelegramClient,
    TelegramMessage, TelegramUpdate,
};
use crate::tenex::TenexContext;

const WEBHOOK_PATH: &str = "/telegram/webhook";
const BIND_ACTION_PREFIX: &str = "bind";

#[derive(Clone)]
struct AppState {
    config: GatewayConfig,
    telegram: TelegramClient,
    bot: TelegramBotIdentity,
    core_handle: tenex_core::runtime::CoreHandle,
    data_store: Arc<Mutex<AppDataStore>>,
    state_store: Arc<Mutex<GatewayStateStore>>,
}

pub async fn run_gateway(config: GatewayConfig, data_dir: &std::path::Path) -> Result<()> {
    let telegram = TelegramClient::new(config.telegram_bot_token.clone());
    let bot = TelegramBotIdentity::from_user(telegram.get_me().await?)?;

    let state_store = Arc::new(Mutex::new(GatewayStateStore::load(data_dir)?));
    let mut tenex = TenexContext::connect(&config, data_dir)?;
    let data_store = tenex.shared_data_store();

    {
        let state = state_store
            .lock()
            .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
        for binding in state.bindings() {
            tenex.subscribe_to_project(&binding.project_a_tag)?;
        }
        if state
            .bindings()
            .iter()
            .any(|binding| binding.trigger_mode == TriggerMode::Listen)
            && !bot.can_read_all_group_messages
        {
            eprintln!(
                "Warning: listen-mode bindings exist, but this bot does not report can_read_all_group_messages=true. Telegram group privacy settings may prevent listen-mode from working."
            );
        }
    }

    let app_state = AppState {
        config: config.clone(),
        telegram: telegram.clone(),
        bot: bot.clone(),
        core_handle: tenex.handle(),
        data_store: data_store.clone(),
        state_store: state_store.clone(),
    };

    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(WEBHOOK_PATH, post(webhook_handler))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(&config.webhook.bind_addr)
        .await
        .with_context(|| {
            format!(
                "Failed to bind Telegram webhook listener on {}",
                config.webhook.bind_addr
            )
        })?;

    let server = tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            eprintln!("Telegram webhook server stopped: {}", err);
        }
    });

    let mut ngrok = if config.ngrok.enabled {
        let tunnel = NgrokTunnel::start(&config.ngrok, &config.webhook.bind_addr).await?;
        eprintln!("ngrok tunnel ready at {}", tunnel.public_url());
        Some(tunnel)
    } else {
        None
    };

    let webhook_base_url = ngrok
        .as_ref()
        .map(|tunnel| tunnel.public_url().to_string())
        .unwrap_or_else(|| config.webhook.public_base_url.clone());
    let webhook_url = GatewayConfig::webhook_url_for_base_url(&webhook_base_url);

    telegram
        .set_webhook(&webhook_url, &config.webhook.secret_token)
        .await
        .with_context(|| format!("Failed to register Telegram webhook at {}", webhook_url))?;
    eprintln!("Telegram webhook registered at {}", webhook_url);
    eprintln!("Gateway pubkey: {}", tenex.gateway_pubkey());

    let gateway_pubkey = tenex.gateway_pubkey();

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                eprintln!("Shutting down telegram gateway");
                break;
            }
            events = tenex.tick(Duration::from_millis(250)) => {
                let events = events?;
                for event in events {
                    handle_core_event(
                        &telegram,
                        &bot,
                        &state_store,
                        &gateway_pubkey,
                        event,
                    )
                    .await?;
                }
            }
        }
    }

    server.abort();
    if let Some(mut tunnel) = ngrok.take() {
        tunnel.shutdown().await?;
    }
    Ok(())
}

async fn webhook_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(update): Json<TelegramUpdate>,
) -> StatusCode {
    match handle_update(state, headers, update).await {
        Ok(()) => StatusCode::OK,
        Err(err) => {
            eprintln!("Telegram update handling failed: {}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

async fn handle_update(state: AppState, headers: HeaderMap, update: TelegramUpdate) -> Result<()> {
    verify_secret(&state.config, &headers)?;

    {
        let mut store = state
            .state_store
            .lock()
            .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
        if !store.mark_update_processed(update.update_id)? {
            return Ok(());
        }
    }

    if let Some(change) = update.my_chat_member {
        let mut store = state
            .state_store
            .lock()
            .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
        store.record_observed_chat(
            change.chat.id,
            None,
            change
                .chat
                .title
                .clone()
                .unwrap_or_else(|| change.chat.id.to_string()),
            change.chat.kind.clone(),
            now_unix_secs(),
        )?;
    }

    if let Some(message) = update.message {
        handle_message_update(&state, message).await?;
    }

    if let Some(callback_query) = update.callback_query {
        handle_callback_query(&state, callback_query).await?;
    }

    Ok(())
}

async fn handle_message_update(state: &AppState, message: TelegramMessage) -> Result<()> {
    {
        let mut store = state
            .state_store
            .lock()
            .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
        store.record_observed_chat(
            message.chat.id,
            message.message_thread_id,
            message.chat_title(),
            message.chat.kind.clone(),
            message.date.unwrap_or_else(now_unix_secs),
        )?;
    }

    if message
        .from
        .as_ref()
        .map(|user| user.is_bot || user.id == state.bot.id)
        .unwrap_or(false)
    {
        return Ok(());
    }

    let explicit_trigger = is_explicit_bot_trigger(&message, &state.bot);
    let binding = {
        let store = state
            .state_store
            .lock()
            .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
        store.find_binding(message.chat.id, message.message_thread_id)
    };

    if binding.is_none() {
        let should_offer = message.chat.kind == "private"
            || bot_was_added(&message, state.bot.id)
            || explicit_trigger;
        if should_offer {
            offer_binding_wizard(state, &message).await?;
        }
        return Ok(());
    }
    let binding = binding.expect("binding checked above");

    let text = match message.text_content() {
        Some(value) => value.to_string(),
        None => return Ok(()),
    };

    let reply_link = {
        let store = state
            .state_store
            .lock()
            .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
        message.reply_to_message.as_ref().and_then(|reply| {
            store.find_link_for_reply(message.chat.id, message.message_thread_id, reply.message_id)
        })
    };

    let should_trigger = reply_link.is_some()
        || explicit_trigger
        || matches!(binding.trigger_mode, TriggerMode::Listen);
    if !should_trigger {
        return Ok(());
    }

    let cleaned_text = strip_trigger_prefix(&text, &state.bot);
    if cleaned_text.trim().is_empty() && reply_link.is_none() {
        return Ok(());
    }

    let (thread_id, reply_to, target_agent_pubkey, event_id, is_new_thread) = match reply_link {
        Some(link) => {
            let target_agent = link
                .author_pubkey
                .clone()
                .unwrap_or_else(|| binding.agent_pubkey.clone());
            let content = format_incoming_message(&message, cleaned_text.trim());
            let event_id = publish_message(
                &state.core_handle,
                &link.thread_id,
                &binding.project_a_tag,
                &content,
                &target_agent,
                Some(link.nostr_event_id.clone()),
            )
            .await?;
            (
                link.thread_id,
                Some(link.nostr_event_id),
                target_agent,
                event_id,
                false,
            )
        }
        None => {
            let content = format_incoming_message(&message, cleaned_text.trim());
            let title = build_thread_title(cleaned_text.trim());
            let event_id = publish_thread(
                &state.core_handle,
                &binding.project_a_tag,
                &title,
                &content,
                &binding.agent_pubkey,
            )
            .await?;
            (
                event_id.clone(),
                None,
                binding.agent_pubkey.clone(),
                event_id,
                true,
            )
        }
    };

    {
        let mut store = state
            .state_store
            .lock()
            .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
        if is_new_thread || store.thread_route(&thread_id).is_none() {
            store.remember_thread_route(
                thread_id.clone(),
                message.chat.id,
                message.message_thread_id,
                target_agent_pubkey.clone(),
                Some(message.message_id),
            )?;
        } else {
            store.update_thread_last_telegram_message(&thread_id, message.message_id)?;
        }

        store.link_telegram_message(MessageLink {
            chat_id: message.chat.id,
            message_thread_id: message.message_thread_id,
            telegram_message_id: message.message_id,
            thread_id,
            nostr_event_id: event_id,
            source: MessageSource::TelegramUser,
            author_pubkey: reply_to.map(|_| target_agent_pubkey),
        })?;
    }

    Ok(())
}

async fn handle_callback_query(state: &AppState, query: TelegramCallbackQuery) -> Result<()> {
    let Some(data) = query.data.as_deref() else {
        return Ok(());
    };
    if !data.starts_with(BIND_ACTION_PREFIX) {
        return Ok(());
    }

    let Some(message) = query.message.as_ref() else {
        state
            .telegram
            .answer_callback_query(AnswerCallbackQueryRequest {
                callback_query_id: query.id,
                text: Some("Binding message is no longer available.".to_string()),
                show_alert: Some(false),
            })
            .await?;
        return Ok(());
    };

    let mut parts = data.split(':');
    let _prefix = parts.next();
    let session_id = parts
        .next()
        .ok_or_else(|| anyhow!("Malformed binding callback payload"))?;
    let action = parts
        .next()
        .ok_or_else(|| anyhow!("Malformed binding callback payload"))?;
    let value = parts.next();

    let session = {
        let store = state
            .state_store
            .lock()
            .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
        store.get_binding_session(session_id)
    };

    let Some(session) = session else {
        answer_callback(
            &state.telegram,
            &query.id,
            "Binding session expired. Send another message to restart.",
        )
        .await?;
        return Ok(());
    };

    if query.from.id != session.requested_by_user_id {
        answer_callback(
            &state.telegram,
            &query.id,
            "Only the user who started binding this chat can complete it.",
        )
        .await?;
        return Ok(());
    }

    match action {
        "project" => {
            let project_index = parse_index(value)?;
            let project = session
                .projects
                .get(project_index)
                .cloned()
                .ok_or_else(|| anyhow!("Selected project index is out of range"))?;
            let agents = current_online_agents(&state.data_store, &project.project_a_tag)?;
            if agents.is_empty() {
                answer_callback(
                    &state.telegram,
                    &query.id,
                    "That project has no online agents right now.",
                )
                .await?;
                return Ok(());
            }

            {
                let mut store = state
                    .state_store
                    .lock()
                    .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
                store.update_binding_session_project(
                    session_id,
                    project.clone(),
                    agents.clone(),
                )?;
            }

            state
                .telegram
                .edit_message_text(EditMessageTextRequest {
                    chat_id: message.chat.id,
                    message_id: message.message_id,
                    text: render_agent_picker_text(&message.chat_title(), &project),
                    reply_markup: Some(build_agent_keyboard(session_id, &agents)),
                })
                .await?;
            answer_callback(&state.telegram, &query.id, "Project selected.").await?;
        }
        "agent" => {
            let agent_index = parse_index(value)?;
            let agent = session
                .agents
                .get(agent_index)
                .cloned()
                .ok_or_else(|| anyhow!("Selected agent index is out of range"))?;
            let project = session
                .selected_project
                .clone()
                .ok_or_else(|| anyhow!("Binding session missing project selection"))?;

            {
                let mut store = state
                    .state_store
                    .lock()
                    .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
                store.update_binding_session_agent(session_id, agent.clone())?;
            }

            state
                .telegram
                .edit_message_text(EditMessageTextRequest {
                    chat_id: message.chat.id,
                    message_id: message.message_id,
                    text: render_mode_picker_text(&message.chat_title(), &project, &agent),
                    reply_markup: Some(build_mode_keyboard(session_id)),
                })
                .await?;
            answer_callback(&state.telegram, &query.id, "Agent selected.").await?;
        }
        "mode" => {
            let trigger_mode = match value.unwrap_or_default() {
                "mention" => TriggerMode::Mention,
                "listen" => TriggerMode::Listen,
                _ => return Err(anyhow!("Unknown binding mode")),
            };

            let project = session
                .selected_project
                .clone()
                .ok_or_else(|| anyhow!("Binding session missing project selection"))?;
            let agent = session
                .selected_agent
                .clone()
                .ok_or_else(|| anyhow!("Binding session missing agent selection"))?;

            {
                let mut store = state
                    .state_store
                    .lock()
                    .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
                store.upsert_binding(ChatBinding {
                    chat_id: session.chat_id,
                    message_thread_id: session.message_thread_id,
                    project_slug: project.project_slug.clone(),
                    project_a_tag: project.project_a_tag.clone(),
                    project_title: project.project_title.clone(),
                    agent_pubkey: agent.agent_pubkey.clone(),
                    agent_name: agent.agent_name.clone(),
                    trigger_mode,
                })?;
                let _ = store.remove_binding_session(session_id)?;
            }

            state
                .core_handle
                .send(NostrCommand::SubscribeToProjectMessages {
                    project_a_tag: project.project_a_tag.clone(),
                })
                .map_err(|err| anyhow!("Failed to subscribe to bound project messages: {err}"))?;

            state
                .telegram
                .edit_message_text(EditMessageTextRequest {
                    chat_id: message.chat.id,
                    message_id: message.message_id,
                    text: render_binding_complete_text(
                        &project,
                        &agent,
                        trigger_mode,
                        state.bot.can_read_all_group_messages,
                    ),
                    reply_markup: None,
                })
                .await?;
            answer_callback(&state.telegram, &query.id, "Binding saved.").await?;
        }
        "cancel" => {
            {
                let mut store = state
                    .state_store
                    .lock()
                    .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
                let _ = store.remove_binding_session(session_id)?;
            }
            state
                .telegram
                .edit_message_text(EditMessageTextRequest {
                    chat_id: message.chat.id,
                    message_id: message.message_id,
                    text: "Binding cancelled.".to_string(),
                    reply_markup: None,
                })
                .await?;
            answer_callback(&state.telegram, &query.id, "Binding cancelled.").await?;
        }
        _ => return Err(anyhow!("Unknown binding callback action")),
    }

    Ok(())
}

async fn offer_binding_wizard(state: &AppState, message: &TelegramMessage) -> Result<()> {
    let requester_user_id = message
        .from
        .as_ref()
        .map(|user| user.id)
        .ok_or_else(|| anyhow!("Telegram message missing sender"))?;

    let existing_session = {
        let store = state
            .state_store
            .lock()
            .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
        store.active_binding_session(message.chat.id, message.message_thread_id)
    };

    if existing_session.is_some() {
        send_text_reply(
            &state.telegram,
            message.chat.id,
            message.message_thread_id,
            Some(message.message_id),
            "Binding is already in progress for this chat. Use the buttons above to finish it.",
            None,
        )
        .await?;
        return Ok(());
    }

    let projects = current_online_projects(&state.data_store)?;
    if projects.is_empty() {
        send_text_reply(
            &state.telegram,
            message.chat.id,
            message.message_thread_id,
            Some(message.message_id),
            "I don't see any online TENEX projects yet. Once a project publishes a live kind:24010 status, send another message here and I'll offer binding.",
            None,
        )
        .await?;
        return Ok(());
    }

    let session_id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
    let session = PendingBindingSession {
        session_id: session_id.clone(),
        chat_id: message.chat.id,
        message_thread_id: message.message_thread_id,
        requested_by_user_id: requester_user_id,
        wizard_message_id: None,
        projects: projects.clone(),
        selected_project: None,
        agents: Vec::new(),
        selected_agent: None,
        created_at: now_unix_secs(),
    };

    {
        let mut store = state
            .state_store
            .lock()
            .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
        store.save_binding_session(session)?;
    }

    let sent = state
        .telegram
        .send_message(SendMessageRequest {
            chat_id: message.chat.id,
            message_thread_id: message.message_thread_id,
            text: render_project_picker_text(&message.chat_title(), &projects),
            reply_to_message_id: Some(message.message_id),
            disable_notification: Some(false),
            reply_markup: Some(build_project_keyboard(&session_id, &projects)),
        })
        .await?;

    {
        let mut store = state
            .state_store
            .lock()
            .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
        store.update_binding_session_message(&session_id, sent.message_id)?;
    }

    Ok(())
}

async fn handle_core_event(
    telegram: &TelegramClient,
    bot: &TelegramBotIdentity,
    state_store: &Arc<Mutex<GatewayStateStore>>,
    gateway_pubkey: &str,
    event: CoreEvent,
) -> Result<()> {
    let CoreEvent::Message(message) = event else {
        return Ok(());
    };

    if message.pubkey == gateway_pubkey || message.is_reasoning || message.tool_name.is_some() {
        return Ok(());
    }

    let should_forward =
        message.ask_event.is_some() || message.p_tags.iter().any(|tag| tag == gateway_pubkey);
    if !should_forward {
        return Ok(());
    }

    let route = {
        let store = state_store
            .lock()
            .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
        if store.has_forwarded_nostr_event(&message.id) {
            return Ok(());
        }
        store.thread_route(&message.thread_id)
    };
    let Some(route) = route else {
        return Ok(());
    };

    let reply_to_message_id = {
        let store = state_store
            .lock()
            .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
        message
            .reply_to
            .as_ref()
            .and_then(|event_id| store.find_telegram_message_for_nostr_event(event_id))
            .map(|link| link.telegram_message_id)
            .or(route.last_telegram_message_id)
    };

    let rendered = render_agent_message(bot, &message);
    let sent = telegram
        .send_message(SendMessageRequest {
            chat_id: route.chat_id,
            message_thread_id: route.message_thread_id,
            text: rendered,
            reply_to_message_id,
            disable_notification: Some(false),
            reply_markup: None,
        })
        .await?;

    {
        let mut store = state_store
            .lock()
            .map_err(|_| anyhow!("Gateway state store is poisoned"))?;
        if !store.mark_nostr_event_forwarded(&message.id)? {
            return Ok(());
        }
        store.link_telegram_message(MessageLink {
            chat_id: route.chat_id,
            message_thread_id: route.message_thread_id,
            telegram_message_id: sent.message_id,
            thread_id: message.thread_id.clone(),
            nostr_event_id: message.id.clone(),
            source: MessageSource::Agent,
            author_pubkey: Some(message.pubkey.clone()),
        })?;
        store.update_thread_last_telegram_message(&message.thread_id, sent.message_id)?;
    }

    Ok(())
}

fn current_online_projects(
    data_store: &Arc<Mutex<AppDataStore>>,
) -> Result<Vec<BindingProjectOption>> {
    let store = data_store
        .lock()
        .map_err(|_| anyhow!("TENEX data store is poisoned"))?;

    let mut projects = store.query_projects_from_ndb();
    projects.sort_by(|left, right| left.id.cmp(&right.id));

    Ok(projects
        .into_iter()
        .filter_map(|project| {
            let project_a_tag = project.a_tag();
            store
                .get_project_status(&project_a_tag)
                .filter(|status| status.is_online())
                .map(|_| BindingProjectOption {
                    project_slug: project.id,
                    project_a_tag,
                    project_title: project.title,
                })
        })
        .collect())
}

fn current_online_agents(
    data_store: &Arc<Mutex<AppDataStore>>,
    project_a_tag: &str,
) -> Result<Vec<BindingAgentOption>> {
    let store = data_store
        .lock()
        .map_err(|_| anyhow!("TENEX data store is poisoned"))?;

    let Some(status) = store
        .get_project_status(project_a_tag)
        .filter(|status| status.is_online())
    else {
        return Ok(Vec::new());
    };

    Ok(status
        .agents
        .iter()
        .map(|agent| BindingAgentOption {
            agent_pubkey: agent.pubkey.clone(),
            agent_name: agent.name.clone(),
        })
        .collect())
}

fn verify_secret(config: &GatewayConfig, headers: &HeaderMap) -> Result<()> {
    let received = headers
        .get("x-telegram-bot-api-secret-token")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| anyhow!("Missing Telegram webhook secret header"))?;

    if received != config.webhook.secret_token {
        return Err(anyhow!("Telegram webhook secret token mismatch"));
    }

    Ok(())
}

fn is_explicit_bot_trigger(message: &TelegramMessage, bot: &TelegramBotIdentity) -> bool {
    if message
        .reply_to_message
        .as_ref()
        .and_then(|reply| reply.from.as_ref())
        .map(|user| user.id == bot.id)
        .unwrap_or(false)
    {
        return true;
    }

    let Some(text) = message.text_content() else {
        return false;
    };

    let mention = format!("@{}", bot.username.to_ascii_lowercase());
    if text.to_ascii_lowercase().contains(&mention) {
        return true;
    }

    let Some(first_token) = text.split_whitespace().next() else {
        return false;
    };
    let Some(command) = first_token.strip_prefix('/') else {
        return false;
    };

    match command.split_once('@') {
        Some((_, username)) => username.eq_ignore_ascii_case(&bot.username),
        None => true,
    }
}

fn strip_trigger_prefix(text: &str, bot: &TelegramBotIdentity) -> String {
    let mention = format!("@{}", bot.username.to_ascii_lowercase());
    let mut words = Vec::new();

    for (index, part) in text.split_whitespace().enumerate() {
        if index == 0 && part.starts_with('/') {
            if let Some((_, username)) = part.trim_start_matches('/').split_once('@') {
                if username.eq_ignore_ascii_case(&bot.username) {
                    continue;
                }
            } else {
                continue;
            }
        }

        if part.to_ascii_lowercase() == mention {
            continue;
        }

        words.push(part);
    }

    if words.is_empty() {
        text.trim().to_string()
    } else {
        words.join(" ")
    }
}

fn bot_was_added(message: &TelegramMessage, bot_id: i64) -> bool {
    message
        .new_chat_members
        .as_ref()
        .map(|members| members.iter().any(|user| user.id == bot_id))
        .unwrap_or(false)
}

fn format_incoming_message(message: &TelegramMessage, text: &str) -> String {
    let username = message
        .from
        .as_ref()
        .and_then(|user| user.username.clone())
        .map(|username| format!(" (@{})", username))
        .unwrap_or_default();

    let reply_context = message
        .reply_to_message
        .as_ref()
        .and_then(|reply| reply.text_content())
        .map(|reply_text| format!("\nreplying_to: {}", shorten(reply_text, 180)))
        .unwrap_or_default();

    format!(
        "[telegram]\nchat: {}{}\nfrom: {}{}\nmessage_id: {}\n\n{}",
        message.chat_title(),
        topic_suffix(message.message_thread_id),
        message.sender_name(),
        username,
        message.message_id,
        format!("{reply_context}\n\n{text}").trim(),
    )
}

fn build_thread_title(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "Telegram conversation".to_string();
    }
    shorten(trimmed, 72)
}

fn shorten(value: &str, max_chars: usize) -> String {
    let mut shortened = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        shortened.push_str("...");
    }
    shortened
}

fn topic_suffix(message_thread_id: Option<i64>) -> String {
    message_thread_id
        .map(|thread_id| format!("\ntopic_id: {}", thread_id))
        .unwrap_or_default()
}

fn render_project_picker_text(chat_title: &str, projects: &[BindingProjectOption]) -> String {
    let mut rendered = format!(
        "This chat is not bound to TENEX yet.\n\nChat: {}\n\nChoose the TENEX project for this chat or topic.",
        chat_title
    );
    rendered.push_str("\n\nOnline projects:");
    for project in projects {
        rendered.push_str(&format!("\n- {}", project.project_title));
    }
    rendered
}

fn render_agent_picker_text(chat_title: &str, project: &BindingProjectOption) -> String {
    format!(
        "Chat: {}\nProject: {}\n\nChoose the default TENEX agent for this chat or topic.",
        chat_title, project.project_title
    )
}

fn render_mode_picker_text(
    chat_title: &str,
    project: &BindingProjectOption,
    agent: &BindingAgentOption,
) -> String {
    format!(
        "Chat: {}\nProject: {}\nAgent: {}\n\nChoose how the bot should trigger in this chat.",
        chat_title, project.project_title, agent.agent_name
    )
}

fn render_binding_complete_text(
    project: &BindingProjectOption,
    agent: &BindingAgentOption,
    mode: TriggerMode,
    can_read_all_group_messages: bool,
) -> String {
    let mut rendered = format!(
        "Binding saved.\n\nProject: {}\nAgent: {}\nMode: {}",
        project.project_title,
        agent.agent_name,
        mode.as_str()
    );

    if mode == TriggerMode::Listen && !can_read_all_group_messages {
        rendered.push_str(
            "\n\nWarning: Telegram may still hide ordinary group messages from this bot until privacy mode is disabled or full group visibility is granted.",
        );
    }

    rendered
}

fn render_agent_message(bot: &TelegramBotIdentity, message: &Message) -> String {
    if let Some(ask_event) = &message.ask_event {
        let mut rendered = String::new();
        rendered.push_str("Agent needs input");
        if let Some(title) = &ask_event.title {
            rendered.push_str(": ");
            rendered.push_str(title);
        }
        if !ask_event.context.trim().is_empty() {
            rendered.push_str("\n\n");
            rendered.push_str(ask_event.context.trim());
        }
        if !ask_event.questions.is_empty() {
            rendered.push_str("\n\n");
            for (index, question) in ask_event.questions.iter().enumerate() {
                if index > 0 {
                    rendered.push('\n');
                }
                rendered.push_str(&format_ask_question(index + 1, question));
            }
        }
        rendered.push_str("\n\nReply to this message to answer.");
        return rendered;
    }

    let content = message.content.trim();
    if content.is_empty() {
        format!("{} sent an empty completion.", bot.username)
    } else {
        content.to_string()
    }
}

fn format_ask_question(index: usize, question: &AskQuestion) -> String {
    match question {
        AskQuestion::SingleSelect {
            title,
            question,
            suggestions,
        } => {
            let mut rendered = format!("{index}. {title}\n{question}");
            if !suggestions.is_empty() {
                rendered.push_str("\nSuggestions: ");
                rendered.push_str(&suggestions.join(", "));
            }
            rendered
        }
        AskQuestion::MultiSelect {
            title,
            question,
            options,
        } => {
            let mut rendered = format!("{index}. {title}\n{question}");
            if !options.is_empty() {
                rendered.push_str("\nOptions: ");
                rendered.push_str(&options.join(", "));
            }
            rendered
        }
    }
}

fn build_project_keyboard(
    session_id: &str,
    projects: &[BindingProjectOption],
) -> InlineKeyboardMarkup {
    let mut rows = projects
        .iter()
        .enumerate()
        .map(|(index, project)| {
            vec![InlineKeyboardButton {
                text: project.project_title.clone(),
                callback_data: format!("{BIND_ACTION_PREFIX}:{session_id}:project:{index}"),
            }]
        })
        .collect::<Vec<_>>();

    rows.push(vec![InlineKeyboardButton {
        text: "Cancel".to_string(),
        callback_data: format!("{BIND_ACTION_PREFIX}:{session_id}:cancel"),
    }]);

    InlineKeyboardMarkup {
        inline_keyboard: rows,
    }
}

fn build_agent_keyboard(session_id: &str, agents: &[BindingAgentOption]) -> InlineKeyboardMarkup {
    let mut rows = agents
        .iter()
        .enumerate()
        .map(|(index, agent)| {
            vec![InlineKeyboardButton {
                text: agent.agent_name.clone(),
                callback_data: format!("{BIND_ACTION_PREFIX}:{session_id}:agent:{index}"),
            }]
        })
        .collect::<Vec<_>>();

    rows.push(vec![InlineKeyboardButton {
        text: "Cancel".to_string(),
        callback_data: format!("{BIND_ACTION_PREFIX}:{session_id}:cancel"),
    }]);

    InlineKeyboardMarkup {
        inline_keyboard: rows,
    }
}

fn build_mode_keyboard(session_id: &str) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup {
        inline_keyboard: vec![
            vec![InlineKeyboardButton {
                text: "Mention-only".to_string(),
                callback_data: format!("{BIND_ACTION_PREFIX}:{session_id}:mode:mention"),
            }],
            vec![InlineKeyboardButton {
                text: "Listen mode".to_string(),
                callback_data: format!("{BIND_ACTION_PREFIX}:{session_id}:mode:listen"),
            }],
            vec![InlineKeyboardButton {
                text: "Cancel".to_string(),
                callback_data: format!("{BIND_ACTION_PREFIX}:{session_id}:cancel"),
            }],
        ],
    }
}

async fn send_text_reply(
    telegram: &TelegramClient,
    chat_id: i64,
    message_thread_id: Option<i64>,
    reply_to_message_id: Option<i64>,
    text: &str,
    reply_markup: Option<InlineKeyboardMarkup>,
) -> Result<()> {
    telegram
        .send_message(SendMessageRequest {
            chat_id,
            message_thread_id,
            text: text.to_string(),
            reply_to_message_id,
            disable_notification: Some(false),
            reply_markup,
        })
        .await?;
    Ok(())
}

async fn answer_callback(
    telegram: &TelegramClient,
    callback_query_id: &str,
    text: &str,
) -> Result<()> {
    telegram
        .answer_callback_query(AnswerCallbackQueryRequest {
            callback_query_id: callback_query_id.to_string(),
            text: Some(text.to_string()),
            show_alert: Some(false),
        })
        .await?;
    Ok(())
}

fn parse_index(value: Option<&str>) -> Result<usize> {
    value
        .ok_or_else(|| anyhow!("Missing callback selection index"))?
        .parse::<usize>()
        .map_err(|err| anyhow!("Invalid callback selection index: {err}"))
}

async fn publish_thread(
    core_handle: &tenex_core::runtime::CoreHandle,
    project_a_tag: &str,
    title: &str,
    content: &str,
    agent_pubkey: &str,
) -> Result<String> {
    let (response_tx, response_rx) = mpsc::sync_channel::<String>(1);
    core_handle
        .send(NostrCommand::PublishThread {
            project_a_tag: project_a_tag.to_string(),
            title: title.to_string(),
            content: content.to_string(),
            agent_pubkey: Some(agent_pubkey.to_string()),
            nudge_ids: Vec::new(),
            skill_ids: Vec::new(),
            reference_conversation_id: None,
            reference_report_a_tag: None,
            fork_message_id: None,
            response_tx: Some(response_tx),
        })
        .map_err(|err| anyhow!("Failed to send TENEX publish thread command: {err}"))?;
    await_publish(response_rx).await
}

async fn publish_message(
    core_handle: &tenex_core::runtime::CoreHandle,
    thread_id: &str,
    project_a_tag: &str,
    content: &str,
    agent_pubkey: &str,
    reply_to: Option<String>,
) -> Result<String> {
    let (response_tx, response_rx) = mpsc::sync_channel::<String>(1);
    core_handle
        .send(NostrCommand::PublishMessage {
            thread_id: thread_id.to_string(),
            project_a_tag: project_a_tag.to_string(),
            content: content.to_string(),
            agent_pubkey: Some(agent_pubkey.to_string()),
            reply_to,
            nudge_ids: Vec::new(),
            skill_ids: Vec::new(),
            ask_author_pubkey: None,
            response_tx: Some(response_tx),
        })
        .map_err(|err| anyhow!("Failed to send TENEX publish message command: {err}"))?;
    await_publish(response_rx).await
}

async fn await_publish(response_rx: mpsc::Receiver<String>) -> Result<String> {
    tokio::task::spawn_blocking(move || response_rx.recv_timeout(Duration::from_secs(10)))
        .await
        .map_err(|err| anyhow!("Publish wait task failed: {err}"))?
        .map_err(|_| anyhow!("Timed out waiting for local TENEX publish confirmation"))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
