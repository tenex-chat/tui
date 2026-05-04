use tenex_core::models::{AgentConfig, AgentInventoryItem, Project, ProjectAgent};
use tenex_core::store::app_data_store::AppDataStore;

/// Local roster-migration shim.
///
/// Replace this with `AppDataStore::get_online_agents` once core returns
/// 31933-ordered roster semantics backed by 24011 availability.
pub(super) fn project_roster_agents(
    store: &AppDataStore,
    project: &Project,
) -> Vec<ProjectAgent> {
    let inventory = store.agent_inventory();

    project
        .agent_pubkeys
        .iter()
        .enumerate()
        .map(|(idx, pubkey)| {
            let config = store.get_agent_config(pubkey);
            let inventory_item = inventory.iter().find(|item| item.pubkey == *pubkey);
            roster_agent_from_sources(store, pubkey, idx == 0, config, inventory_item)
        })
        .collect()
}

pub(super) fn project_has_available_agent(store: &AppDataStore, project: &Project) -> bool {
    project_roster_agents(store, project)
        .iter()
        .any(|agent| agent.is_online)
}

pub(super) fn default_agent_pubkey(project: &Project) -> Option<String> {
    project.agent_pubkeys.first().cloned()
}

fn roster_agent_from_sources(
    store: &AppDataStore,
    pubkey: &str,
    is_pm: bool,
    config: Option<&AgentConfig>,
    inventory_item: Option<&AgentInventoryItem>,
) -> ProjectAgent {
    let name = store
        .get_profile_name_if_known(pubkey)
        .or_else(|| config.map(|cfg| cfg.slug.clone()))
        .or_else(|| inventory_item.map(|item| item.slug.clone()))
        .unwrap_or_else(|| store.get_profile_name(pubkey));

    let backend_pubkey = config
        .and_then(|cfg| {
            cfg.backend_pubkey.as_ref().and_then(|backend_pubkey| {
                inventory_item
                    .filter(|item| {
                        item.backends
                            .iter()
                            .any(|backend| backend.backend_pubkey == *backend_pubkey)
                    })
                    .map(|_| backend_pubkey.clone())
            })
        })
        .or_else(|| {
            inventory_item.and_then(|item| {
                item.backends
                    .first()
                    .map(|backend| backend.backend_pubkey.clone())
            })
        })
        .or_else(|| config.and_then(|cfg| cfg.backend_pubkey.clone()))
        .unwrap_or_default();

    ProjectAgent {
        pubkey: pubkey.to_string(),
        name,
        backend_pubkey,
        is_pm,
        is_online: inventory_item.is_some(),
        model: config.and_then(|cfg| cfg.active_model.clone()),
        tools: config
            .map(|cfg| cfg.active_tools.clone())
            .unwrap_or_default(),
        skills: config
            .map(|cfg| cfg.active_skills.clone())
            .unwrap_or_default(),
        mcp_servers: config
            .map(|cfg| cfg.active_mcps.clone())
            .unwrap_or_default(),
    }
}
