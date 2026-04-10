//! Mapper - Map notes to chops with time stretching and pitch shifting.
//!
//! This module handles:
//! - Matching notes to the closest chops by pitch
//! - Time stretching to match note durations
//! - Pitch shifting to match note pitches
//! - Velocity-based gain adjustment

use crate::error::HumChopError;
use crate::hum_analyzer::{HumAnalyzer, Note};
use crate::sample_chopper::{Chop, ChopMode, SampleChopper};
use std::collections::VecDeque;

/// Configuration for the mapper.
#[derive(Debug, Clone)]
pub struct MapperConfig {
    /// Enable pitch shifting (can be computationally expensive)
    pub enable_pitch_shift: bool,
    /// Enable time stretching
    pub enable_time_stretch: bool,
    /// Output sample rate
    pub output_sample_rate: u32,
    /// Maximum time stretch ratio (1.0 = no stretch, 2.0 = double length)
    pub max_stretch_ratio: f64,
    /// Minimum time stretch ratio
    pub min_stretch_ratio: f64,
}

impl Default for MapperConfig {
    fn default() -> Self {
        Self {
            enable_pitch_shift: false,
            enable_time_stretch: true,
            output_sample_rate: 44100,
            max_stretch_ratio: 4.0,
            min_stretch_ratio: 0.25,
        }
    }
}

/// A mapped chop with timing and processing applied.
#[derive(Debug, Clone)]
pub struct MappedChop {
    pub samples: Vec<f32>,
    pub chop_index: usize,
    pub output_onset: f64,
    pub output_duration: f64,
}

impl MappedChop {
    pub fn new(samples: Vec<f32>, chop_index: usize, output_onset: f64, output_duration: f64) -> Self {
        Self { samples, chop_index, output_onset, output_duration }
    }

    pub fn len(&self) -> usize { self.samples.len() }
    pub fn is_empty(&self) -> bool { self.samples.is_empty() }
}

/// The mapper that handles note-to-chop assignment.
pub struct Mapper {
    config: MapperConfig,
    sample_rate: u32,
}

impl Mapper {
    pub fn new(sample_rate: u32) -> Self {
        Self { config: MapperConfig::default(), sample_rate }
    }

    pub fn with_config(sample_rate: u32, config: MapperConfig) -> Self {
        Self { config, sample_rate }
    }

    pub fn with_pitch_shift(mut self, enabled: bool) -> Self {
        self.config.enable_pitch_shift = enabled;
        self
    }

    pub fn with_time_stretch(mut self, enabled: bool) -> Self {
        self.config.enable_time_stretch = enabled;
        self
    }

    /// Estimate the dominant pitch of a chop using HumAnalyzer.
    fn estimate_chop_pitch(&self, chop: &Chop) -> f32 {
        let analyzer = HumAnalyzer::new(self.sample_rate);
        let pitches = analyzer.detect_pitch(&chop.samples);

        let valid: Vec<f32> = pitches.into_iter().filter(|&p| p > 0.0).collect();
        if valid.is_empty() {
            return 0.0;
        }

        // Use median for robustness
        let mut sorted = valid.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        sorted[sorted.len() / 2]
    }

    /// Find the chop whose estimated pitch is closest to the note's pitch.
    /// Falls back to index-based sequential matching if no chop has a detected pitch.
    pub fn find_best_chop(&self, note: &Note, chops: &[Chop], used: &[bool]) -> Option<usize> {
        if chops.is_empty() {
            return None;
        }

        // Collect available chops with their estimated pitches
        let candidates: Vec<(usize, f32)> = chops
            .iter()
            .enumerate()
            .filter(|(i, _)| !used[*i])
            .map(|(i, chop)| {
                let pitch = self.estimate_chop_pitch(chop);
                (i, pitch)
            })
            .collect();

        if candidates.is_empty() {
            // All used; pick sequential fallback
            return Some(chops.len() % chops.len());
        }

        // Prefer pitch-based matching if the note has a valid pitch
        // and at least one chop has a detected pitch
        let has_pitch_info = candidates.iter().any(|(_, p)| *p > 0.0);

        if note.pitch_hz > 0.0 && has_pitch_info {
            // Match by minimum pitch distance in semitones
            let best = candidates
                .iter()
                .filter(|(_, p)| *p > 0.0)
                .min_by(|(_, p_a), (_, p_b)| {
                    let dist_a = ((*p_a / note.pitch_hz).log2().abs());
                    let dist_b = ((*p_b / note.pitch_hz).log2().abs());
                    dist_a.partial_cmp(&dist_b).unwrap_or(std::cmp::Ordering::Equal)
                });

            if let Some(&(idx, _)) = best {
                return Some(idx);
            }
        }

        // Fallback: use first available (sequential order)
        candidates.first().map(|(i, _)| *i)
    }

    /// Map notes to chops, prioritizing pitch matching.
    /// Each chop is used at most once; if notes > chops, chops are reused.
    pub fn map_notes_to_chops(&self, notes: &[Note], chops: &[Chop]) -> Vec<usize> {
        if notes.is_empty() || chops.is_empty() {
            return vec![];
        }

        // Pre-compute all chop pitches once (avoid redundant analysis)
        let chop_pitches: Vec<f32> = chops
            .iter()
            .map(|c| self.estimate_chop_pitch(c))
            .collect();

        let mut used = vec![false; chops.len()];
        let mut mappings: Vec<usize> = Vec::with_capacity(notes.len());

        for note in notes {
            // Build candidate list from unused chops
            let candidates: Vec<(usize, f32)> = chops
                .iter()
                .enumerate()
                .filter(|(i, _)| !used[*i])
                .map(|(i, _)| (i, chop_pitches[i]))
                .collect();

            let chosen = if candidates.is_empty() {
                // All chops used; reset and reuse
                used = vec![false; chops.len()];
                let all: Vec<(usize, f32)> = chops
                    .iter()
                    .enumerate()
                    .map(|(i, _)| (i, chop_pitches[i]))
                    .collect();
                Self::best_pitch_match(note, &all)
            } else {
                Self::best_pitch_match(note, &candidates)
            };

            used[chosen] = true;
            mappings.push(chosen);
        }

        mappings
    }

    /// Select the index of the candidate whose pitch is closest to the note.
    /// Falls back to sequential (first candidate) if no pitch info is available.
    fn best_pitch_match(note: &Note, candidates: &[(usize, f32)]) -> usize {
        let has_pitch = candidates.iter().any(|(_, p)| *p > 0.0);

        if note.pitch_hz > 0.0 && has_pitch {
            candidates
                .iter()
                .filter(|(_, p)| *p > 0.0)
                .min_by(|(_, p_a), (_, p_b)| {
                    let da = (*p_a / note.pitch_hz).log2().abs();
                    let db = (*p_b / note.pitch_hz).log2().abs();
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| *i)
                .unwrap_or(candidates[0].0)
        } else {
            candidates[0].0
        }
    }

    /// Apply time stretch to a chop so it matches `target_duration_secs`.
    ///
    /// `stretch_ratio = target / current`:
    ///   > 1.0 → stretch (output is longer)
    ///   < 1.0 → compress (output is shorter)
    pub fn apply_time_stretch(&self, chop: &Chop, target_duration_secs: f64) -> Vec<f32> {
        let current_duration = chop.duration;

        if !self.config.enable_time_stretch
            || (current_duration - target_duration_secs).abs() < 0.005
            || current_duration <= 0.0
            || target_duration_secs <= 0.0
        {
            return chop.samples.clone();
        }

        // ratio > 1 means we want MORE samples (slower / longer output)
        let ratio = target_duration_secs / current_duration;
        let ratio = ratio.clamp(self.config.min_stretch_ratio, self.config.max_stretch_ratio);

        let target_samples = (chop.samples.len() as f64 * ratio).round() as usize;

        if target_samples == 0 || target_samples == chop.samples.len() {
            return chop.samples.clone();
        }

        self.linear_resample(&chop.samples, target_samples)
    }

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

    pub fn apply_pitch_shift(&self, chop: &Chop, semitones: i32) -> Vec<f32> {
        if !self.config.enable_pitch_shift || semitones == 0 {
            return chop.samples.clone();
        }

        // Pitch shift: resample by 2^(semitones/12), then re-stretch to original length
        let resample_ratio = 2.0_f64.powf(semitones as f64 / 12.0);
        let resampled_len = (chop.samples.len() as f64 / resample_ratio).round() as usize;
        let resampled = self.linear_resample(&chop.samples, resampled_len);

        // Re-stretch to original length to keep duration constant
        self.linear_resample(&resampled, chop.samples.len())
    }

    pub fn pitch_diff_semitones(&self, from_hz: f32, to_hz: f32) -> i32 {
        if from_hz <= 0.0 || to_hz <= 0.0 { return 0; }
        (12.0 * (to_hz / from_hz).log2()).round() as i32
    }

    pub fn apply_velocity_gain(&self, samples: &mut [f32], velocity: f32) {
        let gain = velocity.clamp(0.0, 1.0);
        for s in samples.iter_mut() { *s *= gain; }
    }

    pub fn process_mapping(&self, note: &Note, chop: &Chop, output_onset: f64) -> MappedChop {
        // 1. Time stretch to match note duration
        let mut samples = if self.config.enable_time_stretch {
            self.apply_time_stretch(chop, note.duration_sec)
        } else {
            chop.samples.clone()
        };

        // 2. Pitch shift if enabled
        if self.config.enable_pitch_shift {
            let chop_pitch = self.estimate_chop_pitch(chop);
            if chop_pitch > 0.0 && note.pitch_hz > 0.0 {
                let semitones = self.pitch_diff_semitones(chop_pitch, note.pitch_hz);
                if semitones != 0 {
                    // Build a temporary chop with already-stretched samples
                    let temp_chop = Chop::new(samples.clone(), chop.index, chop.start_time, self.sample_rate);
                    samples = self.apply_pitch_shift(&temp_chop, semitones);
                }
            }
        }

        // 3. Velocity gain
        self.apply_velocity_gain(&mut samples, note.velocity);

        let output_duration = samples.len() as f64 / self.sample_rate as f64;
        MappedChop::new(samples, chop.index, output_onset, output_duration)
    }

    pub fn process(&self, notes: &[Note], chops: &[Chop]) -> Result<Vec<MappedChop>, HumChopError> {
        if notes.is_empty() {
            return Err(HumChopError::InvalidAudio("No notes to process".to_string()));
        }
        if chops.is_empty() {
            return Err(HumChopError::InvalidAudio("No chops to map".to_string()));
        }

        let mappings = self.map_notes_to_chops(notes, chops);
        let mut mapped_chops: Vec<MappedChop> = Vec::with_capacity(notes.len());
        let mut current_onset = 0.0;

        for (note_idx, &chop_idx) in mappings.iter().enumerate() {
            if chop_idx >= chops.len() { continue; }
            let note = &notes[note_idx];
            let chop = &chops[chop_idx];

            let mapped = self.process_mapping(note, chop, current_onset);
            current_onset += mapped.output_duration;
            mapped_chops.push(mapped);
        }

        Ok(mapped_chops)
    }

    pub fn render_output(&self, mapped_chops: &[MappedChop]) -> Vec<f32> {
        if mapped_chops.is_empty() { return vec![]; }

        let total_samples = mapped_chops
            .iter()
            .map(|mc| (mc.output_onset * self.sample_rate as f64) as usize + mc.len())
            .max()
            .unwrap_or(0);

        let mut output = vec![0.0f32; total_samples];

        for mc in mapped_chops {
            let start = (mc.output_onset * self.sample_rate as f64) as usize;
            for (i, &s) in mc.samples.iter().enumerate() {
                let idx = start + i;
                if idx < output.len() { output[idx] += s; }
            }
        }

        // Peak normalize
        let max = output.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if max > 1.0 {
            for s in output.iter_mut() { *s /= max; }
        }

        output
    }

    pub fn render(
        &self,
        sample: &[f32],
        notes: &[Note],
        num_chops: usize,
        chop_mode: ChopMode,
    ) -> Result<Vec<f32>, HumChopError> {
        let chopper = SampleChopper::new(self.sample_rate);
        let chops = chopper.chop(sample, num_chops, chop_mode)?;
        let mapped_chops = self.process(notes, &chops)?;
        Ok(self.render_output(&mapped_chops))
    }
}

pub fn simple_resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || samples.is_empty() { return samples.to_vec(); }

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
    use crate::sample_chopper::SampleChopper;

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
        let mapper = Mapper::new(44100).with_pitch_shift(true).with_time_stretch(false);
        assert!(mapper.config.enable_pitch_shift);
        assert!(!mapper.config.enable_time_stretch);
    }

    #[test]
    fn test_map_notes_to_chops() {
        let mapper = Mapper::new(44100);
        let chopper = SampleChopper::new(44100);
        let sample = create_test_sample(44100, 1.0);
        let chops = chopper.chop_equal(&sample, 4).unwrap();
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
    fn test_time_stretch_longer() {
        let mapper = Mapper::new(44100);
        let sample = create_test_sample(44100, 0.25); // 0.25s
        let chop = Chop::new(sample.clone(), 0, 0.0, 44100);
        // Stretch to 0.5s → should be ~2x longer
        let stretched = mapper.apply_time_stretch(&chop, 0.5);
        assert!(stretched.len() > sample.len());
    }

    #[test]
    fn test_time_stretch_shorter() {
        let mapper = Mapper::new(44100);
        let sample = create_test_sample(44100, 0.5); // 0.5s
        let chop = Chop::new(sample.clone(), 0, 0.0, 44100);
        // Compress to 0.25s → should be ~2x shorter
        let compressed = mapper.apply_time_stretch(&chop, 0.25);
        assert!(compressed.len() < sample.len());
    }

    #[test]
    fn test_process_empty_notes() {
        let mapper = Mapper::new(44100);
        let chopper = SampleChopper::new(44100);
        let sample = create_test_sample(44100, 1.0);
        let chops = chopper.chop_equal(&sample, 4).unwrap();
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
    fn test_simple_resample() {
        let samples = vec![0.0f32, 1.0, 0.0, -1.0, 0.0];
        let up = simple_resample(&samples, 44100, 88200);
        assert!(up.len() >= samples.len() * 2 - 1);
        let down = simple_resample(&samples, 88200, 44100);
        assert!(down.len() <= samples.len() / 2 + 1);
    }
}
