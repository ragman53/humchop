//! Audio playback module using rodio.
//!
//! Provides audio preview functionality for loaded samples
//! and generated output.

use crate::error::HumChopError;
use rodio::{OutputStream, Sink, Source};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Audio player state.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackState {
    /// Not playing.
    Idle,
    /// Currently playing.
    Playing,
    /// Playback paused.
    Paused,
    /// Error occurred.
    Error(String),
}

/// Audio player for previewing samples.
pub struct Player {
    /// Playback state.
    state: PlaybackState,
    /// Flag to control playback.
    is_playing: Arc<AtomicBool>,
    /// Sample rate of current audio.
    sample_rate: u32,
}

impl Player {
    /// Create a new Player instance.
    pub fn new() -> Self {
        Self {
            state: PlaybackState::Idle,
            is_playing: Arc::new(AtomicBool::new(false)),
            sample_rate: 44100,
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

    /// Play a WAV/MP3/FLAC file.
    #[allow(dead_code)]
    pub fn play_file(&mut self, path: &std::path::Path) -> Result<(), HumChopError> {
        use std::io::BufReader;

        // Stop any current playback
        self.stop();

        // Get output device
        let (stream, stream_handle) = OutputStream::try_default().map_err(|e| {
            log::error!("Failed to get audio output: {}", e);
            HumChopError::Other(format!("Failed to get audio output: {}", e))
        })?;

        let file = std::fs::File::open(path)
            .map_err(|e| HumChopError::IoError(format!("Failed to open audio file: {}", e)))?;

        let reader = BufReader::new(file);

        // Decode the audio file
        let source = rodio::Decoder::new(reader)
            .map_err(|e| HumChopError::DecodeError(format!("Failed to decode audio: {}", e)))?;

        self.sample_rate = source.sample_rate();

        // Create sink and play
        let sink = Sink::try_new(&stream_handle)
            .map_err(|e| HumChopError::Other(format!("Failed to create sink: {}", e)))?;

        sink.append(source);
        self.is_playing.store(true, Ordering::SeqCst);
        self.state = PlaybackState::Playing;

        // Keep stream alive
        std::mem::forget(stream);

        log::info!("Playback started from file: {:?}", path);
        Ok(())
    }

    /// Play raw audio samples (f32 mono).
    pub fn play_samples(&mut self, samples: &[f32], sample_rate: u32) -> Result<(), HumChopError> {
        // Stop any current playback
        self.stop();

        self.sample_rate = sample_rate;

        // Convert to rodio source
        let source = rodio::buffer::SamplesBuffer::new(1, sample_rate, samples);

        self.play_buffer(source)
    }

    /// Preview samples with limited duration.
    pub fn preview(
        &mut self,
        samples: &[f32],
        sample_rate: u32,
        duration_secs: f32,
    ) -> Result<(), HumChopError> {
        // Stop any current playback
        self.stop();

        self.sample_rate = sample_rate;

        // Limit duration
        let max_samples = (sample_rate as f32 * duration_secs) as usize;
        let preview_samples = if samples.len() > max_samples {
            &samples[..max_samples]
        } else {
            samples
        };

        let source = rodio::buffer::SamplesBuffer::new(1, sample_rate, preview_samples);
        self.play_buffer(source)
    }

    /// Play from a decoded source (using rodio Decoder).
    fn play_decoded(&mut self, path: &std::path::Path) -> Result<(), HumChopError> {
        use std::io::BufReader;

        // Get output device
        let (stream, stream_handle) = OutputStream::try_default().map_err(|e| {
            log::error!("Failed to get audio output: {}", e);
            HumChopError::Other(format!("Failed to get audio output: {}", e))
        })?;

        // Open file
        let file = std::fs::File::open(path)
            .map_err(|e| HumChopError::IoError(format!("Failed to open audio file: {}", e)))?;

        // Create decoder
        let reader = BufReader::new(file);
        let source = rodio::Decoder::new(reader)
            .map_err(|e| HumChopError::DecodeError(format!("Failed to decode audio: {}", e)))?;

        // Create sink and play
        let sink = Sink::try_new(&stream_handle)
            .map_err(|e| HumChopError::Other(format!("Failed to create sink: {}", e)))?;

        sink.append(source);
        self.is_playing.store(true, Ordering::SeqCst);
        self.state = PlaybackState::Playing;
        self.sample_rate = 44100;

        // Keep stream alive
        std::mem::forget(stream);

        log::info!("Playback started from file: {:?}", path);
        Ok(())
    }

    /// Play from a buffer source.
    fn play_buffer(
        &mut self,
        source: rodio::buffer::SamplesBuffer<f32>,
    ) -> Result<(), HumChopError> {
        // Get output device
        let (stream, stream_handle) = OutputStream::try_default().map_err(|e| {
            log::error!("Failed to get audio output: {}", e);
            HumChopError::Other(format!("Failed to get audio output: {}", e))
        })?;

        // Create sink with the stream handle
        let sink = Sink::try_new(&stream_handle)
            .map_err(|e| HumChopError::Other(format!("Failed to create sink: {}", e)))?;

        // Play the source
        sink.append(source);
        self.is_playing.store(true, Ordering::SeqCst);
        self.state = PlaybackState::Playing;

        log::info!("Playback started");

        Ok(())
    }

    /// Stop playback.
    pub fn stop(&mut self) {
        self.is_playing.store(false, Ordering::SeqCst);
        self.state = PlaybackState::Idle;
        log::info!("Playback stopped");
    }

    /// Pause playback.
    #[allow(dead_code)]
    pub fn pause(&mut self) {
        if self.state == PlaybackState::Playing {
            self.state = PlaybackState::Paused;
            log::info!("Playback paused");
        }
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

/// List available audio output devices.
#[allow(dead_code)]
pub fn list_output_devices() -> Vec<String> {
    // Note: rodio doesn't expose device enumeration directly
    // This is a placeholder for future enhancement
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
