use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use nostr_sdk::prelude::{FromBech32, Keys, SecretKey, ToBech32};
use serde::{Deserialize, Serialize};
use tenex_core::models::PreferencesStorage;

const CONFIG_FILE_NAME: &str = "config.json";
const TENEX_CACHE_DIR_NAME: &str = "tenex-cache";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub bind_addr: String,
    pub public_base_url: String,
    pub secret_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NgrokConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_ngrok_api_addr")]
    pub api_addr: String,
    #[serde(default)]
    pub requested_url: Option<String>,
}

impl Default for NgrokConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_addr: default_ngrok_api_addr(),
            requested_url: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub telegram_bot_token: String,
    pub gateway_nsec: String,
    pub relay_urls: Vec<String>,
    pub approved_backend_pubkeys: Vec<String>,
    pub blocked_backend_pubkeys: Vec<String>,
    pub webhook: WebhookConfig,
    #[serde(default)]
    pub ngrok: NgrokConfig,
}

#[derive(Debug, Clone)]
pub struct ImportedTenexPreferences {
    pub relay_urls: Vec<String>,
    pub approved_backend_pubkeys: Vec<String>,
    pub blocked_backend_pubkeys: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct InitInputs {
    pub telegram_bot_token: Option<String>,
    pub gateway_nsec: Option<String>,
    pub relay_urls: Vec<String>,
    pub bind_addr: String,
    pub public_base_url: Option<String>,
    pub import_from_tenex_cli: bool,
}

impl GatewayConfig {
    pub fn config_path(data_dir: &Path) -> PathBuf {
        data_dir.join(CONFIG_FILE_NAME)
    }

    pub fn tenex_cache_dir(data_dir: &Path) -> PathBuf {
        data_dir.join(TENEX_CACHE_DIR_NAME)
    }

    pub fn load(data_dir: &Path) -> Result<Self> {
        let path = Self::config_path(data_dir);
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))
    }

    pub fn save(&self, data_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(data_dir)
            .with_context(|| format!("Failed to create {}", data_dir.display()))?;
        let path = Self::config_path(data_dir);
        let json = serde_json::to_string_pretty(self)
            .with_context(|| format!("Failed to serialize {}", path.display()))?;
        std::fs::write(&path, json)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn webhook_url(&self) -> String {
        Self::webhook_url_for_base_url(&self.webhook.public_base_url)
    }

    pub fn webhook_url_for_base_url(base_url: &str) -> String {
        format!("{}/telegram/webhook", base_url.trim_end_matches('/'))
    }
}

pub fn default_gateway_data_dir() -> PathBuf {
    if let Ok(base_dir) = std::env::var("TENEX_BASE_DIR") {
        return PathBuf::from(base_dir).join("telegram-gateway");
    }
    dirs::home_dir()
        .map(|home| home.join(".tenex").join("telegram-gateway"))
        .unwrap_or_else(|| PathBuf::from(".tenex/telegram-gateway"))
}

pub fn default_tenex_cli_data_dir() -> PathBuf {
    if let Ok(base_dir) = std::env::var("TENEX_BASE_DIR") {
        return PathBuf::from(base_dir).join("cli");
    }
    dirs::home_dir()
        .map(|home| home.join(".tenex").join("cli"))
        .unwrap_or_else(|| PathBuf::from(".tenex/cli"))
}

pub fn load_imported_tenex_preferences(cli_data_dir: &Path) -> Option<ImportedTenexPreferences> {
    let cli_dir = cli_data_dir.to_str()?;
    let storage = PreferencesStorage::new(cli_dir);
    let relay_urls = storage
        .configured_relay_url()
        .map(|url| vec![url.to_string()])
        .unwrap_or_default();
    let approved_backend_pubkeys = storage
        .approved_backend_pubkeys()
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    let blocked_backend_pubkeys = storage
        .blocked_backend_pubkeys()
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    Some(ImportedTenexPreferences {
        relay_urls,
        approved_backend_pubkeys,
        blocked_backend_pubkeys,
    })
}

pub fn build_init_config(data_dir: &Path, inputs: InitInputs) -> Result<GatewayConfig> {
    let imported = if inputs.import_from_tenex_cli {
        load_imported_tenex_preferences(&default_tenex_cli_data_dir())
    } else {
        None
    };

    let telegram_bot_token = match inputs.telegram_bot_token {
        Some(token) if !token.trim().is_empty() => token.trim().to_string(),
        _ => prompt_required("Telegram bot token", None)?,
    };

    let gateway_nsec = match inputs.gateway_nsec {
        Some(value) if !value.trim().is_empty() => value.trim().to_string(),
        _ => {
            let generated = generate_gateway_nsec()?;
            println!("Generated gateway nsec for this bot identity.");
            generated
        }
    };

    if !gateway_nsec.starts_with("nsec1") {
        return Err(anyhow!("Gateway key must be an nsec"));
    }

    let relay_urls = if !inputs.relay_urls.is_empty() {
        normalize_list(inputs.relay_urls)
    } else if let Some(imported) = &imported {
        if !imported.relay_urls.is_empty() {
            imported.relay_urls.clone()
        } else {
            normalize_list(parse_csv(&prompt_required(
                "Relay URLs (comma separated)",
                Some("wss://relay.example.com"),
            )?))
        }
    } else {
        normalize_list(parse_csv(&prompt_required(
            "Relay URLs (comma separated)",
            Some("wss://relay.example.com"),
        )?))
    };

    if relay_urls.is_empty() {
        return Err(anyhow!("At least one relay URL is required"));
    }

    let public_base_url = match inputs.public_base_url {
        Some(url) if !url.trim().is_empty() => url.trim().trim_end_matches('/').to_string(),
        _ => prompt_required(
            "Public HTTPS base URL exposing this gateway",
            Some("https://telegram-gateway.example.com"),
        )?
        .trim_end_matches('/')
        .to_string(),
    };

    if !public_base_url.starts_with("https://") {
        return Err(anyhow!(
            "Webhook public base URL must start with https:// for Telegram webhooks"
        ));
    }

    let approved_backend_pubkeys = imported
        .as_ref()
        .map(|value| normalize_list(value.approved_backend_pubkeys.clone()))
        .unwrap_or_default();
    let blocked_backend_pubkeys = imported
        .as_ref()
        .map(|value| normalize_list(value.blocked_backend_pubkeys.clone()))
        .unwrap_or_default();

    let config = GatewayConfig {
        telegram_bot_token,
        gateway_nsec,
        relay_urls,
        approved_backend_pubkeys,
        blocked_backend_pubkeys,
        webhook: WebhookConfig {
            bind_addr: inputs.bind_addr,
            public_base_url,
            secret_token: uuid::Uuid::new_v4().to_string(),
        },
        ngrok: NgrokConfig::default(),
    };

    config.save(data_dir)?;
    Ok(config)
}

pub fn prompt_required(label: &str, example: Option<&str>) -> Result<String> {
    loop {
        let prompt = match example {
            Some(example) => format!("{label} [{example}]: "),
            None => format!("{label}: "),
        };
        let value = prompt_line(&prompt)?;
        if !value.trim().is_empty() {
            return Ok(value.trim().to_string());
        }
    }
}

fn prompt_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    io::stdout().flush().context("Failed to flush stdout")?;
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .context("Failed to read stdin")?;
    Ok(value)
}

fn generate_gateway_nsec() -> Result<String> {
    let secret_key = SecretKey::generate();
    let keys = Keys::new(secret_key);
    keys.secret_key()
        .to_bech32()
        .map_err(|err| anyhow!("Failed to encode generated nsec: {err}"))
}

pub fn parse_gateway_keys(nsec: &str) -> Result<Keys> {
    let secret_key =
        SecretKey::from_bech32(nsec).map_err(|err| anyhow!("Invalid gateway nsec: {err}"))?;
    Ok(Keys::new(secret_key))
}

fn default_ngrok_api_addr() -> String {
    "127.0.0.1:4040".to_string()
}

pub fn normalize_list(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !normalized.iter().any(|existing| existing == trimmed) {
            normalized.push(trimmed.to_string());
        }
    }
    normalized
}

pub fn parse_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}
