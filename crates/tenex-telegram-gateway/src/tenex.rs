use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use nostr_sdk::prelude::Keys;
use tenex_core::config::CoreConfig;
use tenex_core::events::CoreEvent;
use tenex_core::models::{Project, ProjectAgent, ProjectStatus};
use tenex_core::nostr::{DataChange, NostrCommand};
use tenex_core::runtime::{process_note_keys, CoreHandle, CoreRuntime};
use tenex_core::store::AppDataStore;

use crate::config::{parse_gateway_keys, GatewayConfig};

pub struct TenexContext {
    core_runtime: CoreRuntime,
    core_handle: CoreHandle,
    shared_data_store: Arc<Mutex<AppDataStore>>,
    data_rx: Receiver<DataChange>,
    gateway_keys: Keys,
}

#[derive(Debug, Clone)]
pub struct BindingResolution {
    pub project: Project,
    pub agent: ProjectAgent,
}

impl TenexContext {
    pub fn connect(config: &GatewayConfig, data_dir: &std::path::Path) -> Result<Self> {
        let gateway_keys = parse_gateway_keys(&config.gateway_nsec)?;
        let tenex_cache_dir = GatewayConfig::tenex_cache_dir(data_dir);
        std::fs::create_dir_all(&tenex_cache_dir)
            .with_context(|| format!("Failed to create {}", tenex_cache_dir.display()))?;

        let mut core_runtime = CoreRuntime::new(CoreConfig::new(&tenex_cache_dir))?;
        let core_handle = core_runtime.handle();
        let shared_data_store = Arc::new(Mutex::new(AppDataStore::new(core_runtime.ndb())));

        {
            let mut store = shared_data_store
                .lock()
                .map_err(|_| anyhow!("Shared TENEX data store is poisoned"))?;
            store.trust.set_trusted_backends(
                config.approved_backend_pubkeys.iter().cloned().collect(),
                config.blocked_backend_pubkeys.iter().cloned().collect(),
            );
        }

        let (response_tx, response_rx) = mpsc::channel();
        core_handle
            .send(NostrCommand::Connect {
                keys: gateway_keys.clone(),
                user_pubkey: gateway_keys.public_key().to_hex(),
                relay_urls: config.relay_urls.clone(),
                response_tx: Some(response_tx),
            })
            .map_err(|err| anyhow!("Failed to send TENEX connect command: {err}"))?;

        match response_rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return Err(anyhow!("TENEX connect failed: {err}")),
            Err(_) => return Err(anyhow!("Timed out waiting for TENEX connect")),
        }

        let data_rx = core_runtime
            .take_data_rx()
            .ok_or_else(|| anyhow!("TENEX runtime data channel was unavailable"))?;

        Ok(Self {
            core_runtime,
            core_handle,
            shared_data_store,
            data_rx,
            gateway_keys,
        })
    }

    pub fn handle(&self) -> CoreHandle {
        self.core_handle.clone()
    }

    pub fn shared_data_store(&self) -> Arc<Mutex<AppDataStore>> {
        self.shared_data_store.clone()
    }

    pub fn gateway_pubkey(&self) -> String {
        self.gateway_keys.public_key().to_hex()
    }

    pub fn subscribe_to_project(&self, project_a_tag: &str) -> Result<()> {
        self.core_handle
            .send(NostrCommand::SubscribeToProjectMessages {
                project_a_tag: project_a_tag.to_string(),
            })
            .map_err(|err| anyhow!("Failed to subscribe to project messages: {err}"))?;
        Ok(())
    }

    pub async fn sync_for(&mut self, timeout: Duration) -> Result<Vec<CoreEvent>> {
        let deadline = Instant::now() + timeout;
        let mut events = Vec::new();

        while Instant::now() < deadline {
            self.drain_data_changes()?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            let slice = remaining.min(Duration::from_millis(250));
            let maybe_events = self.tick(slice).await?;
            if !maybe_events.is_empty() {
                events.extend(maybe_events);
            }
        }

        self.drain_data_changes()?;
        Ok(events)
    }

    pub async fn tick(&mut self, wait: Duration) -> Result<Vec<CoreEvent>> {
        self.drain_data_changes()?;

        let note_keys = tokio::select! {
            note_keys = self.core_runtime.next_note_keys() => note_keys,
            _ = tokio::time::sleep(wait) => None,
        };

        let Some(note_keys) = note_keys else {
            self.drain_data_changes()?;
            return Ok(Vec::new());
        };

        let ndb = self.core_runtime.ndb();
        let mut store = self
            .shared_data_store
            .lock()
            .map_err(|_| anyhow!("Shared TENEX data store is poisoned"))?;
        let events = process_note_keys(ndb.as_ref(), &mut store, &self.core_handle, &note_keys)?;
        drop(store);
        self.drain_data_changes()?;
        Ok(events)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let store = self
            .shared_data_store
            .lock()
            .map_err(|_| anyhow!("Shared TENEX data store is poisoned"))?;
        let mut projects = store.query_projects_from_ndb();
        projects.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(projects)
    }

    pub fn resolve_binding(
        &self,
        project_slug: &str,
        agent_selector: Option<&str>,
    ) -> Result<BindingResolution> {
        let store = self
            .shared_data_store
            .lock()
            .map_err(|_| anyhow!("Shared TENEX data store is poisoned"))?;
        let project = store
            .query_projects_from_ndb()
            .into_iter()
            .find(|project| project.id == project_slug)
            .ok_or_else(|| anyhow!("Project '{}' was not found", project_slug))?;
        let status = store
            .get_project_status(&project.a_tag())
            .cloned()
            .filter(|status| status.is_online())
            .ok_or_else(|| anyhow!("Project '{}' is not online", project_slug))?;

        let agent = match agent_selector {
            Some(selector) => select_agent(&status, selector)?,
            None => status
                .pm_agent()
                .cloned()
                .ok_or_else(|| anyhow!("Project '{}' has no PM agent online", project_slug))?,
        };

        Ok(BindingResolution { project, agent })
    }

    pub fn online_agents(&self, project_slug: &str) -> Result<(Project, ProjectStatus)> {
        let store = self
            .shared_data_store
            .lock()
            .map_err(|_| anyhow!("Shared TENEX data store is poisoned"))?;
        let project = store
            .query_projects_from_ndb()
            .into_iter()
            .find(|project| project.id == project_slug)
            .ok_or_else(|| anyhow!("Project '{}' was not found", project_slug))?;
        let status = store
            .get_project_status(&project.a_tag())
            .cloned()
            .filter(|status| status.is_online())
            .ok_or_else(|| anyhow!("Project '{}' is not online", project_slug))?;
        Ok((project, status))
    }

    fn drain_data_changes(&mut self) -> Result<()> {
        loop {
            match self.data_rx.try_recv() {
                Ok(DataChange::ProjectStatus { json }) => {
                    let mut store = self
                        .shared_data_store
                        .lock()
                        .map_err(|_| anyhow!("Shared TENEX data store is poisoned"))?;
                    store.handle_status_event_json(&json);
                }
                Ok(_) => {}
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(anyhow!("TENEX runtime data channel disconnected"));
                }
            }
        }
        Ok(())
    }
}

fn select_agent(status: &ProjectStatus, selector: &str) -> Result<ProjectAgent> {
    if let Some(agent) = status
        .agents
        .iter()
        .find(|agent| agent.pubkey.eq_ignore_ascii_case(selector))
    {
        return Ok(agent.clone());
    }

    let selector_lower = selector.to_ascii_lowercase();
    status
        .agents
        .iter()
        .find(|agent| agent.name.to_ascii_lowercase() == selector_lower)
        .cloned()
        .ok_or_else(|| anyhow!("Agent '{}' is not online in this project", selector))
}
