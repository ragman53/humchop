# HumChop - Development Plan

## Current Status: v0.3.1 (Production Ready)

**Completion**: 92%

---

## What's Implemented

### Core Features ✅

| Feature | Version | Status |
|---------|---------|--------|
| JDilla-style chopping | v0.1.0 | ✅ Complete |
| Strength-based matching | v0.1.0 | ✅ Complete |
| Pitch-based matching | v0.1.0 | ✅ Complete |
| Pre-emphasis filter | v0.1.2 | ✅ Complete |
| Multi-band transient detection | v0.1.2 | ✅ Complete |
| MAD normalization | v0.1.2 | ✅ Complete |
| Peak picking with prominence | v0.1.2 | ✅ Complete |
| Multi-scale energy splitting | v0.1.2 | ✅ Complete |
| Soft-knee compression | v0.1.3 | ✅ Complete |
| Dithering (TPDF) | v0.1.3 | ✅ Complete |
| Configurable bit depth | v0.1.3 | ✅ Complete |

### v0.3.1 New Features ✅

| Feature | Status | Description |
|---------|--------|-------------|
| **Crossfade** | ✅ Done | `enable_crossfade: true` in MapperConfig |
| **Sinc Resampling** | ✅ Done | Rubato SincFixedIn for pitch shift |
| **Chop Preview** | ✅ Done | [1-9] keys in TUI with waveform |
| **Waveform Display** | ✅ Done | ASCII ░▒▓█ blocks |
| **Batch Processing** | ✅ Done | `--batch` + `--pattern` |

### Quality Metrics

| Metric | Value |
|--------|-------|
| Unit Tests | 44 passing |
| Clippy Warnings | 0 |
| Build Status | ✅ Success |
| Documentation | ✅ Complete |

---

## Version History

### v0.3.1 (Current - 2026-04-11) ✅

**Milestone**: Production-ready with high-quality audio processing

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
- Pre-emphasis filter
- Multi-band onset detection
- Median-based normalization (MAD)
- Peak picking with prominence
- Multi-scale energy splitting

### v0.1.0 (2026-04-09)
- MVP with core JDilla-style chopping
- Demo mode for testing
- TUI framework
- 44 unit tests

---

## Technical Debt

### Completed ✅

| Item | Status |
|------|--------|
| Remove unused imports | ✅ Done |
| Fix Clippy warnings (10 → 0) | ✅ Done |
| HumAnalyzer caching | ✅ Done |
| NaN-safe f32 sorting | ✅ Done |
| Better dither RNG | ✅ Done |

### Remaining

| Item | Priority | Notes |
|------|----------|-------|
| Add rustdoc comments | Low | For public APIs |
| Integration tests | Medium | Real audio recordings |
| Cross-platform testing | Medium | macOS, Windows |
| Benchmark pitch detection | Low | Performance profiling |

---

## Contribution Guidelines

1. Ensure tests pass: `cargo test`
2. Run clippy: `cargo clippy --all-targets`
3. Format code: `cargo fmt`
4. Update documentation if needed
5. Add tests for new features

---

## Roadmap

### ✅ Phase 1: Core Quality (Complete)

- [x] JDilla-style chopping
- [x] Strength/Pitch matching
- [x] Multi-band transient detection
- [x] Pre-emphasis filtering
- [x] MAD normalization
- [x] Peak picking with prominence
- [x] **Crossfade between chops**
- [x] **Rubato sinc resampling**

### ✅ Phase 2: Workflow (Complete)

- [x] Headless CLI mode
- [x] **Chop preview in TUI**
- [x] **Waveform visualization**
- [x] **Batch processing**

### Phase 3: Enhanced Features (Future)

| Feature | Priority | Description |
|---------|----------|-------------|
| Beat grid quantization | High | Snap chops to BPM grid |
| SFZ export | Medium | Generate sampler patches |
| MIDI output | Medium | Export detected notes as .mid |
| Basic Pitch (ONNX) | Low | ML-based pitch detection |
| Multi-sample blending | Low | Layer multiple sources |

### Phase 4: GUI (Future)

| Feature | Priority | Status |
|---------|----------|--------|
| Dioxus GUI | Medium | Waveform display, drag-drop |
| WASM version | Low | Browser-based tool |

---

## Known Constraints

- Monophonic humming only (polyphonic unreliable)
- Background noise affects pitch detection
- Commercial-friendly licenses only (Apache 2.0, MIT)
- Output format is WAV only
- Max recording duration: 15 seconds (TUI)

---

## Project Statistics

| Metric | Value |
|--------|-------|
| Source Files | 9 |
| Unit Tests | 44 |
| Clippy Warnings | 0 |
| Rust Source Lines | ~4000 |
| Dependencies | 16 |
| Completion | 92% |