# HumChop - Specification

## 1. Project Overview

**Title**: HumChop - Hum-to-Chop Sampling Tool

**Concept**: Transform audio samples by humming melodies. Record a hum → Analyze pitch → Auto-chop samples using JDilla-style processing.

**Target Users**:
- Beat makers and producers who use sampling heavily
- Musicians who want to quickly prototype ideas from samples
- Anyone who hums melodies to compose

**MVP Goal**: Rust implementation with TUI, achieving: hum recording → analysis → chopping → mapping → WAV output.

---

## 2. Functional Requirements

### 2.1 Core Features (MVP)

**1. Sample Loading**
- Formats: WAV, MP3, FLAC
- Libraries: `symphonia` for decoding, `hound` for WAV output
- CLI: Accept file path argument

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

### 2.2 Error Handling

| Error | Response |
|-------|----------|
| No microphone | Display setup instructions |
| Sample too short | Fallback to equal division + warning |
| Single note detected | Prompt user to re-record |
| Unsupported format | Display supported formats |
| WSL2 without PulseAudio | Show setup instructions |

### 2.3 Future Features (Post-MVP)

- GUI with Dioxus
- Basic Pitch (ONNX) for higher accuracy
- MIDI output
- SFZ/Sampler patch export
- Stem separation for drum mode
- Multi-sample layering
- Web version (Dioxus WASM)

---

## 3. Non-Functional Requirements

- **Language**: Rust (stable, MSRV 1.75+)
- **Performance**: Analysis + chopping complete within 10 seconds (MVP)
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
│     ├── Pre-emphasis filter
│     ├── Multi-band onset strength (RMS + flux + high-flux + mid-flux)
│     ├── Median-based normalization (MAD scaling)
│     ├── Peak picking with prominence
│     └── Integrated strength scoring
│     └── Vec<Chop> with strength scores
├── Mapper             (strength/pitch matching + fade)
└── Output Writer      (hound)
```

**Module Structure**:
```
src/
├── main.rs           - Entry point, CLI, audio-io integration
├── tui.rs            - Terminal UI
├── hum_analyzer.rs   - Pitch detection, transcription
├── sample_chopper.rs - Multi-band transient detection + JDilla-style chopping
├── mapper.rs         - Note-to-chop matching
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
| Resampling | rubato | 0.15 | Sample rate conversion |
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

### MapperConfig

| Parameter | Default | Description |
|-----------|---------|-------------|
| `enable_pitch_shift` | false | Apply pitch correction |
| `strength_matching` | true | Match by strength (not pitch) |
| `crossfade_samples` | 256 | Crossfade length |

---

## 8. Key Algorithms

### JDilla-Style Chopping (v0.3.0)

1. Apply pre-emphasis filter to boost high-frequency transients
2. Compute multi-band onset strength curve (RMS derivative + full-band spectral flux + high-flux + mid-flux)
3. Normalize using sliding median + MAD scaling for consistent detection
4. Peak picking with prominence: find local maxima → compute prominence → non-maximum suppression
5. Enforce min/max chop length constraints
6. Apply boundary jitter for imperfect-feel
7. Score each chop by integrated energy over its region (60% mean + 40% peak)
8. Multi-scale energy splitting fallback (tries 5 frame sizes)

### Strength Matching

High-velocity notes match strong transient chops:
- Loud hum → punchy hits (kick, snare attacks)
- Soft hum → quiet tails (fills, ambient)

---

## 9. Version History

### v0.3.0 (Current)
- Pre-emphasis filter for high-frequency transient boost
- Multi-band onset detection (full-band + high-flux + mid-flux)
- Median-based normalization (MAD scaling)
- Peak picking with prominence detection
- Multi-scale energy splitting fallback
- Integrated strength scoring over chop region
- Tighter defaults: 2048 FFT window, 30ms min chop, 0.4 energy weight

### v0.2.0
- Fixed audio recording normalization
- Fixed chop count consistency
- Added click noise prevention (fades)
- Fixed recording drain loop

### v0.1.0
- JDilla-style mode only (removed TimeStretch mode)
- Strength-based matching
- Pitch-based matching option
- Improved transient detection

### v0.1.0
- MVP with basic chopping
- 40 unit tests passing
