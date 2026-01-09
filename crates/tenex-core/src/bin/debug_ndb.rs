use anyhow::Result;
use nostrdb::{Config, FilterBuilder, Ndb};

fn main() -> Result<()> {
    println!("Opening nostrdb...");
    let ndb = Ndb::new("tenex_data", &Config::new())?;

    println!("Querying for kind 31933 (projects)...");

    let filter = FilterBuilder::new()
        .kinds([31933])
        .build();

    let txn = nostrdb::Transaction::new(&ndb)?;
    let results = ndb.query(&txn, &[filter], 100)?;

    println!("Found {} projects in nostrdb:", results.len());

    for result in results.iter().take(10) {
        let note = ndb.get_note_by_key(&txn, result.note_key)?;
        let id_bytes = note.id();
        let id_hex: String = id_bytes.iter().map(|b| format!("{:02x}", b)).collect();
        println!("  id: {}", &id_hex[..16]);
        println!("  kind: {}", note.kind());
        println!("  created_at: {}", note.created_at());
        println!("  content: {}", &note.content()[..50.min(note.content().len())]);
        println!();
    }

    if results.len() > 10 {
        println!("  ... and {} more", results.len() - 10);
    }

    Ok(())
}
