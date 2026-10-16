# Effects

After the generators and the layers are mixed, the sound runs through one
**FX chain** in a fixed order. Each module has an on/off switch; a module
that is off is skipped. Code: `syn-core/src/fx/` (`chain.rs` is the signal
path); fields and defaults: `defaultFx` and `fxParamRanges` in
`assets/schema.json`.

```text
mix ─► filter ─► chorus / flanger ─► phaser ─► delay (+ shimmer) ─► reverb ─► limiter ─► (+ dry layers) ─► master
       biquad │ formant │ comb
```

The chain follows the web app's audio graph — the same order and the same
parameter meanings — with its own DSP (the browser's nodes are not
re-implemented), calibrated against the browser so the presets sound as
they were tuned.

In a **stereo** app the chain runs once per ear with the same settings:
the right ear's chorus and phaser LFOs run `width` × 90° ahead, the reverb
is one room whose even lines are heard on the left and odd ones on the
right, the binaural beat's shift sits between the reverb and the limiter,
and one limiter gain holds both ears. See
[Space and binaural beats](binaural.md).

## Shared behaviour

- **Every numeric parameter glides** to its new value with a 30 ms time
  constant. Values arrive at block rate from LFO routes and from the morph
  between points; stepping them would zipper.
- **Feedback loops carry 128 samples of extra delay**, as a Web Audio
  feedback loop does (one render quantum). The presets were tuned through
  it, and without it the comb resonated on the wrong partials.
- **A hard switch** to another point (loading a preset, 🎲) drops every
  tail — the old reverb and echoes never bleed into the new sound.
- Feedback amounts are clamped below 1, so the chain stays stable with
  every module at full feedback (a test checks it).

Parameters marked **(mod)** can be moved by an LFO (see
[LFOs and modulation](lfo.md)); **exp** ones move in octaves.

## Filter

`filterOn`, `filterType`, and the sliders below. The type decides what the
filter is and which sliders apply.

| type | what it does | uses |
|---|---|---|
| `lowpass` | removes what is above the cutoff | cutoff, Q (resonance) |
| `highpass` | removes what is below the cutoff | cutoff, Q |
| `bandpass` | keeps a band around the frequency | frequency, Q (narrowness) |
| `notch` | cuts a narrow band out | frequency, Q |
| `peaking` | boosts or cuts a band | frequency, Q, gain |
| `lowshelf` | boosts or cuts everything below | frequency, gain |
| `highshelf` | boosts or cuts everything above | frequency, gain |
| `allpass` | changes phase only (audible against the dry sound or in other modules) | frequency, Q |
| `formant` | **a vowel**: three band-passes at the formant frequencies of A, E, I, O, U, weighted like a voice | vowel (morphs smoothly A→E→I→O→U), cutoff as a formant shift (1000 Hz = unshifted), Q as sharpness |
| `comb` | a short delay fed back on itself: a metallic resonance at the cutoff frequency and its harmonics | frequency (the pitch it rings at), comb feedback |

The first eight are standard RBJ biquads (`biquad.rs`).

| slider | field | range (default) |
|---|---|---|
| Cutoff / frequency (mod, exp) | `filterFreq` | 20–2000 Hz (1000) |
| Q (mod) | `filterQ` | 0.1–30 (0.7) |
| Gain (mod) | `filterGain` | −24–24 dB (0) |
| Vowel (mod) | `filterVowel` | 0–1 (0) = A … U |
| Comb feedback (mod) | `filterCombFb` | 0–0.95 (0.5) |

## Chorus / flanger

A copy of the sound through a short delay whose length an internal sine
LFO sweeps; mixed with the dry sound it thickens it (chorus) or, with a
shorter delay and feedback, sweeps a comb of notches through it (flanger —
the "jet" sound).

| slider | field | range (default) |
|---|---|---|
| Mode | `chorusMode` | `chorus` (12 ms base delay) or `flanger` (2 ms) |
| Rate (mod, exp) | `chorusRate` | 0.01–8 Hz (0.35) |
| Depth (mod) | `chorusDepth` | 0–20 ms (6) — how far the delay swings |
| Mix (mod) | `chorusMix` | 0–1 (0.35), dry ↔ wet |
| Feedback (mod) | `chorusFb` | 0–0.95 (0.15) — stronger, more metallic sweep |

## Phaser

2, 4, 6 or 8 all-pass stages whose centre frequency an internal sine LFO
sweeps between 200 Hz and 200 + 3600·depth Hz, with feedback around them;
mixed with the dry sound, the phase shifts become moving notches — a
softer, vocal sweep than the flanger.

| slider | field | range (default) |
|---|---|---|
| Stages | `phaserStages` | 2, 4, 6, 8 (4) — more stages, more notches |
| Rate (mod, exp) | `phaserRate` | 0.1–10 Hz (0.5) |
| Depth (mod) | `phaserDepth` | 0–1 (0.7) — the width of the sweep |
| Feedback (mod) | `phaserFb` | 0–0.9 (0.3) — sharper notches |
| Mix (mod) | `phaserMix` | 0–1 (0.5) |

## Delay / echo, with shimmer

A feedback delay up to 3 s. Its feedback runs **through an octave-up
pitch shifter**: with *shimmer* at 0 the echo repeats as it is; turned up,
every repeat comes back an octave higher and blooms into a halo above the
drone — the Eno/Lanois "shimmer" sound. The shifter reads a grain buffer at
double speed with two crossfaded taps (80 ms grains, sin² windows), is
low-passed at 4.5 kHz so the climbing octaves die away, and soft-limited,
so the loop is never louder than the plain echo.

| slider | field | range (default) |
|---|---|---|
| Time (mod, exp) | `delayTime` | 0.05–2 s (0.35) |
| Feedback (mod) | `delayFb` | 0–0.9 (0.4) — how many repeats |
| Mix (mod) | `delayMix` | 0–1 (0.3) |
| Shimmer (mod) | `delayShimmer` | 0–1 (0) — how much of the feedback comes back an octave up |

## Reverb

An 8-line feedback delay network (mutually prime line lengths of 23–80 ms,
a Householder mixing matrix, a damping low-pass in each line). Longer
decays are darker, as in a real room (the damping cutoff falls from 6 kHz
toward 1.5 kHz), and the wet level is normalized so that a long tail is
not louder than a short one — calibrated against the browser's convolution
reverb that the presets were tuned on.

| slider | field | range (default) |
|---|---|---|
| Decay | `reverbDecay` | 0.1–8 s (2.8) — RT60, the time the tail takes to fall 60 dB. Not modulatable |
| Mix (mod) | `reverbMix` | 0–1 (0.25) |

## Limiter

On by default. A peak limiter with a 3 ms look-ahead and a 20:1 ratio
keeps a loud point from clipping, with make-up gain so that switching it
on does not make the point quieter (the browser's compressor's curve,
copied because the presets were tuned through it).

| slider | field | range (default) |
|---|---|---|
| Threshold (mod) | `limiterThr` | −40–0 dB (−12) |
| Release (mod) | `limiterRel` | 0.02–1 s (0.15) |

## Effect presets

⚙ Settings has an **Effects preset** menu (`assets/fx-presets.json`). A
preset sets only its own module's fields over the current ones; the
formulas, the modulation and the picture are untouched.

| group | presets |
|---|---|
| Filter | Warm low-pass, Underwater, Telephone, Radio, Vowel "ah", Vowel "ooh", Metallic comb, Presence (air), Hollow notch |
| Flanger | Jet flanger, Slow flanger, Subtle flanger |
| Phaser | Deep phaser, Fast phaser, Phaser swirl (+delay) |

## In the search

Every module's switch, the filter type, the chorus mode, the phaser's
stages and every slider are genes (`fx.*`), so 👍/👎 can turn an effect on,
change its type or move its sliders. The master gain is **not** a gene:
the search never changes the volume.
