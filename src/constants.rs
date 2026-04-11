//! HumChop constants — shared values used across modules.
//!
//! MINOR-3 fix: magic numbers extracted to named constants for maintainability.

/// Maximum recording duration in seconds.
pub const MAX_RECORDING_DURATION_SECS: f64 = 15.0;

/// Maximum duration for headless/demo sample processing in seconds.
pub const MAX_DEMO_DURATION_SECS: f64 = 10.0;

/// Fade duration in milliseconds (used for click prevention at boundaries).
pub const FADE_MS: f64 = 5.0;

/// Minimum gap between chops in seconds (JDilla-style).
pub const MIN_CHOP_GAP_SECS: f64 = 0.005;

/// Default crossfade length in samples (at 44100Hz ≈ ~5.8ms).
pub const DEFAULT_CROSSFADE_SAMPLES: usize = 256;

/// Minimum crossfade length to avoid artifacts.
pub const MIN_CROSSFADE_SAMPLES: usize = 8;
