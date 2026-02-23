use super::*;

// Standalone FFI functions — no TenexCore instance needed, bypasses actor serialization.

/// List all audio notifications (pure filesystem read).
#[uniffi::export]
pub fn list_audio_notifications() -> Result<Vec<AudioNotificationInfo>, TenexError> {
    let data_dir = get_data_dir();
    let manager =
        crate::ai::AudioNotificationManager::new(data_dir.to_str().unwrap_or("tenex_data"));

    let notifications = manager
        .list_notifications()
        .map_err(|e| TenexError::Internal {
            message: format!("Failed to list audio notifications: {}", e),
        })?;

    Ok(notifications
        .into_iter()
        .map(|n| AudioNotificationInfo {
            id: n.id,
            agent_pubkey: n.agent_pubkey,
            conversation_title: n.conversation_title,
            original_text: n.original_text,
            massaged_text: n.massaged_text,
            voice_id: n.voice_id,
            audio_file_path: n.audio_file_path,
            created_at: n.created_at,
        })
        .collect())
}

/// Delete an audio notification by ID (pure filesystem operation).
#[uniffi::export]
pub fn delete_audio_notification(id: String) -> Result<(), TenexError> {
    let data_dir = get_data_dir();
    let manager =
        crate::ai::AudioNotificationManager::new(data_dir.to_str().unwrap_or("tenex_data"));

    manager
        .delete_notification(&id)
        .map_err(|e| TenexError::Internal {
            message: format!("Failed to delete audio notification: {}", e),
        })?;

    Ok(())
}

#[uniffi::export]
pub fn fetch_elevenlabs_voices(api_key: String) -> Result<Vec<VoiceInfo>, TenexError> {
    let client = crate::ai::ElevenLabsClient::new(api_key);
    let runtime = get_tokio_runtime();

    let voices = runtime
        .block_on(client.get_voices())
        .map_err(|e| TenexError::Internal {
            message: format!("Failed to fetch voices: {}", e),
        })?;

    Ok(voices
        .into_iter()
        .map(|v| VoiceInfo {
            voice_id: v.voice_id,
            name: v.name,
            category: v.category,
            description: v.description,
            preview_url: v.preview_url,
        })
        .collect())
}

#[uniffi::export]
pub fn fetch_openrouter_models(api_key: String) -> Result<Vec<ModelInfo>, TenexError> {
    let client = crate::ai::OpenRouterClient::new(api_key);
    let runtime = get_tokio_runtime();

    let models = runtime
        .block_on(client.get_models())
        .map_err(|e| TenexError::Internal {
            message: format!("Failed to fetch models: {}", e),
        })?;

    Ok(models
        .into_iter()
        .map(|m| ModelInfo {
            id: m.id,
            name: m.name,
            description: m.description,
            context_length: m.context_length,
        })
        .collect())
}
