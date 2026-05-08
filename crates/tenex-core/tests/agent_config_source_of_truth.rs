use std::collections::HashMap;

use serde_json::json;
use tenex_core::models::{AgentConfig, Project};
use tenex_core::store::roster::build_project_roster;

fn project_with_agents(agent_pubkeys: Vec<String>) -> Project {
    Project {
        id: "source-of-truth".to_string(),
        title: "Source of Truth".to_string(),
        description: None,
        repo_url: None,
        picture_url: None,
        is_deleted: false,
        is_private: false,
        pubkey: "owner".to_string(),
        participants: Vec::new(),
        agent_pubkeys,
        mcp_tool_ids: Vec::new(),
        created_at: 1,
    }
}

fn agent_config_event(kind: u64, agent_pubkey: &str) -> serde_json::Value {
    json!({
        "kind": kind,
        "pubkey": agent_pubkey,
        "created_at": 1_700_000_000_u64,
        "content": "{\"name\":\"Planner\"}",
        "tags": [
            ["slug", "planner"],
            ["p", "backend-pubkey"],
            ["model", "opus"],
            ["tool", "shell", "active"],
            ["skill", "audit", "active"],
            ["mcp", "github", "active"]
        ]
    })
}

#[test]
fn historical_34011_does_not_parse_as_agent_config() {
    let agent_pubkey = "a".repeat(64);
    let event = agent_config_event(34_011, &agent_pubkey);

    assert!(
        AgentConfig::from_value(&event).is_none(),
        "historical kind:34011 must not be treated as current agent config"
    );
}

#[test]
fn roster_projection_uses_kind0_config_not_historical_34011() {
    let agent_pubkey = "a".repeat(64);
    let project = project_with_agents(vec![agent_pubkey.clone()]);

    let historical_34011 = agent_config_event(34_011, &agent_pubkey);
    let mut configs = HashMap::new();
    if let Some(config) = AgentConfig::from_value(&historical_34011) {
        configs.insert(config.pubkey.clone(), config);
    }

    let roster = build_project_roster(
        &project,
        &HashMap::new(),
        &configs,
        |pubkey| format!("{}...", &pubkey[..8]),
        |_| false,
    );
    assert_eq!(roster.len(), 1);
    assert!(roster[0].model.is_none());
    assert!(roster[0].tools.is_empty());
    assert!(roster[0].skills.is_empty());
    assert!(roster[0].mcp_servers.is_empty());

    let kind0 = AgentConfig::from_value(&agent_config_event(0, &agent_pubkey))
        .expect("kind:0 should parse as current agent config");
    configs.insert(kind0.pubkey.clone(), kind0);

    let roster = build_project_roster(
        &project,
        &HashMap::new(),
        &configs,
        |pubkey| format!("{}...", &pubkey[..8]),
        |_| false,
    );
    assert_eq!(roster.len(), 1);
    assert_eq!(roster[0].model.as_deref(), Some("opus"));
    assert_eq!(roster[0].tools, vec!["shell"]);
    assert_eq!(roster[0].skills, vec!["audit"]);
    assert_eq!(roster[0].mcp_servers, vec!["github"]);
}
