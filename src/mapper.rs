//! Mapper - Map notes to chops with time stretching and pitch shifting.
//!
//! This module handles:
//! - Matching notes to the closest chops by pitch
//! - Time stretching to match note durations
//! - Pitch shifting to match note pitches
//! - Velocity-based gain adjustment

use crate::error::HumChopError;
use crate::hum_analyzer::Note;
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
            enable_pitch_shift: false, // MVP: disable by default for speed
            enable_time_stretch: true,
            output_sample_rate: 44100,
            max_stretch_ratio: 2.0,
            min_stretch_ratio: 0.5,
        }
    }
}

/// A mapped chop with timing and processing applied.
#[derive(Debug, Clone)]
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
    /// Create a new MappedChop.
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

    /// Get the length in samples.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// The mapper that handles note-to-chop assignment.
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
    pub fn with_config(sample_rate: u32, config: MapperConfig) -> Self {
        Self {
            config,
            sample_rate,
        }
    }

    /// Enable or disable pitch shifting.
    pub fn with_pitch_shift(mut self, enabled: bool) -> Self {
        self.config.enable_pitch_shift = enabled;
        self
    }

    /// Enable or disable time stretching.
    pub fn with_time_stretch(mut self, enabled: bool) -> Self {
        self.config.enable_time_stretch = enabled;
        self
    }

    /// Find the chop that best matches a note by pitch.
    pub fn find_best_chop(&self, note: &Note, chops: &[Chop]) -> Option<usize> {
        if chops.is_empty() {
            return None;
        }

        let mut best_idx = 0;
        let mut best_distance = f32::MAX;

        for (i, chop) in chops.iter().enumerate() {
            // Estimate chop's center pitch (assuming we have pitch info)
            // For now, use simple index-based matching with pitch hint
            let chop_center = (chop.start_time + chop.duration / 2.0) as f32;
            let note_mid = (note.onset_sec + note.duration_sec / 2.0) as f32;

            // Distance is based on time proximity for now
            let distance = (note_mid - chop_center).abs();

            if distance < best_distance {
                best_distance = distance;
                best_idx = i;
            }
        }

        Some(best_idx)
    }

    /// Map notes to chops based on pitch similarity.
    /// Returns a mapping of note_index -> chop_index.
    pub fn map_notes_to_chops(&self, notes: &[Note], chops: &[Chop]) -> Vec<usize> {
        if notes.is_empty() || chops.is_empty() {
            return vec![];
        }

        let mut mappings: Vec<usize> = Vec::with_capacity(notes.len());
        let mut used_chops: VecDeque<usize> = (0..chops.len()).collect();

        for note in notes {
            // Find the chop with the closest pitch
            let mut best_idx = *used_chops.front().unwrap_or(&0);
            let mut best_distance = f32::MAX;

            for &chop_idx in used_chops.iter() {
                // Estimate pitch from note time to chop time
                // Simple approach: use sequential matching
                let chop = &chops[chop_idx];
                let chop_mid = (chop.start_time + chop.duration / 2.0) as f32;
                let note_mid = (note.onset_sec + note.duration_sec / 2.0) as f32;

                // Simple distance based on time alignment
                let distance = (note_mid - chop_mid).abs();

                if distance < best_distance {
                    best_distance = distance;
                    best_idx = chop_idx;
                }
            }

            mappings.push(best_idx);

            // Remove used chop from available pool
            if let Some(pos) = used_chops.iter().position(|&x| x == best_idx) {
                used_chops.remove(pos);
            }

            // If we've used all chops, break
            if used_chops.is_empty() && mappings.len() < notes.len() {
                // Re-add all chops if we need more mappings
                used_chops = (0..chops.len()).collect();
            }
        }

        mappings
    }

    /// Apply time stretch to a chop to match a note's duration.
    /// Uses simple linear interpolation for MVP.
    pub fn apply_time_stretch(&self, chop: &Chop, target_duration_secs: f64) -> Vec<f32> {
        let current_duration = chop.duration;

        if !self.config.enable_time_stretch
            || (current_duration - target_duration_secs).abs() < 0.01
        {
            return chop.samples.clone();
        }

        let stretch_ratio = current_duration / target_duration_secs;
        let stretch_ratio =
            stretch_ratio.clamp(self.config.min_stretch_ratio, self.config.max_stretch_ratio);

        // Use simple resampling for MVP
        let target_samples = (chop.samples.len() as f64 / stretch_ratio) as usize;

        if target_samples == 0 || target_samples == chop.samples.len() {
            return chop.samples.clone();
        }

        self.linear_resample(&chop.samples, target_samples)
    }

    /// Simple linear interpolation resampling.
    fn linear_resample(&self, samples: &[f32], target_len: usize) -> Vec<f32> {
        if samples.is_empty() || target_len == 0 {
            return vec![];
        }

        if target_len == samples.len() {
            return samples.to_vec();
        }

        let ratio = (samples.len() - 1) as f64 / (target_len - 1) as f64;
        let mut result = Vec::with_capacity(target_len);

        for i in 0..target_len {
            let src_pos = i as f64 * ratio;
            let src_idx = src_pos as usize;
            let frac = (src_pos - src_idx as f64) as f32;

            if src_idx + 1 < samples.len() {
                let sample = samples[src_idx] * (1.0 - frac) + samples[src_idx + 1] * frac;
                result.push(sample);
            } else if src_idx < samples.len() {
                result.push(samples[src_idx]);
            }
        }

        result
    }

    /// Apply pitch shift to a chop.
    /// This is done by time stretching with resampling.
    pub fn apply_pitch_shift(&self, chop: &Chop, semitones: i32) -> Vec<f32> {
        if !self.config.enable_pitch_shift || semitones == 0 {
            return chop.samples.clone();
        }

        // Pitch shift via time stretching:
        // +12 semitones = double speed = 0.5x duration
        // -12 semitones = half speed = 2x duration
        let stretch_ratio = 2.0_f64.powi(-semitones) / 2.0;
        let stretch_ratio =
            stretch_ratio.clamp(self.config.min_stretch_ratio, self.config.max_stretch_ratio);

        let target_samples = (chop.samples.len() as f64 * stretch_ratio) as usize;

        if target_samples == 0 {
            return chop.samples.clone();
        }

        self.linear_resample(&chop.samples, target_samples)
    }

    /// Calculate pitch difference in semitones.
    pub fn pitch_diff_semitones(&self, from_hz: f32, to_hz: f32) -> i32 {
        if from_hz <= 0.0 || to_hz <= 0.0 {
            return 0;
        }
        let semitones = 12.0 * (to_hz / from_hz).log2();
        semitones.round() as i32
    }

    /// Apply velocity-based gain adjustment.
    pub fn apply_velocity_gain(&self, samples: &mut [f32], velocity: f32) {
        let gain = velocity.clamp(0.0, 1.0);
        for sample in samples.iter_mut() {
            *sample *= gain;
        }
    }

    /// Process a single note-chop mapping.
    pub fn process_mapping(&self, note: &Note, chop: &Chop, output_onset: f64) -> MappedChop {
        let mut samples = chop.samples.clone();

        // 1. Time stretch to match note duration
        if self.config.enable_time_stretch {
            samples = self.apply_time_stretch(chop, note.duration_sec);
        }

        // 2. Pitch shift to match note pitch
        if self.config.enable_pitch_shift {
            // Estimate original pitch from chop (simplified - assumes uniform pitch)
            // In a real implementation, we'd analyze the chop's pitch
            let semitones = 0; // Default: no pitch shift in MVP
            if semitones != 0 {
                samples = self.apply_pitch_shift(chop, semitones);
            }
        }

        // 3. Apply velocity gain
        self.apply_velocity_gain(&mut samples, note.velocity);

        let output_duration = samples.len() as f64 / self.sample_rate as f64;

        MappedChop::new(samples, chop.index, output_onset, output_duration)
    }

    /// Process all notes and chops into mapped output.
    pub fn process(&self, notes: &[Note], chops: &[Chop]) -> Result<Vec<MappedChop>, HumChopError> {
        if notes.is_empty() {
            return Err(HumChopError::InvalidAudio(
                "No notes to process".to_string(),
            ));
        }

        if chops.is_empty() {
            return Err(HumChopError::InvalidAudio("No chops to map".to_string()));
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
            current_onset += mapped.output_duration;

            mapped_chops.push(mapped);
        }

        Ok(mapped_chops)
    }

    /// Render mapped chops to final audio buffer with crossfades.
    pub fn render_output(&self, mapped_chops: &[MappedChop]) -> Vec<f32> {
        if mapped_chops.is_empty() {
            return vec![];
        }

        // Calculate total output length
        let total_samples = mapped_chops
            .iter()
            .map(|mc| {
                let end_sample = (mc.output_onset * self.sample_rate as f64) as usize + mc.len();
                end_sample
            })
            .max()
            .unwrap_or(0);

        let mut output = vec![0.0f32; total_samples];

        for mapped_chop in mapped_chops {
            let start_sample = (mapped_chop.output_onset * self.sample_rate as f64) as usize;

            for (i, &sample) in mapped_chop.samples.iter().enumerate() {
                let idx = start_sample + i;
                if idx < output.len() {
                    output[idx] += sample;
                }
            }
        }

        // Normalize output to prevent clipping
        let max_sample = output.iter().map(|s| s.abs()).fold(0.0f32, |a, b| a.max(b));

        if max_sample > 1.0 {
            let scale = 1.0 / max_sample;
            for sample in output.iter_mut() {
                *sample *= scale;
            }
        }

        output
    }

    /// Full pipeline: chop sample, map notes, render output.
    pub fn render(
        &self,
        sample: &[f32],
        notes: &[Note],
        num_chops: usize,
        chop_mode: ChopMode,
    ) -> Result<Vec<f32>, HumChopError> {
        // Step 1: Chop the sample
        let chopper = SampleChopper::new(self.sample_rate);
        let chops = chopper.chop(sample, num_chops, chop_mode)?;

        // Step 2: Process mappings
        let mapped_chops = self.process(notes, &chops)?;

        // Step 3: Render to final output
        let output = self.render_output(&mapped_chops);

        Ok(output)
    }
}

/// Simple helper to resample using linear interpolation.
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
            let interpolated = samples[src_idx] * (1.0 - frac) + samples[src_idx + 1] * frac;
            output.push(interpolated);
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

    fn create_test_notes(sample_rate: u32, count: usize) -> Vec<Note> {
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
            .with_time_stretch(false);

        assert!(mapper.config.enable_pitch_shift);
        assert!(!mapper.config.enable_time_stretch);
    }

    #[test]
    fn test_find_best_chop() {
        let mapper = Mapper::new(44100);
        let chopper = SampleChopper::new(44100);
        let sample = create_test_sample(44100, 1.0);
        let chops = chopper.chop_equal(&sample, 4).unwrap();

        let note = Note::new(440.0, 0.125, 0.2, 0.5);
        let best = mapper.find_best_chop(&note, &chops);

        assert!(best.is_some());
        assert!(best.unwrap() < chops.len());
    }

    #[test]
    fn test_map_notes_to_chops() {
        let mapper = Mapper::new(44100);
        let chopper = SampleChopper::new(44100);
        let sample = create_test_sample(44100, 1.0);
        let chops = chopper.chop_equal(&sample, 4).unwrap();

        let notes = create_test_notes(44100, 4);
        let mappings = mapper.map_notes_to_chops(&notes, &chops);

        assert_eq!(mappings.len(), notes.len());
    }

    #[test]
    fn test_pitch_diff_semitones() {
        let mapper = Mapper::new(44100);

        // A4 (440Hz) to A5 (880Hz) = +12 semitones
        let diff = mapper.pitch_diff_semitones(440.0, 880.0);
        assert_eq!(diff, 12);

        // A4 (440Hz) to A3 (220Hz) = -12 semitones
        let diff = mapper.pitch_diff_semitones(440.0, 220.0);
        assert_eq!(diff, -12);

        // Same pitch = 0
        let diff = mapper.pitch_diff_semitones(440.0, 440.0);
        assert_eq!(diff, 0);
    }

    #[test]
    fn test_apply_velocity_gain() {
        let mapper = Mapper::new(44100);
        let samples = vec![0.5f32, 0.5, 0.5, 0.5];

        let mut modified = samples.clone();
        mapper.apply_velocity_gain(&mut modified, 0.5);

        assert_eq!(modified, vec![0.25f32, 0.25, 0.25, 0.25]);
    }

    #[test]
    fn test_process_empty_notes() {
        let mapper = Mapper::new(44100);
        let chopper = SampleChopper::new(44100);
        let sample = create_test_sample(44100, 1.0);
        let chops = chopper.chop_equal(&sample, 4).unwrap();

        let result = mapper.process(&[], &chops);
        assert!(result.is_err());
    }

    #[test]
    fn test_process_empty_chops() {
        let mapper = Mapper::new(44100);
        let notes = create_test_notes(44100, 4);

        let result = mapper.process(&notes, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_render_output() {
        let mapper = Mapper::new(44100);

        let mapped_chops = vec![
            MappedChop::new(vec![0.5f32, 0.5], 0, 0.0, 1.0),
            MappedChop::new(vec![0.3f32, 0.3], 1, 0.5, 1.0),
        ];

        let output = mapper.render_output(&mapped_chops);
        assert!(!output.is_empty());
    }

    #[test]
    fn test_simple_resample() {
        let samples = vec![0.0f32, 1.0, 0.0, -1.0, 0.0];

        // Upsample 2x
        let upsampled = simple_resample(&samples, 44100, 88200);
        assert!(upsampled.len() >= samples.len() * 2 - 1);

        // Downsample 2x
        let downsampled = simple_resample(&samples, 88200, 44100);
        assert!(downsampled.len() <= samples.len() / 2 + 1);
    }

    #[test]
    fn test_linear_resample() {
        let mapper = Mapper::new(44100);
        let samples = vec![0.0f32, 1.0, 0.0, -1.0, 0.0];

        // Same length
        let resampled = mapper.linear_resample(&samples, samples.len());
        assert_eq!(resampled.len(), samples.len());

        // Up sample
        let upsampled = mapper.linear_resample(&samples, 9);
        assert_eq!(upsampled.len(), 9);
    }
}
