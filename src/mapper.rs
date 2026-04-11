//! Mapper - Map notes to chops with JDilla-style processing.
//!
//! This module handles:
//! - Matching notes to chops by PITCH (not time)
//! - Strength-based matching for JDilla mode
//! - Velocity-based gain adjustment
//! - JDilla-style: chops keep original length, play at note positions

use crate::error::HumChopError;
use crate::hum_analyzer::{HumAnalyzer, Note};
use crate::sample_chopper::{Chop, SampleChopper};
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

// ─────────────────────────────────────────────────────────────
// Configuration
// ─────────────────────────────────────────────────────────────

/// Configuration for the mapper.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MapperConfig {
    /// Enable pitch shifting (can be computationally expensive)
    pub enable_pitch_shift: bool,
    /// Output sample rate
    pub output_sample_rate: u32,
    /// Crossfade length in samples for smooth transitions
    pub crossfade_samples: usize,
    /// JDilla-style: match by strength (transient) rather than pitch
    pub strength_matching: bool,
    /// Enable soft-knee compression to prevent clipping
    pub soft_clip: bool,
    /// Soft clip threshold (in dB, e.g., -1.0 = -1dBFS). Only used if soft_clip is true.
    pub soft_clip_threshold_db: f32,
    /// Enable crossfade between chops (smooth overlap instead of gaps)
    pub enable_crossfade: bool,
}

impl Default for MapperConfig {
    fn default() -> Self {
        Self {
            enable_pitch_shift: false,
            output_sample_rate: 44100,
            crossfade_samples: 256,
            strength_matching: true,      // JDilla style - match by strength
            soft_clip: true,              // Enable soft clipping by default
            soft_clip_threshold_db: -1.0, // -1dBFS threshold
            enable_crossfade: true,       // Enable smooth crossfade by default
        }
    }
}

impl MapperConfig {
    /// Enable/disable soft-knee compression (prevents harsh clipping).
    #[allow(dead_code)]
    pub fn with_soft_clip(mut self, enabled: bool) -> Self {
        self.soft_clip = enabled;
        self
    }

    /// Set soft clip threshold in dB (e.g., -1.0 = -1dBFS).
    #[allow(dead_code)]
    pub fn with_soft_clip_threshold(mut self, db: f32) -> Self {
        self.soft_clip_threshold_db = db.clamp(-12.0, 0.0);
        self
    }
}

// ─────────────────────────────────────────────────────────────
// Soft-knee compression / soft clipping
// ─────────────────────────────────────────────────────────────

/// Apply soft-knee compression/limiting to prevent harsh digital clipping.
/// Uses a smooth hyperbolic tangent (tanh) saturation for natural limiting.
///
/// This is more musical than hard clipping - it gently shapes peaks rather than
/// hard-limiting them, preserving more of the transient character.
///
/// The compression ratio increases smoothly as the signal approaches the threshold,
/// giving it a "soft knee" characteristic.
pub fn soft_knee_compress(samples: &[f32], threshold_db: f32) -> Vec<f32> {
    if samples.is_empty() {
        return vec![];
    }

    // Convert threshold from dB to linear
    let threshold = 10.0_f32.powf(threshold_db / 20.0);

    // Soft knee width in dB
    let knee_db = 6.0_f32;
    let knee_start_linear = 10.0_f32.powf((threshold_db - knee_db) / 20.0);

    let mut output = Vec::with_capacity(samples.len());

    for &sample in samples {
        let abs_input = sample.abs();
        let sign = sample.signum();

        if abs_input <= knee_start_linear {
            // Below knee - linear pass-through
            output.push(sample);
        } else if abs_input <= threshold {
            // Within knee region - gradual compression using smooth interpolation
            // Use a cosine-based curve for smooth transition
            let t = (abs_input - knee_start_linear) / (threshold - knee_start_linear);
            let curve = 0.5 * (1.0 - (std::f32::consts::PI * t).cos());
            let compressed = knee_start_linear + (threshold - knee_start_linear) * curve;
            output.push(compressed * sign);
        } else {
            // Above threshold - soft limiting with tanh saturation
            // Using soft saturation: output = input / sqrt(1 + excess^2)
            // This provides smooth limiting that approaches threshold asymptotically
            let excess = (abs_input - threshold) / (1.0 - threshold + f32::EPSILON);
            let compressed = threshold * (abs_input / (1.0 + excess * excess).sqrt());
            output.push(compressed * sign);
        }
    }

    // Final peak normalization to ensure no samples exceed 1.0
    let max_amp = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    if max_amp > 1.0 {
        let norm = 1.0 / max_amp;
        for s in output.iter_mut() {
            *s *= norm;
        }
    }

    output
}

// ─────────────────────────────────────────────────────────────
// MappedChop
// ─────────────────────────────────────────────────────────────

/// A mapped chop with timing and processing applied.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MappedChop {
    /// The processed audio samples
    pub samples: Vec<f32>,
    /// Original chop index
    pub chop_index: usize,
    /// Note onset in the output (seconds)
    pub output_onset: f64,
    /// Duration in output (seconds)
    pub output_duration: f64,
}

impl MappedChop {
    pub fn new(
        samples: Vec<f32>,
        chop_index: usize,
        output_onset: f64,
        output_duration: f64,
    ) -> Self {
        Self {
            samples,
            chop_index,
            output_onset,
            output_duration,
        }
    }

    /// Get the number of samples.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Check if there are no samples.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// Mapper
// ─────────────────────────────────────────────────────────────

/// Create a new Mapper with default settings.
pub struct Mapper {
    config: MapperConfig,
    sample_rate: u32,
    /// Cached HumAnalyzer for pitch estimation (avoids creating new instance per chop)
    hum_analyzer: HumAnalyzer,
}

impl Mapper {
    /// Create a new Mapper with default settings.
    #[allow(dead_code)]
    pub fn new(sample_rate: u32) -> Self {
        Self {
            config: MapperConfig::default(),
            sample_rate,
            hum_analyzer: HumAnalyzer::new(sample_rate),
        }
    }

    /// Create with custom configuration.
    #[allow(dead_code)]
    pub fn with_config(sample_rate: u32, config: MapperConfig) -> Self {
        Self {
            config,
            sample_rate,
            hum_analyzer: HumAnalyzer::new(sample_rate),
        }
    }

    /// Enable or disable pitch shifting.
    #[allow(dead_code)]
    pub fn with_pitch_shift(mut self, enabled: bool) -> Self {
        self.config.enable_pitch_shift = enabled;
        self
    }

    /// Enable/disable strength matching (JDilla style).
    /// When true, matches note velocity to chop strength.
    /// When false, matches by pitch proximity.
    #[allow(dead_code)]
    pub fn with_strength_matching(mut self, enabled: bool) -> Self {
        self.config.strength_matching = enabled;
        self
    }

    /// Estimate the dominant pitch of a chop using cached HumAnalyzer.
    /// Returns 0.0 if no clear pitch detected (e.g., percussion).
    ///
    /// Uses a pre-allocated HumAnalyzer instance to avoid creating new FFT planners
    /// on every call, improving performance significantly.
    pub fn estimate_chop_pitch(&self, chop: &Chop) -> f32 {
        // Use cached HumAnalyzer (stored in self.hum_analyzer)
        let pitches = self.hum_analyzer.detect_pitch(&chop.samples);

        // Filter out invalid pitches
        let valid: Vec<f32> = pitches.into_iter().filter(|&p| p > 0.0).collect();
        if valid.is_empty() {
            return 0.0; // No pitch detected (percussion)
        }

        // Use median for robustness against outliers
        // Using total_cmp for proper NaN handling (Rust 1.62+)
        let mut sorted = valid;
        sorted.sort_by(|a, b| a.total_cmp(b));
        sorted[sorted.len() / 2]
    }

    /// Match a note to a chop by strength (JDilla mode).
    /// High-velocity note → strong transient chop; soft note → quiet tail chop.
    pub fn match_by_strength(&self, note: &Note, chops: &[Chop], pool: &[usize]) -> usize {
        pool.iter()
            .copied()
            .min_by(|&a, &b| {
                let da = (chops[a].strength - note.velocity).abs();
                let db = (chops[b].strength - note.velocity).abs();
                da.total_cmp(&db)
            })
            .unwrap_or(pool[0])
    }

    /// Match a note to a chop by pitch proximity.
    fn match_by_pitch(
        &self,
        note: &Note,
        _chops: &[Chop],
        pitches: &[f32],
        pool: &[usize],
    ) -> usize {
        let pitched: Vec<usize> = pool.iter().copied().filter(|&i| pitches[i] > 0.0).collect();

        if note.pitch_hz > 0.0 && !pitched.is_empty() {
            pitched
                .iter()
                .copied()
                .min_by(|&a, &b| {
                    let da = (pitches[a] / note.pitch_hz).log2().abs();
                    let db = (pitches[b] / note.pitch_hz).log2().abs();
                    da.total_cmp(&db)
                })
                .unwrap_or(pool[0])
        } else {
            pool[0]
        }
    }

    /// Map notes to chops based on strength (JDilla) or pitch matching.
    /// Each chop is used once, then reused if more notes than chops.
    pub fn map_notes_to_chops(&self, notes: &[Note], chops: &[Chop]) -> Vec<usize> {
        if notes.is_empty() || chops.is_empty() {
            return vec![];
        }

        // Pre-compute all chop pitches once (avoid redundant analysis)
        let chop_pitches: Vec<f32> = chops.iter().map(|c| self.estimate_chop_pitch(c)).collect();

        let mut used = vec![false; chops.len()];
        let mut mappings: Vec<usize> = Vec::with_capacity(notes.len());

        for note in notes {
            // Build candidate list from unused chops
            let pool: Vec<usize> = (0..chops.len()).filter(|&i| !used[i]).collect();

            let pool = if pool.is_empty() {
                // All chops used; reset and start over
                used = vec![false; chops.len()];
                (0..chops.len()).collect::<Vec<_>>()
            } else {
                pool
            };

            let chosen = if self.config.strength_matching {
                self.match_by_strength(note, chops, &pool)
            } else {
                self.match_by_pitch(note, chops, &chop_pitches, &pool)
            };

            used[chosen] = true;
            mappings.push(chosen);
        }

        mappings
    }

    /// Simple linear interpolation resampling.
    fn linear_resample(&self, samples: &[f32], target_len: usize) -> Vec<f32> {
        if samples.is_empty() || target_len == 0 {
            return vec![];
        }
        if target_len == samples.len() {
            return samples.to_vec();
        }

        let ratio = (samples.len() - 1) as f64 / (target_len - 1).max(1) as f64;
        let mut result = Vec::with_capacity(target_len);

        for i in 0..target_len {
            let src_pos = i as f64 * ratio;
            let src_idx = src_pos as usize;
            let frac = (src_pos - src_idx as f64) as f32;

            if src_idx + 1 < samples.len() {
                result.push(samples[src_idx] * (1.0 - frac) + samples[src_idx + 1] * frac);
            } else if src_idx < samples.len() {
                result.push(samples[src_idx]);
            }
        }

        result
    }

    /// Apply pitch shift by semitones using high-quality sinc interpolation.
    /// Uses rubato SincFixedIn for band-limited resampling that prevents aliasing.
    ///
    /// Note: Double-resampling is intentional for JDilla-style chopping.
    /// The output must match the original chop length for proper sequencing.
    /// This is a trade-off between quality and consistent chop durations.
    pub fn apply_pitch_shift(&self, chop: &Chop, semitones: i32) -> Vec<f32> {
        if !self.config.enable_pitch_shift || semitones == 0 {
            return chop.samples.clone();
        }

        // Pitch shift ratio: 12 semitones = octave = 2x frequency
        let resample_ratio = 2.0_f64.powf(semitones as f64 / 12.0);

        // For pitch up (ratio > 1), we need to slow down the audio (more samples)
        // For pitch down (ratio < 1), we need to speed up the audio (fewer samples)
        // Then resample back to original length (required for JDilla-style chop sequencing)

        // First resample to target length (inverse of pitch shift)
        let target_len = (chop.samples.len() as f64 / resample_ratio).round() as usize;
        let resampled = self.high_quality_resample(&chop.samples, target_len);

        // Then resample back to original length (preserves JDilla chop timing)
        self.high_quality_resample(&resampled, chop.samples.len())
    }

    /// High-quality resampling using rubato SincFixedIn.
    /// Provides band-limited interpolation that prevents aliasing artifacts.
    fn high_quality_resample(&self, samples: &[f32], target_len: usize) -> Vec<f32> {
        if samples.is_empty() || target_len == 0 {
            return vec![];
        }
        if target_len == samples.len() {
            return samples.to_vec();
        }

        // Calculate resampling ratio
        let ratio = target_len as f64 / samples.len() as f64;

        // Clamp ratio to reasonable range (1/8x to 8x)
        let ratio = ratio.clamp(0.125, 8.0);

        // Use SincFixedIn for high-quality async resampling
        // Parameters chosen for good quality with reasonable performance
        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 256,
            window: WindowFunction::BlackmanHarris2,
        };

        // Use f64 for internal processing (rubato works best with f64)
        let input: Vec<f64> = samples.iter().map(|&s| s as f64).collect();

        // Create resampler with target ratio
        let chunk_size = 1024.min(input.len().max(1));
        let min_ratio = ratio * 0.5;

        let mut resampler = match SincFixedIn::<f64>::new(ratio, min_ratio, params, chunk_size, 1) {
            Ok(r) => r,
            Err(_) => return self.linear_resample(samples, target_len), // Fallback
        };

        // Process in chunks
        let waves_in = vec![input]; // Single channel
        match resampler.process(&waves_in, None) {
            Ok(waves_out) => {
                let output = &waves_out[0];
                // Convert back to f32
                let result: Vec<f32> = output.iter().map(|&s| s as f32).collect();

                // If we need exact length, resample again with linear
                if result.len() != target_len {
                    return self.linear_resample(&result, target_len);
                }
                result
            }
            Err(_) => self.linear_resample(samples, target_len), // Fallback
        }
    }

    /// Calculate semitone difference between two pitches.
    pub fn pitch_diff_semitones(&self, from_hz: f32, to_hz: f32) -> i32 {
        if from_hz <= 0.0 || to_hz <= 0.0 {
            return 0;
        }
        (12.0 * (to_hz / from_hz).log2()).round() as i32
    }

    /// Apply fade in/out to prevent click noise at boundaries.
    fn apply_fade(samples: &mut [f32], fade_samples: usize) {
        if samples.len() < 2 || fade_samples == 0 {
            return;
        }

        let fade_len = fade_samples.min(samples.len() / 4);
        let len = samples.len();

        // Apply fade in (attack): 0.0 → 1.0
        for (i, sample) in samples.iter_mut().enumerate().take(fade_len) {
            let gain = i as f32 / fade_len as f32;
            *sample *= gain;
        }

        // Fade out (release): 1.0 → 0.0 (BUG-6 fix: invert the gain direction)
        #[allow(clippy::needless_range_loop)]
        for i in 0..fade_len {
            let idx = len - 1 - i;
            let gain = 1.0 - (i as f32 / fade_len as f32); // descending: 1.0 → ~0.0
            samples[idx] *= gain;
        }
    }

    /// Apply velocity-based gain.
    pub fn apply_velocity_gain(&self, samples: &mut [f32], velocity: f32) {
        let gain = velocity.clamp(0.0, 1.0);
        for s in samples.iter_mut() {
            *s *= gain;
        }
    }

    /// Process a single note-to-chop mapping.
    /// In JDilla mode: chops keep their original length, velocity is applied.
    ///
    /// Duration mode: chops can be trimmed to note duration, looped to fill note,
    /// or played at full length (classic JDilla behavior).
    pub fn process_mapping(&self, note: &Note, chop: &Chop, output_onset: f64) -> MappedChop {
        // JDilla style: chops keep original length (no time stretch)
        // NOTE: We apply velocity gain only; full chop length is preserved.
        // For trimmed-to-note behavior, use process_mapping_trimmed() instead.
        let mut samples = chop.samples.clone();

        // Pitch shift if enabled
        if self.config.enable_pitch_shift {
            let chop_pitch = self.estimate_chop_pitch(chop);
            if chop_pitch > 0.0 && note.pitch_hz > 0.0 {
                let semitones = self.pitch_diff_semitones(chop_pitch, note.pitch_hz);
                if semitones != 0 {
                    samples = self.apply_pitch_shift(chop, semitones);
                }
            }
        }

        // Velocity
        self.apply_velocity_gain(&mut samples, note.velocity);

        // Apply fade to prevent click noise at boundaries
        // Fade length: ~5ms at 44100Hz = ~220 samples
        let fade_samples = (self.sample_rate as f64 * 0.005) as usize;
        Self::apply_fade(&mut samples, fade_samples);

        let output_duration = samples.len() as f64 / self.sample_rate as f64;
        MappedChop::new(samples, chop.index, output_onset, output_duration)
    }

    /// Process a single note-to-chop mapping with note-duration trimming.
    /// The chop is trimmed (or padded) to match the note's duration.
    /// Uses a short fade-out at the end to avoid clicks.
    ///
    /// BUG-2 fix: chop length is now trimmed to note.duration_sec
    /// with a clean fade-out, avoiding muddy overlaps.
    pub fn process_mapping_trimmed(
        &self,
        note: &Note,
        chop: &Chop,
        output_onset: f64,
    ) -> MappedChop {
        let target_samples = (note.duration_sec * self.sample_rate as f64) as usize;

        // Clone and apply pitch shift BEFORE trimming (keeps analysis pristine)
        let mut samples = if self.config.enable_pitch_shift {
            let chop_pitch = self.estimate_chop_pitch(chop);
            if chop_pitch > 0.0 && note.pitch_hz > 0.0 {
                let semitones = self.pitch_diff_semitones(chop_pitch, note.pitch_hz);
                if semitones != 0 {
                    chop.samples.clone() // pitch shift applied below
                } else {
                    chop.samples.clone()
                }
            } else {
                chop.samples.clone()
            }
        } else {
            chop.samples.clone()
        };

        // Apply pitch shift if enabled (on full chop, then trim)
        if self.config.enable_pitch_shift {
            let chop_pitch = self.estimate_chop_pitch(chop);
            if chop_pitch > 0.0 && note.pitch_hz > 0.0 {
                let semitones = self.pitch_diff_semitones(chop_pitch, note.pitch_hz);
                if semitones != 0 {
                    // Pitch shift to match note pitch
                    // For trimmed mode, shift directly to target length
                    let shifted =
                        self.apply_pitch_shift_raw(&chop.samples, note.pitch_hz / chop_pitch);
                    samples = shifted;
                }
            }
        }

        // Velocity gain
        self.apply_velocity_gain(&mut samples, note.velocity);

        // Trim or pad to target length
        let mut output_samples: Vec<f32>;
        if samples.len() > target_samples {
            // Trim with fade-out to avoid clicks
            output_samples = samples[..target_samples].to_vec();
            let fade_samples = 32.min(target_samples / 4);
            Self::apply_fade_out(&mut output_samples, fade_samples);
        } else if samples.len() < target_samples {
            // Pad with silence (note continues but sample ended)
            output_samples = samples.clone();
            output_samples.resize(target_samples, 0.0);
        } else {
            output_samples = samples;
        }

        // Apply fade in to avoid clicks at the start
        let fade_samples = 32.min(output_samples.len() / 4);
        Self::apply_fade_in(&mut output_samples, fade_samples);

        let output_duration = output_samples.len() as f64 / self.sample_rate as f64;
        MappedChop::new(output_samples, chop.index, output_onset, output_duration)
    }

    /// Apply pitch shift directly by pitch ratio (used by trimmed mode).
    fn apply_pitch_shift_raw(&self, samples: &[f32], pitch_ratio: f32) -> Vec<f32> {
        if pitch_ratio == 1.0 || pitch_ratio <= 0.0 {
            return samples.to_vec();
        }

        let target_len = (samples.len() as f64 / pitch_ratio as f64) as usize;
        let target_len = target_len.max(1);
        self.high_quality_resample(samples, target_len)
    }

    /// Apply a fade-in ramp (0.0 → 1.0) at the start of samples.
    fn apply_fade_in(samples: &mut [f32], fade_samples: usize) {
        if samples.len() < 2 || fade_samples == 0 {
            return;
        }
        let fade_len = fade_samples.min(samples.len() / 4);
        for (i, sample) in samples.iter_mut().enumerate().take(fade_len) {
            let gain = i as f32 / fade_len as f32;
            *sample *= gain;
        }
    }

    /// Apply a fade-out ramp (1.0 → 0.0) at the end of samples.
    fn apply_fade_out(samples: &mut [f32], fade_samples: usize) {
        if samples.len() < 2 || fade_samples == 0 {
            return;
        }
        let fade_len = fade_samples.min(samples.len() / 4);
        let len = samples.len();
        #[allow(clippy::needless_range_loop)]
        for i in 0..fade_len {
            let idx = len - 1 - i;
            // idx is always valid because i < fade_len <= len/4 <= len-1 when len>=2
            let gain = 1.0 - (i as f32 / fade_len as f32);
            samples[idx] *= gain;
        }
    }

    /// Process all notes and chops.
    /// Chops are placed at note onset times (BUG-1 fix: hum timing is now respected).
    pub fn process(&self, notes: &[Note], chops: &[Chop]) -> Result<Vec<MappedChop>, HumChopError> {
        if notes.is_empty() {
            return Err(HumChopError::InvalidAudio(
                "No notes to process".to_string(),
            ));
        }
        if chops.is_empty() {
            return Err(HumChopError::InvalidAudio(
                "No chops to process".to_string(),
            ));
        }

        let mappings = self.map_notes_to_chops(notes, chops);
        let mut mapped_chops: Vec<MappedChop> = Vec::with_capacity(notes.len());

        for (note_idx, &chop_idx) in mappings.iter().enumerate() {
            if chop_idx >= chops.len() {
                continue;
            }

            let note = &notes[note_idx];
            let chop = &chops[chop_idx];

            // BUG-1 fix: place chop at note's hummed onset time, not sequentially
            // The hummed rhythm now drives the playback timing in the output audio.
            let mapped = self.process_mapping(note, chop, note.onset_sec);
            mapped_chops.push(mapped);
        }

        Ok(mapped_chops)
    }

    /// Process all notes and chops with note-duration trimming.
    /// Chops are trimmed (or padded) to match note duration for cleaner output.
    ///
    /// BUG-1 + BUG-2 fix: places chops at note.onset_sec AND trims to note.duration_sec.
    pub fn process_trimmed(
        &self,
        notes: &[Note],
        chops: &[Chop],
    ) -> Result<Vec<MappedChop>, HumChopError> {
        if notes.is_empty() {
            return Err(HumChopError::InvalidAudio(
                "No notes to process".to_string(),
            ));
        }
        if chops.is_empty() {
            return Err(HumChopError::InvalidAudio(
                "No chops to process".to_string(),
            ));
        }

        let mappings = self.map_notes_to_chops(notes, chops);
        let mut mapped_chops: Vec<MappedChop> = Vec::with_capacity(notes.len());

        for (note_idx, &chop_idx) in mappings.iter().enumerate() {
            if chop_idx >= chops.len() {
                continue;
            }

            let note = &notes[note_idx];
            let chop = &chops[chop_idx];

            // BUG-1 fix: place at note onset time
            // BUG-2 fix: trim/pad to note duration
            let mapped = self.process_mapping_trimmed(note, chop, note.onset_sec);
            mapped_chops.push(mapped);
        }

        Ok(mapped_chops)
    }

    pub fn render_output(&self, mapped_chops: &[MappedChop]) -> Vec<f32> {
        if mapped_chops.is_empty() {
            return vec![];
        }

        // For crossfade mode, we need to process overlaps
        // For non-crossfade mode, simple placement with gaps
        if self.config.enable_crossfade && mapped_chops.len() > 1 {
            self.render_with_crossfade(mapped_chops)
        } else {
            self.render_simple(mapped_chops)
        }
    }

    /// Simple rendering without crossfade (original behavior with gaps).
    fn render_simple(&self, mapped_chops: &[MappedChop]) -> Vec<f32> {
        let total_samples = mapped_chops
            .iter()
            .map(|mc| (mc.output_onset * self.sample_rate as f64) as usize + mc.len())
            .max()
            .unwrap_or(0);

        let mut output = vec![0.0f32; total_samples];

        for mc in mapped_chops {
            let start = (mc.output_onset * self.sample_rate as f64) as usize;
            for (i, &sample) in mc.samples.iter().enumerate() {
                let idx = start + i;
                if idx < output.len() {
                    output[idx] += sample;
                }
            }
        }

        // Apply soft clipping or hard normalization
        if self.config.soft_clip {
            output = soft_knee_compress(&output, self.config.soft_clip_threshold_db);
        } else {
            let max_amp = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            if max_amp > 1.0 {
                let norm = 1.0 / max_amp;
                for s in output.iter_mut() {
                    *s *= norm;
                }
            }
        }

        output
    }

    /// Render with crossfade between overlapping chops.
    /// Uses sine-based (half-hann) crossfade envelopes for smooth transitions.
    ///
    /// BUG-3 fix: proper half-Hann crossfade using sample position within chop,
    /// ramp up at the head and ramp down at the tail.
    /// BUG-4 fix: overlap detection is removed (dead code, was never used).
    fn render_with_crossfade(&self, mapped_chops: &[MappedChop]) -> Vec<f32> {
        let crossfade_samples = self.config.crossfade_samples.min(1024).max(1);
        let sample_rate = self.sample_rate as f64;

        // Calculate total output length including overlaps
        let mut max_end = 0usize;
        for mc in mapped_chops {
            let start = (mc.output_onset * sample_rate) as usize;
            let end = start + mc.len();
            max_end = max_end.max(end);
        }

        let mut output = vec![0.0f32; max_end];
        let mut envelope = vec![0.0f32; max_end];

        for mc in mapped_chops {
            let start = (mc.output_onset * sample_rate) as usize;
            let end = start + mc.len().min(max_end.saturating_sub(start));

            for i in start..end {
                let local_idx = i - start;
                let chop_len = end - start;

                // BUG-3 fix: half-Hann crossfade at both edges of the chop
                // fade_in: ramp up from 0→1 in the first crossfade_samples
                // fade_out: ramp down from 1→0 in the last crossfade_samples
                // Multiply both envelopes (not min) so the weight is 1.0 in the
                // middle and ramps to 0 at both edges.
                let fade_in = if crossfade_samples > 0 {
                    let ramp = (local_idx as f32 / crossfade_samples as f32).min(1.0);
                    (std::f32::consts::PI * 0.5 * ramp).sin()
                } else {
                    1.0
                };

                let fade_out = if crossfade_samples > 0 {
                    let remaining = chop_len.saturating_sub(local_idx);
                    let ramp = (remaining as f32 / crossfade_samples as f32).min(1.0);
                    (std::f32::consts::PI * 0.5 * ramp).sin()
                } else {
                    1.0
                };

                // Multiply envelopes so weight is 1.0 in the middle
                let weight = fade_in * fade_out;

                if i < output.len() {
                    output[i] += mc.samples[i - start] * weight;
                    envelope[i] += weight;
                }
            }
        }

        // Normalize overlapping regions by envelope sum
        for i in 0..output.len() {
            if envelope[i] > 1.0 {
                output[i] /= envelope[i];
            }
        }

        // Apply soft clipping or hard normalization
        if self.config.soft_clip {
            output = soft_knee_compress(&output, self.config.soft_clip_threshold_db);
        } else {
            let max_amp = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
            if max_amp > 1.0 {
                let norm = 1.0 / max_amp;
                for s in output.iter_mut() {
                    *s *= norm;
                }
            }
        }

        output
    }

    /// Full render pipeline: chop → map → render.
    #[allow(dead_code)]
    pub fn render(
        &self,
        sample: &[f32],
        notes: &[Note],
        num_chops: usize,
    ) -> Result<Vec<f32>, HumChopError> {
        let chopper = SampleChopper::new(self.sample_rate);
        let chops = chopper.chop(sample, num_chops)?;
        let mapped_chops = self.process(notes, &chops)?;
        Ok(self.render_output(&mapped_chops))
    }
}

/// Simple resampling utility for sample rate conversion.
#[allow(dead_code)]
pub fn simple_resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() {
        return samples.to_vec();
    }

    let ratio = to_rate as f64 / from_rate as f64;
    let output_len = (samples.len() as f64 * ratio).ceil() as usize;
    let mut output = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let src_pos = i as f64 / ratio;
        let src_idx = src_pos as usize;
        if src_idx + 1 < samples.len() {
            let frac = (src_pos - src_idx as f64) as f32;
            output.push(samples[src_idx] * (1.0 - frac) + samples[src_idx + 1] * frac);
        } else if src_idx < samples.len() {
            output.push(samples[src_idx]);
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_sample(sample_rate: u32, duration_secs: f32) -> Vec<f32> {
        let num_samples = (sample_rate as f32 * duration_secs) as usize;
        (0..num_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5
            })
            .collect()
    }

    fn create_test_notes(count: usize) -> Vec<Note> {
        let note_duration = 0.2;
        let gap = 0.05;
        let base_pitch = 440.0;
        (0..count)
            .map(|i| {
                let pitch = base_pitch * (1.0 + i as f32 * 0.1);
                Note::new(pitch, i as f64 * (note_duration + gap), note_duration, 0.8)
            })
            .collect()
    }

    #[test]
    fn test_mapper_creation() {
        let mapper = Mapper::new(44100);
        assert_eq!(mapper.sample_rate, 44100);
    }

    #[test]
    fn test_mapper_with_options() {
        let mapper = Mapper::new(44100)
            .with_pitch_shift(true)
            .with_strength_matching(true);

        assert!(mapper.config.enable_pitch_shift);
        assert!(mapper.config.strength_matching);
    }

    #[allow(clippy::unwrap_used)]
    #[test]
    fn test_map_notes_to_chops() {
        let mapper = Mapper::new(44100);
        let chopper = SampleChopper::new(44100);
        let sample = create_test_sample(44100, 1.0);
        let chops = chopper.chop(&sample, 4).unwrap();
        let notes = create_test_notes(4);
        let mappings = mapper.map_notes_to_chops(&notes, &chops);
        assert_eq!(mappings.len(), notes.len());
    }

    #[test]
    fn test_pitch_diff_semitones() {
        let mapper = Mapper::new(44100);
        assert_eq!(mapper.pitch_diff_semitones(440.0, 880.0), 12);
        assert_eq!(mapper.pitch_diff_semitones(440.0, 220.0), -12);
        assert_eq!(mapper.pitch_diff_semitones(440.0, 440.0), 0);
    }

    #[test]
    fn test_apply_velocity_gain() {
        let mapper = Mapper::new(44100);
        let mut samples = vec![0.5f32; 4];
        mapper.apply_velocity_gain(&mut samples, 0.5);
        assert_eq!(samples, vec![0.25f32; 4]);
    }

    #[allow(clippy::unwrap_used)]
    #[test]
    fn test_process_empty_notes() {
        let mapper = Mapper::new(44100);
        let chopper = SampleChopper::new(44100);
        let sample = create_test_sample(44100, 1.0);
        let chops = chopper.chop(&sample, 4).unwrap();
        assert!(mapper.process(&[], &chops).is_err());
    }

    #[test]
    fn test_process_empty_chops() {
        let mapper = Mapper::new(44100);
        let notes = create_test_notes(4);
        assert!(mapper.process(&notes, &[]).is_err());
    }

    #[test]
    fn test_render_output() {
        let mapper = Mapper::new(44100);
        let mapped = vec![
            MappedChop::new(vec![0.5f32, 0.5], 0, 0.0, 1.0),
            MappedChop::new(vec![0.3f32, 0.3], 1, 0.5, 1.0),
        ];
        let output = mapper.render_output(&mapped);
        assert!(!output.is_empty());
    }

    #[allow(clippy::unwrap_used)]
    #[test]
    fn test_jdilla_keeps_original_length() {
        let mapper = Mapper::new(44100);
        let chopper = SampleChopper::new(44100);
        let sample = create_test_sample(44100, 1.0);
        let chops = chopper.chop(&sample, 4).unwrap();
        let notes = create_test_notes(4);

        let mapped_chops = mapper.process(&notes, &chops).unwrap();

        // In JDilla mode, mapped chops should have the same length as the chops they came from
        // (not time-stretched). Check that each mapped chop's length matches the
        // corresponding chop's length in the source chops.
        for mc in &mapped_chops {
            let source_chop = &chops[mc.chop_index];
            // Note: mapped chop length should equal source chop length in JDilla mode
            // (velocity is applied but doesn't change sample count)
            assert_eq!(
                mc.len(),
                source_chop.len(),
                "Mapped chop {} should keep length of source chop {}",
                mc.chop_index,
                mc.chop_index
            );
        }
    }

    #[test]
    fn test_simple_resample() {
        let samples = vec![0.0f32, 1.0, 0.0, -1.0, 0.0];
        let up = simple_resample(&samples, 44100, 88200);
        assert!(up.len() >= samples.len() * 2 - 1);
        let down = simple_resample(&samples, 88200, 44100);
        assert!(down.len() <= samples.len() / 2 + 1);
    }

    #[test]
    fn test_strength_matching() {
        let mapper = Mapper::new(44100).with_strength_matching(true);
        let sr = 44100;
        let s = vec![1.0f32; 4410]; // 0.1s of ones
        let mut c0 = Chop::new(s.clone(), 0, 0.0, sr);
        c0.strength = 0.9;
        let mut c1 = Chop::new(s.clone(), 1, 0.1, sr);
        c1.strength = 0.1;
        let chops = vec![c0, c1];

        let loud = Note::new(440.0, 0.0, 0.1, 0.9);
        assert_eq!(
            mapper.match_by_strength(&loud, &chops, &[0, 1]),
            0,
            "Loud note should match strong chop"
        );

        let soft = Note::new(440.0, 0.0, 0.1, 0.1);
        assert_eq!(
            mapper.match_by_strength(&soft, &chops, &[0, 1]),
            1,
            "Soft note should match weak chop"
        );
    }

    #[test]
    fn test_soft_knee_compress_empty() {
        let result = soft_knee_compress(&[], -1.0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_soft_knee_compress_no_clipping() {
        // Signal below threshold should pass through unchanged
        let samples = vec![0.3f32, 0.5, -0.4, 0.6];
        let result = soft_knee_compress(&samples, -1.0);

        // All samples should be within [-1, 1]
        for s in &result {
            assert!((*s).abs() <= 1.0, "Sample {} exceeds bounds", s);
        }
    }

    #[test]
    fn test_soft_knee_compress_reduces_peaks() {
        // Very loud signal that would clip
        let samples = vec![1.5f32, 1.8, -1.6, 2.0];
        let result = soft_knee_compress(&samples, -1.0);

        // After compression, max should be <= 1.0
        let max_amp = result.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max_amp <= 1.0, "Max amplitude {} should be <= 1.0", max_amp);

        // But relative dynamics should be preserved (not all normalized to same level)
        let min_abs = result.iter().map(|s| s.abs()).fold(f32::MAX, f32::min);
        assert!(min_abs < max_amp, "Dynamics should be preserved");
    }

    #[test]
    fn test_soft_clip_preserves_shape() {
        // Verify soft clipping doesn't completely eliminate peaks
        let samples: Vec<f32> = (0..1000).map(|i| (i as f32 / 100.0).sin() * 1.5).collect();
        let result = soft_knee_compress(&samples, -1.0);

        // Peak reduction should be less than 100% (soft, not hard clipping)
        let original_peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        let compressed_peak = result.iter().map(|s| s.abs()).fold(0.0f32, f32::max);

        // Soft clip should reduce but not eliminate the dynamics
        let reduction_ratio = compressed_peak / original_peak;
        assert!(
            reduction_ratio > 0.5,
            "Soft clip should preserve some dynamics"
        );
    }
}
