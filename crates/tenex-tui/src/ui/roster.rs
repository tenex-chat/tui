use crate::models::{Project, ProjectAgent};
use crate::store::AppDataStore;

pub(crate) fn project_roster_agents(store: &AppDataStore, project: &Project) -> Vec<ProjectAgent> {
    store
        .get_project_roster(&project.a_tag())
        .unwrap_or_default()
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
