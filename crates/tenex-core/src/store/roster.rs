use crate::models::{AgentConfig, InstalledAgent, Project, ProjectAgent};
use std::collections::{HashMap, HashSet};

fn fallback_agent_name(pubkey: &str) -> String {
    pubkey.chars().take(16).collect()
}

fn best_approved_inventory<'a, F>(
    agent_pubkey: &str,
    installed_agents_by_backend: &'a HashMap<String, Vec<InstalledAgent>>,
    is_backend_approved: &F,
) -> Option<&'a InstalledAgent>
where
    F: Fn(&str) -> bool,
{
    let mut candidates: Vec<&InstalledAgent> = installed_agents_by_backend
        .iter()
        .filter(|(backend_pubkey, _)| is_backend_approved(backend_pubkey))
        .flat_map(|(_, agents)| agents.iter())
        .filter(|agent| agent.pubkey == agent_pubkey)
        .collect();

    candidates.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| a.backend_pubkey.cmp(&b.backend_pubkey))
            .then_with(|| a.slug.cmp(&b.slug))
    });

    candidates.into_iter().next()
}

/// Build the canonical project roster.
///
/// Membership and order come from the project's kind:31933 `p` tags. The first
/// roster entry is the PM/default. kind:24011 inventories from approved
/// backends mark entries online and provide backend-hosted slugs. kind:34011
/// configs enrich entries with the agent's current model/tools/skills/MCP.
pub fn build_project_roster<F>(
    project: &Project,
    installed_agents_by_backend: &HashMap<String, Vec<InstalledAgent>>,
    agent_configs_by_pubkey: &HashMap<String, AgentConfig>,
    is_backend_approved: F,
) -> Vec<ProjectAgent>
where
    F: Fn(&str) -> bool,
{
    let mut seen = HashSet::new();
    let mut roster = Vec::new();

    for pubkey in &project.agent_pubkeys {
        if !seen.insert(pubkey.clone()) {
            continue;
        }

        let config = agent_configs_by_pubkey.get(pubkey);
        let inventory =
            best_approved_inventory(pubkey, installed_agents_by_backend, &is_backend_approved);
        let name = inventory
            .map(|agent| agent.slug.clone())
            .or_else(|| config.map(|cfg| cfg.slug.clone()))
            .unwrap_or_else(|| fallback_agent_name(pubkey));

        roster.push(ProjectAgent {
            pubkey: pubkey.clone(),
            name,
            backend_pubkey: inventory
                .map(|agent| agent.backend_pubkey.clone())
                .unwrap_or_default(),
            is_pm: roster.is_empty(),
            is_online: inventory.is_some(),
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
        });
    }

    roster
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(agent_pubkeys: Vec<&str>) -> Project {
        Project {
            id: "project".to_string(),
            title: "Project".to_string(),
            description: None,
            repo_url: None,
            picture_url: None,
            is_deleted: false,
            pubkey: "owner".to_string(),
            participants: Vec::new(),
            agent_pubkeys: agent_pubkeys.into_iter().map(str::to_string).collect(),
            mcp_tool_ids: Vec::new(),
            created_at: 1,
        }
    }

    fn installed_agent(backend_pubkey: &str, pubkey: &str, slug: &str) -> InstalledAgent {
        InstalledAgent {
            backend_pubkey: backend_pubkey.to_string(),
            pubkey: pubkey.to_string(),
            slug: slug.to_string(),
            created_at: 1,
        }
    }

    fn agent_config(pubkey: &str) -> AgentConfig {
        AgentConfig {
            pubkey: pubkey.to_string(),
            slug: "config-name".to_string(),
            backend_pubkey: Some("backend".to_string()),
            created_at: 2,
            active_model: Some("model-active".to_string()),
            models: vec!["model-active".to_string()],
            active_tools: vec!["tool-active".to_string()],
            tools: vec!["tool-active".to_string()],
            active_skills: vec!["skill-active".to_string()],
            skills: vec!["skill-active".to_string()],
            active_mcps: vec!["mcp-active".to_string()],
            mcps: vec!["mcp-active".to_string()],
        }
    }

    #[test]
    fn preserves_project_order_and_marks_first_agent_as_pm() {
        let roster = build_project_roster(
            &project(vec!["agent-b", "agent-a", "agent-c"]),
            &HashMap::new(),
            &HashMap::new(),
            |_| false,
        );

        assert_eq!(
            roster.iter().map(|agent| agent.pubkey.as_str()).collect::<Vec<_>>(),
            vec!["agent-b", "agent-a", "agent-c"]
        );
        assert!(roster[0].is_pm);
        assert!(!roster[1].is_pm);
        assert!(!roster[2].is_pm);
    }

    #[test]
    fn marks_online_from_approved_24011_inventory_only() {
        let mut installed = HashMap::new();
        installed.insert(
            "approved-backend".to_string(),
            vec![installed_agent("approved-backend", "agent-a", "available-a")],
        );
        installed.insert(
            "untrusted-backend".to_string(),
            vec![installed_agent("untrusted-backend", "agent-b", "available-b")],
        );

        let roster = build_project_roster(
            &project(vec!["agent-a", "agent-b"]),
            &installed,
            &HashMap::new(),
            |backend| backend == "approved-backend",
        );

        assert!(roster[0].is_online);
        assert_eq!(roster[0].backend_pubkey, "approved-backend");
        assert_eq!(roster[0].name, "available-a");
        assert!(!roster[1].is_online);
        assert!(roster[1].backend_pubkey.is_empty());
    }

    /// Two distinct agent pubkeys may share the same effective display name
    /// (their 24011 slug or 34011 slug, or downstream a kind:0 display_name).
    /// The roster identifies entries by *pubkey*, so a duplicate name must
    /// still leave two distinct roster entries that each round-trip back to
    /// the right pubkey.
    #[test]
    fn duplicate_display_names_remain_distinct_by_pubkey() {
        let mut installed = HashMap::new();
        installed.insert(
            "approved-backend".to_string(),
            vec![
                installed_agent("approved-backend", "agent-a", "duplicate-name"),
                installed_agent("approved-backend", "agent-b", "duplicate-name"),
            ],
        );

        let roster = build_project_roster(
            &project(vec!["agent-a", "agent-b"]),
            &installed,
            &HashMap::new(),
            |backend| backend == "approved-backend",
        );

        assert_eq!(roster.len(), 2, "two distinct pubkeys must yield two entries");

        // Both rows have the same display name…
        assert_eq!(roster[0].name, "duplicate-name");
        assert_eq!(roster[1].name, "duplicate-name");

        // …but the underlying pubkeys are distinct and order is preserved
        // from the project's 31933 `p` tags.
        assert_eq!(roster[0].pubkey, "agent-a");
        assert_eq!(roster[1].pubkey, "agent-b");

        // Lookup-by-pubkey: each query returns exactly the right record.
        let by_pubkey = |pk: &str| {
            roster
                .iter()
                .filter(|a| a.pubkey == pk)
                .collect::<Vec<_>>()
        };
        let only_a = by_pubkey("agent-a");
        let only_b = by_pubkey("agent-b");
        assert_eq!(only_a.len(), 1);
        assert_eq!(only_b.len(), 1);
        assert_eq!(only_a[0].pubkey, "agent-a");
        assert_eq!(only_b[0].pubkey, "agent-b");
    }

    /// Acceptance: 34011 config slugs may collide just like 24011 slugs.
    /// Even when both pubkeys publish a 34011 with the same slug, the roster
    /// must treat them as separate identities.
    #[test]
    fn duplicate_34011_config_slugs_remain_distinct_by_pubkey() {
        let mut configs = HashMap::new();
        configs.insert("agent-a".to_string(), agent_config("agent-a"));
        configs.insert("agent-b".to_string(), agent_config("agent-b"));
        // Both configs share the slug "config-name" (set in `agent_config`).

        let roster = build_project_roster(
            &project(vec!["agent-a", "agent-b"]),
            &HashMap::new(),
            &configs,
            |_| false,
        );

        assert_eq!(roster.len(), 2);
        assert_eq!(roster[0].name, "config-name");
        assert_eq!(roster[1].name, "config-name");
        assert_eq!(roster[0].pubkey, "agent-a");
        assert_eq!(roster[1].pubkey, "agent-b");
        // First slot is PM; identity is pubkey-keyed.
        assert!(roster[0].is_pm);
        assert!(!roster[1].is_pm);
    }

    #[test]
    fn enriches_roster_from_34011_config() {
        let mut configs = HashMap::new();
        configs.insert("agent-a".to_string(), agent_config("agent-a"));

        let roster = build_project_roster(
            &project(vec!["agent-a"]),
            &HashMap::new(),
            &configs,
            |_| false,
        );

        assert_eq!(roster[0].name, "config-name");
        assert_eq!(roster[0].model.as_deref(), Some("model-active"));
        assert_eq!(roster[0].tools, vec!["tool-active"]);
        assert_eq!(roster[0].skills, vec!["skill-active"]);
        assert_eq!(roster[0].mcp_servers, vec!["mcp-active"]);
    }
}
