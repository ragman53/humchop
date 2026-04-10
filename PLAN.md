# HumChop - Development Plan

## Current Status

### ✅ v0.1.3 - Current Release

**Milestones Completed:**
- [x] All 44 unit tests passing
- [x] JDilla-style chopping only (single mode)
- [x] Strength-based matching (default)
- [x] Pitch-based matching (optional)
- [x] Click noise prevention (fade in/out)
- [x] Audio recording normalization fixed
- [x] Chop count consistency (loop limits)
- [x] All Clippy warnings resolved (0 warnings)
- [x] Updated documentation (README, SPEC.md, TESTING.md, PLAN.md)

**Headless Mode (v0.1.3):**
- [x] `--no-tui` for scripting/batch processing
- [x] `--num-chops` for custom chop count

**Output Quality (v0.1.3):**
- [x] `--dither` for triangular noise dithering
- [x] `--bits` for configurable bit depth (16/24/32)
- [x] Soft-knee compression (enabled by default)
- [x] Cosine-based knee interpolation + soft saturation formula

**Design Improvements (v0.1.3):**
- [x] HumAnalyzer caching in Mapper (avoids FFT planner allocation per chop)
- [x] Fixed soft clip algorithm (`output = input / sqrt(1 + excess²)`)
- [x] NaN safety: `total_cmp()` for all f32 sorting
- [x] Better dither RNG (content-based seed + LCG + xorshift)

**Chopping Quality (v0.1.2):**
- [x] Pre-emphasis filter (high-frequency boost)
- [x] Multi-band onset detection (full-band + high-flux + mid-flux)
- [x] Median-based normalization (MAD scaling)
- [x] Peak picking with prominence detection
- [x] Multi-scale energy splitting fallback
- [x] Integrated strength scoring over chop region
- [x] Tighter defaults: 2048 FFT, 30ms min chop, 0.4 energy weight

---

## Version History

### v0.1.3 (Current - 2026-04-10)
- **Headless Mode**: `--no-tui` and `--num-chops` for scripting
- **Dithering**: `--dither` with TPDF (content-seeded RNG + LCG + xorshift)
- **Bit Depth**: `--bits` for 16/24/32-bit output
- **Soft-Knee Compression**: cosine-based knee + soft saturation formula, enabled by default
- **Design**: HumAnalyzer caching in Mapper, NaN-safe `total_cmp()` sorting
- **Code Quality**: All 10 clippy warnings fixed, 44 tests passing

### v0.1.2 (Previous)
- **Pre-Emphasis Filter**: High-frequency boost (y[n] = x[n] - 0.97·x[n-1])
- **Multi-Band Onset Detection**: Full-band + high-flux (3kHz+) + mid-flux (300Hz–3kHz)
- **Median-Based Normalization**: Sliding window with MAD scaling
- **Peak Picking with Prominence**: 3-pass algorithm for precise boundary placement
- **Multi-Scale Energy Splitting**: 5 frame sizes for optimal fallback splits
- **Integrated Strength Scoring**: 60% mean + 40% peak over chop region

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
- [x] Fix Clippy warnings (10 → 0)
- [ ] Add rustdoc comments for public APIs
- [ ] Benchmark pitch detection accuracy
- [x] Clippy warnings in non-chopper modules resolved

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
- [x] Soft-knee compression on output to prevent clipping (v0.1.3)
- [x] Optional dithering for 16-bit output (v0.1.3)

### Phase 2: Workflow Improvements
- [ ] Waveform visualization in TUI
- [ ] Preview individual chops before processing
- [ ] Undo/redo support in TUI
- [ ] Batch processing mode (multiple samples at once)
- [x] `--no-tui` headless CLI mode for scripting (v0.1.3)

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
| Unit Tests | 44 |
| Clippy Warnings | 0 |
| Rust Source Lines | ~3500 |
| Dependencies | 16 |
