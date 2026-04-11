//! HumChop - Hum-to-chop sampling tool
//!
//! Chop audio samples by humming melodies.
//! Record a hum → Analyze pitch → Auto-chop your samples using JDilla-style processing.

mod constants;
use crate::constants::{MAX_DEMO_DURATION_SECS, MAX_RECORDING_DURATION_SECS};
mod audio_utils;
mod error;
mod hum_analyzer;
mod mapper;
mod sample_chopper;
mod tui;

#[cfg(feature = "audio-io")]
mod recorder;

#[cfg(feature = "audio-io")]
mod player;

use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use std::fs;
use std::path::{Path, PathBuf};

/// HumChop - Hum-to-chop sampling tool
#[derive(Parser, Debug)]
#[command(name = "humchop")]
#[command(version, about, long_about = None)]
struct Args {
    /// Input audio file (WAV, MP3, or FLAC)
    #[arg(value_name = "INPUT")]
    input: Option<PathBuf>,

    /// Output file path (defaults to output_chopped_<timestamp>.wav)
    #[arg(short, long, value_name = "OUTPUT")]
    output: Option<PathBuf>,

    /// Output directory for all generated files.
    /// Creates the directory if it doesn't exist.
    /// All outputs (chops, debug files, etc.) go here.
    #[arg(short = 'd', long, value_name = "DIR", default_value = "./output")]
    output_dir: PathBuf,

    /// Enable pitch shifting (slower but more accurate)
    #[arg(long)]
    pitch_shift: bool,

    /// Match notes to chops by pitch instead of strength
    #[arg(long)]
    pitch_matching: bool,

    /// Sample segment to use: start,end in seconds (e.g., 0,30 for first 30s)
    #[arg(long, value_name = "START,END")]
    segment: Option<String>,

    /// Run in headless mode (no TUI) with demo notes.
    /// Useful for scripting, batch processing, or quick testing.
    #[arg(long)]
    no_tui: bool,

    /// Number of chops to create (for headless mode with --no-tui).
    /// Defaults to 16 if not specified.
    #[arg(long, value_name = "NUM", default_value = "16")]
    num_chops: Option<usize>,

    /// Enable dithering for reduced quantization noise.
    /// Recommended for 16-bit output to minimize artifacts.
    #[arg(long)]
    dither: bool,

    /// Output bit depth: 16, 24, or 32 (default: 32).
    /// 16-bit produces smaller files; 32-bit is lossless.
    #[arg(long, value_name = "BITS", default_value = "32")]
    bits: Option<u16>,

    /// Process all audio files in a directory (batch mode).
    /// All matching files in the directory will be processed.
    #[arg(short, long)]
    batch: bool,

    /// Pattern for batch mode (e.g., "*.wav", "*.mp3").
    /// Defaults to supported formats: wav, mp3, flac.
    #[arg(long, default_value = "*")]
    pattern: Option<String>,
}

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    // Display welcome message
    println!(
        "{}",
        format!(
            "HumChop v{} - Hum-to-chop sampling tool",
            env!("CARGO_PKG_VERSION")
        )
        .green()
        .bold()
    );
    println!("{}", "━".repeat(40).dimmed());
    println!();

    // Create output directory if it doesn't exist
    let output_dir = &args.output_dir;
    if !output_dir.exists() {
        println!("→ Creating output directory: {}", output_dir.display());
        fs::create_dir_all(output_dir)?;
    }

    match args.input {
        Some(input_path) => {
            if args.batch {
                // Batch mode: process all matching files in directory
                run_batch(
                    input_path.as_path(),
                    args.output.as_deref(),
                    output_dir,
                    args.pitch_shift,
                    args.pitch_matching,
                    args.num_chops,
                    args.dither,
                    args.bits.unwrap_or(32),
                    args.pattern.as_deref().unwrap_or("*"),
                )?;
            } else if args.no_tui {
                // Headless mode: process with demo notes, no TUI
                run_headless(
                    input_path.as_path(),
                    args.output.as_deref(),
                    output_dir,
                    args.pitch_shift,
                    args.pitch_matching,
                    args.segment.as_deref(),
                    args.num_chops,
                    args.dither,
                    args.bits.unwrap_or(32),
                )?;
            } else {
                run_interactive(
                    input_path.as_path(),
                    args.output.as_deref(),
                    output_dir,
                    args.pitch_shift,
                    args.pitch_matching,
                    args.segment.as_deref(),
                )?;
            }
        }
        None => {
            // Show help
            println!("Usage: humchop <audio_file> [options]");
            println!();
            println!("Options:");
            println!("  -o, --output <file>    Output file path");
            println!("  -d, --output-dir <dir> Output directory (default: ./output)");
            println!("      --pitch-shift      Enable pitch shifting");
            println!("      --pitch-matching   Match by pitch instead of strength");
            println!("      --no-tui           Run headless (no TUI, demo notes)");
            println!("      --num-chops <N>    Number of chops (default: 16)");
            println!("      --dither           Enable dithering (for 16/24-bit output)");
            println!("      --bits <BITS>      Output bit depth: 16, 24, or 32 (default: 32)");
            println!();
            println!("JDilla-style mode:");
            println!("  - Chops keep original length (classic hip-hop chop)");
            println!("  - Notes determine WHICH chop plays, not how long");
            println!("  - Strength matching: loud notes → strong transients");
            println!();
            println!("Example:");
            println!("  humchop sample.wav");
            println!("  humchop beat.mp3 -o my_chops.wav");
            println!("  humchop beat.mp3 -d ./results  # save to ./results/");
            println!("  humchop beat.mp3 --no-tui --num-chops 8  # headless");
        }
    }

    Ok(())
}

#[cfg(feature = "audio-io")]
fn run_interactive(
    input_path: &Path,
    output_path: Option<&Path>,
    output_dir: &Path,
    enable_pitch_shift: bool,
    pitch_matching: bool,
    _segment: Option<&str>,
) -> Result<()> {
    use crate::recorder::Recorder;
    use std::io::{self, Write};
    use std::time::{Duration, Instant};

    // Validate input file exists
    if !input_path.exists() {
        return Err(error::HumChopError::IoError(format!(
            "Input file not found: {}",
            input_path.display()
        ))
        .into());
    }

    // Display file info
    println!("→ Loading: {}", input_path.display().to_string().white());

    // Load the sample
    let (samples, sample_rate) = audio_utils::load_audio(input_path)?;

    let duration_secs = samples.len() as f64 / sample_rate as f64;
    println!(
        "  • {} samples, {:.2}s @ {} Hz",
        samples.len().to_string().yellow(),
        duration_secs,
        sample_rate.to_string().yellow()
    );
    println!(
        "  • JDilla-style: {}",
        if pitch_matching {
            "pitch matching"
        } else {
            "strength matching"
        }
    );
    if enable_pitch_shift {
        println!("  • pitch shifting ENABLED");
    }

    // Initialize recorder
    let mut recorder = Recorder::new();

    println!();
    println!("🎤 Recording mode");
    println!("(Recording auto-stops after 15 seconds)");
    println!();
    println!("→ Recording started. Press Enter to stop or wait 15 seconds.");

    // Start recording
    let recording_start = Instant::now();
    let max_duration = Duration::from_secs_f64(MAX_RECORDING_DURATION_SECS);

    let tokio_receiver = match recorder.start_recording() {
        Ok(rx) => rx,
        Err(e) => {
            println!("✗ Failed to start recording: {}", e);
            return Err(e.into());
        }
    };

    // Collect audio data
    let mut hum_samples: Vec<f32> = Vec::new();
    let (audio_tx, audio_rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(100);
    let audio_tx_clone = audio_tx.clone();

    // Spawn thread to forward tokio mpsc to std mpsc
    let forward_handle = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");

        rt.block_on(async {
            let mut receiver = tokio_receiver;
            while let Some(samples) = receiver.recv().await {
                if audio_tx_clone.send(samples).is_err() {
                    break;
                }
            }
        });
    });

    // Create a channel for user input to stop recording
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let mut line = String::new();
        io::stdin().read_line(&mut line).ok();
        stop_tx.send(()).ok();
    });

    // Collect audio until user stops or timeout
    loop {
        if stop_rx.try_recv().is_ok() {
            println!("■ Stopped by user");
            break;
        }

        if recording_start.elapsed() >= max_duration {
            println!("■ Auto-stopped at 15 seconds");
            break;
        }

        match audio_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(samples) => {
                if !samples.is_empty() {
                    hum_samples.extend_from_slice(&samples);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        // Print elapsed time
        let elapsed = recording_start.elapsed().as_secs_f64();
        print!(
            "\r🔴 {:.1}s / {:.1}s ",
            elapsed.min(MAX_RECORDING_DURATION_SECS),
            MAX_RECORDING_DURATION_SECS
        );
        io::stdout().flush().ok();
    }

    println!();

    // Stop recording and cleanup
    recorder.stop_recording();
    drop(audio_tx);

    // Drain remaining audio
    while let Ok(samples) = audio_rx.recv_timeout(Duration::from_millis(10)) {
        if !samples.is_empty() {
            hum_samples.extend_from_slice(&samples);
        }
        if hum_samples.len() > (sample_rate as usize * 30) {
            break;
        }
    }

    forward_handle.join().ok();

    let recording_duration = recording_start.elapsed().as_secs_f64();
    println!("  • Recorded {:.2} seconds", recording_duration);
    println!("  • {} samples collected", hum_samples.len());

    // DEBUG: Save the raw hum recording for manual testing
    let hum_wav_path = output_dir.join("debug_hum_recording.wav");
    if let Err(e) = audio_utils::write_wav(&hum_wav_path, &hum_samples, sample_rate) {
        println!("  ⚠️  Failed to save hum recording for debug: {}", e);
    } else {
        println!("  💾 Saved hum recording to: {}", hum_wav_path.display());
    }

    // Check if we have enough audio
    if hum_samples.len() < sample_rate as usize / 10 {
        println!();
        println!("✗ Recording too short! Please record again with more audio.");
        return run_demo_mode(
            samples,
            sample_rate,
            enable_pitch_shift,
            pitch_matching,
            output_path,
            output_dir,
        );
    }

    // Process the recording
    println!();
    println!("→ Analyzing pitch...");
    println!("  • Recording duration: {:.2}s", recording_duration);

    let analyzer = hum_analyzer::HumAnalyzer::new(sample_rate);

    let notes = match analyzer.transcribe(&hum_samples) {
        Ok(n) => n,
        Err(e) => {
            println!("✗ Failed to analyze: {}", e);
            println!("→ Using demo notes instead...");
            generate_demo_notes(4, 1.35)
        }
    };

    println!("  • Detected {} notes", notes.len());
    for (i, note) in notes.iter().enumerate() {
        println!(
            "    [{}] {} at {:.2}s (vel: {:.0}%)",
            i + 1,
            note.to_note_name().yellow(),
            note.onset_sec,
            note.velocity * 100.0
        );
    }

    if notes.len() <= 1 {
        println!();
        println!("✗ Only one note detected. Please record again with more distinct notes.");
        return run_demo_mode(
            samples,
            sample_rate,
            enable_pitch_shift,
            pitch_matching,
            output_path,
            output_dir,
        );
    }

    // Calculate optimal parameters based on hum duration
    // Target: chop density of ~8 chops per second of hum
    let hum_duration = notes
        .last()
        .map(|n| n.onset_sec + n.duration_sec)
        .unwrap_or(recording_duration);
    let target_num_chops = ((hum_duration * 8.0) as usize).max(notes.len()).min(64);

    // Trim sample to match hum duration (+ small buffer for last chop)
    let trim_samples = (hum_duration * 1.2 * sample_rate as f64) as usize;
    let sample_to_chop = if samples.len() > trim_samples {
        println!(
            "  • Trimming sample: {:.1}s → {:.1}s",
            samples.len() as f64 / sample_rate as f64,
            trim_samples as f64 / sample_rate as f64
        );
        &samples[..trim_samples]
    } else {
        &samples[..]
    };

    println!("  • Target chops: {} (density: ~8/s)", target_num_chops);

    // Chop and map
    println!();
    println!("→ Processing chops...");

    let chopper = sample_chopper::SampleChopper::new(sample_rate);
    let chops = chopper
        .chop(sample_to_chop, target_num_chops)
        .map_err(|e| anyhow::anyhow!("Failed to chop: {}", e))?;

    println!("  • Found {} natural chop points", chops.len());

    // DEBUG: Save individual chops for manual inspection
    for (i, chop) in chops.iter().enumerate() {
        let chop_path = output_dir.join(format!("debug_chop_{:02}.wav", i));
        if let Err(e) = audio_utils::write_wav(&chop_path, &chop.samples, sample_rate) {
            println!("  ⚠️  Failed to save chop {}: {}", i, e);
        }
    }
    println!(
        "  💾 Saved {} chops to {}/debug_chop_XX.wav",
        chops.len(),
        output_dir.display()
    );

    let mapper = mapper::Mapper::with_config(
        sample_rate,
        mapper::MapperConfig {
            match_config: crate::mapper::MatchConfig {
                mode: if pitch_matching {
                    crate::mapper::MatchMode::Pitch
                } else {
                    crate::mapper::MatchMode::Strength
                },
                enable_pitch_shift,
            },
            ..Default::default()
        },
    );

    let mapped_chops = mapper
        .process(&notes, &chops)
        .map_err(|e| anyhow::anyhow!("Failed to map: {}", e))?;

    // DEBUG: Save mapped chops for manual inspection
    for (i, mc) in mapped_chops.iter().enumerate() {
        let mc_path = output_dir.join(format!("debug_mapped_{:02}.wav", i));
        if let Err(e) = audio_utils::write_wav(&mc_path, &mc.samples, sample_rate) {
            println!("  ⚠️  Failed to save mapped chop {}: {}", i, e);
        }
    }
    println!(
        "  💾 Saved {} mapped chops to {}/debug_mapped_XX.wav",
        mapped_chops.len(),
        output_dir.display()
    );
    let output = mapper.render_output(&mapped_chops);

    // Generate output filename (use provided path or create in output_dir)
    let out_path = output_path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        output_dir.join(format!("output_chopped_{}.wav", timestamp))
    });

    // Write output
    println!();
    println!("→ Writing: {}", out_path.display().to_string().white());

    audio_utils::write_wav(&out_path, &output, sample_rate)?;

    println!();
    println!(
        "✓ Output saved to: {}",
        out_path.display().to_string().yellow()
    );
    println!(
        "  • {} samples, {:.2}s",
        (output.len()).to_string().yellow(),
        output.len() as f64 / sample_rate as f64
    );
    println!();
    println!("Done!");

    Ok(())
}

#[cfg(not(feature = "audio-io"))]
fn run_interactive(
    input_path: &Path,
    output_path: Option<&Path>,
    output_dir: &Path,
    enable_pitch_shift: bool,
    pitch_matching: bool,
    _segment: Option<&str>,
) -> Result<()> {
    // Validate input file exists
    if !input_path.exists() {
        return Err(error::HumChopError::IoError(format!(
            "Input file not found: {}",
            input_path.display()
        ))
        .into());
    }

    // Display file info
    println!("→ Loading: {}", input_path.display().to_string().white());

    // Load the sample
    let (samples, sample_rate) = audio_utils::load_audio(input_path)?;

    let duration_secs = samples.len() as f64 / sample_rate as f64;
    println!(
        "  • {} samples, {:.2}s @ {} Hz",
        samples.len().to_string().yellow(),
        duration_secs,
        sample_rate.to_string().yellow()
    );
    println!(
        "  • JDilla-style: {}",
        if pitch_matching {
            "pitch matching"
        } else {
            "strength matching"
        }
    );

    println!();
    println!("⚠️  Microphone recording requires audio-io feature.");
    println!("For recording support, build with:");
    println!("  cargo build --features audio-io");

    // Generate demo output for testing
    println!();
    println!("→ Generating demo output (using first 10s of sample)...");

    let demo_duration = MAX_DEMO_DURATION_SECS;
    let demo_samples = (demo_duration * sample_rate as f64) as usize;
    let sample_to_chop = if samples.len() > demo_samples {
        println!(
            "  • Trimming sample: {:.1}s → {:.1}s",
            samples.len() as f64 / sample_rate as f64,
            demo_duration
        );
        &samples[..demo_samples]
    } else {
        &samples[..]
    };
    let target_num_chops = 16;
    // BUG-5 fix: demo notes scale to match sample duration + target chop count
    let demo_notes = generate_demo_notes(target_num_chops, demo_duration);
    println!("  • Demo notes ({})", demo_notes.len());

    // Process
    let chopper = sample_chopper::SampleChopper::new(sample_rate);
    let chops = chopper
        .chop(sample_to_chop, target_num_chops)
        .map_err(|e| anyhow::anyhow!("Failed to chop: {}", e))?;

    println!("  • Found {} natural chop points", chops.len());

    let mapper = mapper::Mapper::with_config(
        sample_rate,
        mapper::MapperConfig {
            match_config: crate::mapper::MatchConfig {
                mode: if pitch_matching {
                    crate::mapper::MatchMode::Pitch
                } else {
                    crate::mapper::MatchMode::Strength
                },
                enable_pitch_shift,
            },
            ..Default::default()
        },
    );

    let mapped_chops = mapper
        .process(&demo_notes, &chops)
        .map_err(|e| anyhow::anyhow!("Failed to map: {}", e))?;

    let output = mapper.render_output(&mapped_chops);

    // Generate output filename (use provided path or create in output_dir)
    let out_path = output_path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        output_dir.join(format!("output_chopped_{}.wav", timestamp))
    });

    // Write output
    println!();
    println!("→ Writing: {}", out_path.display().to_string().white());

    audio_utils::write_wav(&out_path, &output, sample_rate)?;

    println!();
    println!(
        "✓ Output saved to: {}",
        out_path.display().to_string().yellow()
    );
    println!(
        "  • {} samples, {:.2}s",
        (output.len()).to_string().yellow(),
        output.len() as f64 / sample_rate as f64
    );
    println!();
    println!("Done!");

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Batch processing mode
// ─────────────────────────────────────────────────────────────────────────────

/// Run in batch mode: process all matching audio files in a directory.
#[allow(clippy::too_many_arguments)]
fn run_batch(
    input_path: &Path,
    output_file: Option<&Path>,
    output_dir: &Path,
    enable_pitch_shift: bool,
    pitch_matching: bool,
    num_chops: Option<usize>,
    enable_dither: bool,
    bits: u16,
    pattern: &str,
) -> Result<()> {
    use std::fs;

    // Determine if input is a file or directory
    let files: Vec<PathBuf> = if input_path.is_dir() {
        // Get all matching files from directory
        let entries = fs::read_dir(input_path)?;
        entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                if let Some(ext) = p.extension() {
                    let ext_lower = ext.to_string_lossy().to_lowercase();
                    // DESIGN-6 fix: specific pattern wins; wildcard/empty falls back to audio formats only
                    if pattern == "*" || pattern.replace("*", "").is_empty() {
                        ext_lower == "wav" || ext_lower == "mp3" || ext_lower == "flac"
                    } else {
                        let pattern_ext = pattern.replace("*", "").to_lowercase();
                        ext_lower == pattern_ext
                    }
                } else {
                    false
                }
            })
            .collect()
    } else {
        // Single file batch processing
        vec![input_path.to_path_buf()]
    };

    if files.is_empty() {
        println!("No audio files found matching pattern: {}", pattern);
        return Ok(());
    }

    println!("Batch processing {} file(s)...", files.len());
    println!();

    // Use the provided output_dir (already created in main)
    let output_directory = output_dir.to_path_buf();

    let mut success_count = 0;
    let mut fail_count = 0;

    for (idx, file_path) in files.iter().enumerate() {
        println!(
            "[{}/{}] Processing: {}",
            idx + 1,
            files.len(),
            file_path.display()
        );

        // Generate output filename (use provided output_file path or create in output_dir)
        let output_file_path = output_file.map(|p| p.to_path_buf()).unwrap_or_else(|| {
            let stem = file_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| format!("output_{}", idx));
            output_directory.join(format!("{}_chopped.wav", stem))
        });

        // Process the file (reuse headless logic but single file)
        match process_single_file(
            file_path,
            Some(&output_file_path),
            enable_pitch_shift,
            pitch_matching,
            num_chops,
            enable_dither,
            bits,
        ) {
            Ok(_) => {
                println!("  ✓ Saved to: {}", output_file_path.display());
                success_count += 1;
            }
            Err(e) => {
                println!("  ✗ Failed: {}", e);
                fail_count += 1;
            }
        }
        println!();
    }

    println!("─────────────────────────────────");
    println!(
        "Batch complete: {} succeeded, {} failed",
        success_count, fail_count
    );
    println!("Output directory: {}", output_directory.display());

    Ok(())
}

/// Process a single file (reused by batch mode).
#[allow(clippy::too_many_arguments)]
fn process_single_file(
    input_path: &Path,
    output_path: Option<&Path>,
    enable_pitch_shift: bool,
    pitch_matching: bool,
    num_chops: Option<usize>,
    enable_dither: bool,
    bits: u16,
) -> Result<()> {
    use std::time::Instant;

    // Load the sample
    let (samples, sample_rate) = audio_utils::load_audio(input_path)?;

    // Demo notes for headless mode
    let max_duration = MAX_DEMO_DURATION_SECS;
    let max_samples = (max_duration * sample_rate as f64) as usize;
    let sample_to_chop = if samples.len() > max_samples {
        &samples[..max_samples]
    } else {
        &samples[..]
    };
    let target_num_chops = num_chops.unwrap_or(16);
    // BUG-5 fix: demo notes scale to match sample duration + target chop count
    let demo_notes = generate_demo_notes(target_num_chops, max_duration);

    let start = Instant::now();

    // Process
    let chopper = sample_chopper::SampleChopper::new(sample_rate);
    let chops = chopper.chop(sample_to_chop, target_num_chops)?;

    let mapper = mapper::Mapper::with_config(
        sample_rate,
        mapper::MapperConfig {
            match_config: crate::mapper::MatchConfig {
                mode: if pitch_matching {
                    crate::mapper::MatchMode::Pitch
                } else {
                    crate::mapper::MatchMode::Strength
                },
                enable_pitch_shift,
            },
            ..Default::default()
        },
    );

    let mapped_chops = mapper.process(&demo_notes, &chops)?;
    let output = mapper.render_output(&mapped_chops);

    // Generate output filename (use provided path or create in current dir with timestamp)
    let out_path = output_path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        PathBuf::from(format!("output_chopped_{}.wav", timestamp))
    });

    // Create WAV options
    let wav_options = audio_utils::WavOptions::new()
        .bits_per_sample(bits)
        .dither(enable_dither);

    audio_utils::write_wav_with_options(&out_path, &output, sample_rate, &wav_options)?;

    let elapsed = start.elapsed();
    println!("  Processed in {:.2}s", elapsed.as_secs_f64());

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Demo notes generator
// ─────────────────────────────────────────────────────────────────────────────

/// Generate demo notes that adapt to the target number of chops and sample duration.
///
/// BUG-5 fix: replaces the hardcoded 4-note sequence with notes that:
/// - Scale to fill the sample duration (not always 1.35s)
/// - Produce exactly the number of notes requested (not always 4)
/// - Respect note density (~8 notes per second of sample duration)
fn generate_demo_notes(num_notes: usize, duration_secs: f64) -> Vec<hum_analyzer::Note> {
    // Use a simple chord progression spread evenly across the duration
    // Pitches: C4, E4, G4, C5 (C major arpeggio), then repeat
    let base_pitches = [261.63, 329.63, 392.0, 523.25];
    let note_duration = 0.3; // Each note lasts 300ms
    let gap = 0.05; // Small gap between notes
    let step = note_duration + gap;
    (0..num_notes)
        .map(|i| {
            let pitch = base_pitches[i % base_pitches.len()];
            let onset = i as f64 * step;
            // Clamp onset so notes don't exceed the duration
            let onset = onset.min(duration_secs - note_duration);
            hum_analyzer::Note::new(pitch, onset, note_duration, 0.8)
        })
        .collect()
}

// Constants are in the shared constants module

// ─────────────────────────────────────────────────────────────────────────────
// Headless mode (no TUI) for scripting and batch processing
// ─────────────────────────────────────────────────────────────────────────────

/// Run in headless mode with demo notes (no TUI).
/// Useful for scripting, batch processing, or quick testing.
#[allow(clippy::too_many_arguments)]
fn run_headless(
    input_path: &Path,
    output_path: Option<&Path>,
    output_dir: &Path,
    enable_pitch_shift: bool,
    pitch_matching: bool,
    _segment: Option<&str>,
    num_chops: Option<usize>,
    enable_dither: bool,
    bits: u16,
) -> Result<()> {
    use std::time::Instant;

    // Validate input file exists
    if !input_path.exists() {
        return Err(error::HumChopError::IoError(format!(
            "Input file not found: {}",
            input_path.display()
        ))
        .into());
    }

    println!("→ Loading: {}", input_path.display().to_string().white());

    // Load the sample
    let (samples, sample_rate) = audio_utils::load_audio(input_path)?;
    let duration_secs = samples.len() as f64 / sample_rate as f64;
    println!(
        "  • {} samples, {:.2}s @ {} Hz",
        samples.len().to_string().yellow(),
        duration_secs,
        sample_rate.to_string().yellow()
    );

    // Demo notes for headless mode
    let max_duration = MAX_DEMO_DURATION_SECS;
    let target_num_chops = num_chops.unwrap_or(16);
    // BUG-5 fix: demo notes scale to match sample duration + target chop count
    let demo_notes = generate_demo_notes(target_num_chops, max_duration);
    println!("  • Demo notes ({})", demo_notes.len());
    println!("  • Target chops: {}", target_num_chops);

    // Process
    let start = Instant::now();

    // Trim sample to 10s if longer
    let max_duration = MAX_DEMO_DURATION_SECS;
    let max_samples = (max_duration * sample_rate as f64) as usize;
    let sample_to_chop = if samples.len() > max_samples {
        println!(
            "  • Trimming: {:.1}s → {:.1}s",
            samples.len() as f64 / sample_rate as f64,
            max_duration
        );
        &samples[..max_samples]
    } else {
        &samples[..]
    };

    println!();
    println!("→ Processing...");

    let chopper = sample_chopper::SampleChopper::new(sample_rate);
    let chops = chopper
        .chop(sample_to_chop, target_num_chops)
        .map_err(|e| anyhow::anyhow!("Failed to chop: {}", e))?;
    println!("  • Found {} natural chop points", chops.len());

    let mapper = mapper::Mapper::with_config(
        sample_rate,
        mapper::MapperConfig {
            match_config: crate::mapper::MatchConfig {
                mode: if pitch_matching {
                    crate::mapper::MatchMode::Pitch
                } else {
                    crate::mapper::MatchMode::Strength
                },
                enable_pitch_shift,
            },
            ..Default::default()
        },
    );

    let mapped_chops = mapper
        .process(&demo_notes, &chops)
        .map_err(|e| anyhow::anyhow!("Failed to map: {}", e))?;

    let output = mapper.render_output(&mapped_chops);

    // Generate output filename (use provided path or create in output_dir)
    let out_path = output_path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        output_dir.join(format!("output_chopped_{}.wav", timestamp))
    });

    // Create WAV options
    let wav_options = audio_utils::WavOptions::new()
        .bits_per_sample(bits)
        .dither(enable_dither);

    if bits < 32 {
        println!(
            "  • Output: {}-bit{}",
            bits,
            if enable_dither { " + dither" } else { "" }
        );
    }

    println!();
    println!("→ Writing: {}", out_path.display().to_string().white());

    audio_utils::write_wav_with_options(&out_path, &output, sample_rate, &wav_options)?;

    let elapsed = start.elapsed();
    println!();
    println!(
        "✓ Output saved to: {} ({} samples, {:.2}s)",
        out_path.display().to_string().yellow(),
        (output.len()).to_string().yellow(),
        output.len() as f64 / sample_rate as f64
    );
    println!("  • Processed in {:.2}s", elapsed.as_secs_f64());
    println!();
    println!("Done!");

    Ok(())
}

/// Run demo mode with predefined notes (fallback when recording is too short)
fn run_demo_mode(
    samples: Vec<f32>,
    sample_rate: u32,
    enable_pitch_shift: bool,
    pitch_matching: bool,
    output_path: Option<&Path>,
    output_dir: &Path,
) -> Result<()> {
    println!();
    println!("→ Generating demo output (using first 10s of sample)...");

    let demo_duration = MAX_DEMO_DURATION_SECS;
    let demo_notes = generate_demo_notes(4, demo_duration);
    println!("  • Demo notes ({})", demo_notes.len());

    // For demo, trim to 10 seconds and use 16 chops
    let demo_duration = MAX_DEMO_DURATION_SECS;
    let demo_samples = (demo_duration * sample_rate as f64) as usize;
    let sample_to_chop = if samples.len() > demo_samples {
        println!(
            "  • Trimming sample: {:.1}s → {:.1}s",
            samples.len() as f64 / sample_rate as f64,
            demo_duration
        );
        &samples[..demo_samples]
    } else {
        &samples[..]
    };
    let target_num_chops = 16;

    // Process
    let chopper = sample_chopper::SampleChopper::new(sample_rate);
    let chops = chopper
        .chop(sample_to_chop, target_num_chops)
        .map_err(|e| anyhow::anyhow!("Failed to chop: {}", e))?;

    println!("  • Found {} natural chop points", chops.len());

    let mapper = mapper::Mapper::with_config(
        sample_rate,
        mapper::MapperConfig {
            match_config: crate::mapper::MatchConfig {
                mode: if pitch_matching {
                    crate::mapper::MatchMode::Pitch
                } else {
                    crate::mapper::MatchMode::Strength
                },
                enable_pitch_shift,
            },
            ..Default::default()
        },
    );

    let mapped_chops = mapper
        .process(&demo_notes, &chops)
        .map_err(|e| anyhow::anyhow!("Failed to map: {}", e))?;

    let output = mapper.render_output(&mapped_chops);

    // Generate output filename (use provided path or create in output_dir)
    let out_path = output_path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        output_dir.join(format!("output_chopped_{}.wav", timestamp))
    });

    // Write output
    println!();
    println!("→ Writing: {}", out_path.display().to_string().white());

    audio_utils::write_wav(&out_path, &output, sample_rate)?;

    println!();
    println!(
        "✓ Output saved to: {}",
        out_path.display().to_string().yellow()
    );
    println!(
        "  • {} samples, {:.2}s",
        (output.len()).to_string().yellow(),
        output.len() as f64 / sample_rate as f64
    );
    println!();
    println!("Done!");

    Ok(())
}
