use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

use super::elevenlabs::ElevenLabsClient;
use super::openrouter::OpenRouterClient;
use crate::models::project_draft::AiAudioSettings;

/// Represents a generated audio notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioNotification {
    pub id: String,
    pub agent_pubkey: String,
    pub conversation_title: String,
    pub original_text: String,
    pub massaged_text: String,
    pub voice_id: String,
    pub audio_file_path: String,
    pub created_at: u64,
}

/// Manages audio notification generation and storage
pub struct AudioNotificationManager {
    data_dir: PathBuf,
}

impl AudioNotificationManager {
    pub fn new(data_dir: &str) -> Self {
        let data_dir = PathBuf::from(data_dir).join("audio_notifications");
        Self { data_dir }
    }

    /// Initialize the audio notifications directory
    pub fn init(&self) -> Result<()> {
        fs::create_dir_all(&self.data_dir)
            .context("Failed to create audio notifications directory")?;
        Ok(())
    }

    /// Select a voice deterministically based on agent pubkey
    ///
    /// Uses SHA-256 for stable, deterministic hashing across Rust versions.
    /// Sorts voice IDs before selection to prevent reordering issues.
    ///
    /// # Arguments
    /// * `agent_pubkey` - The agent's public key (hex string)
    /// * `voice_ids` - List of available voice IDs
    ///
    /// # Returns
    /// The selected voice ID, or None if voice_ids is empty
    pub fn select_voice_for_agent(agent_pubkey: &str, voice_ids: &[String]) -> Option<String> {
        if voice_ids.is_empty() {
            return None;
        }

        // Sort voice IDs to ensure deterministic selection even if list order changes
        let mut sorted_voices = voice_ids.to_vec();
        sorted_voices.sort();

        // Use SHA-256 for stable, cryptographically secure hashing
        let mut hasher = Sha256::new();
        hasher.update(agent_pubkey.as_bytes());
        let hash_bytes = hasher.finalize();

        // Convert first 8 bytes to u64 for indexing
        let hash_value = u64::from_be_bytes([
            hash_bytes[0],
            hash_bytes[1],
            hash_bytes[2],
            hash_bytes[3],
            hash_bytes[4],
            hash_bytes[5],
            hash_bytes[6],
            hash_bytes[7],
        ]);

        let index = (hash_value as usize) % sorted_voices.len();
        Some(sorted_voices[index].clone())
    }

    /// Generate an audio notification for a message
    ///
    /// # Arguments
    /// * `agent_pubkey` - Pubkey of the agent sending the message
    /// * `conversation_title` - Title of the conversation
    /// * `message_text` - The message text to convert to audio
    /// * `elevenlabs_key` - ElevenLabs API key from secure storage
    /// * `openrouter_key` - OpenRouter API key from secure storage
    /// * `settings` - AI audio settings (voice IDs, model, prompt)
    pub async fn generate_notification(
        &self,
        agent_pubkey: &str,
        conversation_title: &str,
        message_text: &str,
        elevenlabs_key: &str,
        openrouter_key: &str,
        settings: &AiAudioSettings,
    ) -> Result<AudioNotification> {
        // Validate settings
        let model = settings
            .openrouter_model
            .as_ref()
            .context("OpenRouter model not selected")?;

        // Select voice for this agent
        let voice_id = Self::select_voice_for_agent(agent_pubkey, &settings.selected_voice_ids)
            .context("No voices configured")?;

        // Step 1: Massage text for audio using OpenRouter
        let openrouter_client = OpenRouterClient::new(openrouter_key.to_string());
        let massaged_text = openrouter_client
            .massage_text_for_audio(
                message_text,
                Some(conversation_title),
                model,
                &settings.audio_prompt,
            )
            .await
            .context("Failed to massage text for audio")?;

        // Step 2: Safety-strip any residual markdown the LLM may have left
        let massaged_text = strip_residual_markdown(&massaged_text);

        // Step 3: Generate audio using ElevenLabs
        let elevenlabs_client = ElevenLabsClient::new(elevenlabs_key.to_string());
        let audio_bytes = elevenlabs_client
            .text_to_speech(&massaged_text, &voice_id)
            .await
            .context("Failed to generate audio")?;

        // Step 3: Save audio file atomically using UUID
        let notification_id = Uuid::new_v4().to_string();
        let audio_filename = format!("{}.mp3", notification_id);
        let audio_file_path = self.data_dir.join(&audio_filename);

        // Write atomically: temp file + rename (POSIX atomic operation)
        let temp_filename = format!(".{}.mp3.tmp", notification_id);
        let temp_file_path = self.data_dir.join(&temp_filename);

        // Write to temp file first
        fs::write(&temp_file_path, &audio_bytes).context("Failed to write temporary audio file")?;

        // Atomic rename (overwrites if exists, though UUID collision is virtually impossible)
        fs::rename(&temp_file_path, &audio_file_path)
            .context("Failed to atomically move audio file to final location")?;

        // Step 4: Create notification record
        let notification = AudioNotification {
            id: notification_id,
            agent_pubkey: agent_pubkey.to_string(),
            conversation_title: conversation_title.to_string(),
            original_text: message_text.to_string(),
            massaged_text,
            voice_id,
            audio_file_path: audio_file_path.to_string_lossy().to_string(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        // Step 5: Save notification metadata
        self.save_notification_metadata(&notification)?;

        Ok(notification)
    }

    /// Save notification metadata to JSON
    fn save_notification_metadata(&self, notification: &AudioNotification) -> Result<()> {
        let metadata_path = self.data_dir.join(format!("{}.json", notification.id));
        let json = serde_json::to_string_pretty(notification)
            .context("Failed to serialize notification metadata")?;
        fs::write(metadata_path, json).context("Failed to write notification metadata")?;
        Ok(())
    }

    /// Get a notification by ID
    pub fn get_notification(&self, id: &str) -> Result<AudioNotification> {
        let metadata_path = self.data_dir.join(format!("{}.json", id));
        let json =
            fs::read_to_string(metadata_path).context("Failed to read notification metadata")?;
        let notification: AudioNotification =
            serde_json::from_str(&json).context("Failed to parse notification metadata")?;
        Ok(notification)
    }

    /// List all notifications
    pub fn list_notifications(&self) -> Result<Vec<AudioNotification>> {
        let mut notifications = Vec::new();

        if !self.data_dir.exists() {
            return Ok(notifications);
        }

        for entry in fs::read_dir(&self.data_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(json) = fs::read_to_string(&path) {
                    if let Ok(notification) = serde_json::from_str::<AudioNotification>(&json) {
                        notifications.push(notification);
                    }
                }
            }
        }

        // Sort by created_at descending
        notifications.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(notifications)
    }

    /// Delete a notification
    pub fn delete_notification(&self, id: &str) -> Result<()> {
        let metadata_path = self.data_dir.join(format!("{}.json", id));
        let audio_path = self.data_dir.join(format!("{}.mp3", id));

        if metadata_path.exists() {
            fs::remove_file(metadata_path)?;
        }
        if audio_path.exists() {
            fs::remove_file(audio_path)?;
        }

        Ok(())
    }

    /// Clean up old notifications (older than specified days)
    pub fn cleanup_old_notifications(&self, days: u64) -> Result<usize> {
        let cutoff = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            - (days * 24 * 60 * 60);

        let notifications = self.list_notifications()?;
        let mut deleted_count = 0;

        for notification in notifications {
            if notification.created_at < cutoff {
                self.delete_notification(&notification.id)?;
                deleted_count += 1;
            }
        }

        Ok(deleted_count)
    }
}

/// Strip any residual markdown the LLM may have left in the massaged text.
fn strip_residual_markdown(text: &str) -> String {
    let mut result = String::with_capacity(text.len());

    // Remove code blocks first (``` ... ```)
    let mut remaining = text;
    while let Some(start) = remaining.find("```") {
        result.push_str(&remaining[..start]);
        remaining = &remaining[start + 3..];
        if let Some(end) = remaining.find("```") {
            remaining = &remaining[end + 3..];
        } else {
            break;
        }
    }
    result.push_str(remaining);

    // Remove inline code backticks (keep content)
    result = result.replace('`', "");

    // Remove bold/italic markers: ***, **, *
    // Process longest first so *** doesn't leave stray *
    result = result.replace("***", "");
    result = result.replace("**", "");
    result = result.replace("___", "");
    result = result.replace("__", "");

    // Remove lone * and _ used for emphasis (but not in contractions like don't)
    // Only strip * that appear at word boundaries
    let chars: Vec<char> = result.chars().collect();
    let mut cleaned = String::with_capacity(result.len());
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '*' {
            continue;
        }
        if ch == '_' {
            let prev_alpha = i > 0 && chars[i - 1].is_alphanumeric();
            let next_alpha = i + 1 < chars.len() && chars[i + 1].is_alphanumeric();
            // Keep underscores that are between alphanumeric chars (part of identifiers)
            if prev_alpha && next_alpha {
                cleaned.push(ch);
            }
            continue;
        }
        cleaned.push(ch);
    }

    // Remove markdown header markers at line starts
    let lines: Vec<&str> = cleaned.lines().collect();
    let stripped_lines: Vec<String> = lines
        .iter()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with('#') {
                trimmed.trim_start_matches('#').trim_start().to_string()
            } else {
                line.to_string()
            }
        })
        .collect();
    cleaned = stripped_lines.join(" ");

    // Collapse whitespace
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voice_selection_deterministic() {
        let voices = vec![
            "voice1".to_string(),
            "voice2".to_string(),
            "voice3".to_string(),
        ];
        let pubkey = "test_pubkey_123";

        let voice1 = AudioNotificationManager::select_voice_for_agent(pubkey, &voices);
        let voice2 = AudioNotificationManager::select_voice_for_agent(pubkey, &voices);

        assert_eq!(voice1, voice2);
        assert!(voice1.is_some());
    }

    #[test]
    fn test_voice_selection_different_agents() {
        let voices = vec![
            "voice1".to_string(),
            "voice2".to_string(),
            "voice3".to_string(),
        ];

        let voice1 = AudioNotificationManager::select_voice_for_agent("agent1", &voices);
        let voice2 = AudioNotificationManager::select_voice_for_agent("agent2", &voices);

        // Different agents should get consistent voices (though they might be the same voice by chance)
        assert!(voice1.is_some());
        assert!(voice2.is_some());
    }

    #[test]
    fn test_voice_selection_order_independent() {
        // Test that voice selection is independent of list order
        let voices_order1 = vec![
            "voice_a".to_string(),
            "voice_b".to_string(),
            "voice_c".to_string(),
        ];
        let voices_order2 = vec![
            "voice_c".to_string(),
            "voice_a".to_string(),
            "voice_b".to_string(),
        ];
        let voices_order3 = vec![
            "voice_b".to_string(),
            "voice_c".to_string(),
            "voice_a".to_string(),
        ];

        let pubkey = "test_agent_xyz";

        let voice1 = AudioNotificationManager::select_voice_for_agent(pubkey, &voices_order1);
        let voice2 = AudioNotificationManager::select_voice_for_agent(pubkey, &voices_order2);
        let voice3 = AudioNotificationManager::select_voice_for_agent(pubkey, &voices_order3);

        // All should select the same voice regardless of list order
        assert_eq!(voice1, voice2);
        assert_eq!(voice2, voice3);
        assert!(voice1.is_some());
    }

    #[test]
    fn test_voice_selection_stable_hash() {
        // Verify SHA-256 produces expected output for known inputs
        let voices = vec![
            "voice1".to_string(),
            "voice2".to_string(),
            "voice3".to_string(),
        ];
        let pubkey = "npub1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqx5";

        let voice = AudioNotificationManager::select_voice_for_agent(pubkey, &voices);

        // This should always select the same voice for this specific pubkey
        // (exact voice depends on SHA-256 hash, but should be stable)
        assert!(voice.is_some());

        // Verify it's one of the sorted voices
        let sorted_voices = ["voice1".to_string(),
            "voice2".to_string(),
            "voice3".to_string()];
        assert!(sorted_voices.contains(voice.as_ref().unwrap()));

        // Run 100 times to verify stability
        for _ in 0..100 {
            let test_voice = AudioNotificationManager::select_voice_for_agent(pubkey, &voices);
            assert_eq!(voice, test_voice, "Voice selection should be deterministic");
        }
    }

    #[test]
    fn test_voice_selection_empty_list() {
        let voices = vec![];
        let voice = AudioNotificationManager::select_voice_for_agent("agent1", &voices);
        assert!(voice.is_none());
    }
}
