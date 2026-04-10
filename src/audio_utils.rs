//! Audio utility functions for loading, saving, and processing audio samples.

use anyhow::Result;
use hound::{WavReader, WavSpec, WavWriter};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use std::fs::File;
use std::path::Path;

use crate::error::HumChopError;

/// Default sample rate for internal processing.
#[allow(dead_code)]
pub const DEFAULT_SAMPLE_RATE: u32 = 44100;

/// Loads an audio file (WAV, MP3, or FLAC) and returns interleaved stereo/mono samples.
///
/// Returns a tuple of (samples, sample_rate) where samples are normalized to f32 [-1.0, 1.0].
/// For multi-channel files, channels are interleaved.
/// All output is converted to mono by averaging channels.
pub fn load_audio(path: &Path) -> Result<(Vec<f32>, u32)> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match extension.as_str() {
        "wav" => load_wav(path),
        "mp3" | "flac" => load_with_symphonia(path),
        _ => {
            Err(HumChopError::UnsupportedFormat(format!("Unknown extension: {}", extension)).into())
        }
    }
}

/// Loads a WAV file using hound and returns mono samples.
fn load_wav(path: &Path) -> Result<(Vec<f32>, u32)> {
    let reader = WavReader::open(path)
        .map_err(|e| HumChopError::DecodeError(format!("Failed to open WAV file: {}", e)))?;

    let spec = reader.spec();
    let sample_rate = spec.sample_rate;

    if spec.sample_format != hound::SampleFormat::Float {
        // Handle integer samples
        let samples: Result<Vec<f32>, _> = reader
            .into_samples::<i32>()
            .map(|s| {
                s.map(|v| {
                    // Normalize based on bits per sample
                    let max_val = (1 << (spec.bits_per_sample - 1)) as f64;
                    (v as f64 / max_val) as f32
                })
            })
            .collect();
        let samples = samples?;

        return Ok((to_mono(&samples, spec.channels), sample_rate));
    }

    // Already float samples
    let samples: Result<Vec<f32>, _> = reader.into_samples().collect();
    let samples = samples.map_err(|e| HumChopError::DecodeError(e.to_string()))?;

    Ok((to_mono(&samples, spec.channels), sample_rate))
}

/// Loads an audio file using symphonia (MP3, FLAC, WAV).
fn load_with_symphonia(path: &Path) -> Result<(Vec<f32>, u32)> {
    let file = File::open(path)?;

    // Create a MediaSourceStream from the file
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }

    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();

    // Use the probe to detect the format
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| HumChopError::DecodeError(format!("Failed to probe audio file: {}", e)))?;

    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or_else(|| HumChopError::DecodeError("No supported audio track found".to_string()))?;

    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| HumChopError::DecodeError("Could not determine sample rate".to_string()))?;
    let n_channels: u16 = track
        .codec_params
        .channels
        .map(|c| c.count() as u16)
        .unwrap_or(1);

    let decoder_opts = DecoderOptions::default();
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &decoder_opts)
        .map_err(|e| HumChopError::DecodeError(format!("Failed to create decoder: {}", e)))?;

    let mut all_samples: Vec<f32> = Vec::new();

    loop {
        match format.next_packet() {
            Ok(packet) => {
                if packet.track_id() != track_id {
                    continue;
                }

                match decoder.decode(&packet) {
                    Ok(decoded) => {
                        // Get the audio buffer with samples
                        let spec = *decoded.spec();
                        let duration = decoded.capacity() as u64;

                        // Create a sample buffer
                        let mut sample_buf = SampleBuffer::<i16>::new(duration, spec);
                        sample_buf.copy_interleaved_ref(decoded);

                        // Get the samples and convert to f32
                        for &sample_i16 in sample_buf.samples() {
                            let sample_f32 = sample_i16 as f32 / 32768.0;
                            all_samples.push(sample_f32);
                        }
                    }
                    Err(SymphoniaError::IoError(_)) => break,
                    Err(e) => {
                        return Err(
                            HumChopError::DecodeError(format!("Decode error: {}", e)).into()
                        );
                    }
                }
            }
            Err(SymphoniaError::IoError(_)) => break,
            Err(e) => {
                return Err(HumChopError::DecodeError(format!("Packet error: {}", e)).into());
            }
        }
    }

    Ok((to_mono(&all_samples, n_channels), sample_rate))
}

/// Converts interleaved samples to mono by averaging channels.
fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels == 1 {
        return samples.to_vec();
    }

    let n_channels = channels as usize;
    let num_frames = samples.len() / n_channels;

    (0..num_frames)
        .map(|frame| {
            let frame_start = frame * n_channels;
            (0..n_channels)
                .map(|ch| samples[frame_start + ch])
                .sum::<f32>()
                / n_channels as f32
        })
        .collect()
}

/// Writes samples to a WAV file at the specified sample rate.
///
/// Output is always mono (1 channel), 32-bit float format.
pub fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) -> Result<()> {
    if samples.is_empty() {
        return Err(HumChopError::InvalidAudio("No samples to write".to_string()).into());
    }

    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };

    let mut writer = WavWriter::create(path, spec)
        .map_err(|e| HumChopError::EncodeError(format!("Failed to create WAV file: {}", e)))?;

    for &sample in samples {
        writer
            .write_sample(sample)
            .map_err(|e| HumChopError::EncodeError(format!("Failed to write sample: {}", e)))?;
    }

    writer
        .finalize()
        .map_err(|e| HumChopError::EncodeError(format!("Failed to finalize WAV file: {}", e)))?;

    Ok(())
}

/// WAV output options for controlling bit depth and dithering.
#[derive(Debug, Clone, Default)]
pub struct WavOptions {
    /// Bit depth for output (16, 24, or 32). Defaults to 32.
    pub bits_per_sample: u16,
    /// Enable dithering for lower bit depths (16, 24).
    /// Dithering reduces quantization noise by adding shaped noise.
    pub dither: bool,
}

impl WavOptions {
    /// Create new options with default settings (32-bit float).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set bit depth (16, 24, or 32).
    pub fn bits_per_sample(mut self, bits: u16) -> Self {
        self.bits_per_sample = bits.min(32);
        self
    }

    /// Enable dithering.
    pub fn dither(mut self, enable: bool) -> Self {
        self.dither = enable;
        self
    }
}

/// Apply triangular noise dithering (TPDF).
/// This adds noise with a triangular probability distribution, which is optimal
/// for reducing quantization artifacts at lower bit depths.
///
/// Uses a seed derived from sample content and a fixed base to ensure:
/// 1. Deterministic enough for reproducible results
/// 2. Different per-file to avoid repetitive artifacts
fn apply_dither(samples: &[f32], bits: u16) -> Vec<f32> {
    if bits >= 32 || samples.is_empty() {
        return samples.to_vec();
    }

    // LSB weight for the target bit depth
    let lsb_weight = 2.0_f32.powf(-(bits as f32));

    // Create seed from sample content (sum of first few samples) + a base
    // This ensures different seeds for different content while being deterministic
    let seed_base: u32 = samples
        .iter()
        .take(1000)
        .map(|s| (s.abs() * 1000.0) as u32)
        .fold(0u32, |acc, x| acc.wrapping_add(x));
    let mut state = seed_base.wrapping_mul(28657); // Use Fibonacci multiplier for better distribution

    let mut next_state = || {
        state = state.wrapping_mul(28657).wrapping_add(12289); // LCG
        state ^= state.rotate_left(13);
        state ^= state.rotate_right(17);
        state
    };

    // Triangular PDF: sum of two uniform random values gives triangular distribution
    let mut output = Vec::with_capacity(samples.len());
    for &sample in samples {
        let r1 = (next_state() as f32 / u32::MAX as f32) * 2.0 - 1.0;
        let r2 = (next_state() as f32 / u32::MAX as f32) * 2.0 - 1.0;
        let dither = (r1 + r2) * lsb_weight * 0.5;
        output.push((sample + dither).clamp(-1.0, 1.0));
    }

    output
}

/// Writes samples to a WAV file with specified options.
///
/// Supports different bit depths (16, 24, 32) and optional dithering.
/// For 32-bit float, no quantization is needed.
/// For 16/24-bit integer output, samples are quantized and dithering can be applied.
pub fn write_wav_with_options(
    path: &Path,
    samples: &[f32],
    sample_rate: u32,
    options: &WavOptions,
) -> Result<()> {
    if samples.is_empty() {
        return Err(HumChopError::InvalidAudio("No samples to write".to_string()).into());
    }

    let bits = options.bits_per_sample.min(32);

    // Quantize and dither if needed
    let output_samples = if bits < 32 && options.dither {
        apply_dither(samples, bits)
    } else {
        samples.to_vec()
    };

    match bits {
        16 | 24 => {
            // Integer output (16 or 24 bit)
            let spec = WavSpec {
                channels: 1,
                sample_rate,
                bits_per_sample: bits,
                sample_format: hound::SampleFormat::Int,
            };

            let mut writer = WavWriter::create(path, spec).map_err(|e| {
                HumChopError::EncodeError(format!("Failed to create WAV file: {}", e))
            })?;

            let max_val = (1i64 << (bits - 1)) as f32;

            for &sample in &output_samples {
                let quantized = (sample.clamp(-1.0, 1.0) * max_val).round() as i32;
                match bits {
                    16 => writer.write_sample(quantized as i16).map_err(|e| {
                        HumChopError::EncodeError(format!("Failed to write sample: {}", e))
                    })?,
                    24 => writer
                        .write_sample(quantized) // hound handles 24-bit as i32
                        .map_err(|e| {
                            HumChopError::EncodeError(format!("Failed to write sample: {}", e))
                        })?,
                    _ => unreachable!(),
                }
            }

            writer.finalize().map_err(|e| {
                HumChopError::EncodeError(format!("Failed to finalize WAV file: {}", e))
            })?;
        }
        _ => {
            // 32-bit float output
            let spec = WavSpec {
                channels: 1,
                sample_rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            };

            let mut writer = WavWriter::create(path, spec).map_err(|e| {
                HumChopError::EncodeError(format!("Failed to create WAV file: {}", e))
            })?;

            for &sample in &output_samples {
                writer.write_sample(sample).map_err(|e| {
                    HumChopError::EncodeError(format!("Failed to write sample: {}", e))
                })?;
            }

            writer.finalize().map_err(|e| {
                HumChopError::EncodeError(format!("Failed to finalize WAV file: {}", e))
            })?;
        }
    }

    Ok(())
}

/// Normalizes samples to have peak amplitude of 1.0 (in-place).
#[allow(dead_code)]
pub fn normalize(samples: &mut [f32]) {
    if samples.is_empty() {
        return;
    }

    let max_abs = samples
        .iter()
        .map(|s| s.abs())
        .fold(0.0f32, |a, b| a.max(b));

    if max_abs > 0.0 && max_abs < 1.0 {
        let scale = 1.0 / max_abs;
        for sample in samples.iter_mut() {
            *sample *= scale;
        }
    }
}

/// Resamples audio from one sample rate to another using linear interpolation.
#[allow(dead_code)]
pub fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
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
            let frac = src_pos - src_idx as f64;
            let interpolated =
                samples[src_idx] * (1.0 - frac as f32) + samples[src_idx + 1] * frac as f32;
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
    use tempfile::NamedTempFile;

    #[allow(dead_code, clippy::unwrap_used)]
    fn create_test_wav(samples: &[f32], sample_rate: u32) -> NamedTempFile {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        let mut writer = WavWriter::create(path, spec).unwrap();
        for &s in samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();

        temp_file
    }

    #[allow(dead_code, clippy::unwrap_used)]
    fn create_named_temp_wav(samples: &[f32], sample_rate: u32) -> NamedTempFile {
        let temp_file = NamedTempFile::new().unwrap();
        // Write wav header manually or use hound
        let path = temp_file.path();

        let spec = WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        let mut writer = WavWriter::create(path, spec).unwrap();
        for &s in samples {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();

        temp_file
    }

    fn vec_diff(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum()
    }

    #[test]
    fn test_load_wav_mono() {
        let original = vec![0.5f32, -0.5, 0.25, -0.25];

        // Create temp directory for wav file with proper extension
        let temp_dir = tempfile::tempdir().unwrap();
        let wav_path = temp_dir.path().join("test.wav");

        let spec = WavSpec {
            channels: 1,
            sample_rate: 44100,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        {
            let mut writer = WavWriter::create(&wav_path, spec).unwrap();
            for &s in &original {
                writer.write_sample(s).unwrap();
            }
            writer.finalize().unwrap();
        }

        let (loaded, rate) = load_audio(&wav_path).unwrap();
        assert_eq!(rate, 44100);
        assert!(vec_diff(&loaded, &original) < 0.001);
    }

    #[test]
    fn test_write_wav() {
        let samples = vec![0.5f32, -0.5, 0.25, -0.25];

        let temp_dir = tempfile::tempdir().unwrap();
        let wav_path = temp_dir.path().join("test.wav");

        write_wav(&wav_path, &samples, 48000).unwrap();

        let (loaded, rate) = load_audio(&wav_path).unwrap();
        assert_eq!(rate, 48000);
        assert!(vec_diff(&loaded, &samples) < 0.001);
    }

    #[test]
    fn test_round_trip() {
        let original = vec![0.1f32, 0.5, -0.3, 0.7, -0.2, 0.0, 0.99, -0.99];

        let temp_dir = tempfile::tempdir().unwrap();
        let wav_path = temp_dir.path().join("test.wav");

        write_wav(&wav_path, &original, 44100).unwrap();
        let (loaded, _) = load_audio(&wav_path).unwrap();

        assert!(vec_diff(&original, &loaded) < 0.001);
    }

    #[test]
    fn test_normalize() {
        let mut samples = vec![0.5f32, -0.25, 0.1];
        normalize(&mut samples);

        let max_abs = samples
            .iter()
            .map(|s| s.abs())
            .fold(0.0f32, |a, b| a.max(b));
        assert!((max_abs - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_to_mono() {
        // Stereo: [L0, R0, L1, R1] -> mono: [(L0+R0)/2, (L1+R1)/2]
        let stereo = vec![1.0f32, 0.5, -1.0, -0.5];
        let mono = to_mono(&stereo, 2);
        assert!((mono[0] - 0.75).abs() < 0.001);
        assert!((mono[1] - (-0.75)).abs() < 0.001);
    }

    #[test]
    fn test_resample() {
        let samples = vec![0.0f32, 1.0, 0.0, -1.0, 0.0];

        // Upsample 2x
        let upsampled = resample(&samples, 44100, 88200);
        assert!(upsampled.len() >= samples.len() * 2 - 1);

        // Downsample 2x
        let downsampled = resample(&samples, 88200, 44100);
        assert!(downsampled.len() <= samples.len() / 2 + 1);
    }

    #[test]
    fn test_empty_audio_error() {
        let temp = NamedTempFile::new().unwrap();
        let result = write_wav(temp.path(), &[], 44100);
        assert!(result.is_err());
    }
}
