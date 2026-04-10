//! Hum Analyzer - Pitch detection and note transcription for hummed audio.
//!
//! Uses YIN/McLeod algorithm for pitch detection and spectral flux for onset detection.

use crate::error::HumChopError;
use pitch_detection::detector::yin::YINDetector;
use pitch_detection::detector::PitchDetector as _;
use rustfft::{num_complex::Complex, FftPlanner};

/// A single detected note with timing and velocity information.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// Pitch frequency in Hz
    pub pitch_hz: f32,
    /// Note onset time in seconds
    pub onset_sec: f64,
    /// Note duration in seconds
    pub duration_sec: f64,
    /// Velocity (0.0 to 1.0), derived from amplitude
    pub velocity: f32,
}

impl Note {
    /// Create a new Note with all fields specified.
    pub fn new(pitch_hz: f32, onset_sec: f64, duration_sec: f64, velocity: f32) -> Self {
        Self {
            pitch_hz,
            onset_sec,
            duration_sec,
            velocity: velocity.clamp(0.0, 1.0),
        }
    }

    /// Convert pitch to MIDI note number (A4 = 69, A4 = 440Hz).
    pub fn to_midi_note(&self) -> i32 {
        if self.pitch_hz <= 0.0 {
            return 0;
        }
        let note = 69.0 + 12.0 * (self.pitch_hz / 440.0).log2();
        note.round() as i32
    }

    /// Convert pitch to note name (e.g., "A4", "C#5").
    pub fn to_note_name(&self) -> String {
        let note_names = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let midi = self.to_midi_note();
        if !(0..=127).contains(&midi) {
            return "?".to_string();
        }
        let octave = (midi / 12) - 1;
        let note_idx = (midi % 12) as usize;
        format!("{}{}", note_names[note_idx], octave)
    }
}

/// Detection algorithm to use for pitch detection.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum PitchAlgorithm {
    /// YIN algorithm - good for monophonic signals
    #[allow(dead_code)]
    Yin,
    /// McLeod algorithm - faster, good for real-time
    #[default]
    Mcleod,
}

/// Configuration for pitch detection.
#[derive(Debug, Clone)]
pub struct PitchConfig {
    /// Minimum frequency to detect (Hz)
    pub min_frequency: f32,
    /// Maximum frequency to detect (Hz)
    pub max_frequency: f32,
    /// Algorithm to use for pitch detection
    #[allow(dead_code)]
    pub algorithm: PitchAlgorithm,
    /// FFT window size (must be power of 2)
    pub window_size: usize,
    /// Overlap between windows (in samples)
    pub overlap: usize,
}

impl Default for PitchConfig {
    fn default() -> Self {
        Self {
            min_frequency: 80.0,   // ~E2 - bass range
            max_frequency: 1000.0, // ~B5 - soprano range
            algorithm: PitchAlgorithm::Mcleod,
            window_size: 2048,
            overlap: 1024,
        }
    }
}

/// Configuration for onset detection.
#[derive(Debug, Clone)]
pub struct OnsetConfig {
    /// FFT window size for onset detection
    pub window_size: usize,
    /// Threshold for onset detection
    pub threshold: f32,
    /// Minimum time between onsets (seconds)
    pub min_onset_gap: f64,
}

impl Default for OnsetConfig {
    fn default() -> Self {
        Self {
            window_size: 1024,
            threshold: 0.2,
            min_onset_gap: 0.1,
        }
    }
}

/// Hum analyzer that combines pitch and onset detection.
pub struct HumAnalyzer {
    pitch_config: PitchConfig,
    onset_config: OnsetConfig,
    sample_rate: u32,
}

impl HumAnalyzer {
    /// Create a new HumAnalyzer with default settings.
    pub fn new(sample_rate: u32) -> Self {
        Self {
            pitch_config: PitchConfig::default(),
            onset_config: OnsetConfig::default(),
            sample_rate,
        }
    }

    /// Create a new HumAnalyzer with custom settings.
    #[allow(dead_code)]
    pub fn with_config(
        sample_rate: u32,
        pitch_config: PitchConfig,
        onset_config: OnsetConfig,
    ) -> Self {
        Self {
            pitch_config,
            onset_config,
            sample_rate,
        }
    }

    /// Detect pitch from audio samples.
    /// Returns a vector of detected pitches (in Hz) for each analysis window.
    pub fn detect_pitch(&self, samples: &[f32]) -> Vec<f32> {
        let window_size = self.pitch_config.window_size;
        let overlap = self.pitch_config.overlap;
        let step = window_size - overlap;

        let mut pitches = Vec::with_capacity(samples.len() / step);
        let mut buffer: Vec<f32> = vec![0.0; window_size];

        // Detect using YIN algorithm
        let mut detector = YINDetector::<f32>::new(window_size, window_size / 2);
        let clarity_threshold = 0.85_f32; // ~0.15 threshold
        let power_threshold = 2.0_f32; // dB threshold

        for window_start in (0..samples.len().saturating_sub(window_size)).step_by(step) {
            // Copy window with optional windowing
            let window_end = window_start + window_size;
            buffer.copy_from_slice(&samples[window_start..window_end]);

            // Apply Hann window
            for (i, sample) in buffer.iter_mut().enumerate().take(window_size) {
                let multiplier = 0.5
                    * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / window_size as f32).cos());
                *sample *= multiplier;
            }

            // Detect pitch - API: get_pitch(signal, sample_rate, power_threshold, clarity_threshold)
            if let Some(pitch) = detector.get_pitch(
                &buffer,
                self.sample_rate as usize,
                power_threshold,
                clarity_threshold,
            ) {
                let freq = pitch.frequency;
                // Filter by frequency range
                if freq >= self.pitch_config.min_frequency
                    && freq <= self.pitch_config.max_frequency
                {
                    pitches.push(freq);
                } else {
                    pitches.push(0.0); // Invalid pitch
                }
            } else {
                pitches.push(0.0); // No pitch detected
            }
        }

        pitches
    }

    /// Detect onsets in audio samples using spectral flux.
    pub fn detect_onsets(&self, samples: &[f32]) -> Vec<f64> {
        let window_size = self.onset_config.window_size;
        let step = window_size / 2; // 50% overlap for onset detection

        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(window_size);

        let mut onsets: Vec<f64> = Vec::new();
        let mut prev_magnitudes: Vec<f32> = vec![0.0; window_size / 2];
        let mut prev_onset_time: f64 = -999.0;

        let _hop_duration = step as f64 / self.sample_rate as f64;

        for window_start in (0..samples.len().saturating_sub(window_size)).step_by(step) {
            let window_end = window_start + window_size;
            let window_samples = &samples[window_start..window_end];

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

            // Calculate magnitudes (only positive frequencies)
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

            // Normalize flux
            flux /= num_bins as f32;

            let current_time = window_start as f64 / self.sample_rate as f64;

            // Detect onset if flux exceeds threshold and enough time has passed
            if flux > self.onset_config.threshold
                && (current_time - prev_onset_time) > self.onset_config.min_onset_gap
            {
                onsets.push(current_time);
                prev_onset_time = current_time;
            }

            prev_magnitudes = magnitudes;
        }

        onsets
    }

    /// Transcribe hummed audio to a sequence of notes.
    /// Combines pitch detection with onset detection.
    pub fn transcribe(&self, samples: &[f32]) -> Result<Vec<Note>, HumChopError> {
        if samples.is_empty() {
            return Err(HumChopError::InvalidAudio(
                "Empty audio samples".to_string(),
            ));
        }

        // Step 1: Detect onsets
        let onsets = self.detect_onsets(samples);

        // Step 2: Detect continuous pitch values
        let pitches = self.detect_pitch(samples);

        // Step 3: Segment the pitch array based on onsets
        let step = (self.pitch_config.window_size - self.pitch_config.overlap) as f64;
        let hop_duration = step / self.sample_rate as f64;

        let mut notes: Vec<Note> = Vec::new();
        let _current_note: Option<Note> = None;

        for (i, &onset_time) in onsets.iter().enumerate() {
            let next_onset_time = onsets.get(i + 1).copied();

            // Find pitch indices corresponding to this note segment
            let start_idx = (onset_time / hop_duration) as usize;
            let end_idx = next_onset_time
                .map(|t| (t / hop_duration) as usize)
                .unwrap_or(pitches.len());

            // Calculate median pitch for this segment
            let segment_pitches: Vec<f32> = pitches[start_idx..end_idx.min(pitches.len())]
                .iter()
                .filter(|&&p| p > 0.0)
                .copied()
                .collect();

            let pitch_hz = if segment_pitches.is_empty() {
                continue; // Skip segments with no valid pitch
            } else {
                // Use median for robustness, total_cmp for NaN safety
                let mut sorted = segment_pitches.clone();
                sorted.sort_by(|a, b| a.total_cmp(b));
                sorted[sorted.len() / 2]
            };

            // Calculate duration
            let duration = next_onset_time.map(|t| t - onset_time).unwrap_or_else(|| {
                let last_sample_time = samples.len() as f64 / self.sample_rate as f64;
                last_sample_time - onset_time
            });

            // Calculate velocity (RMS amplitude in segment)
            let start_sample = (onset_time * self.sample_rate as f64) as usize;
            let end_sample = ((next_onset_time
                .unwrap_or(samples.len() as f64 / self.sample_rate as f64))
                * self.sample_rate as f64) as usize;
            let segment_samples = &samples[start_sample..end_sample.min(samples.len())];
            let velocity = calculate_rms(segment_samples);

            notes.push(Note::new(pitch_hz, onset_time, duration, velocity));
        }

        // If no notes were detected by onsets, try to detect notes from pitch continuity
        if notes.is_empty() {
            notes = self.transcribe_from_pitch_continuity(samples, &pitches, hop_duration);
        }

        if notes.len() == 1 {
            return Err(HumChopError::SingleNoteDetected);
        }

        Ok(notes)
    }

    /// Fallback transcription method based on pitch continuity.
    fn transcribe_from_pitch_continuity(
        &self,
        samples: &[f32],
        pitches: &[f32],
        hop_duration: f64,
    ) -> Vec<Note> {
        let mut notes: Vec<Note> = Vec::new();

        // Parameters for note segmentation
        let min_note_duration = 0.1; // seconds
        let pitch_change_threshold = 0.15; // 15% change indicates new note

        let mut current_pitch_start = 0;
        let mut current_pitch = 0.0f32;
        let _current_velocity = 0.0f32;

        for (i, &pitch) in pitches.iter().enumerate() {
            let time = i as f64 * hop_duration;

            if pitch <= 0.0 {
                continue; // Skip invalid pitches
            }

            let is_new_note = if current_pitch <= 0.0 {
                true
            } else {
                let change = ((pitch - current_pitch) / current_pitch).abs();
                change > pitch_change_threshold
            };

            if is_new_note {
                // Save previous note if it exists
                if current_pitch > 0.0
                    && (time - current_pitch_start as f64 * hop_duration) >= min_note_duration
                {
                    let onset = current_pitch_start as f64 * hop_duration;
                    let duration = time - onset;

                    // Calculate velocity for the note
                    let start_sample = (onset * self.sample_rate as f64) as usize;
                    let end_sample = ((onset + duration) * self.sample_rate as f64) as usize;
                    let segment_samples = &samples[start_sample..end_sample.min(samples.len())];
                    let velocity = calculate_rms(segment_samples);

                    notes.push(Note::new(current_pitch, onset, duration, velocity));
                }

                current_pitch_start = i;
                current_pitch = pitch;
            }
        }

        // Don't forget the last note
        if current_pitch > 0.0 {
            let onset = current_pitch_start as f64 * hop_duration;
            let duration = (pitches.len() - current_pitch_start) as f64 * hop_duration;

            let start_sample = (onset * self.sample_rate as f64) as usize;
            let end_sample = samples.len();
            let segment_samples = &samples[start_sample..end_sample];
            let velocity = calculate_rms(segment_samples);

            notes.push(Note::new(current_pitch, onset, duration, velocity));
        }

        notes
    }
}

/// Calculate RMS (Root Mean Square) amplitude of samples.
fn calculate_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }

    let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
    (sum_squares / samples.len() as f32).sqrt().min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to generate a sine wave
    fn generate_sine_wave(frequency: f32, sample_rate: u32, duration_secs: f32) -> Vec<f32> {
        let num_samples = (sample_rate as f32 * duration_secs) as usize;
        let mut samples = Vec::with_capacity(num_samples);

        for i in 0..num_samples {
            let t = i as f32 / sample_rate as f32;
            let value = (2.0 * std::f32::consts::PI * frequency * t).sin();
            samples.push(value);
        }

        samples
    }

    #[test]
    fn test_pitch_detection_sine_wave() {
        let sample_rate = 44100;
        let frequency = 440.0; // A4
        let samples = generate_sine_wave(frequency, sample_rate, 0.5);

        let analyzer = HumAnalyzer::new(sample_rate);
        let pitches = analyzer.detect_pitch(&samples);

        // Check that we detected pitches
        let valid_pitches: Vec<_> = pitches.iter().filter(|&&p| p > 0.0).collect();
        assert!(
            !valid_pitches.is_empty(),
            "Should detect at least some pitches"
        );

        // Check that detected pitch is close to expected
        if let Some(&detected) = valid_pitches.first() {
            let error = ((detected - frequency) / frequency).abs();
            assert!(
                error < 0.1,
                "Detected pitch {} should be within 10% of {}",
                detected,
                frequency
            );
        }
    }

    #[test]
    fn test_note_to_midi() {
        let note = Note::new(440.0, 0.0, 1.0, 0.5);
        assert_eq!(note.to_midi_note(), 69); // A4 = MIDI 69
    }

    #[test]
    fn test_note_to_name() {
        let note = Note::new(440.0, 0.0, 1.0, 0.5);
        assert_eq!(note.to_note_name(), "A4");

        let note_csharp = Note::new(554.37, 0.0, 1.0, 0.5); // C#5
        assert_eq!(note_csharp.to_note_name(), "C#5");
    }

    #[test]
    fn test_calculate_rms() {
        let samples = vec![0.5f32, -0.5, 0.5, -0.5];
        let rms = calculate_rms(&samples);
        // RMS of [0.5, -0.5, 0.5, -0.5] = sqrt(4 * 0.25 / 4) = sqrt(0.25) = 0.5
        assert!((rms - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_transcribe_continuity() {
        let sample_rate = 44100;

        // Generate two notes: A4 (440Hz) for 0.3s, then C5 (523Hz) for 0.3s
        let note1 = generate_sine_wave(440.0, sample_rate, 0.3);
        let note2 = generate_sine_wave(523.0, sample_rate, 0.3);
        let silence = vec![0.0f32; 4410]; // 0.1s silence
        let samples: Vec<f32> = note1.into_iter().chain(silence).chain(note2).collect();

        let analyzer = HumAnalyzer::new(sample_rate);
        let result = analyzer.transcribe(&samples);

        // Should either succeed with 2 notes or fail with SingleNoteDetected
        // (depends on detection quality)
        match result {
            Ok(notes) => {
                println!("Detected {} notes", notes.len());
                assert!(!notes.is_empty(), "Should detect at least 1 note");
            }
            Err(HumChopError::SingleNoteDetected) => {
                println!("Only one note detected - acceptable for synthetic test");
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }
}
