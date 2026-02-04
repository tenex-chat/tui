//! Audio player module for playing notification sounds in the TUI
//!
//! Uses rodio for cross-platform audio playback. Supports:
//! - Playing MP3 files from the audio_notifications directory
//! - Status indicators (playing/stopped)
//! - Replay functionality

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// State of the audio player
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioPlaybackState {
    /// No audio is playing
    Stopped,
    /// Audio is currently playing
    Playing,
    /// Paused (can be resumed)
    Paused,
}

/// Audio player for notification sounds
pub struct AudioPlayer {
    /// Output stream handle (must be kept alive for playback)
    _stream: Option<OutputStream>,
    /// Stream handle for creating sinks
    stream_handle: Option<OutputStreamHandle>,
    /// Current audio sink for playback control
    sink: Arc<Mutex<Option<Sink>>>,
    /// Path to the currently playing or last played audio file
    current_file: Arc<Mutex<Option<PathBuf>>>,
    /// Current playback state
    state: Arc<Mutex<AudioPlaybackState>>,
}

impl AudioPlayer {
    /// Create a new audio player
    pub fn new() -> Self {
        // Try to initialize the audio output stream
        let (stream, stream_handle) = match OutputStream::try_default() {
            Ok((stream, handle)) => (Some(stream), Some(handle)),
            Err(e) => {
                tracing::warn!("Failed to initialize audio output: {}", e);
                (None, None)
            }
        };

        Self {
            _stream: stream,
            stream_handle,
            sink: Arc::new(Mutex::new(None)),
            current_file: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(AudioPlaybackState::Stopped)),
        }
    }

    /// Check if audio system is available
    pub fn is_available(&self) -> bool {
        self.stream_handle.is_some()
    }

    /// Play an audio file from the given path
    pub fn play(&self, path: &PathBuf) -> Result<(), String> {
        let stream_handle = self
            .stream_handle
            .as_ref()
            .ok_or_else(|| "Audio output not available".to_string())?;

        // Open the audio file
        let file = File::open(path).map_err(|e| format!("Failed to open audio file: {}", e))?;
        let reader = BufReader::new(file);

        // Decode the audio file
        let source =
            Decoder::new(reader).map_err(|e| format!("Failed to decode audio file: {}", e))?;

        // Create a new sink for playback
        let sink =
            Sink::try_new(stream_handle).map_err(|e| format!("Failed to create audio sink: {}", e))?;

        // Append the audio source to the sink
        sink.append(source);

        // Store the sink and update state
        {
            let mut sink_guard = self.sink.lock().unwrap();
            *sink_guard = Some(sink);
        }
        {
            let mut file_guard = self.current_file.lock().unwrap();
            *file_guard = Some(path.clone());
        }
        {
            let mut state_guard = self.state.lock().unwrap();
            *state_guard = AudioPlaybackState::Playing;
        }

        Ok(())
    }

    /// Stop the current playback
    pub fn stop(&self) {
        let mut sink_guard = self.sink.lock().unwrap();
        if let Some(sink) = sink_guard.take() {
            sink.stop();
        }
        drop(sink_guard);

        let mut state_guard = self.state.lock().unwrap();
        *state_guard = AudioPlaybackState::Stopped;
    }

    /// Pause the current playback
    pub fn pause(&self) {
        let sink_guard = self.sink.lock().unwrap();
        if let Some(ref sink) = *sink_guard {
            sink.pause();
            drop(sink_guard);

            let mut state_guard = self.state.lock().unwrap();
            *state_guard = AudioPlaybackState::Paused;
        }
    }

    /// Resume paused playback
    pub fn resume(&self) {
        let sink_guard = self.sink.lock().unwrap();
        if let Some(ref sink) = *sink_guard {
            sink.play();
            drop(sink_guard);

            let mut state_guard = self.state.lock().unwrap();
            *state_guard = AudioPlaybackState::Playing;
        }
    }

    /// Replay the last audio file
    pub fn replay(&self) -> Result<(), String> {
        let path = {
            let file_guard = self.current_file.lock().unwrap();
            file_guard.clone()
        };

        if let Some(path) = path {
            self.play(&path)
        } else {
            Err("No audio file to replay".to_string())
        }
    }

    /// Get the current playback state
    pub fn state(&self) -> AudioPlaybackState {
        // First check if the sink has finished playing
        {
            let sink_guard = self.sink.lock().unwrap();
            if let Some(ref sink) = *sink_guard {
                if sink.empty() {
                    drop(sink_guard);
                    let mut state_guard = self.state.lock().unwrap();
                    *state_guard = AudioPlaybackState::Stopped;
                    return AudioPlaybackState::Stopped;
                }
            }
        }

        let state_guard = self.state.lock().unwrap();
        *state_guard
    }

    /// Check if audio is currently playing
    pub fn is_playing(&self) -> bool {
        self.state() == AudioPlaybackState::Playing
    }

    /// Get the path to the current or last played file
    pub fn current_file(&self) -> Option<PathBuf> {
        let guard = self.current_file.lock().unwrap();
        guard.clone()
    }

    /// Get a display-friendly name for the current audio
    pub fn current_audio_name(&self) -> Option<String> {
        self.current_file().map(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("audio")
                .to_string()
        })
    }
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_player_creation() {
        let player = AudioPlayer::new();
        // Player should be created even if audio is not available
        assert_eq!(player.state(), AudioPlaybackState::Stopped);
    }

    #[test]
    fn test_replay_without_file() {
        let player = AudioPlayer::new();
        let result = player.replay();
        assert!(result.is_err());
    }
}
