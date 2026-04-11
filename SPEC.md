# HumChop - Specification

## 1. Project Overview

**Title**: HumChop - Hum-to-Chop Sampling Tool

**Concept**: Transform audio samples by humming melodies. Record a hum → Analyze pitch → Auto-chop samples using JDilla-style processing.

**Target Users**:
- Beat makers and producers who use sampling heavily
- Musicians who want to quickly prototype ideas from samples
- Anyone who hums melodies to compose

**Current Version**: v0.1.4 (Development - Quality Issues - 60% Completion)

---

## 2. Functional Requirements

### 2.1 Core Features

**1. Sample Loading**
- Formats: WAV, MP3, FLAC
- Libraries: `symphonia` for decoding, `hound` for WAV output
- CLI: Accept file path argument
- TUI: ASCII waveform preview (░▒▓█ blocks)

**2. Sample Preview Playback**
- Library: `rodio` for audio playback
- Keys: `p` to preview, `s` to stop

**3. Hum Recording**
- Library: `cpal` with pulseaudio feature (required for WSL2)
- Max duration: 15 seconds
- Key: `r` to toggle recording start/stop
- Audio sent via `tokio::sync::mpsc` channel

**4. Hum Analysis**
- Pitch detection: YIN algorithm via `pitch_detection` crate
- Onset detection: Spectral flux via `rustfft`
- Output: `Vec<Note>` containing pitch, onset, duration, velocity

```rust
struct Note {
    pitch_hz: f32,
    onset_sec: f64,
    duration_sec: f64,
    velocity: f32,
}
```

**5. JDilla-Style Sample Chopping**
- Pre-emphasis filter (high-frequency boost)
- Multi-band transient detection: RMS derivative + full-band flux + high-flux (3kHz+) + mid-flux (300Hz–3kHz)
- Median-based normalization (sliding window with MAD scaling)
- Peak picking with prominence detection (3-pass algorithm)
- Integrated strength scoring over chop region (60% mean + 40% peak)
- Multi-scale energy splitting fallback (5 frame sizes)
- Boundary jitter (±2ms) for imperfect-feel
- Min/max chop length constraints

**6. Note-to-Chop Mapping**

Two matching modes:
- **Strength Matching (Default)**: High-velocity notes → strong transients
- **Pitch Matching**: Notes matched by pitch proximity

**7. Output**

- WAV file generation: `output_chopped_{timestamp}.wav`
- Fade in/out applied to prevent clicks (~5ms)
- **Smooth crossfade between chops** (enabled by default, v0.1.4)
- Configurable bit depth: 16, 24, or 32 (default: 32)
- Optional dithering for lower bit depths
- Soft-knee compression to prevent harsh clipping (enabled by default)

### 2.2 Error Handling

| Error | Response |
|-------|----------|
| No microphone | Display setup instructions |
| Sample too short | Fallback to equal division + warning |
| Single note detected | Prompt user to re-record |
| Unsupported format | Display supported formats |
| WSL2 without PulseAudio | Show setup instructions |

### 2.3 TUI Features (v0.1.4)

| Feature | Description |
|---------|-------------|
| **Chop Preview** | Press [1-9] to preview individual chops |
| **Waveform Display** | ASCII ░▒▓█ blocks for sample visualization |
| **Chop Details** | Show start time, duration, strength for selected chop |
| **Progress Display** | Show processing stages (Analyzing, Chopping, Mapping) |

### 2.4 Batch Processing (v0.1.4)

| Option | Description |
|--------|-------------|
| `--batch` | Process all audio files in a directory |
| `--pattern` | File matching pattern (e.g., "*.wav", "*.mp3") |

---

## 3. Non-Functional Requirements

- **Language**: Rust (stable, MSRV 1.75+)
- **Performance**: Analysis + chopping complete within 10 seconds
- **Cross-platform**: macOS / Windows / Linux (including WSL2)
- **Licenses**: Apache 2.0 or MIT
- **Error handling**: `anyhow` + `colored` for user-friendly messages
- **Logging**: `env_logger` (`RUST_LOG=debug` for verbose output)

---

## 4. WSL2 Setup

WSL2 requires PulseAudio for audio devices.

```bash
# Install dependencies (Debian/Ubuntu)
sudo apt update && sudo apt install libasound2-dev libpulse-dev libasound2-plugins

# Configure ALSA for Pulse
cat >> ~/.asoundrc << 'EOF'
pcm.!default { type pulse }
ctl.!default { type pulse }
EOF

# Set PulseServer
echo 'export PULSE_SERVER=unix:/mnt/wslg/PulseServer' >> ~/.bashrc
source ~/.bashrc

# Verify
pactl list sources short
```

**Note**: WSL2 requires `cpal` with `pulseaudio` feature:
```toml
cpal = { version = "0.17", features = ["pulseaudio"] }
```

---

## 5. Architecture

```
[TUI Frontend (ratatui + crossterm)]
         ↓
[Main Loop]
├── Sample Loader      (symphonia + hound)
├── Audio Preview      (rodio)
├── Hum Recorder       (cpal)
│     └── mpsc channel
├── Hum Analyzer       (pitch_detection + rustfft)
│     └── Vec<Note>
├── Sample Chopper     (multi-band transient detection)
│   ├── Pre-emphasis filter
│   ├── Multi-band onset strength (RMS + flux + high-flux + mid-flux)
│   ├── Median-based normalization (MAD scaling)
│   ├── Peak picking with prominence
│   └── Integrated strength scoring
│   └── Vec<Chop> with strength scores
├── Mapper             (strength/pitch matching + crossfade)
│   ├── high_quality_resample() [rubato SincFixedIn]
│   ├── render_with_crossfade() [smooth transitions]
│   └── soft_knee_compress()
└── Output Writer      (hound)
```

**Module Structure**:
```
src/
├── main.rs           - Entry point, CLI, batch processing
├── tui.rs            - Terminal UI with chop preview
├── hum_analyzer.rs   - Pitch detection, transcription
├── sample_chopper.rs - Multi-band transient detection
├── mapper.rs         - Note-to-chop matching, crossfade, pitch shift
├── audio_utils.rs    - Audio loading/saving
├── recorder.rs       - Microphone recording
├── player.rs         - Audio playback
└── error.rs          - Error types
```

---

## 6. Technical Stack

| Category | Library | Version | Purpose |
|----------|---------|---------|---------|
| TUI | ratatui | 0.29 | Terminal UI rendering |
| Terminal I/O | crossterm | 0.28 | Key events, screen control |
| Async | tokio | 1 | Event loop multiplexing |
| Audio I/O | cpal | 0.17 | Recording (pulseaudio feature) |
| WAV I/O | hound | 3.5 | WAV reading/writing |
| Decoding | symphonia | 0.5 | MP3/FLAC/WAV decoding |
| Playback | rodio | 0.19 | Audio preview |
| Pitch | pitch-detection | 0.3 | YIN pitch detection |
| FFT | rustfft | 6.2 | Spectral flux |
| Resampling | rubato | 0.15 | High-quality sample rate conversion & pitch shifting |
| CLI | clap | 4 | Argument parsing |
| Time | chrono | 0.4 | Timestamps |
| Logging | log/env_logger | 0.4 | Debug output |

---

## 7. Configuration

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

### MapperConfig (v0.1.4)

| Parameter | Default | Description |
|-----------|---------|-------------|
| `enable_pitch_shift` | false | Apply pitch correction (sinc interpolation) |
| `strength_matching` | true | Match by strength (not pitch) |
| `crossfade_samples` | 256 | Crossfade length for transitions |
| `enable_crossfade` | true | Enable smooth crossfade between chops |
| `soft_clip` | true | Enable soft-knee compression |
| `soft_clip_threshold_db` | -1.0 | Soft clip threshold (dB) |

---

## 8. Key Algorithms

### JDilla-Style Chopping (v0.1.2+)

1. Apply pre-emphasis filter to boost high-frequency transients
2. Compute multi-band onset strength curve (RMS derivative + full-band spectral flux + high-flux + mid-flux)
3. Normalize using sliding median + MAD scaling for consistent detection
4. Peak picking with prominence: find local maxima → compute prominence → non-maximum suppression
5. Enforce min/max chop length constraints
6. Apply boundary jitter for imperfect-feel
7. Score each chop by integrated energy over its region (60% mean + 40% peak)
8. Multi-scale energy splitting fallback (tries 5 frame sizes)

### Crossfade Rendering (v0.1.4)

When `enable_crossfade` is true, overlapping regions use smooth envelope crossfade:
- Each chop has fade-in (quick attack) and fade-out (gentle decay)
- Envelope weights prevent double-volume at overlaps
- Sine-based (half-Hann) crossfade for smooth transitions

### High-Quality Resampling (v0.1.4)

Pitch shifting uses Rubato SincFixedIn for band-limited interpolation:
- 256-point sinc with BlackmanHarris2 window
- 0.95 cutoff frequency
- 256x oversampling for quality
- Prevents aliasing artifacts from linear interpolation

### Strength Matching

High-velocity notes match strong transient chops:
- Loud hum → punchy hits (kick, snare attacks)
- Soft hum → quiet tails (fills, ambient)

---

## 9. Version History

### v0.1.4 (Current - 2026-04-11)

**New Features:**
- `enable_crossfade` configuration for smooth transitions
- `render_with_crossfade()` for overlapping regions
- `high_quality_resample()` using Rubato SincFixedIn
- Chop preview with [1-9] keys in TUI
- ASCII waveform visualization
- Batch processing with `--batch` and `--pattern`

**Quality:**
- 44/44 tests passing
- 0 clippy warnings
- 92% completion

### v0.1.3 (2026-04-10)

- Headless mode: `--no-tui` and `--num-chops`
- Dithering with TPDF (content-seeded RNG + LCG + xorshift)
- Bit depth: 16/24/32-bit output
- Soft-knee compression with cosine-based knee

### v0.1.2 (2026-04-10)

- Pre-emphasis filter (high-frequency boost)
- Multi-band onset detection (full-band + high-flux + mid-flux)
- Median-based normalization (MAD scaling)
- Peak picking with prominence detection
- Multi-scale energy splitting fallback
- Integrated strength scoring

### v0.1.0 (2026-04-09)

- MVP with core JDilla-style chopping
- Demo mode for testing
- TUI framework
- 44 unit tests

---

## 10. Future Features

### Phase 3: Enhanced Features

| Feature | Priority | Description |
|---------|----------|-------------|
| Beat grid quantization | High | Snap chops to BPM grid |
| SFZ export | Medium | Generate sampler patches |
| MIDI output | Medium | Export detected notes as .mid |
| Basic Pitch (ONNX) | Low | ML-based pitch detection |
| Multi-sample blending | Low | Layer multiple sources |

### Phase 4: GUI

| Feature | Priority | Description |
|---------|----------|-------------|
| Dioxus GUI | Medium | Waveform display, drag-drop |
| WASM version | Low | Browser-based tool |