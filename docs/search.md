# Points, search and presets

Everything Synesthesia plays and draws is one **point**: a JSON object
(`AppState`, `syn-core/src/state.rs`). The listener does not edit it
directly — they press 👍 or 👎 and the search moves through the space of
points. Code: `syn-core/src/genome/` (genes, codec, evolution, explorer,
scout), `syn-core/src/analysis/fractal.rs` (the score),
`syn-session/` (what happens between a press and the sound).

## The point

```text
AppState v1
├── audio
│   ├── masterGain           0.75 by default; never touched by the search
│   ├── fx                   the FX chain's fields          → effects.md
│   └── formulas             23 × { enabled, params }       → generators.md
├── visual.cards             reaction, fieldVariation, flow, palette → picture.md
├── mod                      4 LFOs and the routes          → lfo.md
├── coupling                 9 sound → image amounts        → picture.md
├── seq                      tempo and 4 layers (omitted when none is on) → layers.md
└── preset_name              optional
```

The shape is shared with every app and every point file, link and stored
point: **it is never changed**, only extended with optional fields that
are omitted when unused.

## The genome

For the search, a point is a vector of **283 genes**, each normalized to
[0, 1] (`genes` in `assets/schema.json`, `genome/codec.rs`):

| group | genes |
|---|---|
| `a.<formula>.*` | each generator's on/off and sliders |
| `fx.*` | module switches, filter type, chorus mode, phaser stages, every FX slider |
| `v.<card>.*` | the picture's cards (Field variation and Flow have an on/off gene) |
| `lfo.<i>.*` | shape, rate, phase |
| `route.<i>.*` | 12 route slots: on, source, target, depth, exp |
| `c.*` | the nine couplings |
| `seq.*` | tempo and per layer: on, sound, rhythm, gain, speed, density, dry, bed |

Genes are continuous (`cont`, some on a log scale), booleans or choices. A
gene can be **gated**: a generator's sliders matter only while it is on. The
order is a compatibility contract — new genes are only ever appended (the
first 250 are the web app's original genome; the layers' 33 came after).
What is not a gene — the name, the master gain, a hand-written pattern's
text — is carried over from the point being listened to.

## 👍 👎 🎲 ↩

The **explorer** (`genome/explorer.rs`) keeps a *current* point, an
*anchor* (the last point the listener endorsed), a *spread* σ and a short
undo history.

- **👍 like** — "more of this". The current point becomes the anchor; the
  next point continues along the step that led here (momentum, weight 0.6)
  with a smaller spread (σ × 0.8, down to 0.04). Structural changes are
  rare (12 %).
- **👎 dislike** — "not this". Go back to the anchor and step somewhere
  else, avoiding the genes that just changed, with a larger spread
  (σ × 1.35, up to 0.35). Structural changes are likelier (30 %).
- **🎲 surprise** — jump near a random built-in preset and restart the
  search there.
- **↩ undo** — back to the previous point (five steps deep).
- **Edit** — a point changed by hand in ⚙ Settings is one undoable step and
  becomes the new anchor.

A step (`genome/evolve.rs`) nudges six continuous genes by Gaussian noise
of width σ, and sometimes makes one **structural move**: toggle a
generator, toggle an effect module, change the filter type / chorus mode
/ phaser stages, toggle a picture card, change the palette, change an LFO
shape, add/move/remove a route — and, when the point has layers, toggle a
layer or choose another sound or rhythm for one. Every proposal is then
**repaired**: one to five generators on, and the explicit sound → image
couplings summing to at least 1.2, so a point never goes silent or deaf.

### The morph

A press does not cut: the session glides from what is audible now to the
new point over **2 s** (0.8 s for undo). Continuous genes are
interpolated; switches and choices take their new value at the start of
the glide — except a switch turning *off*, which waits for the end — and a
generator or layer switching on rises from silence while one switching
off sinks to it, so nothing appears or vanishes with a jump. A press in
the middle of a morph starts from what is audible, not from where the
last press was heading. 🎲 morphs too, but starts the picture afresh.
Loading a preset or a point is a hard switch: the effects' tails are
dropped and the picture starts over.

## The scout

While the listener sits on a settled point (0.8 s after the morph ends),
the **scout** renders the point and several 👍 and 👎 candidates offline
(30 s at 22 050 Hz, on the app's worker threads) and scores each. A press
takes the best candidate for its direction instead of a blind draw.

The score is **fractality** (`analysis/fractal.rs`): people tend to like
sound whose fluctuations look alike across time scales (Voss & Clarke,
1975). From one STFT:

- β of the loudness envelope's 1/f^β spectrum — preferred near 1 (pink);
- β of the spectral-centroid (pitch/brightness) contour — near 1;
- the box-counting dimension of the spectrogram's salient cells — near 1.6
  (structured, neither empty nor solid);

folded into a score in [0, 1] (the two βs weigh twice the box dimension).
A candidate more than 6 dB quieter than the point loses 1/24 of a point
of score per extra dB, and silence ranks last — the search must not sink
into silence one step at a time. The scout renders without the layers.

## Presets

The built-in points are `assets/presets.json`, edited here by hand: 15
from the web app (Fractal garden, Stillness ink, Whale coral, Molten
Polivoks, Aurora, Silver maze, Space breccia, Cave coral, Loom & copper,
Wind ash, Bell spots, Subway basalt, Overtone steppe, Candle glaze,
Tanpura halo) and five with sample and drum layers (Night meadow, Workshop,
Glass loom, Magic forest, Sea shore).
`syn-core/tests/presets.rs` holds what every preset must be; the
`new-preset` skill describes how one is made and checked.

## Points from outside

- **Sanitize** (`point.rs`): anything that arrives — a link, a file, a
  stored point — is made into a complete, safe point: every number clamped
  to its range, unknown keys and malformed parts dropped (never the whole
  point), an unknown sound dropped, an unparsable pattern kept as silence.
- **Id**: a point's id is the first 10 base62 digits of the SHA-256 of its
  canonical JSON — byte for byte what the web app computes, so a shared
  link gets the same id everywhere.
- **Share links** (`share.rs`): a `#s=` token is base64url of the point's
  JSON; it carries a point between apps by copy and paste, with no server.
