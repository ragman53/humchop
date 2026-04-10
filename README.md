# HumChop - Hum-to-Chop Sampling Tool

A command-line tool and TUI application for transforming audio samples by humming melodies. Record a hum → Analyze pitch → Auto-chop your samples using JDilla-style processing.

## Features

- **JDilla-Style Chopping**: Chops keep their original length and play back-to-back, mimicking the classic hip-hop chop technique
- **Strength-Based Matching**: Loud notes match strong transients, soft notes match quiet tails
- **Multi-Band Transient Detection**: RMS energy + spectral flux + high-frequency flux for accurate detection across all content types
- **Pre-Emphasis Filtering**: Boosts high-frequency transients so snares and hi-hats aren't masked by bass
- **Peak Picking with Prominence**: Only meaningful transients selected — no noise, no double-detection
- **Pitch Detection**: Detects notes from hummed audio using YIN/McLeod algorithm
- **Configurable Matching**: Choose between strength matching (default) or pitch matching
- **Click Prevention**: Fade in/out applied to each chop to prevent audio artifacts

## Installation

```bash
# Build without audio recording support
cargo build --no-default-features --features core-only

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

### TUI Mode (with audio-io feature)

The TUI provides an interactive experience:
- Audio preview before recording
- Real-time recording with level meter
- Note detection visualization
- Interactive controls: `r`=record, `m`=toggle mode, `q`=quit

## Architecture

```
humchop/
├── src/
│   ├── main.rs           # CLI entry point and audio-io integration
│   ├── sample_chopper.rs # Multi-band transient detection & JDilla-style chopping
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

Chops play back-to-back with tiny gaps (5ms) to prevent clicks. No time stretching - each chop plays at its natural length and speed.

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
let mapper = Mapper::with_config(44100, config);

// Process notes to chops
let mapped = mapper.process(&notes, &chops)?;
let output = mapper.render_output(&mapped);
```

## Configuration

### DillaConfig (Chopper)

| Parameter | Default | Description |
|-----------|---------|-------------|
| `fft_window` | 2048 | FFT window size |
| `hop_size` | 256 | Analysis hop (~5.8ms at 44100Hz) |
| `energy_weight` | 0.4 | Energy vs spectral flux balance |
| `threshold_factor` | 1.2 | Onset detection threshold multiplier |
| `adaptive_window` | 30 | Lookback frames for normalization |
| `forward_window` | 30 | Lookahead frames for normalization |
| `pre_emphasis` | 0.97 | High-frequency boost coefficient |
| `peak_prominence` | 0.3 | Minimum peak prominence threshold |
| `peak_min_distance` | 5 | Minimum frames between peaks |
| `min_chop_secs` | 0.03 | Minimum chop length (30ms) |
| `max_chop_secs` | 2.0 | Maximum chop length |
| `boundary_jitter_secs` | 0.002 | Random boundary offset (±2ms) |

### MapperConfig (Mapper)

| Parameter | Default | Description |
|-----------|---------|-------------|
| `enable_pitch_shift` | false | Apply pitch correction |
| `strength_matching` | true | Match by strength (not pitch) |
| `crossfade_samples` | 256 | Crossfade length for transitions |

## Changelog

### v0.3.0 (Current)

#### Chopping Quality Improvements
- **Pre-Emphasis Filter**: High-frequency boost (y[n] = x[n] - 0.97·x[n-1]) prevents bass masking of transients
- **Multi-Band Onset Detection**: Full-band + high-flux (3kHz+) + mid-flux (300Hz–3kHz) for accurate detection across all content
- **Median-Based Normalization**: Sliding MAD scaling replaces naive mean threshold — consistent detection across loud and quiet sections
- **Peak Picking with Prominence**: 3-pass algorithm (local maxima → prominence → non-maximum suppression) — only meaningful transients selected
- **Multi-Scale Energy Splitting**: Fallback tries 5 frame sizes (64/128/256/512/1024) — picks optimal split by energy contrast
- **Integrated Strength Scoring**: 60% mean + 40% peak of onset energy over entire chop region — accurate mapper scores

#### Configuration Changes
- FFT window: 1024 → 2048 (better frequency resolution)
- Min chop: 50ms → 30ms (catches faster transients)
- Energy weight: 0.6 → 0.4 (more spectral-driven for musical content)

---

### v0.2.0

#### Bug Fixes
- **Audio Recording Normalization**: i16/U16 samples now properly normalized to ±1.0
- **Chop Count Consistency**: Added loop limit to prevent infinite loops
- **Click Noise Prevention**: Fade in/out applied to each chop
- **Recording Drain Loop**: Dynamic limit based on max recording duration

#### Improvements
- **Single JDilla Mode**: Removed TimeStretch mode, simplified API
- **Strength-Based Matching**: Notes matched by velocity to chop strength
- **Improved Transient Detection**: Combined RMS + spectral flux with adaptive threshold

#### Code Quality
- Fixed all Clippy warnings
- Added `#[derive(Default)]` where appropriate
- Removed dead code
- Improved loop patterns

---

### v0.1.0 - MVP
- Core JDilla-style chopping
- Demo mode for testing
- TUI framework
- 40 unit tests passing

## Testing

Run all tests:
```bash
cargo test
```

Run with output:
```bash
cargo test -- --nocapture
```

Run specific module tests:
```bash
cargo test hum_analyzer
cargo test mapper
cargo test sample_chopper
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
| `rubato` | Sample rate conversion |
| `dasp` | Signal interpolation |
| `clap` | CLI argument parsing |
| `colored` | Terminal colors |
| `crossterm` / `ratatui` | TUI framework |

## License

MIT
