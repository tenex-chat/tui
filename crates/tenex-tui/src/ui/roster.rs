use crate::models::{AgentConfig, AgentInventoryItem, Project, ProjectAgent};
use crate::store::AppDataStore;

/// Local roster-migration shim.
///
/// Replace this with `AppDataStore::get_online_agents` once core returns
/// 31933-ordered roster semantics backed by 24011 availability.
pub(crate) fn project_roster_agents(store: &AppDataStore, project: &Project) -> Vec<ProjectAgent> {
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

pub(crate) fn default_project_agent(
    store: &AppDataStore,
    project: &Project,
) -> Option<ProjectAgent> {
    project_roster_agents(store, project).into_iter().next()
}

/// Whether the project is currently online — i.e. has a fresh kind:24010
/// heartbeat. Do NOT confuse with `ProjectAgent.is_online`, which only reflects
/// kind:24011 inventory presence (agent installed in some backend) and is not a
/// liveness signal.
pub(crate) fn project_has_available_agent(store: &AppDataStore, project: &Project) -> bool {
    store.is_project_online(&project.a_tag())
}

/// Pubkey of a backend currently running the project (fresh kind:24010
/// heartbeat). Use this when routing commands to a live backend. Falls back to
/// `None` if no backend is running.
pub(crate) fn first_available_backend_for_project(
    store: &AppDataStore,
    project: &Project,
) -> Option<String> {
    store.first_online_backend_for_project(&project.a_tag())
}

pub(crate) fn resolve_selected_agent_from_roster(
    current: Option<&ProjectAgent>,
    roster: &[ProjectAgent],
) -> Option<ProjectAgent> {
    if let Some(current_agent) = current {
        if let Some(updated) = roster
            .iter()
            .find(|agent| agent.pubkey == current_agent.pubkey)
        {
            return Some(updated.clone());
        }
        return Some(current_agent.clone());
    }

    roster.first().cloned()
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

#[cfg(test)]
mod tests {
    use super::resolve_selected_agent_from_roster;
    use crate::models::ProjectAgent;

    fn agent(pubkey: &str, is_pm: bool, is_online: bool) -> ProjectAgent {
        ProjectAgent {
            pubkey: pubkey.to_string(),
            name: pubkey.to_string(),
            backend_pubkey: String::new(),
            is_pm,
            is_online,
            model: None,
            tools: vec![],
            skills: vec![],
            mcp_servers: vec![],
        }
    }

    #[test]
    fn default_agent_is_first_roster_entry_not_online_or_status_pm() {
        let roster = vec![
            agent("first-31933", true, false),
            agent("online-agent", false, true),
        ];

        let resolved = resolve_selected_agent_from_roster(None, &roster)
            .expect("first roster agent should be selected");

        assert_eq!(resolved.pubkey, "first-31933");
    }

    #[test]
    fn current_agent_is_refreshed_from_roster_without_reordering() {
        let current = agent("second", false, false);
        let roster = vec![agent("first", true, true), agent("second", false, true)];

        let resolved = resolve_selected_agent_from_roster(Some(&current), &roster)
            .expect("current agent should resolve");

        assert_eq!(resolved.pubkey, "second");
        assert!(resolved.is_online);
    }
}
