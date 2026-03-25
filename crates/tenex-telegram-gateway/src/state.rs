use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

const STATE_FILE_NAME: &str = "state.json";
const MAX_RECENT_UPDATES: usize = 2048;
const MAX_FORWARDED_EVENTS: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum TriggerMode {
    Mention,
    Listen,
}

impl TriggerMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mention => "mention",
            Self::Listen => "listen",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatBinding {
    pub chat_id: i64,
    pub message_thread_id: Option<i64>,
    pub project_slug: String,
    pub project_a_tag: String,
    pub project_title: String,
    pub agent_pubkey: String,
    pub agent_name: String,
    pub trigger_mode: TriggerMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservedChat {
    pub chat_id: i64,
    pub message_thread_id: Option<i64>,
    pub chat_title: String,
    pub chat_type: String,
    pub last_seen_at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageSource {
    TelegramUser,
    TelegramBot,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadRoute {
    pub thread_id: String,
    pub chat_id: i64,
    pub message_thread_id: Option<i64>,
    pub agent_pubkey: String,
    pub last_telegram_message_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageLink {
    pub chat_id: i64,
    pub message_thread_id: Option<i64>,
    pub telegram_message_id: i64,
    pub thread_id: String,
    pub nostr_event_id: String,
    pub source: MessageSource,
    pub author_pubkey: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingProjectOption {
    pub project_slug: String,
    pub project_a_tag: String,
    pub project_title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingAgentOption {
    pub agent_pubkey: String,
    pub agent_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingBindingSession {
    pub session_id: String,
    pub chat_id: i64,
    pub message_thread_id: Option<i64>,
    pub requested_by_user_id: i64,
    pub wizard_message_id: Option<i64>,
    pub projects: Vec<BindingProjectOption>,
    pub selected_project: Option<BindingProjectOption>,
    pub agents: Vec<BindingAgentOption>,
    pub selected_agent: Option<BindingAgentOption>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GatewayState {
    pub bindings: HashMap<String, ChatBinding>,
    pub observed_chats: HashMap<String, ObservedChat>,
    pub thread_routes: HashMap<String, ThreadRoute>,
    pub telegram_message_links: HashMap<String, MessageLink>,
    pub nostr_event_to_telegram: HashMap<String, String>,
    #[serde(default)]
    pub pending_binding_sessions: HashMap<String, PendingBindingSession>,
    pub recent_update_ids: Vec<i64>,
    pub forwarded_nostr_event_ids: Vec<String>,
}

pub struct GatewayStateStore {
    path: PathBuf,
    state: GatewayState,
}

impl GatewayStateStore {
    pub fn load(data_dir: &Path) -> Result<Self> {
        fs::create_dir_all(data_dir)
            .with_context(|| format!("Failed to create state dir {}", data_dir.display()))?;
        let path = data_dir.join(STATE_FILE_NAME);
        let state = if path.exists() {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("Failed to read {}", path.display()))?;
            serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse {}", path.display()))?
        } else {
            GatewayState::default()
        };
        Ok(Self { path, state })
    }

    pub fn bindings(&self) -> Vec<ChatBinding> {
        let mut bindings: Vec<_> = self.state.bindings.values().cloned().collect();
        bindings.sort_by_key(|binding| (binding.chat_id, binding.message_thread_id.unwrap_or(0)));
        bindings
    }

    pub fn active_binding_session(
        &self,
        chat_id: i64,
        message_thread_id: Option<i64>,
    ) -> Option<PendingBindingSession> {
        self.state
            .pending_binding_sessions
            .values()
            .find(|session| {
                session.chat_id == chat_id && session.message_thread_id == message_thread_id
            })
            .cloned()
    }

    pub fn get_binding_session(&self, session_id: &str) -> Option<PendingBindingSession> {
        self.state.pending_binding_sessions.get(session_id).cloned()
    }

    pub fn save_binding_session(&mut self, session: PendingBindingSession) -> Result<()> {
        if let Some(existing) =
            self.active_binding_session(session.chat_id, session.message_thread_id)
        {
            self.state
                .pending_binding_sessions
                .remove(&existing.session_id);
        }

        self.state
            .pending_binding_sessions
            .insert(session.session_id.clone(), session);
        self.save()
    }

    pub fn remove_binding_session(
        &mut self,
        session_id: &str,
    ) -> Result<Option<PendingBindingSession>> {
        let removed = self.state.pending_binding_sessions.remove(session_id);
        if removed.is_some() {
            self.save()?;
        }
        Ok(removed)
    }

    pub fn update_binding_session_message(
        &mut self,
        session_id: &str,
        wizard_message_id: i64,
    ) -> Result<()> {
        if let Some(session) = self.state.pending_binding_sessions.get_mut(session_id) {
            session.wizard_message_id = Some(wizard_message_id);
            self.save()?;
        }
        Ok(())
    }

    pub fn update_binding_session_project(
        &mut self,
        session_id: &str,
        project: BindingProjectOption,
        agents: Vec<BindingAgentOption>,
    ) -> Result<()> {
        if let Some(session) = self.state.pending_binding_sessions.get_mut(session_id) {
            session.selected_project = Some(project);
            session.agents = agents;
            session.selected_agent = None;
            self.save()?;
        }
        Ok(())
    }

    pub fn update_binding_session_agent(
        &mut self,
        session_id: &str,
        agent: BindingAgentOption,
    ) -> Result<()> {
        if let Some(session) = self.state.pending_binding_sessions.get_mut(session_id) {
            session.selected_agent = Some(agent);
            self.save()?;
        }
        Ok(())
    }

    pub fn observed_chats(&self) -> Vec<ObservedChat> {
        let mut chats: Vec<_> = self.state.observed_chats.values().cloned().collect();
        chats.sort_by_key(|chat| (chat.chat_id, chat.message_thread_id.unwrap_or(0)));
        chats
    }

    pub fn upsert_binding(&mut self, binding: ChatBinding) -> Result<()> {
        self.state.bindings.insert(
            scope_key(binding.chat_id, binding.message_thread_id),
            binding,
        );
        self.save()
    }

    pub fn remove_binding(&mut self, chat_id: i64, message_thread_id: Option<i64>) -> Result<bool> {
        let removed = self
            .state
            .bindings
            .remove(&scope_key(chat_id, message_thread_id))
            .is_some();
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    pub fn find_binding(
        &self,
        chat_id: i64,
        message_thread_id: Option<i64>,
    ) -> Option<ChatBinding> {
        if let Some(thread_id) = message_thread_id {
            if let Some(binding) = self
                .state
                .bindings
                .get(&scope_key(chat_id, Some(thread_id)))
            {
                return Some(binding.clone());
            }
        }

        self.state.bindings.get(&scope_key(chat_id, None)).cloned()
    }

    pub fn record_observed_chat(
        &mut self,
        chat_id: i64,
        message_thread_id: Option<i64>,
        chat_title: String,
        chat_type: String,
        last_seen_at: u64,
    ) -> Result<()> {
        let key = scope_key(chat_id, message_thread_id);
        self.state.observed_chats.insert(
            key,
            ObservedChat {
                chat_id,
                message_thread_id,
                chat_title,
                chat_type,
                last_seen_at,
            },
        );
        self.save()
    }

    pub fn mark_update_processed(&mut self, update_id: i64) -> Result<bool> {
        if self.state.recent_update_ids.contains(&update_id) {
            return Ok(false);
        }
        self.state.recent_update_ids.push(update_id);
        trim_recent(&mut self.state.recent_update_ids, MAX_RECENT_UPDATES);
        self.save()?;
        Ok(true)
    }

    pub fn mark_nostr_event_forwarded(&mut self, event_id: &str) -> Result<bool> {
        if self
            .state
            .forwarded_nostr_event_ids
            .iter()
            .any(|seen| seen == event_id)
        {
            return Ok(false);
        }
        self.state
            .forwarded_nostr_event_ids
            .push(event_id.to_string());
        trim_recent(
            &mut self.state.forwarded_nostr_event_ids,
            MAX_FORWARDED_EVENTS,
        );
        self.save()?;
        Ok(true)
    }

    pub fn has_forwarded_nostr_event(&self, event_id: &str) -> bool {
        self.state
            .forwarded_nostr_event_ids
            .iter()
            .any(|seen| seen == event_id)
    }

    pub fn remember_thread_route(
        &mut self,
        thread_id: String,
        chat_id: i64,
        message_thread_id: Option<i64>,
        agent_pubkey: String,
        last_telegram_message_id: Option<i64>,
    ) -> Result<()> {
        self.state.thread_routes.insert(
            thread_id.clone(),
            ThreadRoute {
                thread_id,
                chat_id,
                message_thread_id,
                agent_pubkey,
                last_telegram_message_id,
            },
        );
        self.save()
    }

    pub fn thread_route(&self, thread_id: &str) -> Option<ThreadRoute> {
        self.state.thread_routes.get(thread_id).cloned()
    }

    pub fn update_thread_last_telegram_message(
        &mut self,
        thread_id: &str,
        telegram_message_id: i64,
    ) -> Result<()> {
        if let Some(route) = self.state.thread_routes.get_mut(thread_id) {
            route.last_telegram_message_id = Some(telegram_message_id);
            self.save()?;
        }
        Ok(())
    }

    pub fn link_telegram_message(&mut self, link: MessageLink) -> Result<()> {
        let key = message_key(
            link.chat_id,
            link.message_thread_id,
            link.telegram_message_id,
        );
        self.state
            .nostr_event_to_telegram
            .insert(link.nostr_event_id.clone(), key.clone());
        self.state.telegram_message_links.insert(key, link);
        self.save()
    }

    pub fn find_link_for_reply(
        &self,
        chat_id: i64,
        message_thread_id: Option<i64>,
        telegram_message_id: i64,
    ) -> Option<MessageLink> {
        self.state
            .telegram_message_links
            .get(&message_key(
                chat_id,
                message_thread_id,
                telegram_message_id,
            ))
            .cloned()
    }

    pub fn find_telegram_message_for_nostr_event(&self, event_id: &str) -> Option<MessageLink> {
        let key = self.state.nostr_event_to_telegram.get(event_id)?;
        self.state.telegram_message_links.get(key).cloned()
    }

    fn save(&self) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.state)
            .with_context(|| format!("Failed to serialize {}", self.path.display()))?;
        fs::write(&self.path, json)
            .with_context(|| format!("Failed to write {}", self.path.display()))?;
        Ok(())
    }
}

pub fn scope_key(chat_id: i64, message_thread_id: Option<i64>) -> String {
    match message_thread_id {
        Some(thread_id) => format!("{chat_id}:{thread_id}"),
        None => format!("{chat_id}:root"),
    }
}

pub fn message_key(chat_id: i64, message_thread_id: Option<i64>, message_id: i64) -> String {
    match message_thread_id {
        Some(thread_id) => format!("{chat_id}:{thread_id}:{message_id}"),
        None => format!("{chat_id}:root:{message_id}"),
    }
}

fn trim_recent<T>(items: &mut Vec<T>, max_len: usize) {
    if items.len() > max_len {
        let overflow = items.len() - max_len;
        items.drain(0..overflow);
    }
}
