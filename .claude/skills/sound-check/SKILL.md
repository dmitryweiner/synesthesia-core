---
name: sound-check
description: Measure Synesthesia's actual sound instead of guessing — fractality of a point, its character, onset hits (does the picture react to bells/drops?), clicks at preset switches, phase continuity under modulation, which scout render ranks like a full one — with the core's syn-bench and tests. Use when changing syn-core's dsp, fx, engine, features, presets, coupling or the scout, or when the user reports beating, clicks, silence or "the picture doesn't react".
---

# Measure the sound

There are no speakers here, and stills show nothing about sound. Every knob
in this project was set from one of these numbers. (Moved here from the web
app with the sound, synesthesia PLAN-CORE.md C6: what `analyze.mjs` did in a
browser, `syn-bench` does on the core, on every CPU core.)

## Tools

```bash
cargo run --release -p syn-bench --                       # fractality of every preset (30 s @ 22050)
cargo run --release -p syn-bench -- --onsets              # onset hits + swell per preset, through the engine
cargo run --release -p syn-bench -- --switch 0,3,10 --at 12   # clicks at switches, the way the apps switch
cargo run --release -p syn-bench -- --configs 30@22050,24@11025 --mutants 6   # cheap vs full render: Spearman ρ
cargo run --release -p syn-bench -- --preset 0 --mutants 8   # what 👍/👎 would propose, scored
cargo run --release -p syn-bench -- --random 12           # presets vs random points
cargo run --release -p syn-bench -- --preset 0 --wav shots/wav   # WAVs to listen to / re-analyze
cargo run --release -p syn-bench -- --preset 12 --secs 60 --png shots/png   # waterfall + loudness PNG: LOOK at it
cargo run --release -p syn-bench -- --character --ref 0,3,5,6,8   # what kind of sound + distance to the liked family
cargo test --release -p syn-core --test continuity --test onsets
cargo test --release -p syn-player --test switch
```

The bench renders with `render_offline` — the engine the apps play,
generators, LFOs, FX and limiter. A render is **deterministic**. Unlike the
browser's convolver, the FDN reverb has no seeded room: `--repeat N` varies
only the noise generators' seeds, so a point without noise reads ±0.00.

## What the numbers mean

| number | healthy | meaning |
|---|---|---|
| `score` (fractality) | presets 0.6–1.0 at 30–60 s | 1/f-ness of loudness and timbre + spectrogram structure |
| `envβ` / `cenβ` | ≈1 (pink) | β of the 1/f^β fit; 0 = restless, 2 = static; `—` = too flat to fit |
| `box` | ~1.5–1.7 | box-counting dimension of the loudest spectrogram cells |
| onset hits / 16 s (`--onsets`) | bells ~1 per strike and their echoes; drones ≤ 4 | each hit seeds growth + a ripple in the picture. Through the core's FX the engine fires more than the browser did (Bell spots 11 vs 7: the delay's echoes, plain on `--png`) |
| swell range | ±0.3…0.9 on drones | drives the exposure pulse |
| clicks at a switch (`--switch`) | 0 near, instant HF ≈ 0 | attacks inside struck presets are *not* faults: read the waterfall |
| roughness ratio (tests/continuity.rs) | < 1.3 | modulated vs unmodulated HF energy; > 2 means phase jumps |
| `drop` (`--character`) | drones 3–6 dB; drips/bells 8–12 | how deep the sound falls out; the liked family never breaks |
| `low` | liked family 0.65–0.96 | energy share < 200 Hz — "fat" |
| `harm` | liked family 0.5–0.9 | one harmonic grid; inharmonic bells rub against a drone |
| `--ref` distance | ≲ 1.5 = inside the family's spread | the metrics > 2 sd away say where a point differs |
| ρ (`--configs`) | the scout's 24 s @ 11 kHz: 0.79 | how well a cheap render ranks candidates like 30 s @ 22 kHz |

## Designing or retuning a preset

Use the `new-preset` skill.

## Tuning a threshold (the method that worked)

1. Write the expectation as behaviour in a test (syn-core/tests/onsets.rs:
   "a bell every ~4 s → 3–6 hits in 16 s", "drones ≤ 4").
2. Compare candidate settings over **all** presets with the bench — never
   tune on one preset.
3. Dry generators are stricter than the engine: FX smooth attacks and add
   echoes. Check both (the onset tests' web-test pipeline = dry,
   `--onsets` = the engine).
4. Keep the bench in `syn-bench` behind a flag, not in a scratch file.

## Frequent causes of complaints

- **"Harsh beating", worse the longer it plays** → an oscillator computing
  `sin(2π·f·t)` from absolute time. Accumulate phase (tests/continuity.rs).
- **The render gets slower over an hour** → a phase never wrapped: wasm's
  `sin` goes multi-precision past ~1.6e6 (generator.rs wraps at 1e5).
- **Clicks when switching points** → a switch without the fade-out
  (syn-player/tests/switch.rs fails then).
- **"The picture doesn't react"** → onset hits first (`--onsets`), then the
  coupling genes, then the exposure in the live app (`body[data-fx-exposure]`).
- **A proposal went silent** → the scout penalizes candidates > 6 dB
  quieter (scout::adjusted_score); check the `dB` column.
