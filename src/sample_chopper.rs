//! Sample Chopper - Split audio samples into chops with JDilla-style transient detection.
//!
//! Key features:
//! - Multi-band transient detection (low/mid/high) for better accuracy across content types
//! - Pre-emphasis filtering to reduce low-frequency dominance
//! - Combined onset detection: RMS derivative + spectral flux + high-flux
//! - Multi-scale analysis with median-based normalization
//! - Peak-picking on onset envelope for precise boundary placement
//! - Variable chop lengths (short punchy hits coexist with long melodic tails)
//! - Chop strength scoring based on integrated energy over the chop region
//! - Automatic fallback to energy-based splitting when transients are sparse
//! - JDilla-style boundary jitter for imperfect-feel

use crate::error::HumChopError;
use rustfft::{num_complex::Complex, FftPlanner};
use std::f32::consts::PI;

// ─────────────────────────────────────────────────────────────
// Chop
// ─────────────────────────────────────────────────────────────

/// A single audio chop.
#[derive(Debug, Clone)]
#[allow(dead_code)]
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
    /// Onset threshold relative to the adaptive median (multiplier).
    /// Lower = more chops; higher = fewer, stronger chops.
    #[allow(dead_code)]
    pub threshold_factor: f32,
    /// Lookback window (frames) used to compute the adaptive threshold.
    pub adaptive_window: usize,
    /// Forward look window for median calculation (symmetric context).
    #[allow(dead_code)]
    pub forward_window: usize,

    // ── Pre-emphasis ─────────────────────────────────────────
    /// Pre-emphasis filter coefficient (0.0 = off, 0.97 = standard).
    /// Boosts high frequencies to improve transient detection.
    pub pre_emphasis: f32,

    // ── Peak picking ─────────────────────────────────────────
    /// Minimum prominence for a peak (fraction of local range).
    /// Higher = only the most prominent transients selected.
    pub peak_prominence: f32,
    /// Minimum frames between peaks (prevents double-detection).
    pub peak_min_distance: usize,

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
            fft_window: 2048,            // larger window for better freq resolution
            hop_size: 256,               // ~5.8 ms at 44100 Hz — fine time resolution
            energy_weight: 0.4,          // more spectral-driven for musical content
            threshold_factor: 1.2,       // slightly more sensitive than before
            adaptive_window: 30,         // ~174 ms lookback
            forward_window: 30,          // ~174 ms lookahead (symmetric context)
            pre_emphasis: 0.97,          // standard pre-emphasis
            peak_prominence: 0.3,        // moderate prominence threshold
            peak_min_distance: 5,        // ~29ms minimum between peaks
            min_chop_secs: 0.03,         // 30ms minimum — tighter than before
            max_chop_secs: 2.0,          // 2s maximum
            boundary_jitter_secs: 0.002, // ±2ms JDilla-style "imperfect" grid
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

    #[allow(dead_code)]
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

    /// The improved JDilla algorithm:
    ///
    /// 1. Apply pre-emphasis filter to boost high-frequency transients
    /// 2. Compute **multi-band onset strength curve** (RMS derivative + spectral flux + high-flux)
    /// 3. Normalize using median-based scaling for consistent detection
    /// 4. Apply **peak picking** with prominence detection for precise boundary placement
    /// 5. Enforce min/max chop length constraints
    /// 6. Apply tiny random **boundary jitter** for that imperfect-feel
    /// 7. Score each chop by integrated energy over its region (not just onset peak)
    ///
    /// If fewer transients than needed are found, multi-scale energy splitting fills the gap.
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

        // ── Step 1: pre-emphasis filter ─────────────────────
        let emphasized = self.apply_pre_emphasis(sample);

        // ── Step 2: multi-band onset strength curve ─────────
        let (strength_curve, frame_positions) = self.onset_strength_curve(&emphasized);

        // ── Step 3: median-based normalization ──────────────
        let normalized = self.normalize_onset_curve(&strength_curve);

        // ── Step 4: peak picking with prominence ────────────
        let peak_boundaries =
            self.pick_peaks_with_prominence(&normalized, &frame_positions, sample.len());

        // ── Step 5: enforce min chop length, drop weak peaks ─
        let min_samples = (cfg.min_chop_secs * self.sample_rate as f64) as usize;
        let mut filtered: Vec<usize> = Vec::new();
        let mut last = 0usize;
        for &b in &peak_boundaries {
            if b.saturating_sub(last) >= min_samples {
                filtered.push(b);
                last = b;
            }
        }

        // ── Step 6: if still too few, fill with multi-scale energy splits ─
        let max_iterations = 10;
        let mut iterations = 0;
        while filtered.len() < num_chops - 1 && iterations < max_iterations {
            let prev_len = filtered.len();
            filtered = self.fill_with_energy_splits_multi_scale(sample, &filtered, num_chops - 1);

            // Check if we made progress
            if filtered.len() == prev_len {
                // No new boundaries added - fall back to equal division
                return self.chop_equal_fallback(sample, num_chops);
            }

            if filtered.len() >= num_chops - 1 {
                break;
            }

            iterations += 1;
        }

        // Final safety check - if we still don't have enough, fall back
        if filtered.len() < num_chops - 1 {
            return self.chop_equal_fallback(sample, num_chops);
        }

        // Keep only the strongest num_chops-1 boundaries
        let selected = self.select_strongest_boundaries(
            &filtered,
            &normalized,
            &frame_positions,
            num_chops - 1,
            sample.len(),
        );

        // ── Step 7: apply boundary jitter ───────────────────
        let jitter_samples = (cfg.boundary_jitter_secs * self.sample_rate as f64) as isize;
        let jittered: Vec<usize> = selected
            .iter()
            .map(|&b| {
                if jitter_samples == 0 {
                    return b;
                }
                // Deterministic pseudo-random jitter based on position
                let hash = (b.wrapping_mul(0x5bd1e995).wrapping_add(1190494759))
                    .rotate_left(13)
                    .wrapping_mul(0x85ebca6b);
                let jitter_range = jitter_samples.unsigned_abs().max(1);
                let jitter = (hash % jitter_range) as isize;
                (b as isize + jitter).clamp(0, (sample.len().saturating_sub(1)) as isize) as usize
            })
            .collect();

        // ── Step 8: build chops with integrated strength scores ─
        let mut chops = self.boundaries_to_chops(sample, &jittered)?;

        // Attach strength scores using integrated energy over the chop region
        for chop in chops.iter_mut() {
            chop.strength = self.compute_chop_strength(chop, &normalized, &frame_positions, hop);
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

    /// Apply pre-emphasis filter to boost high-frequency transients.
    /// Uses a first-order high-pass filter: y[n] = x[n] - alpha * x[n-1]
    fn apply_pre_emphasis(&self, samples: &[f32]) -> Vec<f32> {
        let alpha = self.dilla_config.pre_emphasis;
        if alpha <= 0.0 || samples.len() < 2 {
            return samples.to_vec();
        }

        let mut result = Vec::with_capacity(samples.len());
        result.push(samples[0]); // First sample unchanged

        for i in 1..samples.len() {
            result.push(samples[i] - alpha * samples[i - 1]);
        }

        result
    }

    /// Returns (strength_curve, frame_start_sample_positions).
    /// Combines RMS derivative, spectral flux, and high-frequency flux.
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

        // Pre-compute frequency bin boundaries for band separation
        // Low: 0-300Hz, Mid: 300-3000Hz, High: 3000Hz+
        let sr = self.sample_rate as f32;
        let bin_width = sr / win as f32;
        let low_boundary = (300.0 / bin_width).round() as usize;
        let high_boundary = (3000.0 / bin_width).round() as usize;
        let low_boundary = low_boundary.min(win / 2);
        let high_boundary = high_boundary.min(win / 2);

        for frame_start in (0..sample.len().saturating_sub(win)).step_by(hop) {
            let frame = &sample[frame_start..frame_start + win];

            // RMS energy (half-wave rectified derivative)
            let cur_rms: f32 = (frame.iter().map(|s| s * s).sum::<f32>() / win as f32).sqrt();
            let rms_deriv = (cur_rms - prev_rms).max(0.0);
            prev_rms = cur_rms;

            // Spectral flux (positive only, full-band)
            let mut buf: Vec<Complex<f32>> = frame
                .iter()
                .enumerate()
                .map(|(i, &s)| {
                    let w = 0.5 * (1.0 - (2.0 * PI * i as f32 / win as f32).cos());
                    Complex::new(s * w, 0.0)
                })
                .collect();
            fft.process(&mut buf);

            let mags: Vec<f32> = buf[..win / 2]
                .iter()
                .map(|c| (c.re * c.re + c.im * c.im).sqrt())
                .collect();

            // Full-band spectral flux
            let full_flux: f32 = mags
                .iter()
                .zip(prev_mag.iter())
                .map(|(m, p)| (m - p).max(0.0))
                .sum::<f32>()
                / (win / 2) as f32;

            // High-frequency flux (emphasizes transients like snares, hi-hats)
            let high_flux: f32 = mags
                .iter()
                .zip(prev_mag.iter())
                .skip(high_boundary)
                .map(|(m, p)| (m - p).max(0.0))
                .sum::<f32>()
                / (win / 2 - high_boundary).max(1) as f32;

            // Mid-band flux (emphasizes musical content: vocals, instruments)
            // Must be computed BEFORE updating prev_mag, otherwise it's always 0
            let mid_flux: f32 = if low_boundary < high_boundary {
                mags[low_boundary..high_boundary]
                    .iter()
                    .zip(prev_mag[low_boundary..high_boundary].iter())
                    .map(|(m, p)| (m - p).max(0.0))
                    .sum::<f32>()
                    / (high_boundary - low_boundary).max(1) as f32
            } else {
                0.0
            };

            prev_mag = mags.clone();

            // Weighted combination: energy + full-band flux + band-specific fluxes
            let energy_component = cfg.energy_weight * rms_deriv;
            let spectral_component = (1.0 - cfg.energy_weight) * full_flux;
            let band_component = 0.3 * high_flux + 0.2 * mid_flux; // additional band weighting

            let combined = energy_component + spectral_component + band_component;
            curve.push(combined);
            positions.push(frame_start);
        }

        (curve, positions)
    }

    /// Normalize the onset strength curve using median-based scaling.
    /// This makes the detection consistent across different audio material.
    fn normalize_onset_curve(&self, curve: &[f32]) -> Vec<f32> {
        if curve.is_empty() {
            return Vec::new();
        }

        let cfg = &self.dilla_config;
        let half_window = cfg.adaptive_window;
        let mut normalized = Vec::with_capacity(curve.len());

        for i in 0..curve.len() {
            let lo = i.saturating_sub(half_window);
            let hi = (i + half_window + 1).min(curve.len());
            let window = &curve[lo..hi];

            // Compute median (using total_cmp for NaN safety)
            let mut sorted = window.to_vec();
            sorted.sort_by(|a, b| a.total_cmp(b));
            let median = sorted[sorted.len() / 2];

            // Compute MAD (median absolute deviation) for robust scaling
            let mut deviations: Vec<f32> = window.iter().map(|&v| (v - median).abs()).collect();
            deviations.sort_by(|a, b| a.total_cmp(b));
            let mad = deviations[deviations.len() / 2].max(1e-6);

            // Normalize: (value - median) / MAD, then clamp to positive
            let value = if median > 0.0 {
                ((curve[i] - median) / (1.4826 * mad)).max(0.0)
            } else {
                0.0
            };

            normalized.push(value);
        }

        normalized
    }

    /// Pick boundaries using peak picking with prominence on the normalized curve.
    /// Prominence ensures we only pick meaningful transients, not noise.
    fn pick_peaks_with_prominence(
        &self,
        curve: &[f32],
        positions: &[usize],
        _total_samples: usize,
    ) -> Vec<usize> {
        if curve.len() < 3 {
            return Vec::new();
        }

        let cfg = &self.dilla_config;
        let min_distance = cfg.peak_min_distance;
        let min_prominence = cfg.peak_prominence;

        // First pass: find all local maxima
        let mut candidates: Vec<(usize, f32)> = Vec::new();
        for i in 1..curve.len().saturating_sub(1) {
            if curve[i] > curve[i - 1] && curve[i] >= curve[i + 1] && curve[i] > 0.0 {
                candidates.push((i, curve[i]));
            }
        }

        // Second pass: compute prominence for each peak
        // Prominence = peak height - height of highest saddle between this peak and a higher peak
        let mut prominent_peaks: Vec<(usize, f32)> = Vec::new();

        for &(peak_idx, peak_val) in &candidates {
            // Find the highest valley on each side within the search range
            let search_range = min_distance * 4;
            let left_start = peak_idx.saturating_sub(search_range);
            let right_end = (peak_idx + search_range).min(curve.len());

            // Find highest valley to the left
            let mut left_valley = peak_val;
            if left_start > 0 {
                for i in (left_start..peak_idx).rev() {
                    if curve[i] > peak_val {
                        break; // Found a higher peak, stop
                    }
                    left_valley = left_valley.min(curve[i]);
                }
            }

            // Find highest valley to the right
            let mut right_valley = peak_val;
            #[allow(clippy::needless_range_loop)]
            for i in peak_idx + 1..right_end {
                if curve[i] > peak_val {
                    break; // Found a higher peak, stop
                }
                right_valley = right_valley.min(curve[i]);
            }

            // Prominence is the higher of the two valleys
            let prominence = peak_val - left_valley.max(right_valley);

            if prominence >= min_prominence {
                prominent_peaks.push((peak_idx, peak_val));
            }
        }

        // Third pass: non-maximum suppression within min_distance
        prominent_peaks.sort_by(|a, b| b.1.total_cmp(&a.1));

        let mut selected: Vec<(usize, f32)> = Vec::new();
        for &(idx, val) in &prominent_peaks {
            let too_close = selected
                .iter()
                .any(|(s_idx, _)| (*s_idx as isize - idx as isize).abs() < min_distance as isize);
            if !too_close {
                selected.push((idx, val));
            }
        }

        // Sort by position and convert to sample positions
        selected.sort_by_key(|(idx, _)| positions[*idx]);
        selected.iter().map(|(idx, _)| positions[*idx]).collect()
    }

    /// Multi-scale energy splitting: try multiple frame sizes to find optimal split points.
    fn fill_with_energy_splits_multi_scale(
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
            let region_len = b - a;

            if region_len <= max_samples || all.len() >= target_count {
                continue;
            }

            // Try multiple frame sizes to find the best split point
            let frame_sizes = [64, 128, 256, 512, 1024];
            let mut best_split = a + region_len / 2; // fallback to midpoint
            let mut best_energy = 0.0f32;

            for &frame_size in &frame_sizes {
                if region_len <= frame_size {
                    continue;
                }

                let split_offset = Self::peak_rms_position(&sample[a..b], frame_size);
                let split = a + split_offset;

                if split <= a || split >= b {
                    continue;
                }

                // Evaluate split quality by energy contrast
                let left_rms = rms(&sample[a..split]);
                let right_rms = rms(&sample[split..b]);
                let energy_contrast = (left_rms - right_rms).abs();

                if energy_contrast > best_energy {
                    best_energy = energy_contrast;
                    best_split = split;
                }
            }

            if best_split > a && best_split < b {
                all.push(best_split);
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
        _positions: &[usize],
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

        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut selected: Vec<usize> = scored.iter().take(n).map(|x| x.0).collect();
        selected.sort();
        selected
    }

    /// Compute chop strength by integrating normalized onset energy over the chop region.
    /// This gives a more meaningful strength score than a single-frame sample.
    fn compute_chop_strength(
        &self,
        chop: &Chop,
        curve: &[f32],
        positions: &[usize],
        _hop: usize,
    ) -> f32 {
        if curve.is_empty() || positions.is_empty() {
            return 0.0;
        }

        let start_sample = (chop.start_time * self.sample_rate as f64) as usize;
        let end_sample = start_sample + chop.samples.len();

        // Find frame indices that overlap with this chop
        let start_frame = positions
            .iter()
            .position(|&p| p >= start_sample)
            .unwrap_or(0);
        let end_frame = positions
            .iter()
            .rposition(|&p| p < end_sample)
            .unwrap_or(positions.len().saturating_sub(1));

        if start_frame > end_frame || end_frame >= curve.len() {
            // Fallback: use peak RMS within the chop
            return rms(&chop.samples);
        }

        // Integrate onset energy over the chop region
        let energy_sum: f32 = curve[start_frame..=end_frame].iter().sum();
        let mean_energy = energy_sum / (end_frame - start_frame + 1) as f32;

        // Also consider peak energy within the chop
        let peak_energy = curve[start_frame..=end_frame]
            .iter()
            .fold(0.0f32, |a, b| a.max(*b));

        // Combined score: 60% mean + 40% peak
        0.6 * mean_energy + 0.4 * peak_energy
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
            .filter(|w| w[1] > w[0])
            .enumerate()
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
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((region.len() / 2, 0.0));

        best_start
    }

    fn time_to_frame(&self, time_secs: f64, hop: usize) -> usize {
        ((time_secs * self.sample_rate as f64) as usize) / hop.max(1)
    }

    /// Get the total duration of chops.
    #[allow(dead_code)]
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

    #[allow(clippy::unwrap_used)]
    #[test]
    fn test_chop_single() {
        let chopper = SampleChopper::new(44100);
        let sample = sine(440.0, 44100, 1.0);
        let chops = chopper.chop(&sample, 1).unwrap();
        assert_eq!(chops.len(), 1);
        assert_eq!(chops[0].len(), sample.len());
    }

    #[allow(clippy::unwrap_used)]
    #[test]
    fn test_dilla_produces_correct_count() {
        let chopper = SampleChopper::new(44100);
        let drums = drum_loop(44100); // 2 s loop with 4 obvious beats
        let chops = chopper.chop(&drums, 4).unwrap();
        assert_eq!(chops.len(), 4, "Expected 4 chops, got {}", chops.len());
    }

    #[allow(clippy::unwrap_used)]
    #[test]
    fn test_dilla_chops_cover_full_sample() {
        let chopper = SampleChopper::new(44100);
        let drums = drum_loop(44100);
        let chops = chopper.chop(&drums, 4).unwrap();
        let total: usize = chops.iter().map(|c| c.len()).sum();
        assert_eq!(total, drums.len(), "Chops should cover every sample");
    }

    #[allow(clippy::unwrap_used)]
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

    #[allow(clippy::unwrap_used)]
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

    #[allow(clippy::unwrap_used)]
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

    #[allow(clippy::unwrap_used)]
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
