//! Audio playback module using rodio.
//!
//! Provides audio preview functionality for loaded samples
//! and generated output.

use crate::error::HumChopError;
use rodio::{OutputStream, Sink};

/// Audio player state.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackState {
    /// Not playing.
    Idle,
    /// Currently playing.
    Playing,
    /// Error occurred.
    Error(String),
}

/// Audio player for previewing samples.
///
/// This player keeps the OutputStream and Sink alive during playback
/// to ensure audio plays correctly.
pub struct Player {
    /// Playback state.
    state: PlaybackState,
    /// Sample rate of current audio.
    sample_rate: u32,
    /// Output stream (must be kept alive during playback).
    #[allow(dead_code)]
    stream: Option<rodio::OutputStream>,
    /// Audio sink (must be kept alive during playback).
    #[allow(dead_code)]
    sink: Option<rodio::Sink>,
}

impl Player {
    /// Create a new Player instance.
    pub fn new() -> Self {
        Self {
            state: PlaybackState::Idle,
            sample_rate: 44100,
            stream: None,
            sink: None,
        }
    }

    /// Get the current playback state.
    pub fn state(&self) -> &PlaybackState {
        &self.state
    }

    /// Get the sample rate.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Check if currently playing.
    pub fn is_playing(&self) -> bool {
        self.state == PlaybackState::Playing
    }

    /// Play raw audio samples (f32 mono).
    pub fn play_samples(&mut self, samples: &[f32], sample_rate: u32) -> Result<(), HumChopError> {
        // Stop any current playback first
        self.stop();

        self.sample_rate = sample_rate;

        // Get default output stream
        let (stream, stream_handle) = OutputStream::try_default().map_err(|e| {
            log::error!("Failed to get audio output: {}", e);
            HumChopError::Other(format!("Failed to get audio output: {}", e))
        })?;

        // Create sink
        let sink = Sink::try_new(&stream_handle).map_err(|e| {
            log::error!("Failed to create sink: {}", e);
            HumChopError::Other(format!("Failed to create sink: {}", e))
        })?;

        // Convert samples to rodio source
        let source = rodio::buffer::SamplesBuffer::new(1, sample_rate, samples);

        // Append source to sink
        sink.append(source);

        // Store stream and sink to keep them alive
        self.stream = Some(stream);
        self.sink = Some(sink);
        self.state = PlaybackState::Playing;

        log::info!(
            "Playback started ({} samples, {} Hz)",
            samples.len(),
            sample_rate
        );
        Ok(())
    }

    /// Preview samples with limited duration.
    pub fn preview(
        &mut self,
        samples: &[f32],
        sample_rate: u32,
        duration_secs: f32,
    ) -> Result<(), HumChopError> {
        // Stop any current playback first
        self.stop();

        self.sample_rate = sample_rate;

        // Limit duration
        let max_samples = (sample_rate as f32 * duration_secs) as usize;
        let preview_samples = if samples.len() > max_samples {
            &samples[..max_samples]
        } else {
            samples
        };

        log::info!(
            "Preview: {} samples, {:.1}s limit",
            preview_samples.len(),
            duration_secs
        );

        // Get default output stream
        let (stream, stream_handle) = OutputStream::try_default().map_err(|e| {
            log::error!("Failed to get audio output: {}", e);
            HumChopError::Other(format!("Failed to get audio output: {}", e))
        })?;

        // Create sink
        let sink = Sink::try_new(&stream_handle).map_err(|e| {
            log::error!("Failed to create sink: {}", e);
            HumChopError::Other(format!("Failed to create sink: {}", e))
        })?;

        // Convert to rodio source
        let source = rodio::buffer::SamplesBuffer::new(1, sample_rate, preview_samples);

        // Append source to sink
        sink.append(source);

        // Store stream and sink to keep them alive
        self.stream = Some(stream);
        self.sink = Some(sink);
        self.state = PlaybackState::Playing;

        log::info!("Preview playback started");
        Ok(())
    }

    /// Play a WAV/MP3/FLAC file.
    #[allow(dead_code)]
    pub fn play_file(&mut self, path: &std::path::Path) -> Result<(), HumChopError> {
        use std::io::BufReader;

        // Stop any current playback first
        self.stop();

        // Get output stream
        let (stream, stream_handle) = OutputStream::try_default().map_err(|e| {
            log::error!("Failed to get audio output: {}", e);
            HumChopError::Other(format!("Failed to get audio output: {}", e))
        })?;

        // Open file
        let file = std::fs::File::open(path)
            .map_err(|e| HumChopError::IoError(format!("Failed to open audio file: {}", e)))?;

        let reader = BufReader::new(file);

        // Decode the audio file
        let source = rodio::Decoder::new(reader)
            .map_err(|e| HumChopError::DecodeError(format!("Failed to decode audio: {}", e)))?;

        // Note: sample_rate will be available from the source when played

        // Create sink
        let sink = Sink::try_new(&stream_handle)
            .map_err(|e| HumChopError::Other(format!("Failed to create sink: {}", e)))?;

        // Play the source
        sink.append(source);

        // Store to keep alive
        self.stream = Some(stream);
        self.sink = Some(sink);
        self.state = PlaybackState::Playing;

        log::info!("Playback started from file: {:?}", path);
        Ok(())
    }

    /// Stop playback.
    pub fn stop(&mut self) {
        // Detach sink to stop playback
        if let Some(sink) = self.sink.take() {
            sink.detach();
        }
        self.stream = None;
        self.sink = None;
        self.state = PlaybackState::Idle;
        log::info!("Playback stopped");
    }

    /// Wait for playback to finish (blocks until done).
    #[allow(dead_code)]
    pub fn wait(&mut self) {
        if let Some(sink) = &self.sink {
            sink.sleep_until_end();
        }
        self.state = PlaybackState::Idle;
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

/// List available audio output devices.
#[allow(dead_code)]
pub fn list_output_devices() -> Vec<String> {
    vec!["default".to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_player_state() {
        let player = Player::new();
        assert_eq!(player.state(), &PlaybackState::Idle);
    }

    #[test]
    fn test_player_default() {
        let player = Player::default();
        assert_eq!(player.state(), &PlaybackState::Idle);
        assert!(!player.is_playing());
    }

    #[test]
    fn test_player_stop_when_idle() {
        let mut player = Player::new();
        player.stop(); // Should not panic
        assert_eq!(player.state(), &PlaybackState::Idle);
    }
}
