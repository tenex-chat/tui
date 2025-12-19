use anyhow::Result;
use nostr_sdk::nips::nip49::EncryptedSecretKey;
use nostr_sdk::prelude::*;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

pub fn login_with_nsec(nsec: &str, password: Option<&str>, conn: &Arc<Mutex<Connection>>) -> Result<Keys> {
    let secret_key = SecretKey::parse(nsec)?;
    let keys = Keys::new(secret_key);

    // Store encrypted if password provided
    if let Some(pwd) = password {
        if !pwd.is_empty() {
            let encrypted = keys.secret_key().encrypt(pwd)?;
            let encrypted_bech32 = encrypted.to_bech32()?;
            store_credentials(conn, &encrypted_bech32)?;
        }
    } else {
        // Store unencrypted (as nsec)
        store_credentials(conn, nsec)?;
    }

    Ok(keys)
}

pub fn load_stored_keys(password: &str, conn: &Arc<Mutex<Connection>>) -> Result<Keys> {
    let ncryptsec = get_stored_credentials(conn)?;

    // Try to decrypt
    let secret_key = if ncryptsec.starts_with("ncryptsec") {
        let encrypted = EncryptedSecretKey::from_bech32(&ncryptsec)?;
        encrypted.to_secret_key(password)?
    } else {
        SecretKey::parse(&ncryptsec)?
    };

    Ok(Keys::new(secret_key))
}

pub fn has_stored_credentials(conn: &Arc<Mutex<Connection>>) -> bool {
    get_stored_credentials(conn).is_ok()
}

fn store_credentials(conn: &Arc<Mutex<Connection>>, ncryptsec: &str) -> Result<()> {
    let conn = conn.lock().unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO credentials (id, ncryptsec) VALUES (1, ?1)",
        [ncryptsec],
    )?;
    Ok(())
}

fn get_stored_credentials(conn: &Arc<Mutex<Connection>>) -> Result<String> {
    let conn = conn.lock().unwrap();
    let result: String = conn.query_row(
        "SELECT ncryptsec FROM credentials WHERE id = 1",
        [],
        |row| row.get(0),
    )?;
    Ok(result)
}

pub fn get_current_pubkey(keys: &Keys) -> String {
    keys.public_key().to_hex()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Database;

    fn clear_credentials(conn: &Arc<Mutex<Connection>>) -> Result<()> {
        let conn = conn.lock().unwrap();
        conn.execute("DELETE FROM credentials WHERE id = 1", [])?;
        Ok(())
    }

    fn is_logged_in(keys: Option<&Keys>) -> bool {
        keys.is_some()
    }

    #[test]
    fn test_login_and_store() {
        let db = Database::in_memory().unwrap();
        let conn = db.connection();

        // Generate a test nsec
        let keys = Keys::generate();
        let nsec = keys.secret_key().to_bech32().unwrap();

        let result = login_with_nsec(&nsec, Some("password123"), &conn);
        assert!(result.is_ok());

        // Should be able to load back
        let loaded = load_stored_keys("password123", &conn);
        assert!(loaded.is_ok());
        assert_eq!(loaded.unwrap().public_key(), keys.public_key());
    }

    #[test]
    fn test_has_stored_credentials() {
        let db = Database::in_memory().unwrap();
        let conn = db.connection();

        assert!(!has_stored_credentials(&conn));

        let keys = Keys::generate();
        let nsec = keys.secret_key().to_bech32().unwrap();
        login_with_nsec(&nsec, None, &conn).unwrap();

        assert!(has_stored_credentials(&conn));
    }

    #[test]
    fn test_clear_credentials() {
        let db = Database::in_memory().unwrap();
        let conn = db.connection();

        let keys = Keys::generate();
        let nsec = keys.secret_key().to_bech32().unwrap();
        login_with_nsec(&nsec, None, &conn).unwrap();

        assert!(has_stored_credentials(&conn));

        clear_credentials(&conn).unwrap();

        assert!(!has_stored_credentials(&conn));
    }

    #[test]
    fn test_get_current_pubkey() {
        let keys = Keys::generate();
        let pubkey = get_current_pubkey(&keys);
        assert_eq!(pubkey, keys.public_key().to_hex());
    }

    #[test]
    fn test_is_logged_in() {
        let keys = Keys::generate();
        assert!(is_logged_in(Some(&keys)));
        assert!(!is_logged_in(None));
    }
}
