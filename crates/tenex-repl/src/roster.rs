use tenex_core::models::ProjectAgent;
use tenex_core::store::app_data_store::AppDataStore;

pub(crate) fn project_roster_agents(store: &AppDataStore, a_tag: &str) -> Vec<ProjectAgent> {
    store.get_project_roster(a_tag).unwrap_or_default()
}

pub(crate) fn default_project_agent(store: &AppDataStore, a_tag: &str) -> Option<ProjectAgent> {
    project_roster_agents(store, a_tag).into_iter().next()
}

/// Whether the project is currently online — fresh kind:24010 heartbeat from
/// any approved backend. Do NOT use `ProjectAgent.is_online`, which only
/// reflects 24011 inventory presence (capability, not liveness).
pub(crate) fn project_has_available_agent(store: &AppDataStore, a_tag: &str) -> bool {
    store.is_project_online(a_tag)
}
