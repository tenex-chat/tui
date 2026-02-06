/// Secure storage for sensitive data like API keys
///
/// Uses OS-backed secure storage:
/// - macOS/iOS: Keychain
/// - Linux: Secret Service API (gnome-keyring, KWallet, etc.)
/// - Windows: Credential Manager
use keyring::Entry;
use std::fmt;

const SERVICE_NAME: &str = "com.tenex.tui-client";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecureKey {
    ElevenLabsApiKey,
    OpenRouterApiKey,
}

impl SecureKey {
    fn key_name(&self) -> &'static str {
        match self {
            SecureKey::ElevenLabsApiKey => "elevenlabs_api_key",
            SecureKey::OpenRouterApiKey => "openrouter_api_key",
        }
    }
}

impl fmt::Display for SecureKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.key_name())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SecureStorageError {
    #[error("Keyring error: {0}")]
    Keyring(#[from] keyring::Error),

    #[error("Key not found: {0}")]
    KeyNotFound(SecureKey),
}

pub struct SecureStorage;

impl SecureStorage {
    /// Store a secret value in secure storage
    pub fn set(key: SecureKey, value: &str) -> Result<(), SecureStorageError> {
        let entry = Entry::new(SERVICE_NAME, key.key_name())?;
        entry.set_password(value)?;
        Ok(())
    }

    /// Retrieve a secret value from secure storage
    pub fn get(key: SecureKey) -> Result<String, SecureStorageError> {
        let entry = Entry::new(SERVICE_NAME, key.key_name())?;
        match entry.get_password() {
            Ok(value) => Ok(value),
            Err(keyring::Error::NoEntry) => Err(SecureStorageError::KeyNotFound(key)),
            Err(e) => Err(SecureStorageError::Keyring(e)),
        }
    }

    /// Delete a secret value from secure storage
    pub fn delete(key: SecureKey) -> Result<(), SecureStorageError> {
        let entry = Entry::new(SERVICE_NAME, key.key_name())?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // Already deleted is success
            Err(e) => Err(SecureStorageError::Keyring(e)),
        }
    }

    /// Check if a key exists in secure storage
    pub fn exists(key: SecureKey) -> bool {
        Self::get(key).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secure_storage_roundtrip() {
        let test_key = SecureKey::ElevenLabsApiKey;
        let test_value = "test_api_key_12345";

        // Clean up any existing value
        let _ = SecureStorage::delete(test_key);

        // Should not exist initially
        assert!(!SecureStorage::exists(test_key));

        // Store value
        SecureStorage::set(test_key, test_value).expect("Failed to set value");

        // Should exist now
        assert!(SecureStorage::exists(test_key));

        // Retrieve value
        let retrieved = SecureStorage::get(test_key).expect("Failed to get value");
        assert_eq!(retrieved, test_value);

        // Delete value
        SecureStorage::delete(test_key).expect("Failed to delete value");

        // Should not exist after deletion
        assert!(!SecureStorage::exists(test_key));
    }

    #[test]
    fn test_get_nonexistent_key() {
        let test_key = SecureKey::OpenRouterApiKey;

        // Clean up any existing value
        let _ = SecureStorage::delete(test_key);

        // Should return KeyNotFound error
        match SecureStorage::get(test_key) {
            Err(SecureStorageError::KeyNotFound(key)) => {
                assert_eq!(key, test_key);
            }
            _ => panic!("Expected KeyNotFound error"),
        }
    }
}
