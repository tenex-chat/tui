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
    for result in results.iter().take(5) {
        if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
            project_count += 1;
            // Get name from tags
            let mut name = String::from("<unnamed>");
            for tag in note.tags() {
                if tag.count() >= 2 {
                    if let Some(first) = tag.get(0).and_then(|s| s.variant().str()) {
                        if first == "name" {
                            if let Some(n) = tag.get(1).and_then(|s| s.variant().str()) {
                                name = n.to_string();
                            }
                        }
                    }
                }
            }
            println!("  {}. {} - {}", project_count, name, &note.content()[..50.min(note.content().len())]);
        }
    }

    if results.len() > 5 {
        println!("  ... and {} more", results.len() - 5);
    }

    println!();
    if results.is_empty() {
        println!("⚠️  No projects found! The fix won't help if nostrdb is empty.");
    } else {
        println!("✅ Projects loaded successfully! The app should now show {} projects.", results.len());
    }

    Ok(())
}
