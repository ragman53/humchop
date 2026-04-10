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
- Transient detection: Combined RMS energy + spectral flux
- Adaptive threshold for loud/quiet sections
- Strength scoring per chop (0.0-1.0)
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
cpal = { version = "0.16", features = ["pulseaudio"] }
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
├── Sample Chopper     (JDilla-style transient detection)
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
├── sample_chopper.rs - JDilla-style chopping
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
| Audio I/O | cpal | 0.16 | Recording (pulseaudio feature) |
| WAV I/O | hound | 3.5 | WAV reading/writing |
| Decoding | symphonia | 0.5 | MP3/FLAC/WAV decoding |
| Playback | rodio | 0.19 | Audio preview |
| Pitch | pitch-detection | 0.3 | YIN pitch detection |
| FFT | rustfft | 6.2 | Spectral flux |
| CLI | clap | 4 | Argument parsing |
| Time | chrono | 0.4 | Timestamps |
| Logging | log/env_logger | 0.4 | Debug output |

---

## 7. Configuration

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

### MapperConfig

| Parameter | Default | Description |
|-----------|---------|-------------|
| `enable_pitch_shift` | false | Apply pitch correction |
| `strength_matching` | true | Match by strength (not pitch) |
| `crossfade_samples` | 256 | Crossfade length |

---

## 8. Key Algorithms

### JDilla-Style Chopping

1. Compute onset strength curve (RMS derivative + spectral flux)
2. Apply adaptive threshold (responds to loud and quiet sections)
3. Pick boundary positions at detected transients
4. Enforce min/max chop length constraints
5. Apply boundary jitter for imperfect-feel
6. Score each chop by transient strength

### Strength Matching

High-velocity notes match strong transient chops:
- Loud hum → punchy hits (kick, snare attacks)
- Soft hum → quiet tails (fills, ambient)

---

## 9. Version History

### v0.3.0 (Current)
- Fixed audio recording normalization
- Fixed chop count consistency
- Added click noise prevention (fades)
- Fixed recording drain loop

### v0.2.0
- JDilla-style mode only (removed TimeStretch mode)
- Strength-based matching
- Pitch-based matching option
- Improved transient detection

### v0.1.0
- MVP with basic chopping
- 40 unit tests passing
