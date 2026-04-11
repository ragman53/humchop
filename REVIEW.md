# HumChop — Deep Code Review & Redesign Proposal

## Review Date: 2026-04-11
## Version Reviewed: v0.1.4 / v0.4.1
## Reviewer: Claude Sonnet 4.6

---

## Executive Summary

The codebase has a solid structural foundation — clean module separation, zero clippy warnings,
44 passing tests, and well-chosen DSP libraries. However, the core hum-to-chop mapping pipeline
contains **fundamental design errors** that make the primary feature (hum-driven sample chopping)
non-functional in a musical sense. The issues are not superficial; they reflect a misunderstanding
of what "JDilla-style" means in practice.

**Root cause in one sentence:** The hummed melody is analyzed but its timing and duration
information is almost entirely discarded when building the output audio.

---

## Critical Bugs (Audio Quality Breakage)

### BUG-1 — Hum Timing Completely Ignored in Output Placement

**File:** `src/mapper.rs`, `process()`, lines ~490–520

**Current code:**
```rust
let mut current_onset = 0.0;
for (note_idx, &chop_idx) in mappings.iter().enumerate() {
    let mapped = self.process_mapping(note, chop, current_onset);
    let gap = 0.005; // 5ms gap
    current_onset += mapped.output_duration + gap;  // ← chop length drives timing
}
```

**Problem:** `current_onset` advances by the *chop's own length*, not by the note's onset time.
The hummed rhythm is completely ignored. Every chop plays back-to-back in chop-length intervals,
producing a mechanical sequence unrelated to the hummed melody.

**Correct behaviour:** Each chop should be placed at the time the corresponding note was hummed.
```rust
// Place chop at the note's hummed onset position
let mapped = self.process_mapping(note, chop, note.onset_sec);
```

---

### BUG-2 — Chop Length Never Trimmed to Note Duration

**File:** `src/mapper.rs`, `process_mapping()`, lines ~455–480

**Current code:**
```rust
let mut samples = chop.samples.clone(); // full chop, untouched
// velocity applied, fades applied, but LENGTH never changed
let output_duration = samples.len() as f64 / self.sample_rate as f64;
```

**Problem:** If a note lasts 0.2 s but the matched chop is 0.8 s, the full 0.8 s chop is
placed at that note's position. This causes chops to massively overlap or extend far beyond
where the next note begins, producing a muddy wash instead of a rhythmic pattern.

**Correct behaviour:** Trim (or loop) the chop to match the note's duration.
```rust
let target_len = (note.duration_sec * self.sample_rate as f64) as usize;
let samples = if chop.samples.len() > target_len {
    // Trim with a short fade-out to avoid click
    let mut trimmed = chop.samples[..target_len].to_vec();
    apply_fade_out(&mut trimmed, fade_samples);
    trimmed
} else {
    chop.samples.clone()
};
```

---

### BUG-3 — Crossfade Fade-In Uses Wrong Index

**File:** `src/mapper.rs`, `render_with_crossfade()`, lines ~580–620

**Current code:**
```rust
let fade_in_len = crossfade_samples.min(local_idx);
// ...
let fade_in = if fade_in_len > 0 {
    let t = fade_in_len as f32 / crossfade_samples as f32;
    (std::f32::consts::PI * 0.5 * t).sin()
} else {
    1.0
};
```

**Problem:** `local_idx` is the *current sample position within the chop*, not the fade-in
counter. When `local_idx` is large, `fade_in_len` saturates at `crossfade_samples` immediately,
so the fade-in only applies to the very first `crossfade_samples` samples and then `t` is always
1.0. The intent (ramp up at the start of each chop) accidentally works for the leading edge but
produces `1.0` (no effect) for all other samples, making the envelope logic a no-op except at
the attack.

More critically, `fade_out_len = crossfade_samples.min(mc.len() - local_idx)` produces 0 when
`local_idx == mc.len()`, which is out of bounds and would panic if `mc.len()` is ever 0.

**Correct code:**
```rust
// Proper half-Hann crossfade
let fade_in = {
    let ramp = (local_idx as f32 / crossfade_samples as f32).min(1.0);
    (std::f32::consts::PI * 0.5 * ramp).sin()
};
let fade_out = {
    let remaining = mc.len().saturating_sub(local_idx);
    let ramp = (remaining as f32 / crossfade_samples as f32).min(1.0);
    (std::f32::consts::PI * 0.5 * ramp).sin()
};
let weight = fade_in * fade_out; // multiply, not min
```

---

### BUG-4 — `render_with_crossfade` Checks for Overlaps but Never Acts On It

**File:** `src/mapper.rs`, lines ~560–565

```rust
let _has_overlaps = mapped_chops.windows(2).any(|w| { ... });
```

The `_has_overlaps` variable is computed but never used. The rendering path is identical whether
or not chops actually overlap. The entire overlap-detection block is dead code.

---

### BUG-5 — Demo Notes Are Hardcoded to 4 Notes Regardless of Sample

**File:** `src/main.rs`, `run_headless()` and `run_demo_mode()`, repeated 3×

```rust
let demo_notes = vec![
    Note::new(440.0, 0.0, 0.3, 0.8),
    Note::new(523.0, 0.35, 0.3, 0.7),
    Note::new(659.0, 0.7, 0.3, 0.9),
    Note::new(784.0, 1.05, 0.3, 0.85),
];
```

4 fixed notes spanning 1.35 s are mapped against any sample of any length and any `--num-chops`
value. This means `--num-chops 16` produces 16 chops but only 4 are ever played, and the note
timings do not adapt to the sample duration.

---

### BUG-6 — `apply_fade` Has Off-by-One in Fade-Out Loop

**File:** `src/mapper.rs`, `apply_fade()`, lines ~430–445

```rust
for i in 0..fade_len {
    let idx = len - 1 - i;
    let gain = i as f32 / fade_len as f32;  // ← starts at 0.0, not 1.0
    samples[idx] *= gain;
}
```

When `i == 0` the gain is `0.0`, so the very last sample is silenced. When `i == fade_len - 1`
the gain is `(fade_len-1)/fade_len ≈ 1.0`. The fade-out therefore ramps the *wrong direction*:
it starts silent at the tail and is nearly full-volume one step before the tail. This inverts the
intended fade-out, causing an audible click at the end of every chop.

**Fix:**
```rust
let gain = 1.0 - (i as f32 / fade_len as f32); // descending: 1.0 → ~0.0
```

---

## Major Design Issues

### DESIGN-1 — The "JDilla Style" Concept Is Not Implemented

The documentation and README describe "JDilla-style chopping" as:
> Chops keep their original length (no time-stretching), creating rhythmic patterns from dynamics.

This is correct as a description of the *chopping* step. But the *output rendering* also needs to
place chops according to the hummed rhythm. Currently:

| What should happen | What actually happens |
|---|---|
| Chop plays at note onset time | Chops play sequentially with 5ms gaps |
| Chop plays for note duration (then silence or next chop) | Chop plays its full original length |
| Loud note → strong-transient chop (correct) | ✓ Implemented |
| Hum rhythm drives playback rhythm | Hum rhythm is discarded |

True JDilla-style requires:
1. **Placement** at `note.onset_sec`
2. **Duration** trimmed (not stretched) to `note.duration_sec` with a clean fade-out
3. **Silence** between notes if there is a gap in the hum

---

### DESIGN-2 — Pitch Matching Is Architecturally Disconnected

When `pitch_matching = true`, `match_by_pitch()` returns a chop index, but `process_mapping()`
never applies pitch correction unless `enable_pitch_shift` is *also* true. These are two
orthogonal concepts conflated in the config:

- **Matching mode**: how to *select* a chop (by strength or by pitch proximity)
- **Pitch shifting**: whether to *retune* the chop to the note's exact pitch

A user can set `pitch_matching = true` but `enable_pitch_shift = false`, in which case they get
chops selected by pitch proximity but played at the wrong pitch. The API makes this confusion
easy to stumble into.

---

### DESIGN-3 — `MapperConfig` Mixes Concerns

`MapperConfig` contains rendering parameters (`crossfade_samples`, `enable_crossfade`,
`soft_clip`, `soft_clip_threshold_db`) alongside matching parameters (`strength_matching`,
`enable_pitch_shift`). This makes the struct hard to reason about and easy to misconfigure.
Suggested split:

```rust
pub struct MatchConfig {
    pub mode: MatchMode,          // Strength | Pitch | PitchWithShift
}
pub struct RenderConfig {
    pub crossfade_samples: usize,
    pub enable_crossfade: bool,
    pub soft_clip: bool,
    pub soft_clip_threshold_db: f32,
}
```

---

### DESIGN-4 — No Silence Handling Between Notes

The hum analyzer produces notes with onset times and durations. If the user hums a short staccato
melody, there are silent gaps between notes. Currently those gaps are not represented in the
output — chops are placed sequentially with only a 5ms gap. The output does not reflect the
rhythmic phrasing of the hum.

---

### DESIGN-5 — `estimate_chop_pitch` Called N×M Times via `map_notes_to_chops`

```rust
// In map_notes_to_chops():
let chop_pitches: Vec<f32> = chops.iter().map(|c| self.estimate_chop_pitch(c)).collect();
```

This correctly pre-caches pitches. However, `estimate_chop_pitch` also runs FFT analysis
internally through `HumAnalyzer::detect_pitch`, which re-creates FFT plans on each call
despite `self.hum_analyzer` being cached. The FFT *plan* is reused but the full pitch
detection window loop still runs over the entire chop samples for every chop.

For a 16-chop session on a 44100 Hz sample, this is 16 × full-chop YIN analyses. For longer
samples this becomes the performance bottleneck.

---

### DESIGN-6 — Batch Mode File Matching Is Fragile

**File:** `src/main.rs`, `run_batch()`

```rust
pattern == "*"
    || pattern_ext.is_empty()
    || ext_lower == pattern_ext
    || ext_lower == "wav"    // ← always includes wav/mp3/flac regardless of pattern
    || ext_lower == "mp3"
    || ext_lower == "flac"
```

If the user specifies `--pattern "*.wav"` intending to process only WAV files, the condition
also matches MP3 and FLAC because of the unconditional fallback. The pattern is effectively
ignored for audio files.

---

## Minor Issues

### MINOR-1 — `Cargo.toml` Version Mismatch

`Cargo.toml` declares `version = "0.1.4"` but `Cargo.lock` records `version = "0.4.1"` and
`REVIEW.md` references both. The version should be unified across all files.

### MINOR-2 — `run_interactive` (core-only build) Generates Demo Output Without Notifying User

In the `#[cfg(not(feature = "audio-io"))]` variant, the function silently falls through to
demo note generation without a clear user-facing message explaining *why* the demo is used.

### MINOR-3 — Magic Constants Scattered Across Files

`0.005` (5ms gap), `0.005` (fade duration), `15.0` (max record seconds), `10.0` (max demo
duration) appear as bare literals in multiple functions. These should be named constants or
`DillaConfig`/`MapperConfig` fields.

### MINOR-4 — `tui.rs` `render_complete_content` Layout Overlap

```rust
let info_area = Rect::new(
    area.x,
    area.y + area.height.saturating_sub(5),
    area.width,
    5,
);
```

The `notes_widget` is rendered over the full `area`, and `info_area` overlaps its bottom
5 rows. On terminals shorter than ~20 rows the output info is rendered on top of the notes list.

### MINOR-5 — `player.rs` `stop()` Calls `sink.detach()` Instead of `sink.stop()`

`Sink::detach()` hands ownership to rodio's background thread and keeps playing; it does not
stop audio. The correct call to immediately stop playback is `sink.stop()` or simply dropping
the `Sink`. Currently `Player::stop()` silently continues playing.

---

## Redesign Proposal

The following is a ground-up redesign of the mapping pipeline to make hum-to-chop actually
musical. All other modules (`sample_chopper`, `hum_analyzer`, `audio_utils`) are sound and
require only minor fixes.

---

### Core Concept Clarification

```
Hum Analysis  →  Vec<Note>  { pitch_hz, onset_sec, duration_sec, velocity }
                                  │              │              │
                          WHICH chop?      WHEN to play?   HOW LONG?
                          (matching)       (placement)     (trimming)
```

All three dimensions must be respected in the output.

---

### Proposed Pipeline

```
[Vec<Note>]  ──►  match_notes_to_chops()  ──►  [(Note, Chop)]
                                                      │
                                               process_pair()
                                                 - trim/loop to note.duration_sec
                                                 - apply velocity gain
                                                 - apply pitch shift (optional)
                                                 - fade in/out at exact boundaries
                                                      │
                                               Vec<PlacedChop>
                                               { samples, onset_sec }
                                                      │
                                               render_timeline()
                                                 - place at onset_sec (not sequential)
                                                 - fill gaps with silence
                                                 - crossfade overlapping regions
                                                 - soft-clip final mix
                                                      │
                                               Vec<f32>  output
```

---

### Proposed Data Types

```rust
/// A chop that has been assigned a position in the output timeline.
pub struct PlacedChop {
    pub samples: Vec<f32>,
    pub onset_sec: f64,      // from note.onset_sec — not computed sequentially
    pub chop_index: usize,
}

/// How to handle the duration mismatch between note and chop.
pub enum DurationMode {
    /// Trim the chop to the note duration (classic JDilla — hard cut with fade).
    Trim,
    /// Loop the chop to fill the note duration (pad/drone style).
    Loop,
    /// Play the full chop regardless of note duration (original behaviour).
    FullChop,
}

pub struct MatchConfig {
    pub mode: MatchMode,
    pub duration_mode: DurationMode,
    pub fade_ms: f64,
}

pub struct RenderConfig {
    pub crossfade_ms: f64,
    pub enable_crossfade: bool,
    pub soft_clip: bool,
    pub soft_clip_threshold_db: f32,
}
```

---

### Proposed `process_pair` Implementation

```rust
fn process_pair(
    note: &Note,
    chop: &Chop,
    config: &MatchConfig,
    sample_rate: u32,
) -> PlacedChop {
    let target_len = match config.duration_mode {
        DurationMode::Trim | DurationMode::Loop => {
            (note.duration_sec * sample_rate as f64) as usize
        }
        DurationMode::FullChop => chop.samples.len(),
    };

    let mut samples = match config.duration_mode {
        DurationMode::Trim => {
            let n = target_len.min(chop.samples.len());
            chop.samples[..n].to_vec()
            // pad with zeros if chop is shorter than note (chop ends before note)
            // extend to target_len with silence
        }
        DurationMode::Loop => {
            // Cycle the chop until target_len is reached
            let mut out = Vec::with_capacity(target_len);
            let src = &chop.samples;
            while out.len() < target_len {
                let remaining = target_len - out.len();
                out.extend_from_slice(&src[..remaining.min(src.len())]);
            }
            out
        }
        DurationMode::FullChop => chop.samples.clone(),
    };

    // Velocity
    let gain = note.velocity.clamp(0.0, 1.0);
    for s in &mut samples { *s *= gain; }

    // Fade in/out (short, just to kill clicks)
    let fade_samples = (config.fade_ms * 0.001 * sample_rate as f64) as usize;
    apply_fade(&mut samples, fade_samples);

    PlacedChop {
        samples,
        onset_sec: note.onset_sec,
        chop_index: chop.index,
    }
}
```

---

### Proposed `render_timeline` Implementation

```rust
fn render_timeline(
    placed: &[PlacedChop],
    sample_rate: u32,
    config: &RenderConfig,
) -> Vec<f32> {
    // Calculate total output length from last note's end time
    let total_samples = placed
        .iter()
        .map(|p| (p.onset_sec * sample_rate as f64) as usize + p.samples.len())
        .max()
        .unwrap_or(0);

    let mut output = vec![0.0f32; total_samples];
    let mut env    = vec![0.0f32; total_samples];

    let xfade = if config.enable_crossfade {
        (config.crossfade_ms * 0.001 * sample_rate as f64) as usize
    } else {
        0
    };

    for p in placed {
        let start = (p.onset_sec * sample_rate as f64) as usize;
        for (i, &s) in p.samples.iter().enumerate() {
            let out_idx = start + i;
            if out_idx >= total_samples { break; }

            // Crossfade envelope: ramp in at head, ramp out at tail
            let w_in  = if xfade > 0 {
                ((i as f32 / xfade as f32).min(1.0) * PI * 0.5).sin()
            } else { 1.0 };
            let w_out = if xfade > 0 {
                let tail = p.samples.len() - i;
                ((tail as f32 / xfade as f32).min(1.0) * PI * 0.5).sin()
            } else { 1.0 };

            let weight = w_in * w_out;
            output[out_idx] += s * weight;
            env[out_idx]    += weight;
        }
    }

    // Normalise overlaps
    for i in 0..total_samples {
        if env[i] > 1.0 { output[i] /= env[i]; }
    }

    // Final limiting
    if config.soft_clip {
        soft_knee_compress(&output, config.soft_clip_threshold_db)
    } else {
        output
    }
}
```

---

### Summary of Changes Required

| # | File | Change | Priority |
|---|------|--------|----------|
| 1 | `mapper.rs` | Place chops at `note.onset_sec`, not sequentially | **Critical** |
| 2 | `mapper.rs` | Trim chop length to `note.duration_sec` | **Critical** |
| 3 | `mapper.rs` | Fix `apply_fade` fade-out direction | **Critical** |
| 4 | `mapper.rs` | Fix crossfade envelope formula | High |
| 5 | `mapper.rs` | Remove dead `_has_overlaps` check | Medium |
| 6 | `mapper.rs` | Split `MapperConfig` into `MatchConfig` + `RenderConfig` | Medium |
| 7 | `mapper.rs` | Add `DurationMode` enum (Trim / Loop / FullChop) | Medium |
| 8 | `main.rs` | Scale demo notes to sample/recording duration | High |
| 9 | `main.rs` | Fix batch pattern matching logic | High |
| 10 | `player.rs` | Use `sink.stop()` instead of `sink.detach()` | Medium |
| 11 | `tui.rs` | Fix overlapping render areas in `render_complete_content` | Low |
| 12 | `Cargo.toml` | Unify version string with Cargo.lock | Low |

---

## Verification Checklist (Post-Fix)

- [ ] Output WAV starts at silence then plays first chop at `notes[0].onset_sec`
- [ ] Gap between notes is reproduced as silence in output
- [ ] Staccato hum produces short chops; legato hum produces longer chops
- [ ] `--no-tui` with 4 demo notes produces 4 distinct events at correct times
- [ ] `render_with_crossfade` produces no clicks at chop boundaries
- [ ] `Player::stop()` actually stops audio
- [ ] `--batch --pattern "*.wav"` does not process MP3/FLAC files
- [ ] All 44 existing unit tests still pass
- [ ] `cargo clippy --all-targets` clean

---

## Conclusion

The DSP subsystems (chopper, pitch detection, resampling) are implemented correctly and can
remain largely unchanged. The fix effort is concentrated in `mapper.rs` (~150 lines) and
`main.rs` (~30 lines). With the critical bugs fixed, the tool will for the first time actually
reproduce the hummed rhythm in the output audio.