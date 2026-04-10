# HumChop - Implementation Status & TODO

## Completed Modules

### ✅ error.rs
- [x] `HumChopError` enum with all variants
- [x] `From` implementations for cpal, hound, symphonia
- [x] `Display` for user-friendly messages

### ✅ audio_utils.rs
- [x] `load_audio()` - WAV/MP3/FLAC loading via symphonia
- [x] `write_wav()` - WAV output via hound
- [x] `write_wav_with_options()` - WAV with configurable options (v0.1.3)
- [x] `apply_dither()` - TPDF dithering with content-seeded RNG (v0.1.3)
- [x] `normalize()` - Peak normalization
- [x] `resample()` - Sample rate conversion
- [x] `to_mono()` - Channel to mono conversion
- [x] Unit tests for load/write round-trip
- [x] Unit tests for normalize
- [x] Unit tests for resample

### ✅ hum_analyzer.rs
- [x] `Note` struct with pitch, onset, duration, velocity
- [x] `detect_pitch()` - YIN algorithm
- [x] `detect_onsets()` - Spectral flux
- [x] `transcribe()` - Combine pitch + onset detection
- [x] Handle single-note detection error
- [x] `to_midi_note()` - MIDI number conversion
- [x] `to_note_name()` - Note name (e.g., "A4", "C#5")
- [x] `#[derive(Default)]` for `PitchAlgorithm`
- [x] Unit tests

### ✅ sample_chopper.rs (v0.1.2 - Improved)
- [x] Pre-emphasis filter (high-frequency boost)
- [x] Multi-band onset strength (RMS + full-band flux + high-flux + mid-flux)
- [x] Median-based normalization (sliding MAD scaling)
- [x] Peak picking with prominence (3-pass algorithm)
- [x] Multi-scale energy splitting fallback (5 frame sizes)
- [x] Integrated strength scoring (60% mean + 40% peak)
- [x] Boundary jitter for imperfect-feel
- [x] Min/max chop length constraints
- [x] NaN-safe `total_cmp()` sorting (v0.1.3)
- [x] Unit tests

### ✅ mapper.rs
- [x] Strength-based matching (default JDilla mode)
- [x] Pitch-based matching (optional)
- [x] `match_by_strength()` - Velocity to chop strength
- [x] `match_by_pitch()` - Pitch proximity matching
- [x] `apply_fade()` - Click noise prevention
- [x] `apply_pitch_shift()` - Optional pitch correction
- [x] `apply_velocity_gain()` - Velocity-based gain
- [x] `process()` - Full mapping pipeline
- [x] `render_output()` - Final audio rendering
- [x] `soft_knee_compress()` - Soft clipping with cosine knee (v0.1.3)
- [x] HumAnalyzer caching for performance (v0.1.3)
- [x] NaN-safe `total_cmp()` sorting (v0.1.3)
- [x] Unit tests (44 total)

### ✅ recorder.rs
- [x] cpal microphone recording
- [x] Sample format handling (F32/I16/U16)
- [x] Proper normalization to ±1.0 range
- [x] Mono channel conversion
- [x] Error handling (device not found, busy)
- [x] WSL2 PulseAudio support
- [x] Unit tests

### ✅ player.rs
- [x] rodio audio playback
- [x] Preview with auto-stop
- [x] Error handling

### ✅ tui.rs
- [x] State machine: Idle/Loading/Ready/Recording/Processing/Complete/Error
- [x] Key bindings: q (quit), r (record), m (toggle mode), p (preview)
- [x] Layout: Header, Main, Footer
- [x] Recording level meter
- [x] Progress display
- [x] `#[derive(Default)]` for `AppState`

### ✅ main.rs
- [x] CLI with clap
- [x] Options: --pitch-shift, --pitch-matching, -o/--output
- [x] Demo mode for testing without microphone
- [x] Full recording workflow
- [x] Error handling and user feedback
- [x] `--no-tui` headless mode (v0.1.3)
- [x] `--num-chops` for custom chop count (v0.1.3)
- [x] `--dither` for triangular noise dithering (v0.1.3)
- [x] `--bits` for configurable bit depth (v0.1.3)

---

## Integration Status

- [x] CLI → Load sample → Process
- [x] Note sequence + sample → Chopper → Chops
- [x] Notes + Chops → Mapper → Final audio
- [x] Final audio → write_wav → output file
- [x] End-to-end test with demo notes
- [x] **44 unit tests passing**

---

## Documentation ✅

- [x] SPEC.md - Complete project specification (updated v0.1.3)
- [x] README.md - Installation, usage, key features (updated v0.1.3)
- [x] PLAN.md - Development roadmap (updated v0.1.3)
- [x] TESTING.md - Manual test verification guide (updated v0.1.3)
- [x] QWEN.md - Project context for AI assistant
- [x] TODO.md - This file

---

## Quality Fixes Applied

### v0.1.3 - Design & Output Quality
1. **HumAnalyzer Caching** - Mapper caches HumAnalyzer (avoids FFT planner allocation per chop)
2. **Soft Clip Fix** - Corrected formula: `output = input / sqrt(1 + excess²)`
3. **NaN Safety** - All f32 sorting uses `total_cmp()` instead of `partial_cmp()`
4. **Better Dither RNG** - Content-seeded + LCG + xorshift
5. **Headless Mode** - `--no-tui` and `--num-chops` for scripting
6. **Dithering** - TPDF triangular noise dithering for 16/24-bit
7. **Bit Depth** - `--bits` for 16/24/32-bit output

### v0.1.2 - Chopping Quality
1. **Pre-Emphasis Filter** - High-frequency boost prevents bass masking
2. **Multi-Band Detection** - Full-band + high-flux + mid-flux for all content types
3. **MAD Normalization** - Sliding median replaces naive mean threshold
4. **Peak Prominence** - 3-pass algorithm for precise boundary placement
5. **Multi-Scale Fallback** - 5 frame sizes for optimal split points
6. **Integrated Scoring** - Chop strength over entire region, not single frame

### v0.2.0
1. **Audio Recording Normalization** - i16/U16 samples properly normalized
2. **Chop Count Consistency** - Loop limit prevents infinite loops
3. **Click Noise Prevention** - Fade in/out applied to each chop
4. **Recording Drain Loop** - Dynamic limit prevents early cutoff

---

## Remaining Clippy Warnings

**All 10 clippy warnings resolved in v0.1.3** ✅

---

## Next Improvement Priorities

### High Priority — Audio Quality

These improvements have the biggest impact on the user's end result:

1. **Crossfade between chops** — Currently 5ms gaps between chops; smooth crossfade would eliminate any remaining click artifacts and create a more polished output
2. **Rubato resampling in mapper** — Replace `linear_resample()` with `rubato::SincResampler` for higher-quality pitch shifting (linear interpolation introduces aliasing artifacts)
3. ✅ **Soft-knee compression on output** — Prevent clipping when multiple chops overlap during rendering (v0.1.3)
4. ✅ **Dithering for 16-bit output** — Optional dither when writing output to reduce quantization noise (v0.1.3)

### Medium Priority — Workflow

These make the tool easier to use and debug:

5. ✅ **`--no-tui` headless mode** — Add a `--no-tui` flag for scripting/batch processing (v0.1.3)
6. **Chop preview in TUI** — Let users preview individual chops before processing (`1-9` keys to hear each chop); helps verify transient detection quality
7. **Waveform visualization** — Simple ASCII waveform in TUI showing chop boundaries; gives visual feedback on where chops land
8. **Batch processing** — Process multiple samples at once from a directory; `humchop ./drums/*.wav --batch`

### Low Priority — Features

Nice-to-have features for future versions:

9. **Beat grid quantization** — Snap chop boundaries to a user-defined BPM/grid; useful for producers who want rhythmic consistency
10. **SFZ export** — Generate an SFZ sampler patch from the chops; lets users play the chops as a virtual instrument in any DAW
11. **MIDI output** — Export detected hum notes as a `.mid` file; useful for further editing in a DAW
12. **Multi-sample blending** — Load 2-3 source samples and blend chops across them; creates richer, layered output

---

## Known Constraints

- Monophonic humming only (polyphonic unreliable)
- Background noise affects pitch detection
- Commercial-friendly licenses only (Apache 2.0, MIT)
- Output format is WAV only (no MP3/FLAC encoding)
- Max recording duration: 15 seconds (TUI)
