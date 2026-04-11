# HumChop - Hum-to-Chop Sampling Tool

A command-line tool and TUI application for transforming audio samples by humming melodies. Record a hum → Analyze pitch → Auto-chop your samples using JDilla-style processing.

![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)
![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)
![Tests](https://img.shields.io/badge/tests-44%20passing-green.svg)

## Features

- **JDilla-Style Chopping**: Chops keep their original length and play back-to-back, mimicking the classic hip-hop chop technique
- **Smooth Crossfade**: Optional crossfade between chops eliminates artifacts at boundaries (enabled by default)
- **Strength-Based Matching**: Loud notes match strong transients, soft notes match quiet tails
- **High-Quality Resampling**: Rubato SincInterpolation for pitch shifting (prevents aliasing)
- **Multi-Band Transient Detection**: RMS energy + spectral flux + high-frequency flux for accurate detection across all content types
- **Pre-Emphasis Filtering**: Boosts high-frequency transients so snares and hi-hats aren't masked by bass
- **Pitch Detection**: Detects notes from hummed audio using YIN/McLeod algorithm
- **Configurable Matching**: Choose between strength matching (default) or pitch matching
- **Chop Preview in TUI**: Press [1-9] to preview individual chops with ASCII waveform display
- **Batch Processing**: Process entire directories with `--batch` option

## Installation

```bash
# Build without audio recording support
cargo build --no-default-features --features core-only

# Build with audio recording support (requires microphone)
cargo build --features audio-io

# Release build
cargo build --release --features audio-io
```

## Usage

### Basic Command Line

```bash
# Process a sample with demo notes (for testing)
cargo run -- test-sample.mp3

# With pitch shifting (high-quality sinc interpolation)
cargo run -- sample.wav --pitch-shift

# Use pitch-based matching instead of strength matching
cargo run -- sample.wav --pitch-matching

# Specify output file
cargo run -- sample.wav -o my_chops.wav
```

### Command Line Options

| Option | Description |
|--------|-------------|
| `INPUT` | Input audio file (WAV, MP3, or FLAC) |
| `-o, --output <FILE>` | Output file path |
| `--pitch-shift` | Enable pitch shifting (sinc interpolation) |
| `--pitch-matching` | Match notes by pitch instead of strength |
| `--no-tui` | Run headless (no TUI, uses demo notes) |
| `--num-chops <N>` | Number of chops in headless mode (default: 16) |
| `--dither` | Enable dithering for reduced quantization noise |
| `--bits <BITS>` | Output bit depth: 16, 24, or 32 (default: 32) |
| `-b, --batch` | Process all audio files in directory |
| `--pattern <GLOB>` | File pattern for batch mode (default: *) |

### Headless Mode

For scripting, batch processing, or quick testing without microphone:

```bash
# Process with demo notes, no TUI
humchop sample.wav --no-tui

# Custom chop count
humchop sample.wav --no-tui --num-chops 8

# 16-bit output with dithering (smaller file size)
humchop sample.wav --no-tui --bits 16 --dither -o output.wav
```

### Batch Processing

Process multiple files at once:

```bash
# Process all audio files in a directory
humchop ./drums/ --batch -o ./output/

# Process only WAV files
humchop ./samples/ --batch --pattern "*.wav" -o ./chopped/

# Batch with pitch shift
humchop ./loops/ --batch --pitch-shift -o ./pitched/
```

### TUI Mode (with audio-io feature)

The TUI provides an interactive experience:
- **Waveform preview** when sample is loaded
- **Real-time recording** with level meter
- **Note detection visualization**
- **Chop preview** - Press [1-9] to see chop details with ASCII waveform

**TUI Key Bindings:**
| Key | Action |
|-----|--------|
| `r` | Start/stop recording |
| `m` | Toggle matching mode (strength/pitch) |
| `q` | Quit |
| `1-9` | Preview individual chops (in Complete state) |

## Architecture

```
humchop/
├── src/
│   ├── main.rs           # CLI entry point, batch processing
│   ├── sample_chopper.rs # Multi-band transient detection & JDilla-style chopping
│   ├── mapper.rs         # Note-to-chop matching, crossfade, pitch shift
│   ├── hum_analyzer.rs   # Pitch detection & transcription
│   ├── audio_utils.rs    # WAV/MP3/FLAC loading & saving
│   ├── tui.rs            # Terminal user interface
│   ├── recorder.rs       # Microphone recording (audio-io feature)
│   ├── player.rs         # Audio playback (audio-io feature)
│   └── error.rs          # Error types
└── Cargo.toml
```

## How It Works

### 1. Load Sample

Load an audio file (WAV, MP3, or FLAC) and display its waveform.

### 2. Record Hum (or use demo notes)

Hum your melody to define which notes to play. The analyzer detects:
- **Pitch**: Frequency in Hz, converted to MIDI note number
- **Onset time**: When the note starts (seconds)
- **Duration**: How long the note lasts (seconds)
- **Velocity**: Loudness (0.0 to 1.0), derived from RMS amplitude

### 3. Chop the Sample (JDilla-Style)

The chopper uses a multi-stage pipeline for high-quality transient detection:

1. **Pre-emphasis filter** — boosts high-frequency content (y[n] = x[n] - 0.97·x[n-1])
2. **Multi-band onset strength** — combines RMS derivative + full-band spectral flux + high-flux (3kHz+) + mid-flux (300Hz–3kHz)
3. **Median-based normalization** — sliding window with MAD scaling for consistent detection across loud and quiet sections
4. **Peak picking with prominence** — 3-pass algorithm (find maxima → compute prominence → non-maximum suppression)
5. **Multi-scale energy splitting** — fallback tries 5 frame sizes to find optimal split points

Each chop is scored by integrated onset energy over its entire region (not just a single frame).

### 4. Map Notes to Chops

**Strength Matching (Default - JDilla Style)**
- High-velocity notes → strong transient chops
- Soft notes → quiet tail chops
- Creates rhythmic patterns from dynamics

**Pitch Matching (Optional)**
- Notes match to chops by pitch proximity
- Useful for melodic reconstruction

### 5. Render Output

**Crossfade Mode (Default)**: Chops can overlap with smooth crossfade envelopes for seamless transitions.

**Standard Mode**: Chops play back-to-back with configurable crossfade length.

**Output Processing**:
- Soft-knee compression prevents harsh digital clipping (enabled by default)
- Optional triangular noise dithering for 16/24-bit output
- Final peak normalization ensures no samples exceed ±1.0

## Configuration

### DillaConfig (Chopper)

| Parameter | Default | Description |
|-----------|---------|-------------|
| `fft_window` | 2048 | FFT window size |
| `hop_size` | 256 | Analysis hop (~5.8ms at 44100Hz) |
| `energy_weight` | 0.4 | Energy vs spectral flux balance |
| `threshold_factor` | 1.2 | Onset detection threshold multiplier |
| `adaptive_window` | 30 | Lookback frames for normalization |
| `pre_emphasis` | 0.97 | High-frequency boost coefficient |
| `peak_prominence` | 0.3 | Minimum peak prominence threshold |
| `peak_min_distance` | 5 | Minimum frames between peaks |
| `min_chop_secs` | 0.03 | Minimum chop length (30ms) |
| `max_chop_secs` | 2.0 | Maximum chop length |
| `boundary_jitter_secs` | 0.002 | Random boundary offset (±2ms) |

### MapperConfig

| Parameter | Default | Description |
|-----------|---------|-------------|
| `enable_pitch_shift` | false | Apply pitch correction (sinc interpolation) |
| `strength_matching` | true | Match by strength (not pitch) |
| `crossfade_samples` | 256 | Crossfade length for transitions |
| `enable_crossfade` | true | Enable smooth crossfade between chops |
| `soft_clip` | true | Enable soft-knee compression |
| `soft_clip_threshold_db` | -1.0 | Soft clip threshold (dB) |

## Library API Usage

```rust
use humchop::{SampleChopper, Mapper, MapperConfig, Note};

// Create chopper and chop a sample
let chopper = SampleChopper::new(44100);
let chops = chopper.chop(&sample, num_chops)?;

// Create mapper with desired matching mode
let mut config = MapperConfig::default();
config.strength_matching = true;  // or false for pitch matching
config.enable_pitch_shift = false;
config.enable_crossfade = true;   // smooth crossfade between chops
let mapper = Mapper::with_config(44100, config);

// Process notes to chops
let mapped = mapper.process(&notes, &chops)?;
let output = mapper.render_output(&mapped);
```

## Changelog

### v0.1.4 (Current)

#### Crossfade Support
- **New**: `enable_crossfade` config option (default: true)
- **New**: `render_with_crossfade()` for smooth overlapping regions
- Smooth transition between chops using envelope-weighted blending

#### High-Quality Pitch Shifting
- **New**: `high_quality_resample()` using Rubato SincFixedIn
- Band-limited interpolation prevents aliasing artifacts
- 256-point sinc with BlackmanHarris2 window at 0.95 cutoff

#### TUI Improvements
- **New**: Chop preview with [1-9] keys
- **New**: ASCII waveform visualization (░▒▓█ blocks)
- Display chop details: start time, duration, strength
- Waveform preview on sample load

#### Batch Processing
- **New**: `--batch` option for processing directories
- **New**: `--pattern` for file matching (e.g., "*.wav")
- Progress display with success/fail counts

#### Code Quality
- All 44 tests passing
- 0 clippy warnings
- 92% completion (production-ready)

---

### v0.1.3 (Previous)

- Headless mode: `--no-tui` and `--num-chops`
- Dithering: TPDF with content-seeded RNG
- Bit depth: 16/24/32-bit output
- Soft-knee compression

---

### v0.1.2 (Previous)

- Pre-emphasis filter
- Multi-band onset detection
- MAD normalization
- Peak picking with prominence
- Multi-scale energy splitting

## Testing

```bash
# Run all unit tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific module tests
cargo test hum_analyzer
cargo test mapper
cargo test sample_chopper

# Run clippy
cargo clippy --all-targets

# Format code
cargo fmt -- --check
```

See [TESTING.md](./TESTING.md) for detailed test verification guide.

## Dependencies

| Library | Purpose |
|---------|---------|
| `rustfft` | FFT for spectral analysis |
| `pitch_detection` | YIN pitch detection |
| `symphonia` | Audio format decoding (MP3/FLAC) |
| `hound` | WAV file I/O |
| `rodio` | Audio playback |
| `cpal` | Audio recording |
| `rubato` | High-quality sample rate conversion & pitch shifting |
| `dasp` | Signal interpolation |
| `clap` | CLI argument parsing |
| `colored` | Terminal colors |
| `crossterm` / `ratatui` | TUI framework |

## License

MIT OR Apache-2.0