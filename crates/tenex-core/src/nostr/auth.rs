use anyhow::Result;
use nostr_sdk::nips::nip49::EncryptedSecretKey;
use nostr_sdk::prelude::*;

use crate::models::PreferencesStorage;

pub fn login_with_nsec(
    nsec: &str,
    password: Option<&str>,
    prefs: &mut PreferencesStorage,
) -> Result<Keys> {
    let secret_key = SecretKey::parse(nsec)?;
    let keys = Keys::new(secret_key);

    // Store encrypted if password provided
    if let Some(pwd) = password {
        if !pwd.is_empty() {
            let encrypted = keys.secret_key().encrypt(pwd)?;
            let encrypted_bech32 = encrypted.to_bech32()?;
            prefs.store_credentials(&encrypted_bech32);
        }
    } else {
        // Store unencrypted (as nsec)
        prefs.store_credentials(nsec);
    }

    Ok(keys)
}

pub fn load_stored_keys(password: &str, prefs: &PreferencesStorage) -> Result<Keys> {
    let ncryptsec = prefs
        .get_stored_credentials()
        .ok_or_else(|| anyhow::anyhow!("No stored credentials"))?;

    // Try to decrypt
    let secret_key = if ncryptsec.starts_with("ncryptsec") {
        let encrypted = EncryptedSecretKey::from_bech32(ncryptsec)?;
        encrypted.decrypt(password)?
    } else {
        SecretKey::parse(ncryptsec)?
    };

    Ok(Keys::new(secret_key))
}

pub fn has_stored_credentials(prefs: &PreferencesStorage) -> bool {
    prefs.has_stored_credentials()
}

/// Check if stored credentials are encrypted (require a password to unlock)
pub fn credentials_need_password(prefs: &PreferencesStorage) -> bool {
    prefs.credentials_need_password()
}

/// Load stored keys that don't require a password (unencrypted nsec)
pub fn load_unencrypted_keys(prefs: &PreferencesStorage) -> Result<Keys> {
    let nsec = prefs
        .get_stored_credentials()
        .ok_or_else(|| anyhow::anyhow!("No stored credentials"))?;

    if nsec.starts_with("ncryptsec") {
        anyhow::bail!("Credentials are encrypted, password required");
    }
    let secret_key = SecretKey::parse(nsec)?;
    Ok(Keys::new(secret_key))
}

pub fn get_current_pubkey(keys: &Keys) -> String {
    keys.public_key().to_hex()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_login_and_store() {
        let dir = tempdir().unwrap();
        let mut prefs = PreferencesStorage::new(dir.path().to_str().unwrap());

        // Generate a test nsec
        let keys = Keys::generate();
        let nsec = keys.secret_key().to_bech32().unwrap();

        let result = login_with_nsec(&nsec, Some("password123"), &mut prefs);
        assert!(result.is_ok());

        // Should be able to load back
        let loaded = load_stored_keys("password123", &prefs);
        assert!(loaded.is_ok());
        assert_eq!(loaded.unwrap().public_key(), keys.public_key());
    }

    #[test]
    fn test_has_stored_credentials() {
        let dir = tempdir().unwrap();
        let mut prefs = PreferencesStorage::new(dir.path().to_str().unwrap());

        assert!(!has_stored_credentials(&prefs));

        let keys = Keys::generate();
        let nsec = keys.secret_key().to_bech32().unwrap();
        login_with_nsec(&nsec, None, &mut prefs).unwrap();

        assert!(has_stored_credentials(&prefs));
    }

    #[test]
    fn test_clear_credentials() {
        let dir = tempdir().unwrap();
        let mut prefs = PreferencesStorage::new(dir.path().to_str().unwrap());

        let keys = Keys::generate();
        let nsec = keys.secret_key().to_bech32().unwrap();
        login_with_nsec(&nsec, None, &mut prefs).unwrap();

        assert!(has_stored_credentials(&prefs));

        prefs.clear_credentials();

        assert!(!has_stored_credentials(&prefs));
    }

    #[test]
    fn test_get_current_pubkey() {
        let keys = Keys::generate();
        let pubkey = get_current_pubkey(&keys);
        assert_eq!(pubkey, keys.public_key().to_hex());
    }

    #[test]
    fn test_is_logged_in() {
        fn is_logged_in(keys: Option<&Keys>) -> bool {
            keys.is_some()
        }
        let keys = Keys::generate();
        assert!(is_logged_in(Some(&keys)));
        assert!(!is_logged_in(None));
    }
}
