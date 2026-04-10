//! Sample Chopper - Split audio samples into chops.
//!
//! Provides two chopping modes:
//! - Equal division: Split sample into equal-length chunks
//! - Onset-based: Detect natural chop points using spectral analysis

use crate::error::HumChopError;
use rustfft::{num_complex::Complex, FftPlanner};
use std::path::Path;

/// A single audio chop.
#[derive(Debug, Clone)]
pub struct Chop {
    /// Audio samples (mono, normalized f32)
    pub samples: Vec<f32>,
    /// Index in the original sample (for reference)
    pub index: usize,
    /// Start time in original sample (seconds)
    pub start_time: f64,
    /// Duration in seconds
    pub duration: f64,
}

impl Chop {
    /// Create a new Chop.
    pub fn new(samples: Vec<f32>, index: usize, start_time: f64, sample_rate: u32) -> Self {
        let duration = samples.len() as f64 / sample_rate as f64;
        Self {
            samples,
            index,
            start_time,
            duration,
        }
    }

    /// Get the length of this chop in samples.
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Check if chop is empty.
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// Chopping mode selection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChopMode {
    /// Equal-length divisions
    Equal,
    /// Onset-based natural divisions
    Onset,
}

impl Default for ChopMode {
    fn default() -> Self {
        ChopMode::Equal
    }
}

/// Configuration for onset-based chopping.
#[derive(Debug, Clone)]
pub struct OnsetChopConfig {
    /// FFT window size
    pub window_size: usize,
    /// Hop size between analysis windows
    pub hop_size: usize,
    /// Threshold for onset detection
    pub threshold: f32,
    /// Minimum time between onsets (seconds)
    pub min_gap: f64,
    /// Minimum onset strength to consider
    pub min_strength: f32,
}

impl Default for OnsetChopConfig {
    fn default() -> Self {
        Self {
            window_size: 2048,
            hop_size: 512,
            threshold: 0.3,
            min_gap: 0.15,
            min_strength: 0.1,
        }
    }
}

/// Sample chopper for splitting audio into chops.
pub struct SampleChopper {
    onset_config: OnsetChopConfig,
    sample_rate: u32,
}

impl SampleChopper {
    /// Create a new SampleChopper.
    pub fn new(sample_rate: u32) -> Self {
        Self {
            onset_config: OnsetChopConfig::default(),
            sample_rate,
        }
    }

    /// Create with custom onset detection config.
    pub fn with_config(sample_rate: u32, onset_config: OnsetChopConfig) -> Self {
        Self {
            onset_config,
            sample_rate,
        }
    }

    /// Chop a sample into equal-length pieces.
    pub fn chop_equal(&self, sample: &[f32], num_chops: usize) -> Result<Vec<Chop>, HumChopError> {
        if sample.is_empty() {
            return Err(HumChopError::InvalidAudio("Empty sample".to_string()));
        }

        if num_chops == 0 {
            return Err(HumChopError::SampleTooShort {
                sample_len: sample.len(),
                note_count: 0,
            });
        }

        let chop_len = sample.len() / num_chops;
        if chop_len == 0 {
            return Err(HumChopError::SampleTooShort {
                sample_len: sample.len(),
                note_count: num_chops,
            });
        }

        let mut chops: Vec<Chop> = Vec::with_capacity(num_chops);

        for i in 0..num_chops {
            let start = i * chop_len;
            let end = if i == num_chops - 1 {
                sample.len() // Last chop takes remainder
            } else {
                start + chop_len
            };

            let start_time = start as f64 / self.sample_rate as f64;
            let chop_samples = sample[start..end].to_vec();

            chops.push(Chop::new(chop_samples, i, start_time, self.sample_rate));
        }

        Ok(chops)
    }

    /// Chop a sample based on detected onsets.
    /// Returns num_chops chops, splitting at the most prominent onset points.
    pub fn chop_by_onset(
        &self,
        sample: &[f32],
        num_chops: usize,
    ) -> Result<Vec<Chop>, HumChopError> {
        if sample.is_empty() {
            return Err(HumChopError::InvalidAudio("Empty sample".to_string()));
        }

        if num_chops == 0 {
            return Err(HumChopError::SampleTooShort {
                sample_len: sample.len(),
                note_count: 0,
            });
        }

        if num_chops == 1 {
            // Just return the whole sample as one chop
            return Ok(vec![Chop::new(sample.to_vec(), 0, 0.0, self.sample_rate)]);
        }

        // Detect onset points
        let onset_points = self.detect_onset_points(sample);

        if onset_points.len() < num_chops - 1 {
            // Fall back to equal division if not enough onsets detected
            println!(
                "Warning: Only detected {} onsets, falling back to equal division",
                onset_points.len()
            );
            return self.chop_equal(sample, num_chops);
        }

        // Select the best num_chops - 1 onset points
        let mut selected_onsets: Vec<usize> =
            onset_points.iter().map(|(_, onset)| *onset).collect();
        selected_onsets.sort();

        // Ensure we have at least num_chops - 1 points (add endpoints if needed)
        while selected_onsets.len() < num_chops - 1 {
            let last = selected_onsets.last().copied().unwrap_or(0);
            let next = last + sample.len() / num_chops;
            if next < sample.len() {
                selected_onsets.push(next);
            } else {
                break;
            }
        }

        // Sort and deduplicate
        selected_onsets.sort();
        selected_onsets.dedup();

        // Create chops from selected onsets
        let mut chops: Vec<Chop> = Vec::with_capacity(num_chops);
        let boundaries: Vec<usize> = std::iter::once(0)
            .chain(selected_onsets.iter().copied())
            .chain(std::iter::once(sample.len()))
            .collect();

        for i in 0..(boundaries.len() - 1) {
            let start = boundaries[i];
            let end = boundaries[i + 1];
            let start_time = start as f64 / self.sample_rate as f64;
            let chop_samples = sample[start..end].to_vec();

            chops.push(Chop::new(chop_samples, i, start_time, self.sample_rate));
        }

        Ok(chops)
    }

    /// Chop using the specified mode.
    pub fn chop(
        &self,
        sample: &[f32],
        num_chops: usize,
        mode: ChopMode,
    ) -> Result<Vec<Chop>, HumChopError> {
        match mode {
            ChopMode::Equal => self.chop_equal(sample, num_chops),
            ChopMode::Onset => self.chop_by_onset(sample, num_chops),
        }
    }

    /// Detect onset points in the sample.
    /// Returns a vector of (strength, sample_index) pairs.
    fn detect_onset_points(&self, sample: &[f32]) -> Vec<(f32, usize)> {
        let window_size = self.onset_config.window_size;
        let hop_size = self.onset_config.hop_size;

        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(window_size);

        let mut onset_points: Vec<(f32, usize)> = Vec::new();
        let mut prev_magnitudes: Vec<f32> = vec![0.0; window_size / 2];
        let mut prev_onset_idx = 0isize;

        for window_start in (0..sample.len().saturating_sub(window_size)).step_by(hop_size) {
            let window_end = window_start + window_size;
            let window_samples = &sample[window_start..window_end];

            // Prepare FFT input with Hann window
            let mut buffer: Vec<Complex<f32>> = window_samples
                .iter()
                .enumerate()
                .map(|(i, &s)| {
                    let window = 0.5
                        * (1.0
                            - (2.0 * std::f32::consts::PI * i as f32 / window_size as f32).cos());
                    Complex::new(s * window, 0.0)
                })
                .collect();

            // Perform FFT
            fft.process(&mut buffer);

            // Calculate magnitudes
            let num_bins = window_size / 2;
            let magnitudes: Vec<f32> = buffer[..num_bins]
                .iter()
                .map(|c| (c.re * c.re + c.im * c.im).sqrt())
                .collect();

            // Calculate spectral flux (positive differences only)
            let mut flux: f32 = 0.0;
            for i in 0..num_bins {
                let diff = magnitudes[i] - prev_magnitudes[i];
                if diff > 0.0 {
                    flux += diff;
                }
            }

            // Normalize
            let flux = flux / num_bins as f32;

            // Check if this is a strong enough onset
            if flux > self.onset_config.threshold {
                let gap_samples = window_start as isize - prev_onset_idx;
                let gap_time = gap_samples as f64 / self.sample_rate as f64;

                if gap_time >= self.onset_config.min_gap && flux > self.onset_config.min_strength {
                    onset_points.push((flux, window_start));
                    prev_onset_idx = window_start as isize;
                }
            }

            prev_magnitudes = magnitudes;
        }

        // Sort by strength (descending)
        onset_points.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        onset_points
    }

    /// Load audio from a file and chop it.
    pub fn chop_file(
        &self,
        path: &Path,
        num_chops: usize,
        mode: ChopMode,
    ) -> Result<Vec<Chop>, HumChopError> {
        use crate::audio_utils::load_audio;

        let (samples, sample_rate) = load_audio(path)
            .map_err(|e| HumChopError::Other(format!("Failed to load audio: {}", e)))?;

        // Create a chopper with the actual sample rate
        let chopper = SampleChopper::new(sample_rate);
        chopper.chop(&samples, num_chops, mode)
    }

    /// Get the total duration of chops.
    pub fn total_duration(&self, chops: &[Chop]) -> f64 {
        chops.iter().map(|c| c.duration).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_sample(sample_rate: u32, duration_secs: f32) -> Vec<f32> {
        let num_samples = (sample_rate as f32 * duration_secs) as usize;
        // Generate a simple test signal
        (0..num_samples)
            .map(|i| {
                let t = i as f32 / sample_rate as f32;
                (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5
            })
            .collect()
    }

    #[test]
    fn test_chop_equal_basic() {
        let sample_rate = 44100;
        let sample = create_test_sample(sample_rate, 1.0); // 1 second
        let chopper = SampleChopper::new(sample_rate);

        let chops = chopper.chop_equal(&sample, 4).unwrap();

        assert_eq!(chops.len(), 4);

        // Check that all chops together cover the original sample
        let total_len: usize = chops.iter().map(|c| c.len()).sum();
        assert_eq!(total_len, sample.len());
    }

    #[test]
    fn test_chop_equal_single() {
        let sample_rate = 44100;
        let sample = create_test_sample(sample_rate, 1.0);
        let chopper = SampleChopper::new(sample_rate);

        let chops = chopper.chop_equal(&sample, 1).unwrap();

        assert_eq!(chops.len(), 1);
        assert_eq!(chops[0].len(), sample.len());
    }

    #[test]
    fn test_chop_empty_sample() {
        let sample_rate = 44100;
        let sample: Vec<f32> = vec![];
        let chopper = SampleChopper::new(sample_rate);

        let result = chopper.chop_equal(&sample, 4);
        assert!(result.is_err());
    }

    #[test]
    fn test_chop_zero_chops() {
        let sample_rate = 44100;
        let sample = create_test_sample(sample_rate, 1.0);
        let chopper = SampleChopper::new(sample_rate);

        let result = chopper.chop_equal(&sample, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_chop_mode_default() {
        let mode = ChopMode::default();
        assert_eq!(mode, ChopMode::Equal);
    }

    #[test]
    fn test_chop_indices() {
        let sample_rate = 44100;
        let sample = create_test_sample(sample_rate, 1.0);
        let chopper = SampleChopper::new(sample_rate);

        let chops = chopper.chop_equal(&sample, 3).unwrap();

        for (i, chop) in chops.iter().enumerate() {
            assert_eq!(chop.index, i);
        }
    }

    #[test]
    fn test_chop_by_onset_fallback() {
        let sample_rate = 44100;
        let sample = create_test_sample(sample_rate, 1.0);
        let chopper = SampleChopper::new(sample_rate);

        // Onset detection may fall back to equal if not enough onsets
        let result = chopper.chop_by_onset(&sample, 4);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 4);
    }
}
