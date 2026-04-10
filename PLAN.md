# HumChop - Development Plan

## Current Status

### ✅ v0.1.0 - MVP Core
- All 40 unit tests passing
- JDilla-style chopping with strength matching
- Pitch-based matching as option
- Click noise prevention (fades)
- Audio recording with proper normalization
- Full TUI workflow

---

## Version History

### v0.2.0 (Planned)
- JDilla-style mode only
- Strength-based matching
- Pitch-based matching option
- Improved transient detection
- Code review fixes (normalization, fades, etc.)

### v0.1.0 - MVP
- Core chopping functionality
- Demo mode for testing
- TUI framework
- 40 tests passing

---

## Technical Debt

### Cleanup
- [x] Remove unused imports
- [ ] Add rustdoc comments for public APIs
- [ ] Benchmark pitch detection accuracy

### Performance
- [ ] Parallel FFT for onset detection
- [ ] Batch process chops
- [ ] Memory profiling for large files

### Testing
- [x] Unit tests for all modules
- [ ] Integration tests with real audio
- [ ] Cross-platform testing

---

## Contribution Guidelines

1. Ensure tests pass: `cargo test`
2. Run clippy: `cargo clippy`
3. Format code: `cargo fmt`
4. Update documentation if needed

---

## Roadmap

### v1.0.0 Target
- [ ] GUI with Dioxus
- [ ] Basic Pitch (ONNX) integration
- [ ] MIDI output
- [ ] SFZ export
- [ ] Cross-platform installers
