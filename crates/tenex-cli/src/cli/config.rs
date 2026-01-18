use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// CLI configuration that can be loaded from a JSON file
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CliConfig {
    /// Custom socket path for daemon communication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket_path: Option<PathBuf>,

    /// Credentials for nostr authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials: Option<Credentials>,
}

/// Nostr credentials configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    /// nsec (unencrypted) or ncryptsec (encrypted) key
    pub key: String,

    /// Password for ncryptsec decryption (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl CliConfig {
    /// Load config from a JSON file
    pub fn load(path: &PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let config: CliConfig = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        Ok(config)
    }

    /// Serialize config to JSON for passing to daemon
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).context("Failed to serialize config")
    }

    /// Deserialize config from JSON
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("Failed to deserialize config")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config_with_socket_path() {
        let json = r#"{"socketPath": "/tmp/test/tenex-cli.sock"}"#;
        let config: CliConfig = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.socket_path,
            Some(PathBuf::from("/tmp/test/tenex-cli.sock"))
        );
        assert!(config.credentials.is_none());
    }

    #[test]
    fn test_parse_config_with_credentials() {
        let json = r#"{
            "socketPath": "/tmp/test.sock",
            "credentials": {
                "key": "nsec1abc123",
                "password": "secret"
            }
        }"#;
        let config: CliConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.socket_path, Some(PathBuf::from("/tmp/test.sock")));
        let creds = config.credentials.unwrap();
        assert_eq!(creds.key, "nsec1abc123");
        assert_eq!(creds.password, Some("secret".to_string()));
    }

    #[test]
    fn test_parse_config_minimal() {
        let json = r#"{}"#;
        let config: CliConfig = serde_json::from_str(json).unwrap();
        assert!(config.socket_path.is_none());
        assert!(config.credentials.is_none());
    }
}
