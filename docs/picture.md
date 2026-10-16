# The picture

The image is a **reaction-diffusion** simulation — two chemicals spreading
and reacting on a grid — coloured like stone or glaze and driven by the
sound. Code: `syn-core/src/sim/` (the CPU picture, and `driver.rs`, which
decides what each frame does for every renderer), the GLSL passes in
[`shaders/`](../shaders/README.md) that the web and Android apps run on the
GPU; the cards and sliders: `cards` in `assets/schema.json`.

The picture's settings are four **cards**. *Reaction* and *Palette* are
always on; *Field variation* and *Flow* can be switched off.

## Each frame

```text
features ─► flash guard ─► couplings ─┬─► card params ─► paramfield + velocity (when their inputs moved)
                                      │                 ─► reaction substeps × speed
                                      │                 ─► one advection step
                                      ├─► onset hits ─► fresh growth + a ripple
                                      └─► display: palette, relief, exposure, flash, tint
```

LFO routes move the card parameters first, then the sound → image
couplings are laid over them (see below).

## Reaction — Gray–Scott

Two fields *u* and *v* on the grid:

```text
∂u/∂t = Du·∇²u − u·v² + f·(1 − u)
∂v/∂t = Dv·∇²v + u·v² − (f + k)·v
```

*u* is fed in at rate *f* (feed), *v* is removed at rate *k* (kill), and
where they meet *v* grows on *u*. Depending on *f* and *k* the pattern is
spots, stripes, labyrinths, splitting cells, waves or nothing — small
moves of the two sliders change the family.

| slider | key | range (default) |
|---|---|---|
| Feed | `feed` | 0.01–0.09 (0.037) |
| Kill | `kill` | 0.03–0.075 (0.06) |
| Diff U | `diffU` | 0.05–0.4 (0.2097) |
| Diff V | `diffV` | 0.02–0.2 (0.105) |
| Speed | `speed` | 1–40 (10) — reaction substeps per frame: how fast the pattern grows |

## Field variation — domain warp

Feed and kill are not the same everywhere: two layers of domain-warped
fractal noise (fbm) offset them across the screen, so different regions
land in different pattern families at the same time. The noise slowly
evolves (Flow's *evolve rate*).

| slider | key | range (default) |
|---|---|---|
| Feed amount / Kill amount | `feedVarAmount`, `killVarAmount` | 0–0.03 (0.015), 0–0.015 (0.008) — how far the offset reaches |
| Feed scale / Kill scale | `feedVarScale`, `killVarScale` | 1–12 (4, 5) — how many regions across the screen |
| Feed warp / Kill warp | `feedVarWarp`, `killVarWarp` | 0–3 (1) — how twisted their borders are |

## Flow — advection

The field is carried along a velocity: curl noise (swirls that neither
compress nor tear the pattern) plus a constant drift ("gravity"), which
gives folded layers and drips. Semi-Lagrangian: each cell takes the state
from where it came from.

| slider | key | range (default) |
|---|---|---|
| Curl strength | `curlStrength` | 0–0.03 (0.01) |
| Curl scale | `curlScale` | 1–12 (3) — size of the swirls |
| Drift X / Drift Y (+down) | `driftX`, `driftY` | −0.02–0.02 (0) |
| Advect amount | `advectAmount` | 0–1 (0.35) |
| Evolve rate | `evolveRate` | 0–0.05 (0.01) — how fast the noise fields change |

## Palette — colour and relief

The height of *v* is mapped through an Inigo Quilez cosine gradient
(`a + b·cos(2π(c·t + d))`), and a bump-mapped relief lights it as if the
pattern were carved stone or a glaze.

| control | key | range (default) |
|---|---|---|
| Palette | `paletteId` | Marble, Glaze, Verdigris, Ink, Basalt |
| Shift | `shift` | 0–1 (0) — rotates the hues |
| Contrast | `contrast` | 0.2–2 (1) |
| Bands | `bands` | 1–8 (1) — repeats the gradient into contour bands |
| Relief | `relief` | 0–3 (0.8) — depth of the bump map |
| Light angle | `lightAngle` | 0–2π (2) |
| Gloss | `gloss` | 0–1 (0.3) — specular sheen |

## How the sound drives it

The engine publishes, 60 times a second, what it hears
(`syn-core/src/features.rs`):

| feature | meaning |
|---|---|
| loudness | 0–1, smoothed RMS |
| swell | −1–1, loudness against its ~4 s average (0 = steady) |
| brightness | 0–1, spectral centroid on a log-frequency scale |
| low / mid / high | 0–1: 30–250 Hz, 250–2000 Hz, 2–10 kHz |
| onset | 0–1, a decaying spike when the spectrum's energy jumps |
| onset hits | a count of detected attacks (a bell strike, a drum, a pluck) |

Nine **coupling** genes say how much of each reaches the picture
(`coupling` in the point; `sim/coupling.rs`):

| coupling | range (default) | what it does |
|---|---|---|
| `loudToFlow` | −1–1 (0) | loudness adds to Flow's advect amount |
| `loudToCurl` | −1–1 (0) | loudness adds to the curl strength |
| `loudToGloss` | −1–1 (0) | loudness adds to the gloss |
| `brightToShift` | −1–1 (0) | brightness turns the hues (wrapping round) |
| `onsetToLight` | −1–1 (0) | an onset turns the light angle (wrapping) |
| `loudToPulse` | 0–1 (0.5) | the swell pulses the exposure, up to ±60 % |
| `onsetToFlash` | 0–1 (0.5) | an onset flares the highlights |
| `onsetToSeed` | 0–1 (0.4) | each onset hit plants fresh growth at a random spot, with a ripple spreading from it (up to 4 ripples alive, 1.6 s each) |
| `spectrumToTint` | 0–1 (0.4) | bass, mids and treble tint the dark, middle and light tones |

The first five offset card parameters and show up with the simulation's
lag; the last four act on the frame directly. The search keeps the last
four summing to at least **1.2** (`couplingFloor`), so a point never
drifts into a picture that ignores its sound.

A touch or a drag on the screen does what an onset hit does — plants
growth — along the stroke.

## The flash guard

**The picture must not strobe** (photosensitivity; WCAG 2.3.1 allows at
most three flashes in any second). A drum layer hits several times a
second, and drawn straight every hit was a flash of the whole screen. So
the features are slewed before the picture reads them (`sim/calm.rs`): an
onset flare rises over ~150 ms, falls over ~0.6 s and is capped at 0.6;
swell, bands, loudness and brightness glide over a fraction of a second.
A run of hits becomes a glow that breathes with the rhythm. The sound is
untouched. `syn-bench --flashes` counts flashes per second the WCAG way;
every preset stays at or below one.

## Quality

The grid's size and the drawn surface are chosen by measuring the device,
not by guessing from its screen (`sim/quality.rs`): at boot a probe walks
**up** a ladder of rungs from the cheapest, each about twice the work of
the one below, and keeps the best one that renders at 20 fps. After that
the rung never moves, so the picture cannot degrade mid-listen. Every renderer — the
GPU passes on the web and Android, the CPU picture on the console — is
driven by the same per-frame decisions (`sim/driver.rs`), so they show the
same picture.
