# HumChop Testing Guide

## Overview

This document describes how to verify HumChop functionality through manual testing.

## Quick Test Commands

```bash
# 1. Run all unit tests
cargo test

# 2. Run clippy for warnings
cargo clippy

# 3. Format code
cargo fmt -- --check

# 4. Build (demo mode)
cargo build --no-default-features --features core-only

# 5. Build (with audio support)
cargo build --features audio-io
```

## Manual Test Scenarios

### 1. Sample Loading Test

**Purpose**: Verify audio file loading works for WAV, MP3, FLAC formats.

**Steps**:
```bash
cargo run -- test-sample.wav
```

**Expected Output**:
- File loaded successfully
- Sample rate, duration displayed
- Demo notes listed (A4, C5, E5, G4)
- Output file created: `output_chopped_*.wav`

**Verification**:
- [ ] No panic or error messages
- [ ] Output file exists and has valid WAV header
- [ ] Duration is reasonable (based on number of notes × chop lengths)

### 2. JDilla-Style Chopping Test

**Purpose**: Verify chops keep original length and are played back-to-back.

**Steps**:
```bash
# Process with different note counts
cargo run -- test-sample.wav
cargo run -- test-sample-01.mp3
```

**Expected Behavior**:
- Chops are detected at transient points (attacks) using multi-band analysis
- Each chop keeps its original length
- Chops play with tiny gaps (5ms) to prevent clicks
- High-velocity notes → strong transient chops (kick, snare)

**Verification**:
- [ ] Output length matches or exceeds input (chops are back-to-back)
- [ ] Click artifacts are minimized (fade working)
- [ ] Variable chop lengths (not equal division)
- [ ] Transient detection is accurate (chops align with musical events)

### 3. Strength Matching Test

**Purpose**: Verify high-velocity notes map to strong transients.

**Test with custom notes**:
```bash
# Check test_strength_matching test output
cargo test test_strength_matching -- --nocapture
```

**Expected**:
- Loud note (vel 0.9) → strong chop (strength 0.9)
- Soft note (vel 0.1) → weak chop (strength 0.1)

### 4. Pitch Detection Test

**Purpose**: Verify accurate pitch detection from sine waves.

**Run test**:
```bash
cargo test test_pitch_detection_sine_wave -- --nocapture
```

**Expected**:
- 440Hz sine wave detected as A4
- 523Hz sine wave detected as C5
- Error within 10% of expected frequency

### 5. Output Playback Test

**Purpose**: Verify output audio sounds correct.

**Steps**:
```bash
# Generate output
cargo run -- test-sample.wav -o test_output.wav

# Play with ffplay (or any audio player)
ffplay test_output.wav
# or
play -t wav test_output.wav  # sox
```

**Expected**:
- Audio plays without clicks
- Chops play in sequence (back-to-back)
- Pitch matches demo notes (A4, C5, E5, G4)

### 6. CLI Options Test

**Test all CLI combinations**:
```bash
# With pitch shift
cargo run -- test-sample.wav --pitch-shift -o pitch_test.wav

# With pitch matching
cargo run -- test-sample.wav --pitch-matching -o pitch_match_test.wav

# Combined
cargo run -- test-sample.wav --pitch-shift --pitch-matching -o combo_test.wav
```

**Expected**:
- All commands complete without error
- Different output files created
- Pitch-shift slightly changes perceived pitch

### 7. Error Handling Test

**Test edge cases**:
```bash
# Empty file
touch empty.wav
cargo run -- empty.wav  # Should fail gracefully

# Non-existent file
cargo run -- nonexistent.mp3  # Should show helpful error

# Sample too short
echo "Test with very short sample" # handled in code
```

**Expected**:
- Clear error messages
- No panics
- Helpful guidance for users

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

### audio_utils

```bash
cargo test audio_utils -- --nocapture
```

Tests:
- `test_load_audio_*`: Various format loading
- `test_write_wav_*`: WAV writing
- `test_round_trip_*`: Load → write → load
- `test_normalize_*`: Peak normalization

## Integration Test Checklist

After any code change, verify:

- [ ] `cargo test` passes (all 40 tests)
- [ ] `cargo clippy` shows no new warnings
- [ ] `cargo fmt` shows no formatting issues
- [ ] `cargo build` succeeds
- [ ] `cargo build --features audio-io` succeeds
- [ ] Demo mode produces valid WAV output
- [ ] Output plays without clicks

## Known Test Limitations

1. **Synthetic audio only**: Tests use sine waves and drum loops, not real recordings
2. **Monophonic assumption**: Polyphonic pitch detection not tested
3. **Noise sensitivity**: High levels of background noise may affect results
4. **WSL2 audio**: Recording requires PulseAudio setup on WSL

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

## Continuous Integration

Before submitting changes, run:
```bash
cargo fmt
cargo clippy
cargo test
cargo build --release  # optional, slower
```
