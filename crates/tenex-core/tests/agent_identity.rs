//! Integration tests for the agent identity / config decisions captured in
//! `docs/agent-identity-config-implementation-decisions.md`.
//!
//! These tests intentionally exercise only the public `tenex_core` API: they
//! ingest real Nostr events into nostrdb, call the same store entry points the
//! production code uses, and assert on the public read model (rosters,
//! profile lookups, agent configs, projects-by-agent fan-out).
//!
//! Acceptance criteria covered:
//! 1. Same kind:0 display name across two pubkeys keeps them distinct in the
//!    `AppDataStore` and in pubkey-keyed lookups.
//! 2. A kind:24010 carrying model/tool/skill/MCP-like tags must not set any
//!    per-agent current model/tools/skills/MCP — that is a kind:34011 job.
//! 3. The set of agent pubkeys eligible for kind:34011 subscription is the
//!    deduplicated union of every project's 31933 `p` tags and every approved
//!    24011 inventory entry (regression test on the inputs the worker uses).
//! 4. Receiving a kind:34011 for an agent fans out to every project whose
//!    31933 membership lists that agent — even projects with no current 24010
//!    runtime advertisement.

use nostr_sdk::prelude::*;
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tempfile::tempdir;
use tenex_core::store::events::ingest_events;
use tenex_core::store::{AppDataStore, Database};

/// Local copy of the `#[cfg(test)]`-gated helper in `tenex_core::store::events`.
/// We can't reach it from an integration test (cfg(test) applies to this
/// crate, not the dependency), so this poll-loop is duplicated here.
fn wait_for_event_processing(
    ndb: &nostrdb::Ndb,
    filter: nostrdb::Filter,
    max_wait_ms: u64,
) -> bool {
    let start = Instant::now();
    let timeout = Duration::from_millis(max_wait_ms);
    loop {
        if let Ok(txn) = nostrdb::Transaction::new(ndb) {
            if let Ok(results) = ndb.query(&txn, std::slice::from_ref(&filter), 1) {
                if !results.is_empty() {
                    return true;
                }
            }
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn make_keys() -> Keys {
    Keys::generate()
}

fn ingest_kind0(db: &Database, store: &mut AppDataStore, keys: &Keys, display_name: &str) {
    // NIP-01 kind:0 metadata. We populate `display_name` (the first source of
    // truth in `agent_display::kind0_display_name`).
    let content = serde_json::json!({
        "display_name": display_name,
        "name": display_name,
    })
    .to_string();
    let event = EventBuilder::new(Kind::Metadata, content)
        .sign_with_keys(keys)
        .unwrap();
    ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

    let filter = nostrdb::Filter::new()
        .kinds([0])
        .authors([keys.public_key().as_bytes()])
        .build();
    assert!(
        wait_for_event_processing(&db.ndb, filter, 5_000),
        "kind:0 for {} not processed",
        display_name
    );
    // Brief settle so the profile index has the latest record.
    std::thread::sleep(Duration::from_millis(50));

    let txn = nostrdb::Transaction::new(&db.ndb).unwrap();
    let filter = nostrdb::Filter::new()
        .kinds([0])
        .authors([keys.public_key().as_bytes()])
        .build();
    let results = db.ndb.query(&txn, &[filter], 1).unwrap_or_default();
    if let Some(first) = results.first() {
        if let Ok(note) = db.ndb.get_note_by_key(&txn, first.note_key) {
            store.handle_event(0, &note);
        }
    }
}

fn ingest_project(
    db: &Database,
    store: &mut AppDataStore,
    owner: &Keys,
    d_tag: &str,
    title: &str,
    agent_pubkeys: &[String],
    created_at: u64,
) -> String {
    let mut builder = EventBuilder::new(Kind::Custom(31933), "")
        .tag(Tag::custom(
            TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
            vec![d_tag.to_string()],
        ))
        .tag(Tag::custom(
            TagKind::Custom(std::borrow::Cow::Borrowed("title")),
            vec![title.to_string()],
        ))
        .custom_created_at(Timestamp::from(created_at));

    for pk in agent_pubkeys {
        builder = builder.tag(Tag::custom(
            TagKind::Custom(std::borrow::Cow::Borrowed("p")),
            vec![pk.clone()],
        ));
    }

    let event = builder.sign_with_keys(owner).unwrap();
    let a_tag = format!("31933:{}:{}", owner.public_key().to_hex(), d_tag);

    ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();
    let filter = nostrdb::Filter::new().kinds([31933]).build();
    wait_for_event_processing(&db.ndb, filter.clone(), 5_000);
    std::thread::sleep(Duration::from_millis(20));

    let txn = nostrdb::Transaction::new(&db.ndb).unwrap();
    let notes: Vec<_> = db
        .ndb
        .query(&txn, &[filter], 100)
        .unwrap()
        .into_iter()
        .filter_map(|r| db.ndb.get_note_by_key(&txn, r.note_key).ok())
        .collect();
    for note in &notes {
        if note.created_at() == created_at {
            store.handle_event(31933, note);
        }
    }

    a_tag
}

fn make_24011_inventory_json(backend_pubkey: &str, agents: &[(&str, &str)], created_at: u64) -> String {
    let mut tags = String::new();
    for (i, (pk, slug)) in agents.iter().enumerate() {
        if i > 0 {
            tags.push(',');
        }
        tags.push_str(&format!(r#"["agent","{}","{}"]"#, pk, slug));
    }
    format!(
        r#"{{"kind":24011,"id":"inv-{}","pubkey":"{}","created_at":{},"tags":[{}]}}"#,
        backend_pubkey, backend_pubkey, created_at, tags
    )
}

fn make_34011_event_json(
    agent_pubkey: &str,
    slug: &str,
    backend_pubkey: &str,
    created_at: u64,
) -> String {
    format!(
        r#"{{
            "kind": 34011,
            "id": "cfg-{}",
            "pubkey": "{}",
            "created_at": {},
            "tags": [
                ["d", "{}"],
                ["p", "{}"],
                ["model", "opus", "active"],
                ["model", "sonnet"],
                ["tool", "shell", "active"],
                ["skill", "rag", "active"],
                ["mcp", "github", "active"]
            ]
        }}"#,
        agent_pubkey, agent_pubkey, created_at, slug, backend_pubkey
    )
}

// ---------------------------------------------------------------------------
// Acceptance #1: Same kind:0 display name across two pubkeys remains distinct
// ---------------------------------------------------------------------------

#[test]
fn duplicate_kind0_display_names_keep_pubkeys_distinct() {
    let dir = tempdir().unwrap();
    let db = Database::new(dir.path()).unwrap();
    let mut store = AppDataStore::new(db.ndb.clone());

    let owner = make_keys();
    let agent_a = make_keys();
    let agent_b = make_keys();
    let agent_a_pk = agent_a.public_key().to_hex();
    let agent_b_pk = agent_b.public_key().to_hex();
    assert_ne!(agent_a_pk, agent_b_pk, "test invariant: distinct keys");

    // Both agents publish kind:0 with the *same* display_name.
    ingest_kind0(&db, &mut store, &agent_a, "Same Name");
    ingest_kind0(&db, &mut store, &agent_b, "Same Name");

    // Profile lookups by pubkey return the same name for both — that is fine,
    // the doc says the *display name* may collide. What must not collide is
    // identity.
    assert_eq!(
        store.get_profile_name_if_known(&agent_a_pk).as_deref(),
        Some("Same Name"),
    );
    assert_eq!(
        store.get_profile_name_if_known(&agent_b_pk).as_deref(),
        Some("Same Name"),
    );

    // Project membership uses pubkeys directly. A roster built for a project
    // containing both pubkeys must have two distinct entries keyed by pubkey.
    let a_tag = ingest_project(
        &db,
        &mut store,
        &owner,
        "duplicate-name-project",
        "Duplicate Name Project",
        &[agent_a_pk.clone(), agent_b_pk.clone()],
        1_700_000_000,
    );

    let roster = store
        .get_project_roster(&a_tag)
        .expect("project roster should exist for known project");
    assert_eq!(roster.len(), 2, "two distinct agents must remain distinct");

    let pubkeys_in_roster: HashSet<&str> =
        roster.iter().map(|agent| agent.pubkey.as_str()).collect();
    assert!(pubkeys_in_roster.contains(agent_a_pk.as_str()));
    assert!(pubkeys_in_roster.contains(agent_b_pk.as_str()));

    // Order is preserved from the 31933 `p` tags.
    assert_eq!(roster[0].pubkey, agent_a_pk);
    assert_eq!(roster[1].pubkey, agent_b_pk);
    assert!(roster[0].is_pm, "first roster slot is PM");
    assert!(!roster[1].is_pm);
}

// ---------------------------------------------------------------------------
// Acceptance #2: kind:24010 must NOT populate per-agent current config
// ---------------------------------------------------------------------------

#[test]
fn kind_24010_does_not_populate_per_agent_current_config() {
    let dir = tempdir().unwrap();
    let db = Database::new(dir.path()).unwrap();
    let mut store = AppDataStore::new(db.ndb.clone());

    let owner = make_keys();
    let agent = make_keys();
    let agent_pk = agent.public_key().to_hex();
    let backend_pk = "b".repeat(64);

    let a_tag = ingest_project(
        &db,
        &mut store,
        &owner,
        "no-24010-config",
        "Project",
        &[agent_pk.clone()],
        1_700_000_000,
    );
    store.add_approved_backend(&backend_pk);

    // 24010 with model/tool/skill/MCP-like tags carrying agent-style 3rd
    // elements. Per the doc, none of these may become per-agent current
    // config.
    let json = format!(
        r#"{{
            "kind": 24010,
            "id": "stat",
            "pubkey": "{backend}",
            "created_at": 1700000010,
            "tags": [
                ["a", "{a_tag}"],
                ["agent", "{agent}", "slug"],
                ["model", "claude-opus", "{agent}"],
                ["tool", "shell", "{agent}"],
                ["tool", "rag_query"],
                ["skill", "code-review", "{agent}"],
                ["mcp", "github", "{agent}"],
                ["branch", "main"]
            ]
        }}"#,
        backend = backend_pk,
        a_tag = a_tag,
        agent = agent_pk
    );
    store.handle_status_event_json(&json);

    // 24010 must not seed an agent config.
    assert!(
        store.get_agent_config(&agent_pk).is_none(),
        "24010 must not produce a per-agent kind:34011 config"
    );

    // The roster for that agent has none of the 24010 model/tool/skill/MCP
    // values bound to the agent.
    let roster = store
        .get_project_roster(&a_tag)
        .expect("expected roster for known project");
    let entry = roster
        .iter()
        .find(|a| a.pubkey == agent_pk)
        .expect("agent should be in roster");
    assert!(
        entry.model.is_none(),
        "current model must come from 34011, not 24010"
    );
    assert!(
        entry.tools.is_empty(),
        "current tools must come from 34011, not 24010"
    );
    assert!(
        entry.skills.is_empty(),
        "current skills must come from 34011, not 24010"
    );
    assert!(
        entry.mcp_servers.is_empty(),
        "current MCP servers must come from 34011, not 24010"
    );

    // The 24010 still gets stored as the project-level status, with the
    // project-level option lists populated. (Sanity check that we did parse
    // the event, we just didn't let it leak into per-agent config.)
    let status = store
        .get_project_status(&a_tag)
        .expect("project status should be aggregated");
    assert!(status.models().contains(&"claude-opus"));
    assert!(status.all_tools().contains(&"rag_query"));
    assert!(
        status.agents.is_empty(),
        "24010 status must not carry per-agent records"
    );
}

// ---------------------------------------------------------------------------
// Acceptance #3: roster + approved inventory yields the deduped 34011 sub set
// ---------------------------------------------------------------------------

/// The worker subscribes to kind:34011 for the union of agent pubkeys it sees in
/// (a) project 31933 `p` tags and (b) approved 24011 inventories.
/// Subscription dedup is a `HashSet`-driven helper inside the worker. We can
/// verify the *inputs* the worker derives from the public store API: every
/// pubkey in 31933 membership and every pubkey in the approved 24011 inventory
/// is enumerable, the same pubkey appearing in both sources only shows up
/// once, and pubkeys from non-approved backends are excluded.
///
/// NOTE: The worker's subscription cache (`subscribed_agent_configs`) is a
/// private `Arc<RwLock<HashSet<...>>>` inside `NostrWorker`. A direct
/// assertion on its contents would require either exposing that set or a test
/// hook that intercepts the subscribe call. See the report for which hook
/// would make this test more direct.
#[test]
fn agent_pubkey_set_for_34011_subscriptions_is_deduplicated_union_of_31933_and_24011() {
    let dir = tempdir().unwrap();
    let db = Database::new(dir.path()).unwrap();
    let mut store = AppDataStore::new(db.ndb.clone());

    let owner = make_keys();
    // Three agent pubkeys that overlap between sources:
    //   - shared_pk: in both 31933 membership AND 24011 inventory.
    //   - only_31933_pk: in 31933 membership only.
    //   - only_24011_pk: in 24011 inventory only.
    //   - untrusted_pk: only in a *non-approved* backend's 24011 (must NOT
    //     contribute to the subscription set).
    let shared_pk = "1".repeat(64);
    let only_31933_pk = "2".repeat(64);
    let only_24011_pk = "3".repeat(64);
    let untrusted_pk = "4".repeat(64);
    let trusted_backend = "a".repeat(64);
    let untrusted_backend = "f".repeat(64);

    // 31933 membership.
    let a_tag = ingest_project(
        &db,
        &mut store,
        &owner,
        "p3",
        "P3",
        &[shared_pk.clone(), only_31933_pk.clone()],
        1_700_000_000,
    );

    // Approved 24011 inventory contains shared_pk + only_24011_pk.
    store.add_approved_backend(&trusted_backend);
    let trusted_json = make_24011_inventory_json(
        &trusted_backend,
        &[
            (shared_pk.as_str(), "shared-slug"),
            (only_24011_pk.as_str(), "only-24011-slug"),
        ],
        1_700_000_010,
    );
    store.handle_status_event_json(&trusted_json);

    // Untrusted 24011 with untrusted_pk — must NOT enter the eligible set.
    let untrusted_json = make_24011_inventory_json(
        &untrusted_backend,
        &[(untrusted_pk.as_str(), "untrusted-slug")],
        1_700_000_020,
    );
    store.handle_status_event_json(&untrusted_json);

    // Build the eligible set the way the worker does it: union of every
    // project's `agent_pubkeys` plus every entry in the public
    // `agent_inventory()` (which is already filtered to approved backends).
    let mut eligible: HashSet<String> = HashSet::new();
    for project in store.get_projects() {
        for pk in &project.agent_pubkeys {
            eligible.insert(pk.clone());
        }
    }
    for item in store.agent_inventory() {
        eligible.insert(item.pubkey);
    }

    assert!(
        eligible.contains(&shared_pk),
        "shared pubkey must be in eligible set"
    );
    assert!(
        eligible.contains(&only_31933_pk),
        "31933-only pubkey must be in eligible set"
    );
    assert!(
        eligible.contains(&only_24011_pk),
        "approved-inventory-only pubkey must be in eligible set"
    );
    assert!(
        !eligible.contains(&untrusted_pk),
        "untrusted-backend pubkey must NOT be in eligible set"
    );
    assert_eq!(
        eligible.len(),
        3,
        "set must dedupe shared pubkey across 31933 + 24011 sources"
    );

    // Sanity: the eligible-by-inventory pubkey is the same one enumerated in
    // the public agent_inventory() — guarding against future drift in how
    // approved-vs-blocked filtering is enforced.
    let inventory_pubkeys: HashSet<String> = store
        .agent_inventory()
        .into_iter()
        .map(|item| item.pubkey)
        .collect();
    assert!(inventory_pubkeys.contains(&shared_pk));
    assert!(inventory_pubkeys.contains(&only_24011_pk));
    assert!(!inventory_pubkeys.contains(&untrusted_pk));

    // Sanity: rosters for the project still resolve via pubkey, regardless of
    // 24010 having never arrived.
    let roster = store.get_project_roster(&a_tag).expect("known project");
    assert_eq!(roster.len(), 2);
    assert_eq!(roster[0].pubkey, shared_pk);
    assert_eq!(roster[1].pubkey, only_31933_pk);
    // shared_pk is in approved inventory so it should appear online.
    assert!(roster[0].is_online);
    assert_eq!(roster[0].backend_pubkey, trusted_backend);
    // only_31933_pk has no approved inventory entry, so offline.
    assert!(!roster[1].is_online);
}

// ---------------------------------------------------------------------------
// Acceptance #4: 34011 refreshes every project where the agent is in 31933
// ---------------------------------------------------------------------------

#[test]
fn kind_34011_refreshes_every_project_where_agent_is_in_31933_membership() {
    let dir = tempdir().unwrap();
    let db = Database::new(dir.path()).unwrap();
    let mut store = AppDataStore::new(db.ndb.clone());

    let owner = make_keys();
    let agent = make_keys();
    let agent_pk = agent.public_key().to_hex();
    let other_agent_pk = "9".repeat(64);
    let backend_pk = "b".repeat(64);

    // Three projects:
    //   running_a / running_b: both have `agent` in 31933 membership.
    //   no_agent: has only `other_agent_pk` — must NOT be in the refresh set.
    let a_tag_running_a = ingest_project(
        &db,
        &mut store,
        &owner,
        "running-a",
        "Running A",
        &[agent_pk.clone()],
        1_700_000_000,
    );
    let a_tag_running_b = ingest_project(
        &db,
        &mut store,
        &owner,
        "running-b",
        "Running B",
        &[agent_pk.clone(), other_agent_pk.clone()],
        1_700_000_001,
    );
    let a_tag_no_agent = ingest_project(
        &db,
        &mut store,
        &owner,
        "no-agent",
        "No Agent",
        &[other_agent_pk.clone()],
        1_700_000_002,
    );

    // Crucial: NO 24010 has been published for any of these projects. The doc
    // explicitly says refresh must not be gated on "currently running" 24010
    // status.

    // 34011 arrives for `agent`.
    let cfg_json = make_34011_event_json(&agent_pk, "planner", &backend_pk, 1_700_000_100);
    store.handle_status_event_json(&cfg_json);

    // The store must now know the config.
    let cfg = store
        .get_agent_config(&agent_pk)
        .expect("34011 must populate agent config");
    assert_eq!(cfg.active_model.as_deref(), Some("opus"));
    assert_eq!(cfg.active_tools, vec!["shell"]);
    assert_eq!(cfg.active_skills, vec!["rag"]);
    assert_eq!(cfg.active_mcps, vec!["github"]);

    // The "refresh affected projects" fan-out uses
    // project_a_tags_for_agent_pubkeys with the just-updated agent pubkey.
    let mut updated_agents = HashSet::new();
    updated_agents.insert(agent_pk.clone());
    let affected = store.project_a_tags_for_agent_pubkeys(&updated_agents);
    let affected_set: HashSet<&str> = affected.iter().map(String::as_str).collect();

    assert!(
        affected_set.contains(a_tag_running_a.as_str()),
        "project running-a must be in refresh set"
    );
    assert!(
        affected_set.contains(a_tag_running_b.as_str()),
        "project running-b must be in refresh set"
    );
    assert!(
        !affected_set.contains(a_tag_no_agent.as_str()),
        "project without the agent in 31933 must NOT be in refresh set"
    );
    assert_eq!(
        affected.len(),
        2,
        "exactly two projects have this agent in 31933 membership"
    );

    // Both affected projects' rosters now reflect the new 34011 config.
    for a_tag in [&a_tag_running_a, &a_tag_running_b] {
        let roster = store
            .get_project_roster(a_tag)
            .expect("known project should have a roster");
        let entry = roster
            .iter()
            .find(|a| a.pubkey == agent_pk)
            .expect("agent must be in this project's roster");
        assert_eq!(
            entry.model.as_deref(),
            Some("opus"),
            "current model must come from the just-arrived 34011"
        );
        assert_eq!(entry.tools, vec!["shell"]);
        assert_eq!(entry.skills, vec!["rag"]);
        assert_eq!(entry.mcp_servers, vec!["github"]);
    }
}
