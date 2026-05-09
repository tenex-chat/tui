//! Standalone probe: open the user's nostrdb cache and query whether
//! the testflight-deployer kind:0 profile is queryable.

use std::path::PathBuf;
use tenex_core::agent_display::{fallback_pubkey_name, kind0_display_name};
use tenex_core::store::Database;

const AGENT_PK: &str = "a84ca311ee4cfba815bfdfb5ec1b45ca7a729828181aec7f39b141d508ddf0e3";

fn main() {
    let candidates = [
        "/Users/pablofernandez/.tenex/cli",
        "/Users/pablofernandez/Library/Application Support/tenex/nostrdb",
    ];
    for path in candidates {
        let p = PathBuf::from(path);
        if !p.join("data.mdb").exists() {
            println!("MISS: {} (no data.mdb)", path);
            continue;
        }
        println!("=== {} ===", path);
        let db = match Database::new(&p) {
            Ok(d) => d,
            Err(e) => {
                println!("  open error: {}", e);
                continue;
            }
        };
        let resolved = kind0_display_name(&db.ndb, AGENT_PK);
        let fb = fallback_pubkey_name(AGENT_PK);
        println!("  kind0_display_name => {:?}", resolved);
        println!("  fallback would be  => {:?}", fb);

        // Also probe a known-working agent for control: ABrouter from backend B.
        let control = "5186276e3ace3cd29eeea0a42c0be37e545a43555d814cbcb201bb28c72e8e83";
        let ctrl = kind0_display_name(&db.ndb, control);
        println!("  control 5186276e (ABrouter) => {:?}", ctrl);
    }
}
