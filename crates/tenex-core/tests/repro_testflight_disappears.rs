//! Repro: when backend A (only testflight) and backend B (~80 agents) both
//! publish 24011, testflight should still be visible in agent_inventory().
//!
//! Replays the two real events the user pasted and asserts the union.

use tenex_core::store::{AppDataStore, Database};

const BACKEND_A: &str = "09417c934409f4c9465fed4b221a512e6cc7437d05e5612ec4286a29c2b19b91";
const BACKEND_B: &str = "9ee7988c6dd0bfed3c0bcac942a691b19349d162dab8948726f391f4dc7a277b";
const TESTFLIGHT_PK: &str = "a84ca311ee4cfba815bfdfb5ec1b45ca7a729828181aec7f39b141d508ddf0e3";

const EVENT_A: &str = r#"{
  "kind": 24011,
  "id": "6e21b0fb6227482810bfb1fabdc015b4b91e426b9c86a9a63d757cdaf8c80426",
  "pubkey": "09417c934409f4c9465fed4b221a512e6cc7437d05e5612ec4286a29c2b19b91",
  "created_at": 1777899320,
  "tags": [
    ["p", "09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7"],
    ["agent", "a84ca311ee4cfba815bfdfb5ec1b45ca7a729828181aec7f39b141d508ddf0e3", "testflight-deployer"]
  ],
  "content": "",
  "sig": "243066e2f55f0690ffbe2d806e769cc1d11689fa971dfbea52349b44df5c82ae7f20643d19bf986196fb6e9f3dc7e650b293ff1ef8f7dd9ac32699cfb56eb085"
}"#;

const EVENT_B: &str = r#"{
  "kind": 24011,
  "id": "cfc139080d291cb5b23528d7559a955d3eeddf312afbddc2d1539c18c5c84a02",
  "pubkey": "9ee7988c6dd0bfed3c0bcac942a691b19349d162dab8948726f391f4dc7a277b",
  "created_at": 1777899328,
  "tags": [
    ["p", "09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7"],
    ["agent", "5186276e3ace3cd29eeea0a42c0be37e545a43555d814cbcb201bb28c72e8e83", "ABrouter"],
    ["agent", "ec24bc1fb730ea99900e7552b431886eb63c146c3025853a35b5eb271dc22847", "adhd-coach"],
    ["agent", "a67b9e48b9b1d8bf80a9786854e42cb6d25229b42f1bdf7b3987a10ec38621b0", "coder"]
  ],
  "content": "",
  "sig": "b93fdb7949b10a3322a3759333c1deeb9aa7c76022c3bd59f682ec28c6809cfd406decfeceddf30f4c501e137798dae19aee09906cf799bd3e81b4314b79cac1"
}"#;

fn slugs(store: &AppDataStore) -> Vec<String> {
    store
        .agent_inventory()
        .into_iter()
        .map(|item| item.slug)
        .collect()
}

#[test]
fn testflight_survives_other_backend_publishing_its_inventory() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path()).unwrap();
    let mut store = AppDataStore::new(db.ndb.clone());
    store.add_approved_backend(BACKEND_A);
    store.add_approved_backend(BACKEND_B);

    eprintln!("=== STEP 1: only backend A (testflight) publishes 24011 ===");
    store.handle_status_event_json(EVENT_A);
    let after_a = slugs(&store);
    eprintln!("after A: {:?}", after_a);
    assert!(
        after_a.iter().any(|s| s == "testflight-deployer"),
        "testflight should be visible after only A published; got {:?}",
        after_a
    );

    eprintln!("=== STEP 2: backend B (other agents) also publishes 24011 ===");
    store.handle_status_event_json(EVENT_B);
    let after_b = slugs(&store);
    eprintln!("after A+B: {:?}", after_b);

    let has_testflight = after_b.iter().any(|s| s == "testflight-deployer");
    let has_b_agent = after_b.iter().any(|s| s == "ABrouter");

    eprintln!("has_testflight = {}", has_testflight);
    eprintln!("has_b_agent    = {}", has_b_agent);
    eprintln!(
        "inventory backend keys: {:?}",
        store.installed_agents_by_backend.keys().collect::<Vec<_>>()
    );

    assert!(has_b_agent, "backend B's agent should appear");
    assert!(
        has_testflight,
        "BUG REPRODUCED: testflight disappeared after backend B published. inventory={:?}",
        after_b
    );

    let testflight_item = store
        .agent_inventory()
        .into_iter()
        .find(|i| i.pubkey == TESTFLIGHT_PK)
        .expect("testflight should still be findable by pubkey");
    eprintln!("testflight backends: {:?}", testflight_item.backends);
}
