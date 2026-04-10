//! Sample Chopper - Split audio samples into chops with JDilla-style transient detection.
//!
//! Key features:
//! - Transient detection via RMS-energy derivative + spectral flux combined
//! - Variable chop lengths (short punchy hits coexist with long melodic tails)
//! - Chop strength scoring (strong transients = high-priority chops)
//! - Automatic fallback to energy-based splitting when transients are sparse
//! - JDilla-style boundary jitter for imperfect-feel

use crate::error::HumChopError;
use rustfft::{num_complex::Complex, FftPlanner};

// ─────────────────────────────────────────────────────────────
// Chop
// ─────────────────────────────────────────────────────────────

/// A single audio chop.
#[derive(Debug, Clone)]
pub struct Chop {
    /// Audio samples (mono, normalized f32 ±1.0)
    pub samples: Vec<f32>,
    /// Index in the chop list
    pub index: usize,
    /// Start position in the original sample (seconds)
    pub start_time: f64,
    /// Duration (seconds)
    pub duration: f64,
    /// Transient strength score (0.0 = silence, 1.0 = strongest hit).
    pub strength: f32,
}

impl Chop {
    pub fn new(samples: Vec<f32>, index: usize, start_time: f64, sample_rate: u32) -> Self {
        let duration = samples.len() as f64 / sample_rate as f64;
        Self {
            samples,
            index,
            start_time,
            duration,
            strength: 0.5,
        }
    }

    pub fn with_strength(mut self, s: f32) -> Self {
        self.strength = s.clamp(0.0, 1.0);
        self
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// Dilla chop configuration
// ─────────────────────────────────────────────────────────────

/// Parameters that control the JDilla-style chopper.
#[derive(Debug, Clone)]
pub struct DillaConfig {
    /// FFT window size for spectral analysis
    pub fft_window: usize,
    /// Hop between analysis frames (smaller = finer time resolution)
    pub hop_size: usize,

    // ── Transient detection ──────────────────────────────────
    /// Weight given to RMS-energy derivative vs. spectral flux (0.0–1.0).
    /// 1.0 = pure energy, 0.0 = pure spectral flux.
    pub energy_weight: f32,
    /// Onset threshold relative to the adaptive mean (multiplier).
    /// Lower = more chops; higher = fewer, stronger chops.
    pub threshold_factor: f32,
    /// Lookback window (frames) used to compute the adaptive threshold.
    pub adaptive_window: usize,

    // ── Chop length constraints ──────────────────────────────
    /// Shortest allowed chop (seconds). Shorter transients are merged forward.
    pub min_chop_secs: f64,
    /// Longest allowed chop (seconds). Longer regions are force-split.
    pub max_chop_secs: f64,

    // ── Humanise ────────────────────────────────────────────
    /// Maximum random jitter applied to each chop boundary (seconds).
    /// Set to 0.0 for perfectly quantised chops.
    pub boundary_jitter_secs: f64,
}

impl Default for DillaConfig {
    fn default() -> Self {
        Self {
            fft_window: 1024,
            hop_size: 256,               // ~5.8 ms at 44100 Hz — fine resolution
            energy_weight: 0.6,          // mostly energy-driven, some spectral colour
            threshold_factor: 1.4,       // 40% above local mean triggers onset
            adaptive_window: 20,         // ~116 ms lookahead
            min_chop_secs: 0.05,         // 50 ms minimum — keeps punchy hits intact
            max_chop_secs: 2.0,          // 2 s maximum — prevents over-long tails
            boundary_jitter_secs: 0.002, // ±2 ms JDilla-style "imperfect" grid
        }
    }
}

// ─────────────────────────────────────────────────────────────
// SampleChopper
// ─────────────────────────────────────────────────────────────

pub struct SampleChopper {
    dilla_config: DillaConfig,
    sample_rate: u32,
}

impl SampleChopper {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            dilla_config: DillaConfig::default(),
            sample_rate,
        }
    }

    pub fn with_dilla_config(mut self, cfg: DillaConfig) -> Self {
        self.dilla_config = cfg;
        self
    }

    /// Chop a sample using JDilla-style transient detection.
    /// This is now the only mode - JDilla-style is the normal mode.
    pub fn chop(&self, sample: &[f32], num_chops: usize) -> Result<Vec<Chop>, HumChopError> {
        self.chop_dilla(sample, num_chops)
    }

    // ── JDilla-style dynamic chopping ────────────────────────

    /// The core JDilla algorithm:
    ///
    /// 1. Compute a **combined onset strength curve** (RMS derivative + spectral flux).
    /// 2. Apply an **adaptive threshold** so it reacts to loud and quiet sections alike.
    /// 3. Pick boundary positions at detected transients.
    /// 4. Enforce min/max chop length constraints.
    /// 5. Apply tiny random **boundary jitter** for that imperfect-feel.
    /// 6. Score each chop by its transient strength (used later by the mapper).
    ///
    /// If fewer transients than needed are found, energy-based sub-splitting fills the gap.
    pub fn chop_dilla(&self, sample: &[f32], num_chops: usize) -> Result<Vec<Chop>, HumChopError> {
        if sample.is_empty() {
            return Err(HumChopError::InvalidAudio("Empty sample".into()));
        }
        if num_chops == 0 {
            return Err(HumChopError::SampleTooShort {
                sample_len: sample.len(),
                note_count: 0,
            });
        }
        if num_chops == 1 {
            return Ok(vec![
                Chop::new(sample.to_vec(), 0, 0.0, self.sample_rate).with_strength(rms(sample))
            ]);
        }

        let cfg = &self.dilla_config;
        let hop = cfg.hop_size;

        // ── Step 1: onset strength curve ────────────────────
        let (strength_curve, frame_positions) = self.onset_strength_curve(sample);

        // ── Step 2: adaptive threshold ───────────────────────
        let boundaries_sample =
            self.pick_transient_boundaries(&strength_curve, &frame_positions, sample.len());

        // ── Step 3: enforce min chop length, drop weak boundaries ─
        let min_samples = (cfg.min_chop_secs * self.sample_rate as f64) as usize;
        let mut filtered: Vec<usize> = Vec::new();
        let mut last = 0usize;
        for &b in &boundaries_sample {
            if b.saturating_sub(last) >= min_samples {
                filtered.push(b);
                last = b;
            }
        }

        // ── Step 4: if still too few, fill with energy-based splits ─
        while filtered.len() < num_chops - 1 {
            filtered = self.fill_with_energy_splits(sample, &filtered, num_chops - 1);
            if filtered.len() >= num_chops - 1 {
                break;
            }
            // Safety: fall back to equal if energy splits also fail
            if filtered.len() < 2 {
                return self.chop_equal_fallback(sample, num_chops);
            }
        }

        // Keep only the strongest num_chops-1 boundaries
        let selected = self.select_strongest_boundaries(
            &filtered,
            &strength_curve,
            &frame_positions,
            num_chops - 1,
            sample.len(),
        );

        // ── Step 5: apply boundary jitter ───────────────────
        let jitter_samples = (cfg.boundary_jitter_secs * self.sample_rate as f64) as isize;
        let jittered: Vec<usize> = selected
            .iter()
            .map(|&b| {
                if jitter_samples == 0 {
                    return b;
                }
                // Deterministic pseudo-random jitter based on position
                // Use a simple hash that won't overflow
                let hash = (b.wrapping_mul(0x5bd1e995).wrapping_add(1190494759))
                    .rotate_left(13)
                    .wrapping_mul(0x85ebca6b);
                let jitter_range = jitter_samples.unsigned_abs().max(1) as usize;
                let jitter = ((hash as usize) % jitter_range) as isize;
                (b as isize + jitter).clamp(0, (sample.len().saturating_sub(1)) as isize) as usize
            })
            .collect();

        // ── Step 6: build chops with strength scores ─────────
        let mut chops = self.boundaries_to_chops(sample, &jittered)?;

        // Attach strength scores
        for chop in chops.iter_mut() {
            let frame_idx = self.time_to_frame(chop.start_time, hop);
            let s = strength_curve.get(frame_idx).copied().unwrap_or(0.0);
            chop.strength = s;
        }

        // Normalise strengths to [0,1]
        let max_s = chops.iter().map(|c| c.strength).fold(0.0f32, f32::max);
        if max_s > 0.0 {
            for c in chops.iter_mut() {
                c.strength /= max_s;
            }
        }

        Ok(chops)
    }

    // ── Fallback equal division ──────────────────────────────

    fn chop_equal_fallback(
        &self,
        sample: &[f32],
        num_chops: usize,
    ) -> Result<Vec<Chop>, HumChopError> {
        let chop_len = sample.len() / num_chops;
        if chop_len == 0 {
            return Err(HumChopError::SampleTooShort {
                sample_len: sample.len(),
                note_count: num_chops,
            });
        }

        let chops = (0..num_chops)
            .map(|i| {
                let start = i * chop_len;
                let end = if i == num_chops - 1 {
                    sample.len()
                } else {
                    start + chop_len
                };
                let start_time = start as f64 / self.sample_rate as f64;
                Chop::new(sample[start..end].to_vec(), i, start_time, self.sample_rate)
            })
            .collect();

        Ok(chops)
    }

    // ─────────────────────────────────────────────────────────
    // Internal helpers — onset strength
    // ─────────────────────────────────────────────────────────

    /// Returns (strength_curve, frame_start_sample_positions).
    fn onset_strength_curve(&self, sample: &[f32]) -> (Vec<f32>, Vec<usize>) {
        let cfg = &self.dilla_config;
        let win = cfg.fft_window;
        let hop = cfg.hop_size;

        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(win);

        let mut curve: Vec<f32> = Vec::new();
        let mut positions: Vec<usize> = Vec::new();

        let mut prev_mag = vec![0.0f32; win / 2];
        let mut prev_rms = 0.0f32;

        for frame_start in (0..sample.len().saturating_sub(win)).step_by(hop) {
            let frame = &sample[frame_start..frame_start + win];

            // RMS energy
            let cur_rms: f32 = (frame.iter().map(|s| s * s).sum::<f32>() / win as f32).sqrt();
            let rms_deriv = (cur_rms - prev_rms).max(0.0); // half-wave rectified
            prev_rms = cur_rms;

            // Spectral flux (positive only)
            let mut buf: Vec<Complex<f32>> = frame
                .iter()
                .enumerate()
                .map(|(i, &s)| {
                    let w =
                        0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / win as f32).cos());
                    Complex::new(s * w, 0.0)
                })
                .collect();
            fft.process(&mut buf);

            let mags: Vec<f32> = buf[..win / 2]
                .iter()
                .map(|c| (c.re * c.re + c.im * c.im).sqrt())
                .collect();

            let flux: f32 = mags
                .iter()
                .zip(prev_mag.iter())
                .map(|(m, p)| (m - p).max(0.0))
                .sum::<f32>()
                / (win / 2) as f32;
            prev_mag = mags;

            // Weighted combination
            let combined = cfg.energy_weight * rms_deriv + (1.0 - cfg.energy_weight) * flux;
            curve.push(combined);
            positions.push(frame_start);
        }

        (curve, positions)
    }

    /// Pick sample positions where the onset strength exceeds the adaptive threshold.
    fn pick_transient_boundaries(
        &self,
        curve: &[f32],
        positions: &[usize],
        _total_samples: usize,
    ) -> Vec<usize> {
        let cfg = &self.dilla_config;
        let win = cfg.adaptive_window;
        let min_frame_gap = {
            let min_s = (cfg.min_chop_secs * self.sample_rate as f64) as usize;
            (min_s / cfg.hop_size).max(1)
        };

        let mut boundaries: Vec<usize> = Vec::new();
        let mut last_onset_frame = 0isize;

        for i in 0..curve.len() {
            // Local adaptive mean over the lookback window
            let lo = i.saturating_sub(win);
            let local_mean = curve[lo..=i].iter().sum::<f32>() / (i - lo + 1) as f32;
            let threshold = local_mean * cfg.threshold_factor;

            let gap = i as isize - last_onset_frame;
            if curve[i] > threshold && gap >= min_frame_gap as isize {
                boundaries.push(positions[i]);
                last_onset_frame = i as isize;
            }
        }

        boundaries
    }

    /// For gaps larger than max_chop_secs, insert energy-weighted sub-split points.
    fn fill_with_energy_splits(
        &self,
        sample: &[f32],
        existing: &[usize],
        target_count: usize,
    ) -> Vec<usize> {
        let max_samples = (self.dilla_config.max_chop_secs * self.sample_rate as f64) as usize;

        let mut all: Vec<usize> = existing.to_vec();
        let boundaries: Vec<usize> = std::iter::once(0)
            .chain(existing.iter().copied())
            .chain(std::iter::once(sample.len()))
            .collect();

        for window in boundaries.windows(2) {
            let (a, b) = (window[0], window[1]);
            if b - a > max_samples && all.len() < target_count {
                // Find the sample of peak absolute energy in this region as the split
                let region = &sample[a..b];
                let split_offset = Self::peak_rms_position(region, 512);
                let split = a + split_offset;
                if split > a && split < b {
                    all.push(split);
                }
            }
        }

        all.sort();
        all.dedup();
        all
    }

    /// Given a list of candidate boundary positions, keep only the `n` with the
    /// highest onset strength, then re-sort them by position.
    fn select_strongest_boundaries(
        &self,
        candidates: &[usize],
        curve: &[f32],
        positions: &[usize],
        n: usize,
        _total_samples: usize,
    ) -> Vec<usize> {
        let hop = self.dilla_config.hop_size;
        let mut scored: Vec<(usize, f32)> = candidates
            .iter()
            .map(|&pos| {
                let frame = self.time_to_frame(pos as f64 / self.sample_rate as f64, hop);
                let s = curve.get(frame).copied().unwrap_or(0.0);
                (pos, s)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut selected: Vec<usize> = scored.iter().take(n).map(|x| x.0).collect();
        selected.sort();
        selected
    }

    /// Convert a list of sorted boundary sample-positions into Chop objects.
    fn boundaries_to_chops(
        &self,
        sample: &[f32],
        boundaries: &[usize],
    ) -> Result<Vec<Chop>, HumChopError> {
        if boundaries.is_empty() {
            return Ok(vec![Chop::new(sample.to_vec(), 0, 0.0, self.sample_rate)]);
        }

        let points: Vec<usize> = std::iter::once(0)
            .chain(boundaries.iter().copied())
            .chain(std::iter::once(sample.len()))
            .collect();

        let chops = points
            .windows(2)
            .enumerate()
            .filter(|(_, w)| w[1] > w[0])
            .map(|(i, w)| {
                let start_time = w[0] as f64 / self.sample_rate as f64;
                Chop::new(sample[w[0]..w[1]].to_vec(), i, start_time, self.sample_rate)
            })
            .collect();

        Ok(chops)
    }

    /// Find the frame index of the sample with the highest frame-RMS.
    /// Used to pick energy-weighted sub-split positions.
    fn peak_rms_position(region: &[f32], frame_size: usize) -> usize {
        if region.len() <= frame_size {
            return region.len() / 2;
        }

        let (best_start, _) = (0..region.len() - frame_size)
            .step_by(frame_size / 2)
            .map(|s| {
                let r = rms(&region[s..s + frame_size]);
                (s + frame_size / 2, r)
            })
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or((region.len() / 2, 0.0));

        best_start
    }

    fn time_to_frame(&self, time_secs: f64, hop: usize) -> usize {
        ((time_secs * self.sample_rate as f64) as usize) / hop.max(1)
    }

    /// Get the total duration of chops.
    pub fn total_duration(&self, chops: &[Chop]) -> f64 {
        chops.iter().map(|c| c.duration).sum()
    }
}

// ─────────────────────────────────────────────────────────────
// Free helpers
// ─────────────────────────────────────────────────────────────

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

// ─────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, sr: u32, dur: f32) -> Vec<f32> {
        let n = (sr as f32 * dur) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin() * 0.5)
            .collect()
    }

    /// Synthetic drum-like signal: silence + impulse + decay, repeated.
    fn drum_loop(sr: u32) -> Vec<f32> {
        let beat = (sr as f32 * 0.5) as usize; // one beat = 0.5 s
        let mut out = vec![0.0f32; beat * 4];
        for beat_idx in 0..4 {
            let onset = beat_idx * beat;
            // sharp impulse
            out[onset] = 1.0;
            // exponential decay
            for j in 1..(beat / 2) {
                let v = (-8.0 * j as f32 / sr as f32).exp();
                out[onset + j] = v;
            }
        }
        out
    }

    #[test]
    fn test_chop_empty_error() {
        let chopper = SampleChopper::new(44100);
        assert!(chopper.chop(&[], 4).is_err());
    }

    #[test]
    fn test_chop_zero_chops_error() {
        let chopper = SampleChopper::new(44100);
        let sample = sine(440.0, 44100, 1.0);
        assert!(chopper.chop(&sample, 0).is_err());
    }

    #[test]
    fn test_chop_single() {
        let chopper = SampleChopper::new(44100);
        let sample = sine(440.0, 44100, 1.0);
        let chops = chopper.chop(&sample, 1).unwrap();
        assert_eq!(chops.len(), 1);
        assert_eq!(chops[0].len(), sample.len());
    }

    #[test]
    fn test_dilla_produces_correct_count() {
        let chopper = SampleChopper::new(44100);
        let drums = drum_loop(44100); // 2 s loop with 4 obvious beats
        let chops = chopper.chop(&drums, 4).unwrap();
        assert_eq!(chops.len(), 4, "Expected 4 chops, got {}", chops.len());
    }

    #[test]
    fn test_dilla_chops_cover_full_sample() {
        let chopper = SampleChopper::new(44100);
        let drums = drum_loop(44100);
        let chops = chopper.chop(&drums, 4).unwrap();
        let total: usize = chops.iter().map(|c| c.len()).sum();
        assert_eq!(total, drums.len(), "Chops should cover every sample");
    }

    #[test]
    fn test_dilla_chop_lengths_are_variable() {
        let chopper = SampleChopper::new(44100);
        let drums = drum_loop(44100);
        let chops = chopper.chop(&drums, 4).unwrap();
        let lengths: Vec<usize> = chops.iter().map(|c| c.len()).collect();
        // At least two chops should differ in length (dynamic, not static)
        let all_equal = lengths.windows(2).all(|w| w[0] == w[1]);
        assert!(
            !all_equal,
            "JDilla chops should have variable lengths: {:?}",
            lengths
        );
    }

    #[test]
    fn test_dilla_strength_scores_in_range() {
        let chopper = SampleChopper::new(44100);
        let drums = drum_loop(44100);
        let chops = chopper.chop(&drums, 4).unwrap();
        for c in &chops {
            assert!(
                (0.0..=1.0).contains(&c.strength),
                "Strength out of range: {}",
                c.strength
            );
        }
    }

    #[test]
    fn test_dilla_min_chop_length_respected() {
        let chopper = SampleChopper::new(44100);
        let drums = drum_loop(44100);
        let chops = chopper.chop(&drums, 4).unwrap();
        let min_samples = (chopper.dilla_config.min_chop_secs * 44100.0) as usize;
        for c in &chops {
            assert!(
                c.len() >= min_samples / 2, // allow some tolerance for edge chops
                "Chop too short: {} samples (min {})",
                c.len(),
                min_samples
            );
        }
    }

    #[test]
    fn test_chop_indices_sequential() {
        let chopper = SampleChopper::new(44100);
        let sample = sine(440.0, 44100, 1.0);
        let chops = chopper.chop(&sample, 3).unwrap();
        for (i, chop) in chops.iter().enumerate() {
            assert_eq!(chop.index, i);
        }
    }
}
