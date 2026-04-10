# HumChop - Project Context

## Project Overview

**HumChop** is a Rust command-line tool and TUI application for "hum-to-chop" sampling — a creative workflow where a user hums a melody, and the program analyzes the humming to automatically chop and reassemble segments of a source audio sample to match the hummed rhythm and dynamics.

### Core Concept

The "Hum-to-Sample-Chop" process works as follows:
1. **Load a source audio sample** (WAV, MP3, or FLAC) — e.g., a drum break, instrument loop, etc.
2. **Hum a melody** into the microphone (or use demo notes for testing)
3. **Analyze the hum** — detect pitch, onset timing, duration, and velocity (loudness)
4. **Chop the source** — use JDilla-style transient detection to find natural chop points
5. **Map notes to chops** — match hummed notes to chops by **strength** (default: loud notes → strong transients) or **pitch proximity** (optional)
6. **Render output** — chops play back-to-back at their original lengths, with fade in/out to prevent clicks

### Key Design Decisions

- **No AI/ML required** — uses traditional DSP: YIN/McLeod pitch detection, spectral flux onset detection, RMS energy analysis
- **Deterministic results** — easy to debug, predictable behavior
- **JDilla-style chopping** — chops keep their original length (no time-stretching), creating rhythmic patterns from dynamics
- **Two matching modes**: Strength matching (default) and pitch matching (optional)

## Tech Stack

| Category | Library | Purpose |
|----------|---------|---------|
| **Language** | Rust 2021 edition (MSRV 1.75) | Core language |
| **CLI** | `clap` 4 | Argument parsing |
| **TUI** | `ratatui` 0.29 + `crossterm` 0.28 | Terminal UI |
| **Async** | `tokio` 1 | Event loop, async channels |
| **Audio I/O** | `hound` 3.5 | WAV read/write |
| **Audio Decode** | `symphonia` 0.5 | MP3/FLAC/WAV decoding |
| **Playback** | `rodio` 0.19 | Audio preview (feature-gated) |
| **Recording** | `cpal` 0.17 | Microphone input (feature-gated) |
| **Pitch Detection** | `pitch-detection` 0.3 | YIN algorithm |
| **FFT** | `rustfft` 6.2 | Spectral analysis |
| **Resampling** | `rubato` 0.15 | Sample rate conversion |
| **Interpolation** | `dasp` 0.11 | Signal interpolation |
| **Error Handling** | `anyhow` 1 | Error propagation |
| **Logging** | `env_logger` 0.11 + `log` 0.4 | Debug logging |
| **Colors** | `colored` 2 | Terminal output colors |
| **Time** | `chrono` 0.4 | Timestamps |

## Architecture & Module Structure

```
humchop/
├── src/
│   ├── main.rs            # CLI entry point, interactive recording flow
│   ├── hum_analyzer.rs    # Pitch detection (YIN) + onset detection (spectral flux) → Vec<Note>
│   ├── sample_chopper.rs  # JDilla-style transient detection → Vec<Chop> with strength scores
│   ├── mapper.rs          # Note-to-chop matching (strength or pitch) + rendering
│   ├── audio_utils.rs     # Audio file loading (WAV/MP3/FLAC), WAV output, resampling
│   ├── tui.rs             # Terminal UI (ratatui + crossterm) — state machine
│   ├── recorder.rs        # Microphone recording via cpal (audio-io feature)
│   ├── player.rs          # Audio playback via rodio (audio-io feature)
│   └── error.rs           # Custom error types (HumChopError)
└── Cargo.toml
```

### Key Data Types

```rust
// Detected note from hum
struct Note {
    pitch_hz: f32,      // Frequency in Hz
    onset_sec: f64,     // When the note starts (seconds)
    duration_sec: f64,  // How long the note lasts (seconds)
    velocity: f32,      // Loudness 0.0–1.0 (from RMS amplitude)
}

// Audio chop from source sample
struct Chop {
    samples: Vec<f32>,  // Audio samples (mono, f32 ±1.0)
    index: usize,       // Index in chop list
    start_time: f64,    // Start position in original sample (seconds)
    duration: f64,      // Duration (seconds)
    strength: f32,      // Transient strength score 0.0–1.0
}

// Mapped chop (note → chop mapping result)
struct MappedChop {
    samples: Vec<f32>,  // Processed audio samples
    chop_index: usize,  // Original chop index
    output_onset: f64,  // Position in output (seconds)
    output_duration: f64, // Duration in output
}
```

### Processing Pipeline

```
[Load Audio File] → audio_utils::load_audio() → (Vec<f32>, sample_rate)
       ↓
[Record Hum] → recorder::Recorder → Vec<f32> hum samples
       ↓
[Analyze Hum] → hum_analyzer::HumAnalyzer::transcribe() → Vec<Note>
       ↓
[Chop Source] → sample_chopper::SampleChopper::chop() → Vec<Chop>
       ↓
[Map Notes→Chops] → mapper::Mapper::process() → Vec<MappedChop>
       ↓
[Render Output] → mapper::Mapper::render_output() → Vec<f32>
       ↓
[Write WAV] → audio_utils::write_wav() → output_chopped_<timestamp>.wav
```

## Building and Running

### Basic Commands

```bash
# Build without audio recording support (no system audio dependencies)
cargo build

# Build with audio recording/playback support (requires ALSA/PulseAudio)
cargo build --features audio-io

# Run in release mode
cargo run --release -- <input_file.wav>

# Run with demo notes (no microphone needed)
cargo run -- test-sample.wav

# With pitch shifting enabled
cargo run -- sample.wav --pitch-shift

# Use pitch-based matching instead of strength matching
cargo run -- sample.wav --pitch-matching

# Specify output file
cargo run -- sample.wav -o my_chops.wav

# Use only a segment of the source (first 30 seconds)
cargo run -- beat.mp3 --segment 0,30
```

### Running Tests

```bash
# Run all unit tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run specific module tests
cargo test hum_analyzer
cargo test sample_chopper
cargo test mapper
cargo test audio_utils

# Run clippy (linting)
cargo clippy

# Format check
cargo fmt -- --check
```

### WSL2 Audio Setup

WSL2 requires PulseAudio for microphone access:

```bash
sudo apt update && sudo apt install libasound2-dev libpulse-dev libasound2-plugins

# Configure ALSA for Pulse
cat >> ~/.asoundrc << 'EOF'
pcm.!default { type pulse }
ctl.!default { type pulse }
EOF

# Set PulseServer
echo 'export PULSE_SERVER=unix:/mnt/wslg/PulseServer' >> ~/.bashrc
source ~/.bashrc
```

## Feature Flags

| Feature | Description | Dependencies |
|---------|-------------|--------------|
| `audio-io` (default) | Microphone recording + audio playback | `cpal`, `rodio` |
| `core-only` | Core processing only, no system audio libs | None extra |

## Configuration Defaults

### DillaConfig (Sample Chopper)

| Parameter | Default | Description |
|-----------|---------|-------------|
| `fft_window` | 1024 | FFT window size |
| `hop_size` | 256 | Analysis hop (~5.8ms at 44100Hz) |
| `energy_weight` | 0.6 | Energy vs spectral flux balance |
| `threshold_factor` | 1.4 | Onset detection threshold multiplier |
| `adaptive_window` | 20 | Lookback frames for threshold |
| `min_chop_secs` | 0.05 | Minimum chop length (50ms) |
| `max_chop_secs` | 2.0 | Maximum chop length |
| `boundary_jitter_secs` | 0.002 | Random boundary offset (±2ms) |

### MapperConfig (Note-to-Chop Mapping)

| Parameter | Default | Description |
|-----------|---------|-------------|
| `enable_pitch_shift` | false | Apply pitch correction to chops |
| `strength_matching` | true | Match by velocity→strength (JDilla style) |
| `crossfade_samples` | 256 | Crossfade length for transitions |

## Key Algorithms

### JDilla-Style Chopping (`sample_chopper.rs`)

1. **Onset strength curve** — combines RMS energy derivative + spectral flux via FFT
2. **Adaptive threshold** — responds to both loud and quiet sections (local mean × threshold_factor)
3. **Boundary selection** — pick transient positions exceeding threshold, enforce min chop length
4. **Energy-based fill** — if too few transients found, split by peak RMS position
5. **Strength selection** — keep the strongest `num_chops-1` boundaries
6. **Boundary jitter** — ±2ms random offset for "imperfect" feel
7. **Strength scoring** — each chop scored by onset strength, normalized to [0, 1]
8. **Fallback** — equal division if transient detection fails

### Hum Analysis (`hum_analyzer.rs`)

1. **Onset detection** — spectral flux via FFT, adaptive threshold
2. **Pitch detection** — YIN algorithm (McLeod variant available), 80–1000Hz range
3. **Note segmentation** — split pitch array by onset times, use median pitch per segment
4. **Velocity calculation** — RMS amplitude per note segment
5. **Fallback** — pitch continuity detection if onsets fail

### Note-to-Chop Matching (`mapper.rs`)

**Strength Matching (default)**:
- High-velocity notes → strong transient chops (punchy hits)
- Soft notes → quiet tail chops (fills, ambient)
- Creates rhythmic patterns from dynamics

**Pitch Matching (optional)**:
- Notes matched to chops by pitch proximity (log2 frequency ratio)
- Useful for melodic reconstruction

**Processing per mapped chop**:
1. Optional pitch shift (resample + time-stretch to match note pitch)
2. Velocity-based gain (multiply samples by note velocity)
3. Fade in/out (~5ms) to prevent click artifacts
4. Chops placed back-to-back with 5ms gaps

## Development Conventions

- **No AI/ML dependencies** — traditional DSP only (YIN, FFT, RMS, spectral flux)
- **Monophonic only** — polyphonic pitch detection is unreliable
- **MIT/Apache 2.0 licenses only** — commercial-friendly
- **Output format: WAV only** — no MP3/FLAC encoding
- **Clippy clean** — zero warnings expected
- **`cargo fmt` compliant** — standard Rust formatting
- **Comprehensive tests** — 40+ unit tests across all modules

## Known Constraints & Limitations

- Monophonic humming only (polyphonic detection unreliable)
- Background noise affects pitch detection accuracy
- Output is WAV only (no MP3/FLAC encoding)
- Max recording duration: 15 seconds
- Synthetic audio tests only — no real recording integration tests
- WSL2 requires PulseAudio configuration for microphone access

## Post-MVP Roadmap (Future Features)

- High-quality time stretching via `rubato` library
- Crossfade between chops
- ADSR envelope on each chop
- Waveform visualization
- Basic Pitch (ONNX) for higher accuracy
- MIDI output
- SFZ/sampler patch export
- Stem separation for drum mode
- Multi-sample layering
- Dioxus GUI
- WebAssembly version

## Common Tasks

### Adding a New Audio Processing Step

1. Create new module file in `src/`
2. Add module declaration in `main.rs`
3. Integrate into the pipeline in `run_interactive()` or `process_hum()`
4. Add unit tests in the module's `#[cfg(test)]` block
5. Run `cargo test && cargo clippy`

### Modifying the TUI

The TUI is a state machine in `tui.rs`:
- `AppState` enum defines states: Idle → Loading → Ready → Recording → Processing → Complete/Error
- `App` struct holds all state
- `render_ui()` dispatches to state-specific render functions
- `handle_key_event()` processes keyboard input

### Changing Matching Behavior

Edit `mapper.rs`:
- `match_by_strength()` — strength matching logic
- `match_by_pitch()` — pitch proximity logic
- `map_notes_to_chops()` — orchestrates matching, ensures each chop used once

### Tweaking Chop Detection

Edit `sample_chopper.rs`:
- `DillaConfig` struct has all tunable parameters
- `onset_strength_curve()` — how transient strength is calculated
- `pick_transient_boundaries()` — threshold-based boundary selection
- `fill_with_energy_splits()` — fallback when transients are sparse
