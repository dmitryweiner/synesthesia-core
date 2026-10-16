# Generators

A point's sound starts with **formula generators**: 23 small synthesizers,
each a formula from maths, physics or the history of computer music. A
point may have **up to five** on at once (`maxEnabledFormulas`); the search
never proposes more. Code: `syn-core/src/dsp/` (`generator.rs`,
`tanpura.rs`, `gate.rs`); sliders and ranges: `formulas` in
`assets/schema.json`.

## What every generator shares

- **`gain`, 0–1.** It is applied twice — inside the generator and again on
  the mix bus — exactly as the web app's audio graph did, so the effective
  level is roughly `gain²`. Every preset was tuned through that.
- **Phase is accumulated**, never computed as `sin(2π·f·t)` from absolute
  time: a frequency that moves (an LFO, a morph) bends the pitch smoothly
  instead of jumping the phase and beating.
- **Parameters are read once per block** (128 samples). LFO routes move
  them at block rate; see [LFOs and modulation](lfo.md).
- **On/off fades** over 5 blocks (~13 ms); a faded-out generator costs
  nothing.
- The noise generators draw from a seeded `mulberry32`, so a render of a
  point is repeatable.
- The generators are bit-exact with the web app's original TypeScript:
  `golden/` holds 69 reference takes and the tests compare them sample by
  sample.

## The catalogue

Below, *f* is a frequency slider, *t* time. Ranges are min–max (default).

### Tones and spectra

| id | title | what it does | sliders |
|---|---|---|---|
| `additive` | Harmonic Sum | Σ aₖ(t)·sin(k·φ): *N* harmonics of a fundamental, harmonic *k* weighted (1/*k*)·sin(2π·move·t + *k*) — a slow wave runs through the harmonics' weights, so the timbre shimmers | `fund` 20–500 Hz (110), `N` 1–40 (12), `move` 0.01–5 Hz (0.35) — how fast the harmonics' weights move |
| `fm` | FM Sine | sin(2π·f꜀·t + I·sin(2π·fₘ·t)) — classic two-operator FM | `fc` 20–2000 Hz (220), `fm` 0.1–60 Hz (2), `I` index 0–20 (3) |
| `pm` | Phase Modulation | sin(2π·f·t + 5·sin(sin(2π·f₂·t))) — a nested, softer phase wobble | `f` 20–2000 Hz (220), `f2pm` 0.1–40 Hz (3) |
| `beats` | Two Sines (Beats) | two sines Δf apart: their sum pulses at Δf | `fbeat` 20–2000 Hz (220), `df` 0–20 Hz (0.8) |
| `dist` | Nonlinear Saturation | tanh(α·sin(2π·f·t)): a sine driven into soft clipping, from pure to nearly square | `fd` 20–2000 Hz (110), `alpha` 0–10 (3) |
| `gliss` | Exponential Glissando | f(t) = f₀·e^{k·t}: an endless slide up (k > 0) or down (k < 0), restarting after four octaves | `f0` 10–400 Hz (55), `k` −2–2 (0.15) |
| `shepard` | Shepard Tone | octaves of one tone under a fixed Gaussian loudness envelope (centred on 440 Hz), all sliding together: the ear hears a pitch that rises (or falls) forever | `shepBase` 20–200 Hz (55), `shepSpeed` −0.5–0.5 octaves/s (0.1), `shepOctaves` 3–10 (6) |

### Chaos

| id | title | what it does | sliders |
|---|---|---|---|
| `lorenz` | Lorenz Attractor | integrates the Lorenz system once per sample; \|x\| sets the pitch around a base, tanh(y) the loudness | `sigma` 0–30 (10), `rho` 0–60 (28), `beta` 0.1–10 (2.667), `lBase` 20–400 Hz (120), `lFreqScale` 0–200 (40), `lAmp` 0–1 (0.25) |
| `rossler` | Rossler Attractor | the Rössler system (dx = −y−z, dy = x+ay, dz = b+z(x−c)), run 100× faster; x is pitch, y loudness | `rossA` 0.01–0.5 (0.2), `rossB` 0.01–0.5 (0.2), `rossC` 2–12 (5.7), `rossBase` 20–400 Hz (120), `rossFreqScale` 0–100 (30), `rossAmp` 0–1 (0.25) |
| `logistic` | Logistic Map | xₙ₊₁ = r·xₙ(1−xₙ), stepped `lfoHz` times a second; x sets the pitch. Below r ≈ 3.57 it settles into cycles, above it is chaotic | `base` 20–800 Hz (110), `depth` 0–1200 Hz (330), `r` 2.8–4 (3.86), `lfoHz` 1–400 (40) |
| `quasi` | Quasi-random LFO | f(t) = f_q + A_q·sin(sin(sin(w·t))): a pitch that wanders in a pattern that almost repeats | `fq` 20–500 Hz (120), `Aq` 0–1200 Hz (220), `wq` 0.05–6 (0.8) |

### Bells and plucked strings

| id | title | what it does | sliders |
|---|---|---|---|
| `bell` | FM Bell | e^{−3t/d}·sin(2π·f·t + I·e^{−3t/d}·sin(2π·ratio·f·t)): Chowning's FM bell — the modulation index decays with the loudness, so the strike is bright and the tail pure. Re-struck every *period* | `bellF0` 100–1000 Hz (320), `bellRatio` 1–3 (1.4), `bellIndex` 0–10 (4), `bellDecay` 0.5–8 s (3), `bellPeriod` 1–20 s (6) |
| `risset` | Risset Bell | Jean-Claude Risset's additive bell (1969): 11 inharmonic partials, each with its own decay and a slight detune, so the bell beats as it fades | `rissF0` 100–1200 Hz (480), `rissDecay` 0.5–12 (5), `rissPeriod` 1–20 s (6) |
| `karplus` | Karplus–Strong (String) | a burst of noise circulating in a delay line one period long, averaged and damped each pass: a plucked string | `ksFreq` 40–880 Hz (110), `ksDamp` 0.9–0.9999 (0.985) — sustain, `ksBright` 0–1 (0.5) |

### Drones

| id | title | what it does | sliders |
|---|---|---|---|
| `tanpura` | Tanpura | four Karplus–Strong strings tuned Pa–Sa–Sa–Sa (3/2, 2, 2, 1 × the low Sa), plucked in turn by a "finger" (low-passed noise). The two middle Sa are ±1.5 cents apart and beat. **Jawari**: each string adds a smoothed contact pulse when it swings past 60 % of its amplitude — the buzz of the curved bridge, blooming after each pluck | `tanSa` 30–220 Hz (55), `tanCycle` 2–40 s (5) — one round of plucks, `tanJawari` 0–1 (0.5), `tanSustain` 2–30 s (16), `tanBright` 0–1 (0.45) |
| `bowl` | Singing bowl | a rubbed bowl's hum: four inharmonic modes (1 : 2.71 : 5.12 : 8.21), each a **pair** of tones a fraction of a hertz apart (a bowl is never perfectly round), so every partial swells and ebbs; higher modes beat faster (∝ √ratio) | `bowlF` 40–800 Hz (130), `bowlBeat` 0.05–3 Hz (0.4), `bowlBright` 0–1 (0.4) — weight of the upper modes |

### Noise and nature

| id | title | what it does | sliders |
|---|---|---|---|
| `noiselp` | Noise → Low-pass | white noise through a one-pole low-pass | `nCut` 20–18000 Hz (800) |
| `pinknoise` | Pink Noise (1/f) | Paul Kellet's pink filter (−3 dB/octave), blendable with white | `pinkBright` 0–1 (0.3) |
| `brownnoise` | Brown Noise | a bounded random walk (−6 dB/octave): a deep rumble | `brownStep` 0.001–0.1 (0.02) |
| `velvetnoise` | Velvet Noise | sparse ±1 impulses at random intervals: from crackle to a smooth hiss | `velvetDensity` 100–10000 /s (2000) |
| `ocean` | Ocean / Wind | white noise through a low-pass whose cutoff and level breathe with two slow, incommensurate swells | `oceanRate` 0.03–0.5 Hz (0.12), `oceanCut` 100–2000 Hz (600), `oceanDepth` 0–1 (0.7) |
| `rain` | Rain / Drops | sparse resonant drops — each a decaying sine whose pitch glides up 50 %, the way a water drop's bubble does — over a soft low-passed noise bed | `rainDensity` 0.5–20 drops/s (4), `rainPitch` 200–3000 Hz (900), `rainBed` 0–1 (0.15) |

### 8-bit

| id | title | what it does | sliders |
|---|---|---|---|
| `bytebeat` | Bytebeat | a one-line integer formula of a counter *t*, its low byte played as a sample — five classic recipes, e.g. `((t>>10)&42)·t` | `bbRecipe` 1–5 (1), `bbRate` 1000–16000 Hz (8000) — how fast *t* counts |

## Adding a generator

A new generator is made here first, with its schema entry, its sliders'
genes and mod targets **appended** (gene order is a compatibility
contract), and a golden take. The checklist and the measuring tools are
the `new-preset` and `sound-check` skills in `.claude/skills/`.
