use anyhow::Result;
use nostrdb::{Config, FilterBuilder, Ndb, Transaction};

fn main() -> Result<()> {
    println!("Testing message loading from nostrdb...\n");

    std::fs::create_dir_all("tenex_data")?;
    let ndb = Ndb::new("tenex_data", &Config::new())?;
    let txn = Transaction::new(&ndb)?;

    // Query kind:1 events (threads + messages)
    let kind1_filter = FilterBuilder::new().kinds([1]).build();
    let kind1_results = ndb.query(&txn, &[kind1_filter], 200)?;
    println!("Found {} kind:1 events\n", kind1_results.len());

    if kind1_results.is_empty() {
        println!("No events found - can't test message loading");
        return Ok(());
    }

    // Find thread candidates: kind:1 with a-tag and no e-tag
    let mut thread_note = None;
    let mut thread_count = 0;
    for result in kind1_results.iter() {
        if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
            let mut has_a_tag = false;
            let mut has_e_tag = false;
            for tag in note.tags() {
                let tag_name = tag.get(0).and_then(|t| t.variant().str());
                if tag_name == Some("a") {
                    has_a_tag = true;
                }
                if matches!(tag_name, Some("e")) {
                    has_e_tag = true;
                }
            }
            if has_a_tag && !has_e_tag {
                thread_count += 1;
                if thread_note.is_none() {
                    thread_note = Some(note);
                }
            }
        }
    }

    println!(
        "Found {} thread candidates (kind:1, a-tag, no e-tag)\n",
        thread_count
    );

    let thread_note = match thread_note {
        Some(note) => note,
        None => {
            println!("No threads found - can't test message loading");
            return Ok(());
        }
    };
    let thread_id = hex::encode(thread_note.id());
    println!("Testing with thread: {}", &thread_id[..16]);
    println!("Thread content: {}\n", &thread_note.content()[..100.min(thread_note.content().len())]);

    // Now find messages: kind:1 with e-tag
    let mut all_messages = Vec::new();
    for result in kind1_results.iter() {
        if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
            let mut has_e_tag = false;
            for tag in note.tags() {
                let tag_name = tag.get(0).and_then(|t| t.variant().str());
                if matches!(tag_name, Some("e")) {
                    has_e_tag = true;
                    break;
                }
            }
            if has_e_tag {
                all_messages.push(note);
            }
        }
    }
    println!("Checking first 100 of kind:1 messages with e-tag\n");

    // Show what e-tags look like
    let mut e_tag_examples = 0;
    for note in all_messages.iter().take(100) {
        for tag in note.tags() {
            if tag.count() >= 2 {
                if let Some(first) = tag.get(0) {
                    let first_str = first.str().unwrap_or("<not-str>");
                        if first_str == "e" {
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
                                    println!("e-tag example {}: tag[0]='{}' tag[1]={}", e_tag_examples, first_str, val);
                            }
                        }
                    }
                }
            }
        }
    }

    println!("\nTotal e-tags found: {}", e_tag_examples);

    // Collect all unique thread IDs from messages
    println!("\n--- Unique thread references in messages ---");
    let mut thread_refs: std::collections::HashSet<String> = std::collections::HashSet::new();
    for note in all_messages.iter().take(100) {
        for tag in note.tags() {
            if tag.count() >= 2 {
                    let is_e_tag = matches!(tag.get(0).and_then(|s| s.str()), Some("e"));
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
