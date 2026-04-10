# HumChop - Hum-to-Chop Sampling Tool

A command-line tool and TUI application for transforming audio samples by humming melodies. Record a hum → Analyze pitch → Auto-chop your samples using JDilla-style processing.

## Features

- **JDilla-Style Chopping**: Chops keep their original length and play back-to-back, mimicking the classic hip-hop chop technique
- **Strength-Based Matching**: Loud notes match strong transients, soft notes match quiet tails
- **Pitch Detection**: Detects notes from hummed audio using YIN/McLeod algorithm
- **Transient Detection**: Identifies natural chop points using RMS energy + spectral flux
- **Configurable Matching**: Choose between strength matching (default) or pitch matching

## Installation

```bash
# Build without audio recording support
cargo build

# Build with audio recording support (requires microphone)
cargo build --features audio-io
```

## Usage

### Basic Command Line

```bash
# Process a sample with demo notes (for testing)
cargo run -- test-sample.wav

# With pitch shifting
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
| `--pitch-shift` | Enable pitch shifting |
| `--pitch-matching` | Match notes by pitch instead of strength |

### TUI Mode

When built with `--features audio-io`, the TUI provides:
- Audio preview before recording
- Real-time recording with level meter
- Note detection visualization
- Interactive controls (r=record, m=toggle mode, q=quit)

## Architecture

```
humchop/
├── src/
│   ├── main.rs           # CLI entry point and audio-io integration
│   ├── sample_chopper.rs # JDilla-style transient detection & chopping
│   ├── mapper.rs         # Note-to-chop matching (strength/pitch)
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

Load an audio file (WAV, MP3, or FLAC) and display its properties.

### 2. Record Hum (or use demo notes)

Hum your melody to define which notes to play. The analyzer detects:
- Pitch (frequency in Hz)
- Onset time (when the note starts)
- Duration (how long the note lasts)
- Velocity (loudness, 0.0 to 1.0)

### 3. Chop the Sample (JDilla-Style)

The chopper finds transient points using:
- **RMS Energy Derivative**: Detects sudden amplitude changes
- **Spectral Flux**: Detects spectral content changes
- **Adaptive Threshold**: Reacts to both loud and quiet sections

Each chop is scored by its transient strength (0.0 = silence, 1.0 = strongest hit).

### 4. Map Notes to Chops

Two matching modes available:

**Strength Matching (Default - JDilla Style)**
- High-velocity notes → strong transient chops
- Soft notes → quiet tail chops
- Creates rhythmic patterns from dynamics

**Pitch Matching (Optional)**
- Notes match to chops by pitch proximity
- Useful for melodic reconstruction

### 5. Render Output

Chops play back-to-back with tiny gaps (5ms) to prevent clicks. No time stretching - each chop plays at its natural length and speed.

## API Usage (Library)

```rust
use humchop::{SampleChopper, Mapper, MapperConfig, Note};

// Create chopper and chop a sample
let chopper = SampleChopper::new(44100);
let chops = chopper.chop(&sample, num_chops)?;

// Create mapper with desired matching mode
let mut config = MapperConfig::default();
config.strength_matching = true;  // or false for pitch matching
config.enable_pitch_shift = false;
let mapper = Mapper::with_config(44100, config);

// Process notes to chops
let mapped = mapper.process(&notes, &chops)?;
let output = mapper.render_output(&mapped);
```

## Configuration

### DillaConfig (Chopper)

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

### MapperConfig (Mapper)

| Parameter | Default | Description |
|-----------|---------|-------------|
| `enable_pitch_shift` | false | Apply pitch correction |
| `strength_matching` | true | Match by strength (not pitch) |
| `crossfade_samples` | 256 | Crossfade length for transitions |

## Spec Changes Log

### v0.2.0 (Current)

Based on code review feedback, the following changes were made:

#### 1. Removed Mode Split (Normal vs JDilla)

**Before:** Two distinct modes (`MappingStyle::TimeStretch` and `MappingStyle::Jdilla`)

**After:** Single JDilla-style mode as the default and only mode
- Chops always keep original length
- No time stretching
- Simple back-to-back playback with gaps

#### 2. Fixed Time Stretch Bug

**Before:**
```rust
let stretch_ratio = current_duration / target_duration_secs;
let target_samples = chop.samples.len() as f64 / stretch_ratio;
```

**After:** Time stretching removed entirely. JDilla mode doesn't stretch chops.

#### 3. Added Strength-Based Matching

**Before:** Notes matched to chops by sequential order or (broken) time proximity

**After:**
```rust
pub fn match_by_strength(&self, note: &Note, chops: &[Chop], pool: &[usize]) -> usize {
    // High-velocity note → strong transient chop
    // Soft note → quiet tail chop
}
```

#### 4. Fixed Pitch Detection

**Before:** `find_best_chop` didn't use pitch information

**After:** Pitch-based matching available via `--pitch-matching` flag

#### 5. Improved Transient Detection

The JDilla chopper now uses:
- Combined RMS energy derivative + spectral flux
- Adaptive threshold that responds to both loud and quiet sections
- Energy-based sub-splitting when transients are sparse
- Strength scoring for each chop

#### 6. Simplified API

**Before:**
```rust
chopper.chop(&sample, num_chops, ChopMode::Equal)?;  // or ChopMode::Onset
mapper.render(&sample, &notes, num_chops, ChopMode::Equal)?;
```

**After:**
```rust
chops = chopper.chop(&sample, num_chops)?;  // Always JDilla-style
output = mapper.render(&sample, &notes, num_chops)?;
```

---

### v0.2.0 - Code Review Fixes

Critical fixes based on Claude code review:

1. **Fixed Audio Recording Normalization** - i16/U16 samples now properly normalized to ±1.0
2. **Fixed Chop Count Consistency** - Added loop limit to prevent infinite loops
3. **Added Click Noise Prevention** - Fade in/out applied to each chop
4. **Fixed Recording Drain Loop** - Dynamic limit based on max recording duration

## Dependencies

- `rustfft` - FFT for spectral analysis
- `pitch_detection` - YIN pitch detection
- `symphonia` - Audio format decoding (MP3/FLAC)
- `hound` - WAV file I/O
- `clap` - CLI argument parsing
- `colored` - Terminal colors
- `crossterm` / `ratatui` - TUI framework (optional)

## License

MIT
