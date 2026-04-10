# HumChop - Implementation Plan

## Current Status

### ✅ Completed (v0.1.0 MVP Core)
- All core modules implemented and tested (30 tests passing)
- Audio loading/saving (WAV, MP3, FLAC)
- Pitch detection via YIN algorithm
- Onset detection via spectral flux
- Sample chopping (equal and onset-based)
- Note-to-chop mapping with time stretching
- TUI framework with state machine
- CLI interface with demo mode

### ⚠️ Incomplete (Blocked by System Dependencies)
- Microphone recording (requires cpal with system audio libraries)
- Real-time audio preview (requires rodio with system audio)
- Full TUI integration with live recording

---

## Next Steps (v0.2.0)

### Priority 1: Microphone Recording Integration
1. Install system dependencies for audio:
   ```bash
   # Ubuntu/Debian
   sudo apt install libasound2-dev libpulse-dev libasound2-plugins
   
   # macOS
   brew install alsa-lib pulseaudio  # via Homebrew
   ```

2. Implement cpal microphone recording:
   - Create `recorder.rs` module
   - Implement `start_recording()` / `stop_recording()`
   - Send audio data via mpsc channel
   - Handle device not found / busy errors

3. Connect TUI to recording:
   - Update `tui.rs` to receive audio buffer
   - Display live recording level meter
   - Auto-stop at 15 seconds

### Priority 2: Audio Preview (rodio)
1. Implement `preview_sample()` in new `player.rs` module
2. Add `p` key binding in TUI for preview
3. Add `s` key binding to stop preview
4. Handle playback errors gracefully

### Priority 3: Improved Pitch Detection
1. Add McLeod pitch detector as alternative (currently only YIN implemented)
2. Implement adaptive threshold for noisy environments
3. Add confidence scoring for detected pitches
4. Consider Basic Pitch (ONNX) for Phase 2

### Priority 4: Polish & Documentation
1. Write comprehensive README.md
2. Add more integration tests
3. Document all public APIs with rustdoc
4. Create architecture diagram

---

## Future Roadmap

### Phase 1.5: Quality Improvements
- [ ] High-quality time stretching (rubato library)
- [ ] Pitch shifting implementation
- [ ] Crossfade between chops
- [ ] ADSR envelope on each chop

### Phase 2: Enhanced Features
- [ ] Basic Pitch (ONNX) for higher accuracy
- [ ] MIDI output
- [ ] SFZ patch export
- [ ] Multiple chop modes (beat-sliced, transient, etc.)

### Phase 3: GUI & Multi-Platform
- [ ] Dioxus GUI
- [ ] macOS app bundle
- [ ] Windows installer
- [ ] WebAssembly version

---

## Technical Debt

### Cleanup Tasks
- [ ] Remove unused imports and variables
- [ ] Add `#[allow(dead_code)]` for intentional unused variants
- [ ] Add more error context to failures
- [ ] Implement proper logging throughout

### Performance Improvements
- [ ] Use parallel FFT for onset detection
- [ ] Batch process chops for better cache locality
- [ ] Consider SIMD for sample processing

### Testing Gaps
- [ ] Integration tests with real audio files
- [ ] Pitch detection accuracy benchmarks
- [ ] Memory profiling for large files
- [ ] Cross-platform testing (macOS, Windows)

---

## Contribution Guidelines

When contributing:
1. Ensure all tests pass: `cargo test --features core-only`
2. Run clippy: `cargo clippy --features core-only`
3. Format code: `cargo fmt`
4. Update TODO.md to reflect changes
5. Add tests for new functionality

---

## Version History

### v0.1.0 (Current)
- MVP with core chopping functionality
- Demo mode for testing without microphone
- 30 unit tests passing

### v0.2.0 (Planned)
- Full microphone recording support
- Audio preview playback
- Improved pitch detection

### v1.0.0 (Target)
- Production-ready with GUI
- Cross-platform support
- Basic Pitch integration
