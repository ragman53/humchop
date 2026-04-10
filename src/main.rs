//! HumChop - Hum-to-chop sampling tool
//!
//! Chop audio samples by humming melodies.
//! Record a hum → Analyze pitch → Auto-chop your samples using JDilla-style processing.

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
}

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    // Display welcome message
    println!(
        "{}",
        "HumChop v0.2.0 - Hum-to-chop sampling tool".green().bold()
    );
    println!("{}", "━".repeat(40).dimmed());
    println!();

    match args.input {
        Some(input_path) => {
            if args.no_tui {
                // Headless mode: process with demo notes, no TUI
                run_headless(
                    input_path.as_path(),
                    args.output.as_deref(),
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
            println!("  humchop beat.mp3 --no-tui --num-chops 8  # headless");
        }
    }

    Ok(())
}

#[cfg(feature = "audio-io")]
fn run_interactive(
    input_path: &Path,
    output_path: Option<&Path>,
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
    let max_duration = Duration::from_secs_f64(15.0);

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
        print!("\r🔴 {:.1}s / 15.0s ", elapsed.min(15.0));
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
            vec![
                hum_analyzer::Note::new(440.0, 0.0, 0.3, 0.8),
                hum_analyzer::Note::new(523.0, 0.35, 0.3, 0.7),
                hum_analyzer::Note::new(659.0, 0.7, 0.3, 0.9),
                hum_analyzer::Note::new(784.0, 1.05, 0.3, 0.85),
            ]
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

    let mapper = mapper::Mapper::with_config(
        sample_rate,
        mapper::MapperConfig {
            enable_pitch_shift,
            strength_matching: !pitch_matching,
            ..Default::default()
        },
    );

    let mapped_chops = mapper
        .process(&notes, &chops)
        .map_err(|e| anyhow::anyhow!("Failed to map: {}", e))?;

    let output = mapper.render_output(&mapped_chops);

    // Generate output filename
    let out_path = output_path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        PathBuf::from(format!("output_chopped_{}.wav", timestamp))
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
    enable_pitch_shift: bool,
    pitch_matching: bool,
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
    println!("⚠️  Microphone recording requires audio-io feature.".yellow());
    println!("For recording support, build with:".yellow());
    println!("  cargo build --features audio-io".yellow());

    // Generate demo output for testing
    println!();
    println!("→ Generating demo output (using first 10s of sample)...");

    let demo_notes = vec![
        hum_analyzer::Note::new(440.0, 0.0, 0.3, 0.8),
        hum_analyzer::Note::new(523.0, 0.35, 0.3, 0.7),
        hum_analyzer::Note::new(659.0, 0.7, 0.3, 0.9),
        hum_analyzer::Note::new(784.0, 1.05, 0.3, 0.85),
    ];

    println!(
        "  • Demo notes: {:?}",
        demo_notes
            .iter()
            .map(|n| n.to_note_name())
            .collect::<Vec<_>>()
    );

    // For demo, trim to 10 seconds and use 16 chops
    let demo_duration = 10.0;
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
            enable_pitch_shift,
            strength_matching: !pitch_matching,
            ..Default::default()
        },
    );

    let mapped_chops = mapper
        .process(&demo_notes, &chops)
        .map_err(|e| anyhow::anyhow!("Failed to map: {}", e))?;

    let output = mapper.render_output(&mapped_chops);

    // Generate output filename
    let out_path = output_path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        PathBuf::from(format!("output_chopped_{}.wav", timestamp))
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
// Headless mode (no TUI) for scripting and batch processing
// ─────────────────────────────────────────────────────────────────────────────

/// Run in headless mode with demo notes (no TUI).
/// Useful for scripting, batch processing, or quick testing.
#[allow(clippy::too_many_arguments)]
fn run_headless(
    input_path: &Path,
    output_path: Option<&Path>,
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
    let demo_notes = vec![
        hum_analyzer::Note::new(440.0, 0.0, 0.3, 0.8),
        hum_analyzer::Note::new(523.0, 0.35, 0.3, 0.7),
        hum_analyzer::Note::new(659.0, 0.7, 0.3, 0.9),
        hum_analyzer::Note::new(784.0, 1.05, 0.3, 0.85),
    ];
    println!("  • Demo notes: {:?}", demo_notes.iter().map(|n| n.to_note_name()).collect::<Vec<_>>());

    // Determine chop count
    let target_num_chops = num_chops.unwrap_or(16);
    println!("  • Target chops: {}", target_num_chops);

    // Process
    let start = Instant::now();

    // Trim sample to 10s if longer
    let max_duration = 10.0;
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
            enable_pitch_shift,
            strength_matching: !pitch_matching,
            ..Default::default()
        },
    );

    let mapped_chops = mapper
        .process(&demo_notes, &chops)
        .map_err(|e| anyhow::anyhow!("Failed to map: {}", e))?;

    let output = mapper.render_output(&mapped_chops);

    // Generate output filename
    let out_path = output_path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        PathBuf::from(format!("output_chopped_{}.wav", timestamp))
    });

    // Create WAV options
    let wav_options = audio_utils::WavOptions::new()
        .bits_per_sample(bits)
        .dither(enable_dither);

    if bits < 32 {
        println!("  • Output: {}-bit{}", bits, if enable_dither { " + dither" } else { "" });
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
) -> Result<()> {
    println!();
    println!("→ Generating demo output (using first 10s of sample)...");

    let demo_notes = vec![
        hum_analyzer::Note::new(440.0, 0.0, 0.3, 0.8),
        hum_analyzer::Note::new(523.0, 0.35, 0.3, 0.7),
        hum_analyzer::Note::new(659.0, 0.7, 0.3, 0.9),
        hum_analyzer::Note::new(784.0, 1.05, 0.3, 0.85),
    ];

    println!(
        "  • Demo notes: {:?}",
        demo_notes
            .iter()
            .map(|n| n.to_note_name())
            .collect::<Vec<_>>()
    );

    // For demo, trim to 10 seconds and use 16 chops
    let demo_duration = 10.0;
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
            enable_pitch_shift,
            strength_matching: !pitch_matching,
            ..Default::default()
        },
    );

    let mapped_chops = mapper
        .process(&demo_notes, &chops)
        .map_err(|e| anyhow::anyhow!("Failed to map: {}", e))?;

    let output = mapper.render_output(&mapped_chops);

    // Generate output filename
    let out_path = output_path.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        PathBuf::from(format!("output_chopped_{}.wav", timestamp))
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
