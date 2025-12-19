use anyhow::Result;
use nostrdb::{Config, FilterBuilder, Ndb, Transaction};

fn main() -> Result<()> {
    println!("Testing message loading from nostrdb...\n");

    std::fs::create_dir_all("tenex_data")?;
    let ndb = Ndb::new("tenex_data", &Config::new())?;
    let txn = Transaction::new(&ndb)?;

    // First, find threads (kind 11)
    let thread_filter = FilterBuilder::new().kinds([11]).build();
    let threads = ndb.query(&txn, &[thread_filter], 100)?;
    println!("Found {} threads (kind 11)\n", threads.len());

    if threads.is_empty() {
        println!("No threads found - can't test message loading");
        return Ok(());
    }

    // Pick the first thread
    let thread_note = ndb.get_note_by_key(&txn, threads[0].note_key)?;
    let thread_id = hex::encode(thread_note.id());
    println!("Testing with thread: {}", &thread_id[..16]);
    println!("Thread content: {}\n", &thread_note.content()[..100.min(thread_note.content().len())]);

    // Now find messages (kind 1111)
    let msg_filter = FilterBuilder::new().kinds([1111]).build();
    let all_messages = ndb.query(&txn, &[msg_filter], 100)?;
    println!("Checking first 100 of kind:1111 messages\n");

    // Show what E tags look like
    let mut e_tag_examples = 0;
    for result in all_messages.iter() {
        if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
            for tag in note.tags() {
                if tag.count() >= 2 {
                    if let Some(first) = tag.get(0) {
                        let first_str = first.str().unwrap_or("<not-str>");
                        if first_str == "E" || first_str == "e" {
                            e_tag_examples += 1;
                            if e_tag_examples <= 5 {
                                if let Some(second) = tag.get(1) {
                                    let val = if let Some(s) = second.str() {
                                        format!("str: {}", s)
                                    } else if let Some(id) = second.variant().id() {
                                        format!("id: {}", hex::encode(id))
                                    } else {
                                        "unknown".to_string()
                                    };
                                    println!("E-tag example {}: tag[0]='{}' tag[1]={}", e_tag_examples, first_str, val);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    println!("\nTotal E/e tags found: {}", e_tag_examples);

    // Collect all unique thread IDs from messages
    println!("\n--- Unique thread references in messages ---");
    let mut thread_refs: std::collections::HashSet<String> = std::collections::HashSet::new();
    for result in all_messages.iter() {
        if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
            for tag in note.tags() {
                if tag.count() >= 2 {
                    let is_e_tag = tag.get(0).and_then(|s| s.str()) == Some("E");
                    if is_e_tag {
                        if let Some(tag_elem) = tag.get(1) {
                            let tag_value = if let Some(s) = tag_elem.str() {
                                s.to_string()
                            } else if let Some(id) = tag_elem.variant().id() {
                                hex::encode(id)
                            } else {
                                continue;
                            };
                            thread_refs.insert(tag_value);
                        }
                    }
                }
            }
        }
    }

    println!("Unique thread refs: {}", thread_refs.len());
    for (i, ref_id) in thread_refs.iter().take(5).enumerate() {
        println!("  {}: {}...", i + 1, &ref_id[..16.min(ref_id.len())]);
    }

    // Check if any thread ref matches our thread
    if thread_refs.contains(&thread_id) {
        println!("\n✅ Thread {} is referenced by messages", &thread_id[..16]);
    } else {
        println!("\n⚠️  Thread {} is NOT referenced by any of the first 100 messages", &thread_id[..16]);
    }

    Ok(())
}
