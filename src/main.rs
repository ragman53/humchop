//! HumChop - Hum-to-chop sampling tool
//!
//! Chop audio samples by humming melodies.
//! Record a hum → Analyze pitch → Auto-chop your samples.

mod audio_utils;
mod error;
mod hum_analyzer;
mod mapper;
mod sample_chopper;
mod tui;

#[cfg(feature = "audio-io")]
mod recorder;

use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use std::path::PathBuf;

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

    /// Enable TUI mode (interactive)
    #[arg(short, long, default_value_t = true)]
    tui: bool,

    /// Enable pitch shifting (slower but more accurate)
    #[arg(long)]
    pitch_shift: bool,

    /// Chop mode: 'equal' or 'onset'
    #[arg(short, long, value_name = "MODE", default_value = "equal")]
    chop_mode: String,
}

fn main() -> Result<()> {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    // Display welcome message
    println!(
        "{}",
        "HumChop v0.1.0 - Hum-to-chop sampling tool".green().bold()
    );
    println!("{}", "━".repeat(40).dimmed());
    println!();

    match args.input {
        Some(input_path) => {
            run_interactive(
                &input_path,
                args.output.as_ref(),
                args.pitch_shift,
                &args.chop_mode,
            )?;
        }
        None => {
            // Show help
            println!("Usage: humchop <audio_file> [options]");
            println!();
            println!("Options:");
            println!("  -o, --output <file>    Output file path");
            println!("  -m, --chop-mode <mode> Chop mode: 'equal' or 'onset'");
            println!("      --pitch-shift      Enable pitch shifting");
            println!();
            println!("Example:");
            println!("  humchop sample.wav");
            println!("  humchop beat.mp3 -m onset -o my_chops.wav");
        }
    }

    Ok(())
}

#[cfg(feature = "audio-io")]
mod recorder_integration {
    use super::*;
    use crate::recorder::{list_input_devices, Recorder, MAX_RECORDING_DURATION_SECS};

    /// Run interactive mode with microphone recording support.
    pub fn run_with_recording(
        input_path: &PathBuf,
        output_path: Option<&PathBuf>,
        enable_pitch_shift: bool,
        chop_mode_str: &str,
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
        println!(
            "{} Loading: {}",
            "→".cyan(),
            input_path.display().to_string().white()
        );

        // Load the sample
        let (samples, sample_rate) = audio_utils::load_audio(input_path)?;

        let duration_secs = samples.len() as f64 / sample_rate as f64;
        println!(
            "  {} {} samples, {:.2}s @ {} Hz",
            "•".dimmed(),
            samples.len().to_string().yellow(),
            duration_secs,
            sample_rate.to_string().yellow()
        );

        // Parse chop mode
        let chop_mode = match chop_mode_str.to_lowercase().as_str() {
            "onset" => sample_chopper::ChopMode::Onset,
            _ => sample_chopper::ChopMode::Equal,
        };

        println!(
            "  {} Chop mode: {}",
            "•".dimmed(),
            match chop_mode {
                sample_chopper::ChopMode::Equal => "Equal Division",
                sample_chopper::ChopMode::Onset => "Onset Detection",
            }
            .yellow()
        );

        // List available input devices
        let devices = list_input_devices();
        println!();
        println!("{} Available input devices: {}", "→".cyan(), devices.len());
        for (i, device) in devices.iter().enumerate() {
            println!("  {} {}", format!("[{}]", i + 1).dimmed(), device);
        }

        // Initialize recorder
        let mut recorder = Recorder::new();

        println!();
        println!(
            "{} Press Enter to start recording (or type 'demo' for demo mode)...",
            "→".cyan()
        );

        // For now, use demo mode (full recording implementation goes in TUI)
        println!();
        println!(
            "{} Starting demo mode with simulated recording...",
            "→".cyan()
        );

        // Generate demo notes for testing
        let demo_notes = vec![
            hum_analyzer::Note::new(440.0, 0.0, 0.3, 0.8),
            hum_analyzer::Note::new(523.0, 0.35, 0.3, 0.7),
            hum_analyzer::Note::new(659.0, 0.7, 0.3, 0.9),
            hum_analyzer::Note::new(784.0, 1.05, 0.3, 0.85),
        ];

        println!(
            "  {} Demo notes: {:?}",
            "•".dimmed(),
            demo_notes
                .iter()
                .map(|n| n.to_note_name())
                .collect::<Vec<_>>()
        );

        // Process
        let chopper = sample_chopper::SampleChopper::new(sample_rate);
        let chops = chopper
            .chop(&samples, demo_notes.len(), chop_mode)
            .map_err(|e| anyhow::anyhow!("Failed to chop: {}", e))?;

        let mut mapper_config = mapper::MapperConfig::default();
        mapper_config.enable_pitch_shift = enable_pitch_shift;
        let mapper = mapper::Mapper::with_config(sample_rate, mapper_config);

        let mapped_chops = mapper
            .process(&demo_notes, &chops)
            .map_err(|e| anyhow::anyhow!("Failed to map: {}", e))?;

        let output = mapper.render_output(&mapped_chops);

        // Generate output filename
        let out_path = output_path.map(|p| p.clone()).unwrap_or_else(|| {
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
            PathBuf::from(format!("output_chopped_{}.wav", timestamp))
        });

        // Write output
        println!();
        println!(
            "{} Writing: {}",
            "→".cyan(),
            out_path.display().to_string().white()
        );

        audio_utils::write_wav(&out_path, &output, sample_rate)?;

        println!();
        println!(
            "{} Output saved to: {}",
            "✓".green(),
            out_path.display().to_string().yellow()
        );
        println!(
            "  {} {} samples, {:.2}s",
            "•".dimmed(),
            output.len().to_string().yellow(),
            output.len() as f64 / sample_rate as f64
        );

        // Show playback hint
        println!();
        println!(
            "  {} Play with: ffplay {}",
            "•".dimmed(),
            out_path.display()
        );

        // Cleanup recorder
        drop(recorder);

        Ok(())
    }
}

#[cfg(feature = "audio-io")]
fn run_interactive(
    input_path: &PathBuf,
    output_path: Option<&PathBuf>,
    enable_pitch_shift: bool,
    chop_mode_str: &str,
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
    println!(
        "{} Loading: {}",
        "→".cyan(),
        input_path.display().to_string().white()
    );

    // Load the sample
    let (samples, sample_rate) = audio_utils::load_audio(input_path)?;

    let duration_secs = samples.len() as f64 / sample_rate as f64;
    println!(
        "  {} {} samples, {:.2}s @ {} Hz",
        "•".dimmed(),
        samples.len().to_string().yellow(),
        duration_secs,
        sample_rate.to_string().yellow()
    );

    // Parse chop mode
    let chop_mode = match chop_mode_str.to_lowercase().as_str() {
        "onset" => sample_chopper::ChopMode::Onset,
        _ => sample_chopper::ChopMode::Equal,
    };

    println!(
        "  {} Chop mode: {}",
        "•".dimmed(),
        match chop_mode {
            sample_chopper::ChopMode::Equal => "Equal Division",
            sample_chopper::ChopMode::Onset => "Onset Detection",
        }
        .yellow()
    );

    // Initialize recorder
    let mut recorder = Recorder::new();

    println!();
    println!("{} {}", "→".cyan(), "🎤 Microphone Recording Ready".bold());
    println!(
        "{}",
        "Press Enter to start recording, then hum your melody...".dimmed()
    );
    println!("{}", "(Recording auto-stops after 15 seconds)".dimmed());

    // Wait for user to press Enter to start
    print!("\nPress Enter to start recording...");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    // Start recording
    println!();
    println!("{} Recording... (Press Enter to stop)", "🔴".red());

    let recording_start = Instant::now();
    let max_duration = Duration::from_secs_f64(15.0);

    // Start the cpal recording
    let tokio_receiver = match recorder.start_recording() {
        Ok(rx) => rx,
        Err(e) => {
            println!("{} Failed to start recording: {}", "✗".red(), e);
            return Err(e.into());
        }
    };

    // Collect audio data
    let mut hum_samples: Vec<f32> = Vec::new();

    // Create a bounded std sync channel to collect audio (buffer of 100 messages)
    let (audio_tx, audio_rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(100);
    let audio_tx_clone = audio_tx.clone();

    // Spawn a thread to forward tokio mpsc to std mpsc
    let forward_handle = std::thread::spawn(move || {
        let receiver = tokio_receiver;
        // Create a tokio runtime for blocking operations
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime");
        
        rt.block_on(async {
            let mut receiver = receiver;
            loop {
                match receiver.recv().await {
                    Some(samples) => {
                        // send() blocks until receiver receives
                        if audio_tx_clone.send(samples).is_err() {
                            break;
                        }
                    }
                    None => {
                        break; // Channel closed
                    }
                }
            }
        });
    });

    // Create a channel for user input to stop recording
    let (stop_tx, stop_rx) = std::sync::mpsc::channel();

    // Spawn a thread to listen for Enter key
    std::thread::spawn(move || {
        let mut line = String::new();
        io::stdin().read_line(&mut line).ok();
        stop_tx.send(()).ok();
    });

    // Collect audio until user stops or timeout
    loop {
        // Check if user wants to stop
        if stop_rx.try_recv().is_ok() {
            println!("{} Stopped by user", "■".yellow());
            break;
        }

        // Check for timeout
        if recording_start.elapsed() >= max_duration {
            println!("{} Auto-stopped at 15 seconds", "■".yellow());
            break;
        }

        // Try to receive audio data
        match audio_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(samples) => {
                if !samples.is_empty() {
                    hum_samples.extend_from_slice(&samples);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // No data available, continue
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }

        // Print elapsed time every second
        let elapsed = recording_start.elapsed().as_secs_f64();
        if (elapsed * 2.0).fract() < 0.05 {
            print!("\r{} {:.1}s / 15.0s ", "🔴", elapsed.min(15.0));
            io::stdout().flush().ok();
        }
    }

    println!();

    // Stop recording and cleanup
    recorder.stop_recording();
    drop(audio_tx); // Signal end to forward thread
    
    // Drain any remaining audio from the channel
    loop {
        match audio_rx.recv_timeout(Duration::from_millis(10)) {
            Ok(samples) => {
                if !samples.is_empty() {
                    hum_samples.extend_from_slice(&samples);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Try again
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
        }
        // Safety: break after a few iterations
        if hum_samples.len() > 100000 {
            break;
        }
    }
    
    forward_handle.join().ok();

    let recording_duration = recording_start.elapsed().as_secs_f64();
    println!(
        "  {} Recorded {:.2} seconds of audio",
        "•".dimmed(),
        recording_duration
    );
    println!("  {} {} samples collected", "•".dimmed(), hum_samples.len());

    // Check if we have enough audio
    if hum_samples.len() < sample_rate as usize / 10 {
        println!();
        println!(
            "{} Recording too short! Please record again with more audio.",
            "✗".red()
        );
        return Ok(()); // Exit gracefully
    }

    // Process the recording
    println!();
    println!("{} Analyzing pitch...", "→".cyan());

    let analyzer = hum_analyzer::HumAnalyzer::new(sample_rate);

    let notes = match analyzer.transcribe(&hum_samples) {
        Ok(n) => n,
        Err(e) => {
            println!("{} Failed to analyze: {}", "✗".red(), e);
            // Fall back to demo notes for testing
            println!("{} Using demo notes instead...", "→".yellow());
            vec![
                hum_analyzer::Note::new(440.0, 0.0, 0.3, 0.8),
                hum_analyzer::Note::new(523.0, 0.35, 0.3, 0.7),
                hum_analyzer::Note::new(659.0, 0.7, 0.3, 0.9),
                hum_analyzer::Note::new(784.0, 1.05, 0.3, 0.85),
            ]
        }
    };

    println!("  {} Detected {} notes", "•".dimmed(), notes.len());
    for (i, note) in notes.iter().enumerate() {
        println!(
            "    [{}] {} at {:.2}s (vel: {:.0}%)",
            i + 1,
            note.to_note_name().yellow(),
            note.onset_sec,
            note.velocity * 100.0
        );
    }

    // Check for single note detection
    if notes.len() <= 1 {
        println!();
        println!(
            "{} Only one note detected. Please record again with more distinct notes.",
            "✗".yellow()
        );
        return Ok(());
    }

    // Chop and map
    println!();
    println!("{} Processing chops...", "→".cyan());

    let chopper = sample_chopper::SampleChopper::new(sample_rate);
    let chops = chopper
        .chop(&samples, notes.len(), chop_mode)
        .map_err(|e| anyhow::anyhow!("Failed to chop: {}", e))?;

    let mut mapper_config = mapper::MapperConfig::default();
    mapper_config.enable_pitch_shift = enable_pitch_shift;
    let mapper = mapper::Mapper::with_config(sample_rate, mapper_config);

    let mapped_chops = mapper
        .process(&notes, &chops)
        .map_err(|e| anyhow::anyhow!("Failed to map: {}", e))?;

    let output = mapper.render_output(&mapped_chops);

    // Generate output filename
    let out_path = output_path.map(|p| p.clone()).unwrap_or_else(|| {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        PathBuf::from(format!("output_chopped_{}.wav", timestamp))
    });

    // Write output
    println!();
    println!(
        "{} Writing: {}",
        "→".cyan(),
        out_path.display().to_string().white()
    );

    audio_utils::write_wav(&out_path, &output, sample_rate)?;

    println!();
    println!(
        "{} Output saved to: {}",
        "✓".green(),
        out_path.display().to_string().yellow()
    );
    println!(
        "  {} {} samples, {:.2}s",
        "•".dimmed(),
        output.len().to_string().yellow(),
        output.len() as f64 / sample_rate as f64
    );

    // Show playback hint
    println!();
    println!(
        "  {} Play with: ffplay {}",
        "•".dimmed(),
        out_path.display()
    );

    Ok(())
}

#[cfg(not(feature = "audio-io"))]
fn run_interactive(
    input_path: &PathBuf,
    output_path: Option<&PathBuf>,
    enable_pitch_shift: bool,
    chop_mode_str: &str,
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
    println!(
        "{} Loading: {}",
        "→".cyan(),
        input_path.display().to_string().white()
    );

    // Load the sample
    let (samples, sample_rate) = audio_utils::load_audio(input_path)?;

    let duration_secs = samples.len() as f64 / sample_rate as f64;
    println!(
        "  {} {} samples, {:.2}s @ {} Hz",
        "•".dimmed(),
        samples.len().to_string().yellow(),
        duration_secs,
        sample_rate.to_string().yellow()
    );

    // Parse chop mode
    let chop_mode = match chop_mode_str.to_lowercase().as_str() {
        "onset" => sample_chopper::ChopMode::Onset,
        _ => sample_chopper::ChopMode::Equal,
    };

    println!(
        "  {} Chop mode: {}",
        "•".dimmed(),
        match chop_mode {
            sample_chopper::ChopMode::Equal => "Equal Division",
            sample_chopper::ChopMode::Onset => "Onset Detection",
        }
        .yellow()
    );

    println!();
    println!(
        "{}",
        "⚠️  Microphone recording requires audio-io feature.".yellow()
    );
    println!("{}", "For recording support, build with:".yellow());
    println!("{}", "  cargo build --features audio-io".yellow());

    // Generate demo output for testing
    println!();
    println!("{} Generating demo output...", "→".cyan());

    // Generate demo notes for testing
    let demo_notes = vec![
        hum_analyzer::Note::new(440.0, 0.0, 0.3, 0.8),
        hum_analyzer::Note::new(523.0, 0.35, 0.3, 0.7),
        hum_analyzer::Note::new(659.0, 0.7, 0.3, 0.9),
        hum_analyzer::Note::new(784.0, 1.05, 0.3, 0.85),
    ];

    println!(
        "  {} Demo notes: {:?}",
        "•".dimmed(),
        demo_notes
            .iter()
            .map(|n| n.to_note_name())
            .collect::<Vec<_>>()
    );

    // Process
    let chopper = sample_chopper::SampleChopper::new(sample_rate);
    let chops = chopper
        .chop(&samples, demo_notes.len(), chop_mode)
        .map_err(|e| anyhow::anyhow!("Failed to chop: {}", e))?;

    let mut mapper_config = mapper::MapperConfig::default();
    mapper_config.enable_pitch_shift = enable_pitch_shift;
    let mapper = mapper::Mapper::with_config(sample_rate, mapper_config);

    let mapped_chops = mapper
        .process(&demo_notes, &chops)
        .map_err(|e| anyhow::anyhow!("Failed to map: {}", e))?;

    let output = mapper.render_output(&mapped_chops);

    // Generate output filename
    let out_path = output_path.map(|p| p.clone()).unwrap_or_else(|| {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        PathBuf::from(format!("output_chopped_{}.wav", timestamp))
    });

    // Write output
    println!();
    println!(
        "{} Writing: {}",
        "→".cyan(),
        out_path.display().to_string().white()
    );

    audio_utils::write_wav(&out_path, &output, sample_rate)?;

    println!();
    println!(
        "{} Output saved to: {}",
        "✓".green(),
        out_path.display().to_string().yellow()
    );
    println!(
        "  {} {} samples, {:.2}s",
        "•".dimmed(),
        output.len().to_string().yellow(),
        output.len() as f64 / sample_rate as f64
    );

    // Show playback hint
    println!();
    println!(
        "  {} Play with: ffplay {}",
        "•".dimmed(),
        out_path.display()
    );

    Ok(())
}
