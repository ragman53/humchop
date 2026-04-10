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
}

impl Default for MapperConfig {
    fn default() -> Self {
        Self {
            enable_pitch_shift: false,
            output_sample_rate: 44100,
            crossfade_samples: 256,
            strength_matching: true, // JDilla style - match by strength
        }
    }
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

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// Mapper
// ─────────────────────────────────────────────────────────────

pub struct Mapper {
    config: MapperConfig,
    sample_rate: u32,
}

impl Mapper {
    /// Create a new Mapper with default settings.
    pub fn new(sample_rate: u32) -> Self {
        Self {
            config: MapperConfig::default(),
            sample_rate,
        }
    }

    /// Create with custom configuration.
    #[allow(dead_code)]
    pub fn with_config(sample_rate: u32, config: MapperConfig) -> Self {
        Self {
            config,
            sample_rate,
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

    /// Estimate the dominant pitch of a chop using HumAnalyzer.
    /// Returns 0.0 if no clear pitch detected (e.g., percussion).
    pub fn estimate_chop_pitch(&self, chop: &Chop) -> f32 {
        let analyzer = HumAnalyzer::new(self.sample_rate);
        let pitches = analyzer.detect_pitch(&chop.samples);

        // Filter out invalid pitches
        let valid: Vec<f32> = pitches.into_iter().filter(|&p| p > 0.0).collect();
        if valid.is_empty() {
            return 0.0; // No pitch detected (percussion)
        }

        // Use median for robustness against outliers
        let mut sorted = valid.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
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
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
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
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
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

    /// Apply pitch shift by semitones.
    pub fn apply_pitch_shift(&self, chop: &Chop, semitones: i32) -> Vec<f32> {
        if !self.config.enable_pitch_shift || semitones == 0 {
            return chop.samples.clone();
        }

        // Resample: pitch up = shorter, pitch down = longer
        let resample_ratio = 2.0_f64.powf(semitones as f64 / 12.0);
        let resampled_len = (chop.samples.len() as f64 / resample_ratio).round() as usize;
        let resampled = self.linear_resample(&chop.samples, resampled_len);

        // Re-stretch to original length to maintain timing
        self.linear_resample(&resampled, chop.samples.len())
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

        // Fade in (attack)
        for i in 0..fade_len {
            let gain = i as f32 / fade_len as f32;
            samples[i] *= gain;
        }

        // Fade out (release)
        for i in 0..fade_len {
            let idx = samples.len() - 1 - i;
            let gain = i as f32 / fade_len as f32;
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
    pub fn process_mapping(&self, note: &Note, chop: &Chop, output_onset: f64) -> MappedChop {
        // JDilla style: chops keep original length (no time stretch)
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

    /// Process all notes and chops.
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
        let mut current_onset = 0.0;

        for (note_idx, &chop_idx) in mappings.iter().enumerate() {
            if chop_idx >= chops.len() {
                continue;
            }

            let note = &notes[note_idx];
            let chop = &chops[chop_idx];

            let mapped = self.process_mapping(note, chop, current_onset);

            // JDilla mode: place chops back-to-back with small gaps to prevent clicks
            let gap = 0.005; // 5ms gap
            current_onset += mapped.output_duration + gap;

            mapped_chops.push(mapped);
        }

        Ok(mapped_chops)
    }

    /// Render mapped chops to final audio output.
    pub fn render_output(&self, mapped_chops: &[MappedChop]) -> Vec<f32> {
        if mapped_chops.is_empty() {
            return vec![];
        }

        // Calculate total output length
        let total_samples = mapped_chops
            .iter()
            .map(|mc| (mc.output_onset * self.sample_rate as f64) as usize + mc.len())
            .max()
            .unwrap_or(0);

        let mut output = vec![0.0f32; total_samples];

        // Place each chop at its output position
        for mc in mapped_chops {
            let start = (mc.output_onset * self.sample_rate as f64) as usize;

            for (i, &sample) in mc.samples.iter().enumerate() {
                let idx = start + i;
                if idx < output.len() {
                    output[idx] += sample;
                }
            }
        }

        // Normalize
        let max_amp = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if max_amp > 1.0 {
            for s in output.iter_mut() {
                *s /= max_amp;
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
}
