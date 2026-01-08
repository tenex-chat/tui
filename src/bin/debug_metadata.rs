use anyhow::Result;
use nostrdb::{Filter, Ndb, Transaction};

fn main() -> Result<()> {
    println!("Opening nostrdb...");
    let ndb = Ndb::new("tenex_data", &nostrdb::Config::new())?;
    let txn = Transaction::new(&ndb)?;

    // Query for kind:513 metadata events
    let filter = Filter::new().kinds([513]).build();
    let results = ndb.query(&txn, &[filter], 100)?;

    println!("Found {} kind:513 metadata events\n", results.len());

    for (idx, result) in results.iter().enumerate() {
        let note = ndb.get_note_by_key(&txn, result.note_key)?;
        let note_id = hex::encode(note.id());

        println!("=== Metadata Event {} ===", idx + 1);
        println!("ID: {}", note_id);
        println!("Created: {}", note.created_at());
        println!("Tags:");

        for tag in note.tags() {
            let mut tag_str = String::new();
            for i in 0..tag.count() {
                if let Some(t) = tag.get(i) {
                    if let Some(s) = t.variant().str() {
                        tag_str.push_str(&format!("\"{}\" ", s));
                    } else if let Some(id_bytes) = t.variant().id() {
                        tag_str.push_str(&format!("[id:{}] ", hex::encode(id_bytes)));
                    }
                }
            }
            println!("  [{}]", tag_str);
        }
        println!();
    }

    // Also query for kind:1 threads (no e-tag, has a-tag)
    println!("\n=== Checking kind:1 threads ===");
    let thread_filter = Filter::new().kinds([1]).build();
    let thread_results = ndb.query(&txn, &[thread_filter], 20)?;

    println!("Found {} kind:1 events (showing first 20)\n", thread_results.len());

    for (idx, result) in thread_results.iter().take(20).enumerate() {
        let note = ndb.get_note_by_key(&txn, result.note_key)?;
        let note_id = hex::encode(note.id());

        let mut has_e_tag = false;
        let mut has_a_tag = false;
        let mut title = None;

        for tag in note.tags() {
            if let Some(tag_name) = tag.get(0).and_then(|t| t.variant().str()) {
                match tag_name {
                    "e" => has_e_tag = true,
                    "a" => has_a_tag = true,
                    "title" => title = tag.get(1).and_then(|t| t.variant().str()).map(String::from),
                    _ => {}
                }
            }
        }

        if has_a_tag && !has_e_tag {
            println!("Thread {}: {}", idx + 1, &note_id[..16]);
            println!("  Title from tags: {:?}", title);
            println!("  Content preview: {}", &note.content()[..50.min(note.content().len())]);
            println!();
        }
    }

    Ok(())
}
