use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const ELEVENLABS_API_BASE: &str = "https://api.elevenlabs.io/v1";

/// Represents a voice from ElevenLabs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voice {
    pub voice_id: String,
    pub name: String,
    pub category: Option<String>,
    pub description: Option<String>,
    pub preview_url: Option<String>,
    pub labels: Option<HashMap<String, String>>,
}

/// Response from ElevenLabs voices API
#[derive(Debug, Deserialize)]
struct VoicesResponse {
    voices: Vec<Voice>,
}

/// ElevenLabs API client
pub struct ElevenLabsClient {
    api_key: String,
    client: reqwest::Client,
}

impl ElevenLabsClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    /// Fetch all available voices from ElevenLabs
    pub async fn get_voices(&self) -> Result<Vec<Voice>> {
        let url = format!("{}/voices", ELEVENLABS_API_BASE);

        let response = self
            .client
            .get(&url)
            .header("xi-api-key", &self.api_key)
            .send()
            .await
            .context("Failed to send request to ElevenLabs API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("ElevenLabs API error ({}): {}", status, error_text);
        }

        let voices_response: VoicesResponse = response
            .json()
            .await
            .context("Failed to parse ElevenLabs voices response")?;

        Ok(voices_response.voices)
    }

    /// Generate audio from text using a specific voice
    /// Returns the audio bytes (MP3 format)
    pub async fn text_to_speech(&self, text: &str, voice_id: &str) -> Result<Vec<u8>> {
        let url = format!("{}/text-to-speech/{}", ELEVENLABS_API_BASE, voice_id);

        let body = serde_json::json!({
            "text": text,
            "model_id": "eleven_v3",
            "voice_settings": {
                "stability": 0.5,
                "similarity_boost": 0.75,
            }
        });

        let response = self
            .client
            .post(&url)
            .header("xi-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send text-to-speech request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("ElevenLabs TTS error ({}): {}", status, error_text);
        }

        let audio_bytes = response
            .bytes()
            .await
            .context("Failed to read audio response")?
            .to_vec();

        Ok(audio_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // Requires actual API key
    async fn test_get_voices() {
        let api_key = std::env::var("ELEVENLABS_API_KEY").expect("ELEVENLABS_API_KEY not set");
        let client = ElevenLabsClient::new(api_key);

        let voices = client.get_voices().await.unwrap();
        assert!(!voices.is_empty());
    }

    #[tokio::test]
    #[ignore] // Requires actual API key
    async fn test_text_to_speech() {
        let api_key = std::env::var("ELEVENLABS_API_KEY").expect("ELEVENLABS_API_KEY not set");
        let client = ElevenLabsClient::new(api_key);

        // First get a voice
        let voices = client.get_voices().await.unwrap();
        let voice_id = &voices[0].voice_id;

        let audio = client
            .text_to_speech("Hello, this is a test.", voice_id)
            .await
            .unwrap();
        assert!(!audio.is_empty());
    }
}
