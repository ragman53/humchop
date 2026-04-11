# HumChop Testing Guide

## Overview

This document describes how to verify HumChop functionality through automated and manual testing.

## Quick Test Commands

```bash
# 1. Run all unit tests
cargo test

# 2. Run clippy for warnings (all targets)
cargo clippy --all-targets

# 3. Format code
cargo fmt -- --check

# 4. Build (demo mode)
cargo build --no-default-features --features core-only

# 5. Build (with audio support)
cargo build --features audio-io
```

---

## Unit Test Coverage

### 44 Tests Passing

| Module | Tests | Description |
|--------|-------|-------------|
| `hum_analyzer` | 4 | Pitch detection, MIDI conversion, RMS calculation |
| `sample_chopper` | 8 | Chopping, boundaries, strength scoring |
| `mapper` | 12 | Matching, rendering, soft clip, crossfade |
| `audio_utils` | 6 | Loading, saving, resampling |
| `recorder` | 4 | Audio level calculation |
| `player` | 4 | State management |

---

## Manual Test Scenarios

### 1. Sample Loading Test

**Purpose**: Verify audio file loading works for WAV, MP3, FLAC formats.

```bash
# Test WAV
cargo run -- test-sample-01.mp3 --no-tui

# Test headless mode
cargo run -- test-sample-01.mp3 --no-tui --num-chops 8
```

**Expected Output**:
- File loaded successfully
- Sample rate, duration displayed
- Demo notes listed (A4, C5, E5, G5)
- Output file created: `output_chopped_*.wav`

---

### 2. JDilla-Style Chopping Test

**Purpose**: Verify chops keep original length and are played back-to-back.

**Steps**:
```bash
cargo run -- test-sample-01.mp3 --no-tui
```

**Expected Behavior**:
- Chops detected at transient points (attacks)
- Each chop keeps original length
- **Smooth crossfade** between overlapping regions (v0.1.4)
- High-velocity notes → strong transient chops

---

### 3. Crossfade Test (v0.1.4)

**Purpose**: Verify smooth transitions between overlapping chops.

**Steps**:
```bash
# Default: crossfade enabled
cargo run -- test-sample-01.mp3 --no-tui

# Check logs for crossfade mode
RUST_LOG=debug cargo run -- test-sample-01.mp3 --no-tui 2>&1 | grep -i crossfade
```

**Expected Behavior**:
- Overlapping regions use smooth crossfade
- No clicks or artifacts at boundaries
- Envelope weighting prevents double-volume

---

### 4. Pitch Shift Quality Test (v0.1.4)

**Purpose**: Verify high-quality sinc interpolation prevents aliasing.

**Steps**:
```bash
# Test with pitch shifting (uses Rubato SincFixedIn)
cargo run -- test-sample-01.mp3 --no-tui --pitch-shift
```

**Expected Behavior**:
- Pitch-shifted audio has no audible aliasing
- Smooth transitions between notes
- Processing may be slightly slower (high quality)

---

### 5. Strength Matching Test

**Purpose**: Verify high-velocity notes map to strong transients.

**Run test**:
```bash
cargo test test_strength_matching -- --nocapture
```

**Expected**:
- Loud note (vel 0.9) → strong chop (strength 0.9)
- Soft note (vel 0.1) → weak chop (strength 0.1)

---

### 6. Pitch Detection Test

**Purpose**: Verify accurate pitch detection from sine waves.

**Run test**:
```bash
cargo test test_pitch_detection_sine_wave -- --nocapture
```

**Expected**:
- 440Hz sine wave detected as A4
- 523Hz sine wave detected as C5
- Error within 10% of expected frequency

---

### 7. Batch Processing Test (v0.1.4)

**Purpose**: Verify batch mode processes multiple files.

**Steps**:
```bash
# Create test directory
mkdir -p test_batch
cp test-sample-01.mp3 test_batch/
cp test-sample-01.mp3 test_batch/sample2.mp3

# Run batch
cargo run -- test_batch/ --batch -o test_batch_output/

# Check output
ls test_batch_output/
```

**Expected**:
- Both files processed
- Output in test_batch_output/
- Progress displayed: [1/2], [2/2]
- Success/fail counts shown

---

### 8. TUI Chop Preview Test (v0.1.4)

**Purpose**: Verify chop preview with [1-9] keys.

**Steps**:
```bash
# Run TUI mode (requires audio-io)
cargo run -- test-sample-01.mp3

# In TUI:
# 1. Load sample (waveform displayed)
# 2. Press 'r' to start recording
# 3. Press 'r' to stop
# 4. Wait for processing
# 5. Press '1' through '9' to preview chops
```

**Expected Behavior**:
- Waveform shown when sample loads
- Chop details displayed when number pressed
- ASCII waveform (░▒▓█) shows chop shape
- Start time, duration, strength shown

---

### 9. Output Playback Test

**Purpose**: Verify output audio sounds correct.

**Steps**:
```bash
# Generate output
cargo run -- test-sample-01.mp3 --no-tui -o test_output.wav

# Play with ffplay
ffplay test_output.wav

# Check file format
file test_output.wav
```

**Expected**:
- Audio plays without clicks
- Chops play in sequence (back-to-back)
- Pitch matches demo notes (A4, C5, E5, G4)

---

### 10. CLI Options Test

**Test all CLI combinations**:
```bash
# With pitch shift
cargo run -- test-sample-01.mp3 --pitch-shift -o pitch_test.wav

# With pitch matching
cargo run -- test-sample-01.mp3 --pitch-matching -o pitch_match_test.wav

# 16-bit with dither
cargo run -- test-sample-01.mp3 --bits 16 --dither -o dither_test.wav

# Batch with pattern
cargo run -- ./samples/ --batch --pattern "*.wav" -o ./output/
```

**Expected**:
- All commands complete without error
- Different output files created
- Correct bit depth in file metadata

---

### 11. Error Handling Test

**Test edge cases**:
```bash
# Non-existent file
cargo run -- nonexistent.mp3  # Should fail gracefully

# Empty directory (batch mode)
mkdir empty_dir
cargo run -- empty_dir/ --batch  # Should handle gracefully

# Very short sample
# (handled in code with fallback)
```

**Expected**:
- Clear error messages
- No panics
- Helpful guidance for users

---

## Module-Specific Tests

### hum_analyzer

```bash
cargo test hum_analyzer -- --nocapture
```

Tests:
- `test_pitch_detection_sine_wave`: YIN algorithm on 440Hz sine
- `test_note_to_midi`: A4 → 69
- `test_note_to_name`: 440Hz → "A4"
- `test_calculate_rms`: RMS of [0.5, -0.5, 0.5, -0.5] → 0.5
- `test_transcribe_continuity`: Two-tone detection

### sample_chopper

```bash
cargo test sample_chopper -- --nocapture
```

Tests:
- `test_chop_empty_error`: Empty sample → error
- `test_chop_zero_chops_error`: 0 chops → error
- `test_chop_single`: 1 chop → full sample
- `test_dilla_produces_correct_count`: 4 beats → 4 chops
- `test_dilla_chops_cover_full_sample`: All samples covered
- `test_dilla_chop_lengths_are_variable`: Not equal division
- `test_dilla_strength_scores_in_range`: All scores 0.0-1.0
- `test_dilla_min_chop_length_respected`: Min 30ms enforced
- `test_chop_indices_sequential`: Indices 0, 1, 2...

### mapper

```bash
cargo test mapper -- --nocapture
```

Tests:
- `test_mapper_creation`: Creates with 44100 Hz
- `test_mapper_with_options`: Builder pattern works
- `test_map_notes_to_chops`: Maps 4 notes to 4 chops
- `test_pitch_diff_semitones`: 440→880Hz = 12 semitones
- `test_apply_velocity_gain`: 0.5 × 0.5 = 0.25
- `test_process_empty_notes`: Empty notes → error
- `test_process_empty_chops`: Empty chops → error
- `test_render_output`: Renders to valid audio
- `test_jdilla_keeps_original_length`: No time stretching
- `test_simple_resample`: Up/down sampling works
- `test_strength_matching`: Loud→strong, soft→weak
- `test_soft_knee_compress_*`: Soft clipping tests

### audio_utils

```bash
cargo test audio_utils -- --nocapture
```

Tests:
- `test_load_wav_mono`: WAV loading
- `test_write_wav`: WAV writing
- `test_round_trip`: Load → write → load
- `test_normalize`: Peak normalization
- `test_to_mono`: Stereo to mono conversion
- `test_resample`: Sample rate conversion
- `test_empty_audio_error`: Empty input handling

---

## Integration Test Checklist

After any code change, verify:

- [ ] `cargo test` passes (all 44 tests)
- [ ] `cargo clippy --all-targets` shows no warnings
- [ ] `cargo fmt -- --check` shows no formatting issues
- [ ] `cargo build` succeeds
- [ ] `cargo build --features audio-io` succeeds
- [ ] Demo mode produces valid WAV output
- [ ] Output plays without clicks
- [ ] Crossfade produces smooth transitions
- [ ] Batch mode processes multiple files

---

## Known Test Limitations

1. **Synthetic audio only**: Tests use sine waves and drum loops, not real recordings
2. **Monophonic assumption**: Polyphonic pitch detection not tested
3. **Noise sensitivity**: High levels of background noise may affect results
4. **WSL2 audio**: Recording requires PulseAudio setup on WSL

---

## Troubleshooting

### Test Failures

If `cargo test` fails:
```bash
# Run with backtrace
RUST_BACKTRACE=1 cargo test

# Run single failing test
cargo test test_name -- --nocapture
```

### Build Issues

If build fails:
```bash
# Clean and rebuild
cargo clean
cargo build

# Update dependencies
cargo update
```

### Audio Playback Issues

If output doesn't play:
```bash
# Check file format
file output_chopped_*.wav

# Check with ffprobe
ffprobe output_chopped_*.wav
```

---

## Continuous Integration

Before submitting changes, run:
```bash
cargo fmt
cargo clippy --all-targets
cargo test
cargo build --release
```