# HumChop - TODO.md

Based on SPEC.md specifications, this document tracks implementation status.

---

## ✅ Completed Modules

### error.rs
- [x] Define `HumChopError` enum with variants:
  - [x] `MicrophoneNotFound`
  - [x] `SampleTooShort`
  - [x] `SingleNoteDetected`
  - [x] `UnsupportedFormat`
  - [x] `Wsl2PulseServerNotSet`
  - [x] `AudioDeviceBusy`
  - [x] `DecodeError`
  - [x] `EncodeError`
  - [x] `IoError`
  - [x] `InvalidAudio`
  - [x] `Other`
- [x] Implement `From` for underlying errors (cpal, hound, symphonia)
- [x] Implement `Display` for user-friendly error messages

### audio_utils.rs
- [x] `fn load_audio(path: &Path) -> Result<(Vec<f32>, u32), HumChopError>` - Load WAV/MP3/FLAC via symphonia
- [x] `fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) -> Result<(), HumChopError>` - Write output WAV via hound
- [x] `fn normalize(samples: &mut [f32])` - Peak normalization to [-1.0, 1.0]
- [x] `fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32>` - Sample rate conversion
- [x] `fn to_mono(samples: &[f32], channels: u16) -> Vec<f32>` - Channel to mono conversion
- [x] Unit tests for load/write round-trip
- [x] Unit tests for normalize
- [x] Unit tests for resample

### hum_analyzer.rs
- [x] Define `struct Note { pitch_hz: f32, onset_sec: f64, duration_sec: f64, velocity: f32 }`
- [x] `fn detect_pitch(samples: &[f32], sample_rate: u32) -> Vec<f32>` - YIN algorithm
- [x] `fn detect_onsets(samples: &[f32], sample_rate: u32) -> Vec<f64>` - Spectral flux via rustfft
- [x] `fn transcribe(samples: &[f32]) -> Result<Vec<Note>, HumChopError>` - Combine pitch + onset detection
- [x] Handle single-note detection edge case (return error prompting re-recording)
- [x] Note::to_midi_note() - Convert pitch to MIDI number
- [x] Note::to_note_name() - Convert pitch to note name (e.g., "A4", "C#5")
- [x] Unit tests for pitch detection
- [x] Unit tests for note conversion

### sample_chopper.rs
- [x] `fn chop_equal(sample: &[f32], num_chops: usize) -> Result<Vec<Chop>, HumChopError>` - Equal division slicing
- [x] `fn chop_by_onset(sample: &[f32], num_chops: usize) -> Result<Vec<Chop>, HumChopError>` - Onset-based slicing
- [x] Handle case where sample length < number of notes (fallback to equal division + warning)
- [x] `Chop` struct with timing information
- [x] `ChopMode` enum (Equal, Onset)
- [x] Unit tests for equal division edge cases
- [x] Unit tests for chop indices

### mapper.rs
- [x] `fn find_best_chop(note: &Note, chops: &[Chop]) -> Option<usize>` - Match notes to nearest chop
- [x] `fn map_notes_to_chops(notes: &[Note], chops: &[Chop]) -> Vec<usize>` - Full mapping
- [x] `fn apply_time_stretch(chop: &Chop, ratio: f64) -> Vec<f32>` - Time stretch via linear interpolation
- [x] `fn apply_pitch_shift(chop: &[f32], semitones: i32) -> Vec<f32>` - Pitch shift via resampling
- [x] `fn apply_velocity_gain(chop: &mut [f32], velocity: f32)` - Velocity-based gain adjustment
- [x] `fn process(notes: &[Note], chops: &[Chop]) -> Result<Vec<MappedChop>, HumChopError>` - Full processing
- [x] `fn render_output(mapped_chops: &[MappedChop]) -> Vec<f32>` - Render final audio
- [x] `fn render(sample: &[f32], notes: &[Note], num_chops: usize, mode: ChopMode) -> Result<Vec<f32>, HumChopError>` - Pipeline
- [x] Unit tests for mapper configuration
- [x] Unit tests for pitch difference calculation
- [x] Unit tests for velocity gain
- [x] Unit tests for output rendering

### tui.rs
- [x] Define TUI state machine: `Idle` | `Loading` | `Ready` | `Recording` | `Processing` | `Complete` | `Error`
- [x] Implement `struct App` with state tracking
- [x] Event loop with `tokio::select!` pattern
- [x] Key bindings:
  - [x] `q` - Quit
  - [x] `r` - Toggle recording (start/stop)
  - [x] `m` - Toggle chop mode
- [x] Layout components:
  - [x] Header: Title, keyboard shortcuts
  - [x] Main: State-specific content
  - [x] Footer: Status bar
- [x] Render functions for each state
- [x] Error display with user-friendly messages

### main.rs
- [x] Initialize logger (env_logger)
- [x] Parse CLI arguments (sample file path, output, chop mode, pitch shift)
- [x] Load sample via audio_utils
- [x] Process with demo notes (for testing without microphone)
- [x] Write output WAV
- [x] Display results

---

## 🔄 Integration & Testing

- [x] Wire: CLI arg → load sample → process
- [x] Wire: Note sequence + loaded sample → sample_chopper → chops
- [x] Wire: Note sequence + chops → mapper → final audio
- [x] Wire: Final audio → hound write → output_chopped_{timestamp}.wav
- [x] End-to-end test with demo notes

---

## 📋 Day-by-Day Milestones (MVP)

### ✅ Day 1: Audio I/O Foundation
- [x] `audio_utils.rs`: Load sample (WAV/MP3/FLAC), write WAV
- [x] CLI accepts file path argument
- [x] Round-trip test: Load → Write produces identical audio

### ✅ Day 2: TUI Foundation
- [x] `tui.rs`: Basic ratatui structure
- [x] Event loop with state machine
- [x] `q` key exits cleanly
- [x] Sample path displayed in UI

### ✅ Day 3: Microphone Recording (Complete)
- [x] TUI recording UI (display, level meter placeholder)
- [x] cpal microphone input infrastructure (feature flag)
- [x] `r` key toggles actual recording (cpal with pulseaudio)
- [x] Recording buffer sent via mpsc channel to TUI
- [x] Error handling for missing microphone

### ✅ Day 4: Pitch & Onset Detection
- [x] `hum_analyzer.rs`: YIN pitch detection
- [x] Onset detection via spectral flux
- [x] Generate `Vec<Note>` from hum audio
- [x] Handle single-note detection error

### ✅ Day 5: Chopping (Basic)
- [x] `sample_chopper.rs`: Equal division mode
- [x] `mapper.rs`: Basic assignment with time stretch
- [x] Output WAV generation
- [x] TUI "Processing..." → "Complete" state transition

### ✅ Day 6: Time Stretch & Integration
- [x] `mapper.rs`: Time stretch integration
- [x] End-to-end test with demo notes
- [x] Refine chop quality

### ⚠️ Day 7: Polish & WSL2 Verification (Pending)
- [ ] Comprehensive error messages (all error.rs variants used in production)
- [ ] WSL2 full workflow test (pulseaudio, recording, playback)
- [ ] README.md: Setup steps, usage examples, troubleshooting
- [ ] Code comments and documentation
- [ ] Final integration test

---

## 📚 Documentation

- [x] SPEC.md: Complete project specification
- [x] TODO.md: Implementation task tracking (this file)
- [ ] README.md: 
  - [ ] Project description
  - [ ] Requirements (Rust 1.75+, WSL2 setup if applicable)
  - [ ] Installation (`cargo build --release`)
  - [ ] Usage (`humchop path/to/sample.wav`)
  - [ ] Key bindings
  - [ ] Supported formats (WAV/MP3/FLAC input, WAV output)
  - [ ] WSL2 audio troubleshooting
  - [ ] License (Apache 2.0 or MIT)
- [ ] Code documentation: rustdoc comments for public APIs
- [ ] Architecture diagram (ASCII in README or separate file)

---

## 🧪 Testing

- [x] Unit tests for `audio_utils`: load/write round-trip
- [x] Unit tests for `hum_analyzer`: pitch detection, note conversion
- [x] Unit tests for `sample_chopper`: equal division edge cases
- [x] Unit tests for `mapper`: gain calculation, stretch ratios
- [x] Integration test: notes → chops → output (via demo)
- [ ] Manual test on WSL2: full workflow with real microphone
- [ ] Manual test on macOS/Linux native: audio device detection

---

## 🚀 Post-MVP (Phase 2+) - NOT FOR INITIAL IMPLEMENTATION

- [ ] GUI with Dioxus 0.7
- [ ] Basic Pitch (ONNX via tract-onnx) for higher accuracy
- [ ] MIDI output
- [ ] SFZ / Sampler patch output
- [ ] Drum mode with stem separation
- [ ] Multi-sample layering
- [ ] Web version (Dioxus WASM)

---

## 🐛 Known Issues & Constraints

- `rubato` is time-stretch only; pitch shift requires dasp resampling combo
- `symphonia` is decode-only; no MP3/FLAC encoding (output WAV only)
- WSL2 requires `pulseaudio` feature for cpal (ALSA backend causes DeviceBusy)
- Monophonic humming only; polyphonic input unreliable
- Background noise affects pitch detection accuracy (recommend quiet environment)
- Commercial-friendly licenses only (Apache 2.0, MIT, BSD-style)
