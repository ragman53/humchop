//! Application-specific error types for HumChop.

use std::fmt;

/// HumChop application errors.
#[derive(Debug)]
pub enum HumChopError {
    /// Microphone device not found or unavailable.
    MicrophoneNotFound(String),
    /// Sample audio is shorter than the number of notes to map.
    SampleTooShort {
        sample_len: usize,
        note_count: usize,
    },
    /// Only a single note was detected in the hum recording.
    SingleNoteDetected,
    /// Unsupported audio format.
    UnsupportedFormat(String),
    /// WSL2 PulseServer environment variable not set.
    Wsl2PulseServerNotSet,
    /// Audio device is busy (common on WSL2 without pulseaudio feature).
    AudioDeviceBusy(String),
    /// Failed to decode audio file.
    DecodeError(String),
    /// Failed to encode/write audio file.
    EncodeError(String),
    /// I/O error.
    IoError(String),
    /// Invalid audio data (e.g., zero samples).
    InvalidAudio(String),
    /// Other errors wrapped from underlying libraries.
    Other(String),
}

impl fmt::Display for HumChopError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HumChopError::MicrophoneNotFound(msg) => {
                write!(f, "Microphone not found: {}\nTip: Check your audio input device settings. On Linux, try 'pactl list sources short'", msg)
            }
            HumChopError::SampleTooShort {
                sample_len,
                note_count,
            } => {
                write!(f, "Sample too short: {} samples available but {} notes detected. Will use equal division instead.", sample_len, note_count)
            }
            HumChopError::SingleNoteDetected => {
                write!(f, "Only one note detected in your humming. Please record again with more distinct notes.")
            }
            HumChopError::UnsupportedFormat(fmt) => {
                write!(
                    f,
                    "Unsupported audio format: {}. Supported formats: WAV, MP3, FLAC",
                    fmt
                )
            }
            HumChopError::Wsl2PulseServerNotSet => {
                write!(f, "WSL2 audio not configured. Add to your shell config (~/.bashrc or ~/.zshrc):\n  export PULSE_SERVER=unix:/mnt/wslg/PulseServer\nThen restart your terminal.")
            }
            HumChopError::AudioDeviceBusy(msg) => {
                write!(f, "Audio device busy: {}\nOn WSL2, ensure cpal is built with 'pulseaudio' feature.", msg)
            }
            HumChopError::DecodeError(msg) => {
                write!(f, "Failed to decode audio: {}", msg)
            }
            HumChopError::EncodeError(msg) => {
                write!(f, "Failed to encode audio: {}", msg)
            }
            HumChopError::IoError(msg) => {
                write!(f, "I/O error: {}", msg)
            }
            HumChopError::InvalidAudio(msg) => {
                write!(f, "Invalid audio: {}", msg)
            }
            HumChopError::Other(msg) => {
                write!(f, "Error: {}", msg)
            }
        }
    }
}

impl std::error::Error for HumChopError {}

impl From<std::io::Error> for HumChopError {
    fn from(err: std::io::Error) -> Self {
        HumChopError::IoError(err.to_string())
    }
}

impl From<hound::Error> for HumChopError {
    fn from(err: hound::Error) -> Self {
        match err {
            hound::Error::IoError(io_err) => HumChopError::IoError(io_err.to_string()),
            hound::Error::FormatError(msg) => {
                HumChopError::DecodeError(format!("WAV format error: {}", msg))
            }
            hound::Error::Unsupported => {
                HumChopError::UnsupportedFormat("WAV format variant not supported".to_string())
            }
            hound::Error::InvalidSampleFormat => {
                HumChopError::DecodeError("Sample format mismatch".to_string())
            }
            hound::Error::TooWide => {
                HumChopError::DecodeError("Sample too wide for format".to_string())
            }
            hound::Error::UnfinishedSample => {
                HumChopError::EncodeError("Unfinished sample in output".to_string())
            }
        }
    }
}
