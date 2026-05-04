use nostrdb::{Ndb, Transaction};

/// Display fallback used only when no kind:0 profile name is available.
pub fn fallback_pubkey_name(pubkey: &str) -> String {
    format!("{}...", &pubkey[..8.min(pubkey.len())])
}

/// Resolve a pubkey's display name from kind:0 profile metadata only.
pub fn kind0_display_name(ndb: &Ndb, pubkey: &str) -> Option<String> {
    let pubkey_bytes = match hex::decode(pubkey) {
        Ok(bytes) if bytes.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
        }
        _ => return None,
    };

    let txn = Transaction::new(ndb).ok()?;
    let profile = ndb.get_profile_by_pubkey(&txn, &pubkey_bytes).ok()?;
    let record = profile.record();
    let profile_data = record.profile()?;

    profile_data
        .display_name()
        .and_then(non_empty_profile_name)
        .or_else(|| profile_data.name().and_then(non_empty_profile_name))
}

/// Resolve the UI display name for an agent pubkey.
///
/// The only semantic name source is kind:0 profile metadata. Status/config
/// slugs are deliberately not used as display fallbacks.
pub fn display_name(ndb: &Ndb, pubkey: &str) -> String {
    kind0_display_name(ndb, pubkey).unwrap_or_else(|| fallback_pubkey_name(pubkey))
}

fn non_empty_profile_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_pubkey_name_truncates_without_panicking_on_short_keys() {
        assert_eq!(fallback_pubkey_name("abcdef"), "abcdef...");
        assert_eq!(fallback_pubkey_name("abcdefghijkl"), "abcdefgh...");
    }
}
