# LFOs and modulation

A point has **four LFOs** (low-frequency oscillators) and up to **twelve
routes**. Each route lets one LFO move one parameter — of a generator, of
the effects, of a layer, or of the picture — around the value the point
gives it. Code: `syn-core/src/modmatrix.rs`; the list of what can be
modulated: `modTargets` in `assets/schema.json`.

## An LFO

```json
{ "shape": "pink", "rate": 0.05, "phase": 0.25 }
```

| field | range | meaning |
|---|---|---|
| `shape` | see below | the waveform |
| `rate` | 0.003–2 Hz (gene range; default 0.05) | cycles per second — from one cycle in about five and a half minutes to two a second |
| `phase` | 0–1 | where in its cycle the LFO starts, as a fraction of a cycle |

An LFO's value is a **pure function of the engine's clock**:
`value = shape(rate · t + phase)`, in [−1, 1]. Nothing is stored or
stepped, so the audio thread, the picture and an offline render all read
the same value at the same moment without talking to each other, and an
LFO keeps its phase across a stop.

### Shapes

| shape | what it does |
|---|---|
| `sine` | a sine |
| `triangle` | linear up and down, starting at −1 |
| `saw` | rises from −1 to 1, then drops |
| `square` | +1 for the first half of the cycle, −1 for the second |
| `random` | sample & hold: a new random level each cycle, held flat. The level is a hash of the cycle number, so it is the same in every render |
| `pink` | 1/f fluctuation: five octaves of smooth value noise summed and soft-limited with `tanh`. It glides like a sine but never repeats; its spectrum falls 3 dB per octave from about rate/2 to 4·rate. Like `random`, a pure function of the phase |

`pink` is the closest to how natural things drift (wind, a voice's pitch,
the tempo of a human player) and suits slow timbre movement; `random` gives
stepped, surprising changes; the periodic shapes give a regular sweep.

## A route

```json
{ "src": 2, "target": "fx", "param": "filterFreq", "depth": 0.3, "exp": true }
```

| field | meaning |
|---|---|
| `src` | which LFO, 0–3 |
| `target` | `"fx"`, a formula id (`"additive"`), a picture card id (`"palette"`), `"seq"` or `"seq.<i>"` for a layer |
| `param` | the parameter's key: a slider key or an FX field |
| `depth` | −1 to 1: how far the LFO moves the parameter, as a fraction of the parameter's whole range. Negative inverts the LFO |
| `exp` | moves the parameter in octaves rather than linearly; set for frequency-like parameters |

### The mapping

With the point's value `base`, the LFO's value `l`, the parameter's range
`[min, max]`:

- **linear**: `base + depth · (max − min) · l`, clamped to the range;
- **exponential** (`exp`): `base · 2^(depth · log₂(max/min) · l)`, clamped —
  so at depth 0.5 and `l = ±1` a cutoff goes the same number of octaves up
  as down.

A parameter with several routes gets them one after another. A route to a
generator or effect module that is off does nothing; a route aimed at
a parameter or LFO that does not exist is skipped.

### When the value is applied

- **Generators**: once per audio block (128 samples, ~2.7 ms at 48 kHz) —
  the parameter is constant inside a block.
- **Effects**: once per block too, and then **smoothed** by the chain
  (30 ms), so even a square LFO on the filter cutoff does not zipper.
- **Layers**: once per block (`cps`, `gain`, `speed`, `density`).
- **Picture**: once per animation frame, on the card parameters, before the
  sound → image couplings.

## What can be modulated

175 targets in all; the full list with labels is `modTargets`.

| target | parameters |
|---|---|
| each generator | its sliders, including `gain` and `az` — where it sits around the head (see [Generators](generators.md)) |
| `fx` | filter cutoff (exp), Q, gain, vowel, comb feedback; chorus rate (exp), depth, mix, feedback; delay time (exp), feedback, mix, shimmer; phaser rate (exp), depth, feedback, mix; reverb mix; limiter threshold and release. Not the on/off switches, the filter type, the chorus mode, the phaser's stage count or the reverb's decay |
| `reaction` | feed, kill, diffU, diffV, speed |
| `fieldVariation` | feed/kill amount, scale (exp), warp |
| `flow` | curl strength, curl scale (exp), drift X/Y, advect amount, evolve rate |
| `palette` | shift, contrast, bands, relief, light angle, gloss |
| `seq` | `cps` (exp) — the tempo |
| `seq.0` … `seq.3` | `gain`, `speed` (exp), `density`, `az` |
| `bin` | `beat` (exp), `width` — see [Space and binaural beats](binaural.md) |

## In the search

The LFOs and routes are genes: an LFO's shape, rate and phase
(`lfo.<i>.*`), and per route slot whether it is used, its source, its
target (an index into `modTargets`), its depth and `exp` (`route.<i>.*`).
👍/👎 mutate them like any other gene, so the search can find "the cutoff
breathes slowly" as readily as a new pitch. In ⚙ Settings they are the
*LFO 1–4* and *Route 1–12* sections; a new route starts at depth 0.2.
