use anyhow::Result;
use nostrdb::{Config, FilterBuilder, Ndb, Transaction};

fn main() -> Result<()> {
    println!("Opening nostrdb...");
    let ndb = Ndb::new("tenex_data", &Config::new())?;

    println!("Querying for kind 4201 (nudges)...");
    let filter = FilterBuilder::new()
        .kinds([4201])
        .build();

    let txn = Transaction::new(&ndb)?;
    let results = ndb.query(&txn, &[filter], 100)?;

    println!("Found {} nudges in nostrdb:", results.len());

    for result in results.iter().take(10) {
        let note = ndb.get_note_by_key(&txn, result.note_key)?;
        let id_bytes = note.id();
        let id_hex: String = id_bytes.iter().map(|b| format!("{:02x}", b)).collect();
        println!("  id: {}", &id_hex[..16]);
        println!("  kind: {}", note.kind());
        println!("  created_at: {}", note.created_at());
        let content = note.content();
        println!("  content: {}", &content[..100.min(content.len())]);
        println!();
    }

    Ok(())
}
