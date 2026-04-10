//! Microphone recording module using cpal.
//!
//! Records audio from the system microphone and streams samples
//! via an mpsc channel to consumers (e.g., the TUI).

#![allow(dead_code)]

use crate::error::HumChopError;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Maximum recording duration in seconds.
#[allow(dead_code)]
pub const MAX_RECORDING_DURATION_SECS: f64 = 15.0;

/// Audio buffer sender type.
pub type AudioSender = mpsc::Sender<Vec<f32>>;
/// Audio buffer receiver type.
pub type AudioReceiver = mpsc::Receiver<Vec<f32>>;

/// Recording state.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordingState {
    /// Not recording.
    Idle,
    /// Actively recording.
    Recording,
    /// Recording stopped, data available.
    Stopped,
    /// Error occurred during recording.
    Error(String),
}

/// Audio recorder for capturing microphone input.
pub struct Recorder {
    /// The cpal input stream.
    _stream: Option<Stream>,
    /// Flag to control recording loop.
    is_recording: Arc<AtomicBool>,
    /// Sample rate of the recording.
    sample_rate: u32,
    /// Number of channels.
    channels: u16,
    /// Current recording state.
    state: RecordingState,
}

impl Recorder {
    /// Create a new Recorder instance.
    pub fn new() -> Self {
        Self {
            _stream: None,
            is_recording: Arc::new(AtomicBool::new(false)),
            sample_rate: 44100,
            channels: 1,
            state: RecordingState::Idle,
        }
    }

    /// Get the sample rate.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get the number of channels.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Get the current recording state.
    pub fn state(&self) -> &RecordingState {
        &self.state
    }

    /// Start recording from the default microphone.
    ///
    /// Returns a receiver channel that will receive audio samples.
    pub fn start_recording(&mut self) -> Result<AudioReceiver, HumChopError> {
        // Check if already recording
        if self.is_recording.load(Ordering::SeqCst) {
            return Err(HumChopError::Other("Already recording".to_string()));
        }

        // Create channel for audio data
        let (tx, rx) = mpsc::channel::<Vec<f32>>(100);

        // Get the audio host
        let host = cpal::default_host();

        // Get the default input device
        let device_name = host.default_input_device().ok_or_else(|| {
            // Check if running on WSL2 without PulseAudio
            if std::env::var("PULSE_SERVER").is_err() && std::env::var("WSL_DISTRO_NAME").is_ok() {
                HumChopError::Wsl2PulseServerNotSet
            } else {
                HumChopError::MicrophoneNotFound("No default input device found".to_string())
            }
        })?;

        #[allow(deprecated)]
        let device_name_str = device_name.name().unwrap_or_else(|_| "Unknown".into());
        log::info!("Using input device: {}", device_name_str);

        // Get the default input config
        let config = device_name.default_input_config().map_err(|e| {
            log::error!("Failed to get default input config: {}", e);
            HumChopError::MicrophoneNotFound(format!(
                "Failed to get default input config: {}. Make sure your microphone is connected and accessible.",
                e
            ))
        })?;

        self.sample_rate = config.sample_rate();
        self.channels = config.channels();

        log::info!(
            "Recording config: {} Hz, {} channels, format: {:?}",
            self.sample_rate,
            self.channels,
            config.sample_format()
        );

        // Create recording flag
        let is_recording = Arc::clone(&self.is_recording);
        is_recording.store(true, Ordering::SeqCst);

        // Create buffer for accumulating samples
        let recording_flag = Arc::clone(&is_recording);
        let channels = self.channels;

        // Build the input stream with proper sample normalization
        let stream = match config.sample_format() {
            SampleFormat::F32 => {
                self.build_stream_f32(&device_name, &config.into(), tx, recording_flag, channels)
            }
            SampleFormat::I16 => {
                self.build_stream_i16(&device_name, &config.into(), tx, recording_flag, channels)
            }
            SampleFormat::U16 => {
                self.build_stream_u16(&device_name, &config.into(), tx, recording_flag, channels)
            }
            format => {
                return Err(HumChopError::Other(format!(
                    "Unsupported sample format: {:?}. Supported: F32, I16, U16",
                    format
                )));
            }
        }?;

        // Play the stream
        stream.play().map_err(|e| {
            log::error!("Failed to start recording stream: {}", e);
            if e.to_string().contains("DeviceBusy") || e.to_string().contains("busy") {
                HumChopError::AudioDeviceBusy(e.to_string())
            } else {
                HumChopError::Other(format!("Failed to start recording: {}", e))
            }
        })?;

        self._stream = Some(stream);
        self.state = RecordingState::Recording;

        log::info!("Recording started");
        Ok(rx)
    }

    /// Convert a sample value to f32 with proper normalization.
    ///
    /// - f32: passed through as-is (already normalized to ±1.0)
    /// - i16: normalized to ±1.0 range
    /// - u16: normalized to ±1.0 range (centered at 0.0)
    #[inline]
    fn sample_to_f32(sample: f32) -> f32 {
        // f32 is already normalized
        sample.clamp(-1.0, 1.0)
    }

    /// Convert i16 sample to normalized f32.
    #[inline]
    fn i16_to_f32(sample: i16) -> f32 {
        // i16 range: -32768 to 32767
        // Normalize to -1.0 to 1.0
        sample as f32 / 32768.0
    }

    /// Convert u16 sample to normalized f32.
    #[inline]
    fn u16_to_f32(sample: u16) -> f32 {
        // u16 range: 0 to 65535, where 32768 is silence (0.0)
        // Normalize to -1.0 to 1.0
        (sample as f32 - 32768.0) / 32768.0
    }

    /// Build a recording stream for f32 samples.
    fn build_stream_f32(
        &self,
        device: &Device,
        config: &StreamConfig,
        tx: AudioSender,
        is_recording: Arc<AtomicBool>,
        channels: u16,
    ) -> Result<Stream, HumChopError> {
        let err_fn = |err| {
            log::error!("Recording error: {}", err);
        };

        device
            .build_input_stream(
                config,
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    if !is_recording.load(Ordering::SeqCst) {
                        return;
                    }

                    // Convert to mono f32 samples (already normalized)
                    let samples: Vec<f32> = if channels == 1 {
                        data.iter().map(|&s| Self::sample_to_f32(s)).collect()
                    } else {
                        // Average channels to mono
                        let mut mono = Vec::with_capacity(data.len() / channels as usize);
                        for chunk in data.chunks(channels as usize) {
                            let sum: f32 = chunk.iter().map(|&s| Self::sample_to_f32(s)).sum();
                            mono.push(sum / channels as f32);
                        }
                        mono
                    };

                    let _ = tx.try_send(samples);
                },
                err_fn,
                None,
            )
            .map_err(|e| HumChopError::Other(format!("Failed to build input stream: {}", e)))
    }

    /// Build a recording stream for i16 samples.
    fn build_stream_i16(
        &self,
        device: &Device,
        config: &StreamConfig,
        tx: AudioSender,
        is_recording: Arc<AtomicBool>,
        channels: u16,
    ) -> Result<Stream, HumChopError> {
        let err_fn = |err| {
            log::error!("Recording error: {}", err);
        };

        device
            .build_input_stream(
                config,
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    if !is_recording.load(Ordering::SeqCst) {
                        return;
                    }

                    // Convert to mono f32 samples with normalization
                    let samples: Vec<f32> = if channels == 1 {
                        data.iter().map(|&s| Self::i16_to_f32(s)).collect()
                    } else {
                        // Average channels to mono
                        let mut mono = Vec::with_capacity(data.len() / channels as usize);
                        for chunk in data.chunks(channels as usize) {
                            let sum: f32 = chunk.iter().map(|&s| Self::i16_to_f32(s)).sum();
                            mono.push(sum / channels as f32);
                        }
                        mono
                    };

                    let _ = tx.try_send(samples);
                },
                err_fn,
                None,
            )
            .map_err(|e| HumChopError::Other(format!("Failed to build input stream: {}", e)))
    }

    /// Build a recording stream for u16 samples.
    fn build_stream_u16(
        &self,
        device: &Device,
        config: &StreamConfig,
        tx: AudioSender,
        is_recording: Arc<AtomicBool>,
        channels: u16,
    ) -> Result<Stream, HumChopError> {
        let err_fn = |err| {
            log::error!("Recording error: {}", err);
        };

        device
            .build_input_stream(
                config,
                move |data: &[u16], _: &cpal::InputCallbackInfo| {
                    if !is_recording.load(Ordering::SeqCst) {
                        return;
                    }

                    // Convert to mono f32 samples with normalization
                    let samples: Vec<f32> = if channels == 1 {
                        data.iter().map(|&s| Self::u16_to_f32(s)).collect()
                    } else {
                        // Average channels to mono
                        let mut mono = Vec::with_capacity(data.len() / channels as usize);
                        for chunk in data.chunks(channels as usize) {
                            let sum: f32 = chunk.iter().map(|&s| Self::u16_to_f32(s)).sum();
                            mono.push(sum / channels as f32);
                        }
                        mono
                    };

                    let _ = tx.try_send(samples);
                },
                err_fn,
                None,
            )
            .map_err(|e| HumChopError::Other(format!("Failed to build input stream: {}", e)))
    }

    /// Stop recording.
    pub fn stop_recording(&mut self) {
        if !self.is_recording.load(Ordering::SeqCst) {
            return;
        }

        self.is_recording.store(false, Ordering::SeqCst);
        self._stream = None;
        self.state = RecordingState::Stopped;

        log::info!("Recording stopped");
    }

    /// Reset to idle state.
    pub fn reset(&mut self) {
        self.stop_recording();
        self.state = RecordingState::Idle;
    }

    /// Get an audio device for playback preview.
    #[allow(dead_code)]
    pub fn get_output_device() -> Result<Device, HumChopError> {
        let host = cpal::default_host();
        host.default_output_device()
            .ok_or_else(|| HumChopError::Other("No default output device found".to_string()))
    }
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

/// Detect the available audio input devices.
#[allow(deprecated)]
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    match host.input_devices() {
        Ok(devices) => devices.filter_map(|dev| dev.name().ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Calculate the RMS (root mean square) level of audio samples.
/// Returns a value between 0.0 and 1.0.
pub fn calculate_audio_level(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }

    // Calculate RMS
    let sum_squares: f32 = samples.iter().map(|&s| s * s).sum();
    let rms = (sum_squares / samples.len() as f32).sqrt();

    // RMS is already in range [0, 1] for normalized audio
    // Apply logarithmic scaling for more natural meter behavior
    // Map [0, 1] to [-60dB, 0dB], then back to [0, 1]
    let db = if rms > 0.001 {
        20.0 * rms.log2()
    } else {
        -60.0
    };

    // Convert dB to normalized level: -60dB -> 0, 0dB -> 1
    ((db + 60.0) / 60.0).max(0.0).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_audio_level_silence() {
        let samples = vec![0.0f32; 100];
        let level = calculate_audio_level(&samples);
        assert!(level < 0.01);
    }

    #[test]
    fn test_calculate_audio_level_max() {
        let samples = vec![1.0f32; 100];
        let level = calculate_audio_level(&samples);
        assert!((level - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_calculate_audio_level_half() {
        let samples = vec![0.5f32; 100];
        let level = calculate_audio_level(&samples);
        // Half amplitude should produce lower level than full
        assert!(level < 1.0 && level > 0.1);
    }

    #[test]
    fn test_recorder_state() {
        let recorder = Recorder::new();
        assert_eq!(recorder.state(), &RecordingState::Idle);
    }

    #[test]
    fn test_recorder_default() {
        let recorder = Recorder::default();
        assert_eq!(recorder.state(), &RecordingState::Idle);
        assert_eq!(recorder.channels(), 1);
    }
}
