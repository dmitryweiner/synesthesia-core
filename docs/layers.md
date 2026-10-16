# Sample and drum layers

Next to the formulas, a point can play **recorded sounds and drums** —
owls, crickets, bottles, a fire, woodblocks, a frame drum, synth kicks —
in rhythms written in [Strudel](https://strudel.cc/)'s mini-notation.
Listen to every sample in the bank on the demo page:
**<https://dmitryweiner.github.io/synesthesia-samples/>**.

Code: `syn-core/src/seq/` (`mini.rs` the parser, `pattern.rs` the pattern
engine, `sequencer.rs`, `voice.rs`, `bank.rs`). The decisions and their
reasons: [PLAN-SAMPLES-SCRIPTS.md](../PLAN-SAMPLES-SCRIPTS.md). The sounds
themselves live in
[synesthesia-samples](https://github.com/dmitryweiner/synesthesia-samples),
never in this repository.

## The `seq` section of a point

```json
"seq": {
  "cps": 0.5,
  "layers": [
    { "on": true, "sound": "owl", "pattern": "<0 3 [5 ~] 10 ~ 11 ~ ~>/2",
      "gain": 0.5, "speed": 1.0, "density": 1.0, "dry": false, "mode": "hits" },
    { "on": true, "sound": "crickets:1", "pattern": "", "mode": "bed",
      "gain": 0.3, "speed": 1.0, "density": 1.0, "dry": false }
  ]
}
```

| field | meaning |
|---|---|
| `cps` | tempo in **cycles per second**, shared by all layers. One cycle is one bar: 0.5 is a four-beat bar at 120 bpm. Gene range 0.25–2 |
| `layers` | up to **four** slots (a fixed count, like the four LFOs, so the genome has a fixed size) |
| `on` | the layer plays |
| `sound` | a sound name from the bank or a synth drum, optionally with a variant: `crickets:3`. What a bare number in the pattern plays, and what a bed loops |
| `pattern` | the rhythm, in mini-notation (below). Ignored by a bed |
| `mode` | `hits` — one shot per event of the pattern; `bed` — the sample looped under everything, the pattern ignored |
| `gain` | 0–1 |
| `speed` | playback rate 0.5–2: pitch and length together (2 = an octave up, half as long) |
| `density` | the fraction of events kept, 0–1 (Strudel's `degradeBy(1 − density)`) |
| `dry` | the layer skips the FX chain and goes straight to the master — for drums that must not be smeared by the reverb or the phaser |
| `humanize` | 0–1: each hit up to 30 % quieter and up to 15 ms late (never early). Optional, omitted when 0, and not a gene |
| `az` | −90 … 90°: where the layer sits around the head in a stereo app ([Space and binaural beats](binaural.md)). Optional, omitted when 0 |

A point with no layer on has no `seq` section at all, so it serializes —
and gets its id — exactly as before layers existed.

## How a layer plays

- **Layers go into the mix before the effects**, like a formula: an owl
  through the point's reverb *is* the point. `dry` is the exception.
- **One clock.** The cycle position advances by `cps · dt` every engine
  block, so the layers stay locked to the same clock as the LFOs; a hard
  switch to another point sets it back to `t · cps`. An LFO may move `cps`
  without the bar jumping.
- **Sample-accurate.** Each block, every layer's pattern is asked for the
  events that start inside it, and each voice starts at its exact sample
  offset — no lookahead, no scheduler.
- **Randomness is a hash**, not a dice roll: `?`, `|` and `density` are
  decided by a hash of the event's time, the layer and the point's seed.
  The same point plays the same bars on every device and in every render.
- **Voices**: at most 16 sound at once; when a 17th starts, the oldest is
  stolen with a 2 ms release. Every voice has 2 ms attack and release
  ramps, so nothing clicks.
- **Beds** loop seamlessly (their 1 s equal-power crossfade is baked into
  the sample); a bed whose sample changes fades out over 0.3 s under the
  new one.
- **Variants.** A sound has several recordings (`owl`, `owl:1`, `owl:2` …).
  A bare number *n* in the pattern plays variant *own + n*, where *own* is
  the variant the layer's `sound` names; numbers past the last variant wrap
  around.
- **Missing sounds are silent**, never errors: a host without a bank still
  plays the synth drums, and a point must open everywhere.

## Synth drums

Five drums are synthesized in the core — no licence, no bytes, identical
on every platform, with golden takes in `golden/drums/`:

| name | sound |
|---|---|
| `bd` | bass drum: a sine swept down, and a click |
| `sd` | snare: a short tone and a burst of filtered noise |
| `hh` | closed hi-hat: six square waves (the TR-808's metal), band-passed high |
| `oh` | open hi-hat: the same, ringing longer |
| `cp` | clap: three quick bursts of band-passed noise and a tail |

As in Strudel, the *name* decides: if the bank ever carries a sample named
`bd`, the sample wins.

## Mini-notation

The subset of Tidal/Strudel syntax the core understands:

| syntax | meaning | example |
|---|---|---|
| `a b c` | steps share the cycle equally | `"0 ~ 0 ~"` — two hits, on beats 1 and 3 |
| `~` | a rest | `"~ 0"` — on the off-beat |
| `[a b]` | a subsequence squeezed into one step | `"0 [~ 0] ~ [0 0]"` |
| `<a b c>` | one step per cycle, in turn | `"<0 1 2 3>"` — a different variant each bar |
| `a , b` | layered: both over the same span | `"0(3,8) , ~ sd ~ sd"` |
| `a \| b` | one of them per cycle, chosen by hash | `"[~ 0? ~ ~] \| ~"` |
| `a*2` `a/2` | faster, slower | `"0*8"` — eighths; `"<…>/2"` — each step lasts two cycles |
| `a!3` `a ! !` | repeat a step | `"0(3,8)!3 0(5,8)"` |
| `a?` `a?0.3` | drop the event with probability ½ (or the given one) | `"0*8?0.3"` |
| `a(3,8)` `a(3,8,2)` | Euclidean rhythm: 3 hits spread over 8 steps; the third number rotates it **right** (as Strudel does; Tidal rotates left) | `"0(3,8)"` = `x..x..x.` |
| `a@3` | a step three times as wide as the others | `"0@3 1"` |
| `name` `name:n` | a named sound (and variant) for this step, overriding the layer's sound | `"bd sd:1"` |
| `n` | variant *n* of the layer's own sound | `"0 2"` |

Polymeter `{ }`, `.` and `_` are not supported yet. Limits, each a parse
error: 512 bytes, 16 levels of brackets, factors and step counts ≤ 128.
A pattern that does not parse plays silence (and is kept as `""` when a
point is sanitized). Patterns are parsed once, when the point changes;
nothing is interpreted while the audio runs.

### Examples from the presets

| preset | layer | pattern | what you hear |
|---|---|---|---|
| Workshop | `woodblock` | `0(3,8)` | a tresillo |
| Night meadow | `owl` | `<0 3 [5 ~] 10 ~ 11 ~ ~>/2` | a different owl every two bars, sometimes none |
| Night meadow | `crickets:1` | — (bed) | crickets under everything |
| Magic forest | `great-tit` | `[~ 0? ~ ~] \| [~ ~ robin:1? ~] \| … \| ~` | each bar, one random bird (or silence) at a random beat |

## The sample bank

The host — never the core — loads `bank.bin` (the release `bank-v1` of
synesthesia-samples) and hands its bytes to the core:

```text
bank.bin = "SYNB" | u32 version = 2 | u32 manifest_len | manifest JSON
         | the FLAC streams (mono, 16-bit), back to back — little-endian
```

The manifest lists every sound's variants with their sample rate (24, 32
or 48 kHz, chosen per sample by its spectrum), mode, checksum and
provenance. Every recording is **CC0 or public domain**, hand-picked,
with its source page, author and trim recorded. A sound is decoded the
first time a point asks for it and kept for as long as the bank lives;
decoding happens on the control side, never inside the audio render.

The catalogue (`seqSounds` in `assets/schema.json`) is the ordered list
the `sound` gene indexes: 42 bank sounds — birds (`blackbird canary crow
dove goldfinch great-tit kookaburra lyrebird nightingale oriole owl
pigeon potoo robin`), other animals (`cicada crickets elk frog whale
wings`), places (`fire night rain stream`), things (`bottle-blow
bottle-clink creak pendulum`), percussion (`bell bongo chimes conga
cowbell darbuka frame-drum hand-bell mark-trees shaker sleigh-bells
slit-drum triangle woodblock`) — then the five synth drums. Each entry
carries the mode it was cut for (`hits` or `bed`); a layer may still play
a bed sample as hits and the other way round.

## Layers and the search

The layers are genes like everything else (33 of them, appended after
the original 250: `seq.cps` and, per layer, `on`, `sound`, `rhythm`,
`gain`, `speed`, `density`, `dry`, `bed`). The `rhythm` gene picks one of
18 built-in patterns (`seqRhythms`); index 0 means "custom".

- 👍/👎 **vary the layers a user put in** — toggle one, choose another
  sound, choose another built-in rhythm — but never add layers to a point
  that has none.
- A **hand-written pattern** is "custom" to the genome, which cannot carry
  its text. The session keeps it: a mutation, a morph, 🎲 or ↩ lays the
  base point's pattern text (and the sound's `:n` variant and `humanize`)
  back over the decoded layers, the way it keeps the volume and the name.
- The LFOs may move `cps` and each layer's `gain`, `speed` and `density`
  (see [LFOs and modulation](lfo.md)).
- The scout renders candidates **without** the layers, so the fractality
  score does not hear them.
