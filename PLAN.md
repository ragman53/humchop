# HumChop - Development Plan

## Current Status

### ✅ v0.3.0 - Current Release

**Milestones Completed:**
- [x] All 40 unit tests passing
- [x] JDilla-style chopping only (single mode)
- [x] Strength-based matching (default)
- [x] Pitch-based matching (optional)
- [x] Click noise prevention (fade in/out)
- [x] Audio recording normalization fixed
- [x] Chop count consistency (loop limits)
- [x] All Clippy warnings resolved
- [x] Updated documentation (README, SPEC.md, TESTING.md)

**Chopping Quality Improvements (v0.3.0):**
- [x] Pre-emphasis filter (high-frequency boost)
- [x] Multi-band onset detection (full-band + high-flux + mid-flux)
- [x] Median-based normalization (MAD scaling)
- [x] Peak picking with prominence detection
- [x] Multi-scale energy splitting fallback
- [x] Integrated strength scoring over chop region
- [x] Tighter defaults: 2048 FFT, 30ms min chop, 0.4 energy weight

---

## Version History

### v0.3.0 (Current - 2026-04-10)
- **Pre-Emphasis Filter**: High-frequency boost (y[n] = x[n] - 0.97·x[n-1])
- **Multi-Band Onset Detection**: Full-band + high-flux (3kHz+) + mid-flux (300Hz–3kHz)
- **Median-Based Normalization**: Sliding window with MAD scaling
- **Peak Picking with Prominence**: 3-pass algorithm for precise boundary placement
- **Multi-Scale Energy Splitting**: 5 frame sizes for optimal fallback splits
- **Integrated Strength Scoring**: 60% mean + 40% peak over chop region
- **Tighter Defaults**: 2048 FFT window, 30ms min chop, 0.4 energy weight

### v0.2.0 (2026-04-10)
- **Single JDilla Mode**: Removed TimeStretch, simplified API
- **Strength-Based Matching**: Notes matched by velocity to chop strength
- **Improved Transient Detection**: Combined RMS + spectral flux with adaptive threshold
- **Bug Fixes**: Normalization, loop limits, fades, drain loop
- **Code Quality**: Clippy fixes, dead code removal, derive attributes
- **Documentation**: README rewrite, TESTING.md guide

### v0.1.0 - MVP (2026-04-09)
- Core JDilla-style chopping
- Demo mode for testing
- TUI framework
- 40 unit tests passing

---

## Technical Debt

### Cleanup
- [x] Remove unused imports
- [x] Fix Clippy warnings
- [ ] Add rustdoc comments for public APIs
- [ ] Benchmark pitch detection accuracy
- [ ] Resolve remaining clippy warnings in non-chopper modules (mapper fade loop, tui patterns)

### Performance
- [ ] Parallel FFT for onset detection
- [ ] Batch process chops
- [ ] Memory profiling for large files
- [ ] Replace linear_resample in mapper.rs with rubato for higher quality
- [ ] Cache HumAnalyzer instance in estimate_chop_pitch()

### Testing
- [x] Unit tests for all modules
- [x] TESTING.md manual verification guide
- [ ] Integration tests with real audio recordings
- [ ] Cross-platform testing (macOS, Windows, Linux)
- [ ] Edge case tests: very short samples, silence-only, clipped audio

---

## Contribution Guidelines

1. Ensure tests pass: `cargo test`
2. Run clippy: `cargo clippy`
3. Format code: `cargo fmt`
4. Update documentation if needed
5. Add tests for new features

---

## Roadmap

### Phase 1: Audio Quality (Next)
- [ ] Crossfade between chops (currently 5ms gaps)
- [ ] ADSR envelope on each chop
- [ ] High-quality resampling via rubato (replace linear interpolation in mapper)
- [ ] Soft-knee compression on output to prevent clipping
- [ ] Optional dithering for 16-bit output

### Phase 2: Workflow Improvements
- [ ] Waveform visualization in TUI
- [ ] Preview individual chops before processing
- [ ] Undo/redo support in TUI
- [ ] Batch processing mode (multiple samples at once)
- [ ] `--no-tui` headless CLI mode for scripting

### Phase 3: Enhanced Features
- [ ] Basic Pitch (ONNX) for higher hum accuracy
- [ ] MIDI output
- [ ] SFZ/sampler patch export
- [ ] Beat grid quantization (snap chops to grid)
- [ ] Multi-sample layering (blend multiple sources)

### Phase 4: GUI & Distribution
- [ ] Dioxus GUI with waveform display
- [ ] macOS app bundle
- [ ] Windows installer
- [ ] WebAssembly version

---

## Known Constraints

- Monophonic humming only (polyphonic unreliable)
- Background noise affects pitch detection
- Commercial-friendly licenses only (Apache 2.0, MIT)
- Output format is WAV only (no MP3/FLAC encoding)
- Max recording duration: 15 seconds (TUI)

---

## Project Statistics

| Metric | Value |
|--------|-------|
| Source Files | 9 |
| Unit Tests | 40 |
| Clippy Warnings | 0 (core), 10 (other modules) |
| Rust Source Lines | ~3200 |
| Dependencies | 16 |
