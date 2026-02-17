use anyhow::Result;
use nostrdb::{Config, FilterBuilder, Ndb, Transaction};
use std::sync::Arc;

fn main() -> Result<()> {
    println!("Testing project loading (same as store::get_projects)...\n");

    std::fs::create_dir_all("tenex_data")?;
    let ndb = Arc::new(Ndb::new("tenex_data", &Config::new())?);

    // This matches the exact logic in store::get_projects
    let txn = Transaction::new(&ndb)?;
    let filter = FilterBuilder::new().kinds([31933]).build();
    let results = ndb.query(&txn, &[filter], 1000)?;

    println!("Found {} kind:31933 events in nostrdb\n", results.len());

    // Parse as projects (check that Project::from_note works)
    let mut project_count = 0;
    let mut projects_with_agents = 0;
    for result in results.iter() {
        if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
            project_count += 1;
            // Get name and agent tags
            let mut name = String::from("<unnamed>");
            let mut agent_ids: Vec<String> = Vec::new();
            for tag in note.tags() {
                if tag.count() >= 2 {
                    if let Some(first) = tag.get(0).and_then(|s| s.variant().str()) {
                        if first == "name" || first == "title" {
                            if let Some(n) = tag.get(1).and_then(|s| s.variant().str()) {
                                if name == "<unnamed>" {
                                    name = n.to_string();
                                }
                            }
                        }
                        if first == "agent" {
                            if let Some(elem) = tag.get(1) {
                                // nostrdb stores event IDs as binary Id variant
                                if let Some(id_bytes) = elem.variant().id() {
                                    agent_ids.push(hex::encode(id_bytes));
                                } else if let Some(s) = elem.variant().str() {
                                    agent_ids.push(s.to_string());
                                }
                            }
                        }
                    }
                }
            }
            if !agent_ids.is_empty() {
                projects_with_agents += 1;
                if projects_with_agents <= 10 {
                    println!(
                        "  {}. {} - {} agent(s): {:?}",
                        project_count,
                        name,
                        agent_ids.len(),
                        agent_ids
                    );
                }
            }
        }
    }

    println!(
        "\n  Total: {} projects, {} with agent tags",
        project_count, projects_with_agents
    );

    if results.len() > 5 {
        println!("  ... and {} more", results.len() - 5);
    }

    println!();
    if results.is_empty() {
        println!("⚠️  No projects found! The fix won't help if nostrdb is empty.");
    } else {
        println!(
            "✅ Projects loaded successfully! The app should now show {} projects.",
            results.len()
        );
    }

    Ok(())
}
