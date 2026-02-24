use super::*;

#[uniffi::export]
impl TenexCore {
    // ===== AI Audio Notification Methods =====

    /// Get AI audio settings (API keys never exposed - only configuration status)
    pub fn get_ai_audio_settings(&self) -> Result<AiAudioSettingsInfo, TenexError> {
        let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
            resource: "preferences".to_string(),
        })?;

        let prefs_storage = prefs_guard.as_ref().ok_or(TenexError::CoreNotInitialized)?;
        let settings = &prefs_storage.prefs.ai_audio_settings;

        // Never expose actual API keys - only return whether they're configured
        Ok(AiAudioSettingsInfo {
            elevenlabs_api_key_configured: prefs_storage.get_elevenlabs_api_key().is_some(),
            openrouter_api_key_configured: prefs_storage.get_openrouter_api_key().is_some(),
            selected_voice_ids: settings.selected_voice_ids.clone(),
            openrouter_model: settings.openrouter_model.clone(),
            audio_prompt: settings.audio_prompt.clone(),
            enabled: settings.enabled,
            tts_inactivity_threshold_secs: settings.tts_inactivity_threshold_secs,
        })
    }

    /// Set selected voice IDs
    pub fn set_selected_voice_ids(&self, voice_ids: Vec<String>) -> Result<(), TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs_storage = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs_storage
            .set_selected_voice_ids(voice_ids)
            .map_err(|e| TenexError::Internal { message: e })?;
        Ok(())
    }

    /// Set OpenRouter model
    pub fn set_openrouter_model(&self, model: Option<String>) -> Result<(), TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs_storage = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs_storage
            .set_openrouter_model(model)
            .map_err(|e| TenexError::Internal { message: e })?;
        Ok(())
    }

    /// Set ElevenLabs API key (stored in OS secure storage)
    pub fn set_elevenlabs_api_key(&self, key: Option<String>) -> Result<(), TenexError> {
        use crate::secure_storage::{SecureKey, SecureStorage};

        if let Some(key_value) = key {
            SecureStorage::set(SecureKey::ElevenLabsApiKey, &key_value).map_err(|e| {
                TenexError::Internal {
                    message: format!("Failed to store ElevenLabs API key: {}", e),
                }
            })?;
        } else {
            // If key is None, delete the existing key
            SecureStorage::delete(SecureKey::ElevenLabsApiKey).map_err(|e| {
                TenexError::Internal {
                    message: format!("Failed to delete ElevenLabs API key: {}", e),
                }
            })?;
        }
        Ok(())
    }

    /// Set OpenRouter API key (stored in OS secure storage)
    pub fn set_openrouter_api_key(&self, key: Option<String>) -> Result<(), TenexError> {
        use crate::secure_storage::{SecureKey, SecureStorage};

        if let Some(key_value) = key {
            SecureStorage::set(SecureKey::OpenRouterApiKey, &key_value).map_err(|e| {
                TenexError::Internal {
                    message: format!("Failed to store OpenRouter API key: {}", e),
                }
            })?;
        } else {
            // If key is None, delete the existing key
            SecureStorage::delete(SecureKey::OpenRouterApiKey).map_err(|e| {
                TenexError::Internal {
                    message: format!("Failed to delete OpenRouter API key: {}", e),
                }
            })?;
        }
        Ok(())
    }

    /// Get the default audio prompt
    pub fn get_default_audio_prompt(&self) -> String {
        crate::models::project_draft::default_audio_prompt()
    }

    /// Set audio prompt
    pub fn set_audio_prompt(&self, prompt: String) -> Result<(), TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs_storage = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs_storage
            .set_audio_prompt(prompt)
            .map_err(|e| TenexError::Internal { message: e })?;
        Ok(())
    }

    /// Set TTS inactivity threshold (seconds of inactivity before TTS fires)
    pub fn set_tts_inactivity_threshold(&self, secs: u64) -> Result<(), TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs_storage = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs_storage
            .set_tts_inactivity_threshold(secs)
            .map_err(|e| TenexError::Internal { message: e })?;
        Ok(())
    }

    /// Enable or disable audio notifications
    pub fn set_audio_notifications_enabled(&self, enabled: bool) -> Result<(), TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs_storage = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs_storage
            .set_audio_notifications_enabled(enabled)
            .map_err(|e| TenexError::Internal { message: e })?;
        Ok(())
    }

    /// Generate audio notification for a message
    /// Note: This is a blocking call that will wait for the async operation to complete
    /// API keys are passed directly so iOS can provide them from its native Keychain.
    pub fn generate_audio_notification(
        &self,
        agent_pubkey: String,
        conversation_title: String,
        message_text: String,
        elevenlabs_api_key: String,
        openrouter_api_key: String,
    ) -> Result<AudioNotificationInfo, TenexError> {
        let settings = self.get_ai_audio_settings()?;

        if !settings.enabled {
            return Err(TenexError::Internal {
                message: "Audio notifications are disabled".to_string(),
            });
        }

        let data_dir = get_data_dir();
        let manager =
            crate::ai::AudioNotificationManager::new(data_dir.to_str().unwrap_or("tenex_data"));

        // Initialize audio notifications directory
        manager.init().map_err(|e| TenexError::Internal {
            message: format!("Failed to initialize audio notifications: {}", e),
        })?;

        // Use shared Tokio runtime (not per-call creation)
        let runtime = get_tokio_runtime();

        let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
            resource: "preferences".to_string(),
        })?;
        let prefs_storage = prefs_guard.as_ref().ok_or(TenexError::CoreNotInitialized)?;
        let ai_settings = &prefs_storage.prefs.ai_audio_settings;

        let notification = runtime
            .block_on(manager.generate_notification(
                &agent_pubkey,
                &conversation_title,
                &message_text,
                &elevenlabs_api_key,
                &openrouter_api_key,
                ai_settings,
            ))
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to generate audio notification: {}", e),
            })?;

        Ok(AudioNotificationInfo {
            id: notification.id,
            agent_pubkey: notification.agent_pubkey,
            conversation_title: notification.conversation_title,
            original_text: notification.original_text,
            massaged_text: notification.massaged_text,
            voice_id: notification.voice_id,
            audio_file_path: notification.audio_file_path,
            created_at: notification.created_at,
        })
    }

    /// Upload an image to Blossom and return the URL.
    ///
    /// This uploads the image data to the Blossom server using the user's Nostr keys
    /// for authentication. The returned URL can be embedded in message content.
    ///
    /// # Arguments
    /// * `data` - Raw image data (PNG, JPEG, etc.)
    /// * `mime_type` - MIME type of the image (e.g., "image/png", "image/jpeg")
    ///
    /// # Returns
    /// The Blossom URL where the image is stored.
    pub fn upload_image(&self, data: Vec<u8>, mime_type: String) -> Result<String, TenexError> {
        // Get the user's keys for authentication
        let keys_guard = self.keys.read().map_err(|_| TenexError::LockError {
            resource: "keys".to_string(),
        })?;
        let keys = keys_guard.as_ref().ok_or(TenexError::NotLoggedIn)?;

        // Use shared Tokio runtime for async upload
        let runtime = get_tokio_runtime();

        let url = runtime
            .block_on(crate::nostr::upload_image(&data, keys, &mime_type))
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to upload image: {}", e),
            })?;

        Ok(url)
    }
}
