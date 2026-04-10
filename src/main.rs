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

    // Create analyzer and mapper
    let _analyzer = hum_analyzer::HumAnalyzer::new(sample_rate);
    let mut mapper_config = mapper::MapperConfig::default();
    mapper_config.enable_pitch_shift = enable_pitch_shift;

    let mapper = mapper::Mapper::with_config(sample_rate, mapper_config);

    println!();
    println!(
        "{} Ready to record hum. Start humming to auto-detect notes.",
        "→".cyan()
    );
    println!(
        "{} {}",
        "•".dimmed(),
        "Recording will auto-stop at 15 seconds".dimmed()
    );

    // Note: In a full implementation, we would:
    // 1. Initialize cpal for microphone recording
    // 2. Start recording when user presses a key
    // 3. Process the hum data in real-time or after recording
    // 4. Generate the output

    println!();
    println!(
        "{}",
        "TUI mode requires cpal with pulseaudio feature.".yellow()
    );
    println!("{}", "For full functionality, build with:".yellow());
    println!("{}", "  cargo build --features audio-io".yellow());

    // Generate demo output for testing
    println!();
    println!(
        "{} Generating demo output (TUI recording not available)...",
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
