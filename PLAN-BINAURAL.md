# Binaural sound — plan

Two things the user asked for on 2026-10-15, under one word:

1. **Binaural beats.** The left and right ears hear the same sound a few
   hertz apart (Δf = 0.5–40 Hz, the delta … gamma bands), and the brain
   hears a beat that is in neither ear. Headphones only: on speakers the two
   channels mix in the air into an ordinary amplitude beat.
2. **Space.** The point's sources sit around the head and drift slowly:
   interaural time and level differences, a head-shadow filter, a reverb
   whose tail differs between the ears — the "wide" headphone sound of
   ambient music, with a direction to each voice.

Both are properties of the point (decision 2 below): genes, presets, 👍/👎.
This file is the record: what was looked at, the decisions and why, the
data shapes, the phases, the budget and the measurement each phase owes.
Numbers go in as they are measured; `TODO.md` carries the open items.

The model is mono today, end to end: `Engine::render` fills one buffer,
`syn-player` hands out mono, the worklet copies one buffer to every output
channel, the FFI returns mono f32, the console pipes `--channels 1`. The
first job is therefore stereo, in a way that breaks no host before it is
ready — and that keeps every frozen fixture exactly where it is.

## What was looked at (2026-10-15)

### Binaural beats on a sound that already exists

The textbook binaural beat is two sine carriers, f and f + Δ. Here the sound
is a point — up to five formula generators through a chain of effects — and
the user wants *that* in the two ears, Δ apart. Options:

| option | verdict | why |
|---|---|---|
| render every generator twice, the right ear's frequency sliders scaled by Δ | **no** | doubles the generators' cost (they are the floor: 2.4 µs a sample on *Fractal garden*), and Δ is a ratio per slider rather than a shift in hertz, so partials beat at different rates |
| a `binaural` generator: sine f left, f + Δ right | later, maybe | the textbook effect, but not "the point's sound in two ears"; cheap to add as a 24th formula once stereo exists |
| **a frequency shifter on the mix: single-sideband, −Δ/2 on the left, +Δ/2 on the right** | **yes — decision 3** | every partial moves by the same Δ, so the whole sound beats at Δ; one Hilbert pair (two 4-section all-pass chains, Niemitalo's design, ±0.05° over 20 Hz–20 kHz) and one quadrature oscillator per channel: ~16 first-order sections a sample in all; the mono downmix is unchanged because the mono path never runs it |

### Putting sources around the head

| option | verdict | why |
|---|---|---|
| measured HRTFs by convolution — [SADIE II](https://www.york.ac.uk/sadie-project/database.html) (Apache-2.0, University of York; 44.1/48/96 kHz WAV and SOFA) | later, behind the same interface | the best cue set (elevation, externalization), but two 128–256-tap FIRs per source: ~2.5 µs a sample for five sources, more than the whole point today; partitioned FFT convolution would fix the cost and add latency and code. MIT KEMAR has no stated licence; CIPIC/ARI are non-commercial |
| **a structural head model (Brown & Duda 1998)**: Woodworth's ray-traced ITD, τ(θ) = (a/c)(θ + sin θ), a = 8.75 cm (≤ 0.65 ms, 31 samples at 48 kHz), and a one-pole one-zero head-shadow filter per ear, H(ω,θ) = (1 + jαω/2ω₀)/(1 + jω/2ω₀), ω₀ = c/a, α(θ) = 1.05 + 0.95·cos(θ − θ_ear) | **yes — decision 4** | no dataset, no licence, a fractional delay and a first-order filter per ear per source (~10 ns a sample each), parameters that are genes (an azimuth is one number), and the known limits: front/back and elevation are not resolved — a drone that drifts from left to right does not need them |
| stereo width tricks only (mid/side, Haas, decorrelated reverb) | **as part of 4, not instead** | the stereo FDN and the 90°-offset chorus/phaser LFOs come for free with the stereo chain; width is the `width` gene scaling every azimuth |

## Decisions (agreed with the user, 2026-10-15)

1. **Stereo arrives beside mono, not instead of it.** `Engine` gets a
   stereo mode (`Engine::new_stereo`, `render_stereo(l, r)`); the mono
   `render` keeps today's path *bit for bit* — one chain, no spatializer, no
   shifter — so `golden/`, `fixtures/`, the scout's scores and the mono
   hosts do not move. `syn-player` gets `render_stereo_into` (interleaved),
   `syn-wasm` `renderStereo`, `syn-ffi` `render_stereo`; the mono calls
   stay. A host switches when it bumps: the worklet's node to two output
   channels, Android's `AudioTrack` to stereo, the console's sink to
   `--channels 2`. An engine is built in one mode and never computes both:
   no double cost.

2. **Binaural parameters live in the point, as genes.** The user's choice:
   a point is wide or narrow, beating or still, the way it is bright or
   dark — the search finds it, a preset states it. Appended after whatever
   the gene list holds when this lands (250 today; PLAN-SAMPLES-SCRIPTS.md
   appends 29 of its own — whichever lands first appends first; neither
   moves the other's), neutral values that make a point without them sound
   as before, optional fields with `serde(default)` and skipped when
   neutral, so canonical JSON, point ids and `fixtures/points.json` are
   untouched. The Settings page, generated from the gene list, shows them
   in every app at once.

3. **Beats by single-sideband shift of the whole mix, ±Δ/2.** After the
   reverb and before the limiter (so the limiter still sees what goes
   out), left −Δ/2 and right +Δ/2: the sum of the ears is centred on the
   original pitch, and Δ is the beat whatever the partial. `bin.beat` in
   hertz (0.5–40, log), `bin.on` to gate it. The quadrature oscillator is a
   rotating phasor with periodic renormalization — no `sin` per sample
   (the wasm rule); the Hilbert pair is eight first-order all-passes per
   channel with equal group delay on both arms, so left and right stay
   aligned.

4. **Space by the structural head model, per source.** Every formula slot
   gets an azimuth gene `a.<id>.az` (−90° … +90°, 0 = centre, gated by its
   `enabled` like its sliders), and the point gets `bin.width` (0–1) that
   scales every azimuth — 0 is today's mono image. Each enabled source is
   rendered to left and right through its own fractional delay (ITD) and
   head-shadow filter (ILD), block-rate parameters from `az · width`, the
   filters' `cos` and `sin` taken once per block. Azimuths are LFO targets,
   so a voice can wander. Elevation, distance and measured HRTFs are
   phase B6, behind the same `Spatializer` trait, when a listening pass
   asks for them.

5. **The chain goes stereo as two chains and one reverb.** Filter, comb,
   formants, chorus, phaser and delay run as a left and a right instance
   with identical parameters; the chorus and phaser LFOs run 90° apart
   between the ears (what a stereo chorus is); the FDN stays one reverb
   with two outputs (even lines left, odd lines right: decorrelated tails
   at no extra lines); the limiter is one gain for both ears (stereo-linked,
   so the image does not lurch). At `width = 0`, `bin.on = false`, the
   stereo downmix must equal the mono render to float precision — the test
   that makes the stereo engine a strict superset.

6. **The analysis stays mono.** Features, onsets, the spectrum, the
   picture's couplings and the scout's score are computed on (L+R)/2 in
   stereo mode and on the mono path offline — one definition of loudness
   and brightness for every host, and a scout that scores what it scores
   today. Whether a wide point and a narrow one should score differently is
   a question for the listening pass, not for the metric.

7. **Headphones are the host's business, not the point's.** The point says
   how wide and how fast it beats; the app says whether headphones are on.
   A host may offer "speakers" (render mono) and "headphones" (render
   stereo) as its setting — `syn-session` does not need to know. The
   console's `config.toml` gets `channels = 2`.

8. **Measure first.** B0 prices the stereo chain before it is written: the
   cost of two chains against one, of the head model per source, of the
   shifter — native and wasm, against the phones' 35 % quantum gate
   (synesthesia PLAN-CORE.md phase 0) — and the one number that can change
   the design: whether the second chain fits on the phone at all.

## The model

### The point

```json
"audio": {
  "masterGain": 0.75,
  "fx": { … },
  "formulas": {
    "additive": { "enabled": true, "params": { "gain": 0.45, "fund": 55, "N": 24, "move": 0.25, "az": -35 } },
    "fm":       { "enabled": true, "params": { "gain": 0.15, "fc": 880, "fm": 55, "I": 5, "az": 40 } }
  }
},
"bin": { "on": true, "beat": 6.0, "width": 0.8 }
```

- `az` is one more slider of a formula (schema: `a.<id>.az`, −90 … 90,
  step 1, default 0): it lives where the formula's other parameters live,
  so `FormulaSnapshot` and `sanitize` need no new shape, and a point from
  before simply lacks the key — the generator pool fills the default, as
  it does for every slider. The generators never read it; the engine does.
- `bin` is a new optional section: `on`, `beat` (Hz), `width`. Absent, or
  `{on:false, width:0}`, it serializes to nothing.
- Neutral values for the morph: `az` 0, `width` 0, `beat` at its minimum
  with `on` off — a morph fades the space in and out instead of jumping.

### The signal path in stereo mode

```
formula slots ──gate·gain──► spatializer (per source: ITD + ILD → L, R)
                                 │ L                        │ R
                             chain L  ───────┐   ┌───────  chain R      filter · chorus · phaser · delay
                                             └─► FDN ◄─┘               one reverb, two outputs
                                   L ◄──────────┘   └──────────► R
                             SSB −Δ/2                          SSB +Δ/2    bin.on · bin.beat
                                   └──────► limiter (linked) ◄──────┘
                                   L ─► master ─► out         R ─► master ─► out
                                               (L+R)/2 ─► analyser → features, onsets, spectrum
```

In mono mode the engine is exactly today's: slots → one chain → master →
analyser. The sample layers of PLAN-SAMPLES-SCRIPTS.md, when they land, are
sources like the slots (each layer gets an `az` too) and join the
spatializer before the chain; their `dry` flag skips the chain, not the
space.

### The head model

- **ITD**: τ(θ) = (a/c)(θ + sin θ) for |θ| ≤ 90°, a = 0.0875 m, c = 343 m/s.
  The near ear gets no delay, the far ear τ; a fractional delay line of
  48 samples per source per ear, linear interpolation, the delay glided at
  block rate so a moving source does not click.
- **ILD**: per ear, H(z) from the bilinear transform of
  (1 + jαω/2ω₀)/(1 + jω/2ω₀) with ω₀ = c/a ≈ 3.9 kHz and
  α = 1.05 + 0.95·cos(θ − θ_ear), θ_ear = ±100°: a shelf that lifts the
  high end on the near side and shadows it on the far side.
- Both take `θ = az · width` of the source at block rate; `cos`/`sin` once
  per block per source, nothing transcendental per sample.

### The shifter

- Hilbert pair: two cascades of four first-order all-pass sections whose
  outputs are 90° apart (Niemitalo's coefficients, verified against a swept
  sine in the test); I from one arm, Q from the other.
- Shift: y = I·cos φ − Q·sin φ, φ advanced by 2π·(±Δ/2)/sr per sample as a
  complex phasor (two multiplies and adds), renormalized every block.
- The mono downmix of a shifted pair has a tremolo at Δ (a monaural beat);
  that is why the mono `render` never shifts (decision 1) and the analysis
  reads the pre-shift sum (decision 6).

### Genes and schema

| gene | kind | range | gated by | neutral |
|---|---|---|---|---|
| `a.<id>.az` (23 formulas) | cont | −90 … 90 | `a.<id>.enabled` | 0 |
| `bin.on` | bool | | — | 0 |
| `bin.beat` | cont, exp | 0.5 … 40 Hz | `bin.on` | min |
| `bin.width` | cont | 0 … 1 | — | 0 |

26 genes appended; mod targets `a.<id>.az` (23), `bin.beat`, `bin.width`:
25 appended. `evolve.rs`: one structural move toggles `bin.on`; the
continuous genes move with the rest. `random_genome` gives `width`
0.3–0.8 and spreads azimuths, so a 🎲 point is not flat by accident.
`settings_page.rs`: a "Binaural" section for `bin.*`; `az` appears in each
formula's section as its slider (no new rule).

## Architecture — where the code goes

```
syn-core/src/fx/hilbert.rs     the all-pass pair and the SSB shifter
syn-core/src/fx/stereo.rs      StereoChain: two Chains, the FDN's two outputs, the shifter, the linked limiter
syn-core/src/fx/reverb.rs      Fdn::tick_stereo → (l, r)
syn-core/src/fx/limiter.rs     a linked stereo gain
syn-core/src/spatial.rs        Spatializer trait; HeadModel (ITD + ILD per source)
syn-core/src/engine.rs         Mode::{Mono, Stereo}; render (unchanged) and render_stereo
syn-core/src/state.rs          BinState (serde default, skipped when neutral)
syn-core/src/point.rs          sanitize for bin; az clamps like any slider
syn-core/src/schema.rs         az sliders, bin genes and mod targets
syn-core/src/genome/           codec, evolve (the toggle), random_genome
syn-core/src/settings_page.rs  the Binaural section
syn-player/src/lib.rs          Player::new_stereo, render_stereo_into (interleaved)
syn-wasm/src/lib.rs            AudioCore::createStereo / renderStereo (one buffer, L R interleaved)
syn-ffi/src/lib.rs             SoundPlayer::new_stereo / render_stereo
syn-bench                      --stereo: cost of the modes, ITD/ILD check, the shifter's spectrum, a stereo WAV
assets/schema.json, presets.json, settings-page.json
```

## Phases

Each phase ends green on `./scripts/check.sh` and with its numbers in this
file.

- **B0 — probes (throwaway, `syn-bench --stereo`)**: the heaviest preset in
  mono and with a second chain instance; the head model for one and five
  sources; the shifter — ns a sample each, native and in wasm (`node
  --cpu-prof`), against PLAN-CORE's phone gate. The number decides whether
  the chain is duplicated (decision 5) or only the chorus/phaser/delay are,
  with the filter before the spatializer.
- **B1 — stereo in the engine, nothing audible yet**: `Mode`,
  `render_stereo`, `StereoChain` with two chains and the FDN's two outputs,
  the linked limiter, the analyser on the downmix; `render` untouched. The
  test: the downmix equals `render` to 1e-6 for every preset, and the
  golden and fixture tests pass unchanged.
- **B2 — space**: `HeadModel`, the `az` sliders and `bin.width`, the
  codec, sanitize, Settings; tests: an impulse at +90° reaches the right
  ear first by Woodworth's τ and louder above 2 kHz; az 0 gives L = R; a
  source swept by an LFO never clicks (the continuity test's method).
- **B3 — beats**: the Hilbert pair (a swept sine shows 90° ± 0.1° from
  20 Hz to 20 kHz), the shifter (a 440 Hz sine comes out at 440 ∓ Δ/2 in
  each ear, measured by FFT), `bin.on`/`bin.beat`, their genes and targets.
- **B4 — hosts**: `render_stereo_into`, `renderStereo`, `render_stereo`;
  then each app bumps on its own schedule: the console first (`channels =
  2`, `pw-cat --channels 2`, the `verify` ladder: `bench` ≥ 8× realtime in
  stereo, `soak.py` 0 xruns in 30 min), the worklet (`outputChannelCount:
  [2]`, one interleaved view, de-interleaved into `outputs[0][0..2]`
  without allocating), Android.
- **B5 — presets and the listening pass** (`new-preset`, `sound-check`):
  the fifteen presets get azimuths and a width by ear on headphones (the
  liked family — *Fractal garden*, *Molten Polivoks*, *Silver maze*, *Space
  breccia*, *Loom & copper* — first), one or two with a beat (a theta
  preset over *Tanpura halo*, an alpha one over *Stillness ink*); the
  stereo WAVs handed to the user; the fixture tests still pass because
  neutral points serialize as before.
- **B6 — by need**: elevation and distance (a second slider pair, or a
  near/far gain + low-pass), measured HRTFs (SADIE II as host-supplied
  bytes, like the sample bank), a `binaural` sine generator, per-ear
  reverb pre-delay. Each a decision when a listening pass asks.

## Budget (thresholds; checked by the bench, filled in as measured)

| metric | target | measured |
|---|---|---|
| stereo mode, heaviest preset | ≤ 1.6× its mono cost native; inside the phones' 35 % quantum gate in wasm | B0 / B1 |
| head model, per source | ≤ 30 ns a sample (both ears) | B0 |
| the shifter, both ears | ≤ 60 ns a sample | B0 |
| downmix vs mono render, width 0, beat off | ≤ 1e-6 on every preset | B1 (test) |
| ITD at ±90° | 0.65 ms ± 1 sample | B2 (test) |
| Hilbert phase error | ≤ 0.1° over 20 Hz – 20 kHz at 48 kHz; ≤ 0.2° at 44.1 and 22.05 kHz | B3 (test) |
| shift accuracy | each ear's peak within 0.05 Hz of f ∓ Δ/2 | B3 (test) |
| allocations in `render_stereo` | 0 | B1 (test) |
| genes | +26, the earlier ones unchanged; `fixtures/points.json` unchanged | B2/B3 (tests) |
| console, stereo live | ≥ 8× realtime offline, 0 xruns in 30 min | B4 |

## Risks and open questions

- **The phone budget.** A second chain is the one cost that may not fit:
  B0 measures it in wasm before B1 is written, and the fallback (filter
  before the spatializer, only the modulated effects doubled) is designed
  in.
- **Beats and harmony.** A shift by Δ makes every partial inharmonic by Δ:
  at 6 Hz on a 55 Hz drone that is the effect wanted; at 40 Hz on a bell
  it is a ring modulator. The range is a gene, the presets set taste, and
  the listening pass may narrow the range.
- **Front/back and elevation are not resolved** by the head model; sources
  at ±90° sound to the side, never behind. Known limit of decision 4; B6
  is the path out.
- **Mono hosts hear nothing new** until they bump, and a point with a
  beat sounds in mono exactly as without one — which is the right
  behaviour on speakers, and a surprise only if someone expects the beat
  from a phone's speaker.
- **Two plans append genes.** This one and PLAN-SAMPLES-SCRIPTS.md both
  extend the gene list; the order is whichever lands first, and each
  plan's tests pin only "the earlier genes are unchanged". The sample
  layers' azimuths belong to this plan's spatializer and that plan's
  section (`seq.<i>.az`), added by whichever lands second.
- **The scout is blind to space** (decision 6). If 👍 consistently prefers
  wide points, the search cannot tell — a listening-pass finding, and the
  metric is the place to fix it, with a number.

## Don'ts (in addition to AGENTS.md)

- Never let the mono `render` run the spatializer or the shifter: the
  fixtures and the scout read it.
- Never compute both modes in one engine.
- Never take a `sin`/`cos` per sample in the head model or the shifter;
  block-rate parameters and a phasor recurrence.
- Never let a neutral `bin` section or an `az` of 0 change a point's
  canonical JSON.
- Never un-link the limiter between the ears.
