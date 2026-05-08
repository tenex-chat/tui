use tenex_core::models::{Project, ProjectAgent};
use tenex_core::store::app_data_store::AppDataStore;

pub(super) fn project_roster_agents(store: &AppDataStore, project: &Project) -> Vec<ProjectAgent> {
    store
        .get_project_roster(&project.a_tag())
        .unwrap_or_default()
}

/// Whether the project is currently online — fresh kind:24010 heartbeat from
/// any approved backend. Do NOT use `ProjectAgent.is_online`, which only
/// reflects 24011 inventory presence (capability, not liveness).
pub(super) fn project_has_available_agent(store: &AppDataStore, project: &Project) -> bool {
    store.is_project_online(&project.a_tag())
}

pub(super) fn default_agent_pubkey(project: &Project) -> Option<String> {
    project.agent_pubkeys.first().cloned()
}
