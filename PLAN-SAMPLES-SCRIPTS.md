# Samples and scripts — plan

Two additions to the model, agreed with the user on 2026-10-14:

1. **Found sounds.** Owls, tits, crickets, creaking wood, bottles, a fire —
   short recorded samples, as one-shot events and as beds, next to the 23
   formula generators.
2. **Drums and other instruments as scripts.** Rhythms written the way
   [strudel.cc](https://strudel.cc/) writes them (`"bd sd [hh hh] ~"`), kept
   in the point, played by the engine, and — like everything else in the
   point — mutated by 👍/👎.

This repository is the specification (synesthesia PLAN-CORE.md C2), so both
are made here first: the engine, the shape of the point, its genes, the
sanitize rules, the catalogue of sounds, the tests and the bench. The apps
get them by bumping their pinned revision; the console (synesthesia-rust)
is the first host, because it is the one with speakers and a measuring kit.
This file is the record of what was looked at, what was decided and why,
the data shapes, the phases, the budget and the measurement each phase owes.
Numbers go in as they are measured; `TODO.md` carries the open items.

## What was looked at (2026-10-14)

### Strudel, and whether it can run inside the core

[Strudel](https://strudel.cc/) is TidalCycles in JavaScript: a **Pattern is a
function from a time span to the events ("haps") inside it**; mini-notation
strings (`"bd [sd sd] ~ hh*2"`, a PEG grammar) build patterns; combinators
(`fast`, `slow`, `every`, `degradeBy`, `stack`, euclid `(3,8)`) transform
them; a scheduler queries the pattern a little ahead of the clock and hands
each hap to the sound layer (superdough: samples by name, `n`, `speed`,
`cut`, `begin/end`, envelopes). Its default drums come from
`tidal-drum-machines`, its instruments from VCSL.

| option | verdict | why |
|---|---|---|
| embed Strudel itself (a JS engine such as `boa`, plus `@strudel/core` + `@strudel/mini`) | **no** | Strudel is **AGPL-3.0**, this repository is GPL-3.0 and the console is MIT; a bundled JS engine is a large dependency and a node build step; the worklet and the FFI hosts would each need it too |
| the Rust port `strudel-core` / `strudel-mini` 0.2.0 (codeberg nukleas/strudel-rs) | **no** | AGPL-3.0 as well; young (its `mini` in core is a placeholder that "does nothing"); pulls `anyhow`, `clap`, `logos`, a music-theory crate |
| `cycles` (mitchmindtree), a Rust port of Tidal's Pattern | watch | permissive, but idle for ~1.5 years and a different API; nothing to gain over a port sized for us |
| Glicol / `glicol_synth` | **no** | a whole graph-based language that would take over the audio chain; our chain is the point's sound |
| a scripting language (`rhai`, MIT) exposing a Strudel-like API | later, if wanted | a 2 MB dependency to get user-defined functions; nothing in the first version needs them |
| **our own: the Pattern model and a mini-notation subset, in `syn-core`** | **yes — decision 1** | ~1–1.5 k lines, no dependency, the realtime and wasm rules can be designed in, and Strudel's documented examples become the test cases |

Strudel's *ideas* are what we take — the pattern-as-function model, the
mini-notation syntax, `s("name:n")`, `speed`, `cut` — not its code.

### Open samples — what is actually CC0

What the user asked for (owls, tits, crickets, creaks, bottles, fire) and
where it exists under a licence we may put in a repository:

| source | licence | what is there | how to get it |
|---|---|---|---|
| [Freesound](https://freesound.org/) with the filter `license:"Creative Commons 0"` | CC0 per sound — **check each page** | 3,483 owl results, 2,188 crickets, 337 fire crackling, 859 wood creak, 857 glass bottle, 38 great tit | the site, by hand; the API needs OAuth2 for the original file |
| [BigSoundBank](https://bigsoundbank.com/) (LaSonothèque, Joseph Sardin) | CC0, no attribution required (stated on every sound page) | Tawny Owl #1–#4, Barn Owl #1–#4, Long-eared Owl #1–#2; Field cricket, Nocturnal insect #1–#3, Insect song #1–#4; Fireplace #1–#5, Big branching fire #1–#3; Creaking door #1–#8, Floating floor squeaks, Squeaky steps; Bottle blow, Bottle clink, Wine stopper, Champagne cork; Nightingale, Blackbird, Robin, Blackcap, Crows | direct download, WAV/FLAC/OGG; 48 kHz 24-bit mono recordings |
| [VCSL](https://github.com/sgossner/VCSL) — Versilian Community Sample Library | **CC0** (README) | percussion: Woodblock, Claves, Shaker (3), Cabasa, Cajon, Bongos, Conga, Darbuka, Frame Drum, Bass Drum, Snare, Hi-Hat, Claps, Cowbells, Agogo, Guiro, Finger Cymbals, Triangles, Hand Bells, Sleigh Bells, Tambourines, Vibraslap, Ratchet, Flexatone, Slapstick, Ocean Drum, Wine Glasses, Kalimba, Mbira … (~5 GB, 4,000+ WAVs in all) | git; take single hits, not whole instruments |
| OpenGameArt (CC0 packs), Wikimedia Commons (CC0/PD items) | per item | animal and creature packs | by hand |
| Strudel's `tidal-drum-machines`, `Dirt-Samples`, `EmuSP12` | **no licence stated**; the Tidal forum calls Dirt-Samples' provenance "largely unknown" | the classic drum machines | **not used** |
| xeno-canto, BBC Sound Effects, Pixabay, Zapsplat, Sonniss | CC BY-NC-SA / RemArc / own terms — no redistribution | birds, everything | **not used** |

Freesound CC0 pages found while looking, a starting list for the curator
(ids, not endorsements — listen first): owls `871239 242772 59462 101344
429186 181357`; great tit `607242 346066 418102 849566`; crickets `822935
495396 402734 852877`; fire `510767 595483 807942 636178`; wood creak
`618082 618083 618084 618085 506665 572645`; bottle blow `447756 846527`.

## Decisions (agreed with the user, 2026-10-14)

1. **Our own pattern engine, in `syn-core`, no dependency.** A `Pattern` is
   a tree evaluated as *span → events*, exactly Strudel's model; a
   mini-notation subset builds it; a fixed set of combinators transforms it.
   Scripts in the point are mini-notation strings plus numbers — not a
   programming language. If user-defined functions are ever wanted, `rhai`
   is the candidate, as a later phase after a listening pass says what is
   missing.

2. **The point gets a `seq` section, and the section gets genes.** Samples
   and patterns live inside `AppState` (the user's choice: scripts are part
   of a preset) as an optional field — `#[serde(default)]`, skipped when
   empty — so a point without it is the point as before, byte for byte:
   `fixtures/points.json` (sanitize, canonical JSON, ids) must not move.
   The genes are **appended** after the 250, the way tanpura and bowl were
   added, so the order share-compatibility depends on stays; the mod
   targets are appended too, so route genes keep their indices. Evolution
   mutates layers like everything else: on/off, which sound, which rhythm,
   gain, speed, density. Because the Settings page is derived from the gene
   list (`settings.rs`), the layers appear there in every app for free; the
   page rules (`settings_page.rs`) get a section for them.

3. **CC0 only, hand-picked, with provenance.** Every file comes with its
   source URL, author, the licence as shown on that page, and the trim
   applied, in a `manifest.json` next to it. No CC-BY (no ATTRIBUTION file
   to carry across four apps), nothing of unstated provenance. A curator
   listens before a file goes in: pretty is the point.

4. **Drums: synthesized in the core, plus VCSL percussion as samples.**
   `bd sd hh oh cp` are deterministic synth voices (a swept sine, tone +
   noise burst, filtered noise, a clap of bursts): no licence, zero bytes,
   testable to the sample. `woodblock claves shaker cajon bongo …` come
   from VCSL. As in Strudel, the *name* decides: a name in the bank is a
   sample; `bd`/`sd`/`hh`/`oh`/`cp` are the synths unless the bank carries a
   sample of that name (then the sample wins).

5. **Made here, heard on the console first.** All of 1–4 lands in this
   repository with its tests and `syn-bench`; synesthesia-rust moves onto
   the core (it still carries its own `syn-core` copy at 237 genes — its
   C0), embeds the bank, shows the layers and does the listening pass.
   `syn-wasm` and `syn-ffi` get a `loadBank(bytes)` when their apps bump.

6. **The core takes samples as bytes, never as files.** *No I/O in
   `syn-core`* holds. The host hands over one blob — `bank.bin`: header +
   manifest JSON + 16-bit PCM 48 kHz mono — and `syn_core::seq::bank`
   parses it into a `SampleBank`. The console embeds it with
   `include_bytes!`; the web app fetches it next to the wasm package;
   Android reads it from assets. A host with no bank still plays the synth
   drums; a sample name the bank lacks is silent, never an error — a point
   must open everywhere.

7. **The samples live in their own repository, `synesthesia-samples`.** The
   web app clones the pinned core on every `npm run core`, so 10–30 MB of
   WAV here would be paid on every build. This repository holds the
   *catalogue* — the ordered list of sound names (`seqSounds` in
   `assets/schema.json`) the choice gene indexes — and a test that a bank's
   manifest covers it. The samples repository holds the WAVs, the manifest,
   `scripts/build-bank.py` and the provenance; apps pin it by revision the
   way they pin the core. Tests here need no real audio: the sampler's tests
   build a two-sample bank of sine blips in memory.

8. **Realtime discipline, extended.** Nothing allocates in `Engine::render`
   (the rule in AGENTS.md): the voices are a fixed pool (16; the oldest is
   stolen with a 2 ms release), a block's events go into a fixed array (64;
   overflow is counted, never grown), the bank is an `Arc` swapped in by a
   `syn-player` command and retired to the control side like a replaced
   state. Randomness in a pattern (`?`, `|`) is a **hash of (cycle,
   position, layer, point seed)** — a pure function of time, as the LFOs
   are: the offline render, the live render and the scout's candidate hear
   the same bar. The wasm rules hold too: no transcendental per sample in a
   voice (the synth drums use recurrences), fractions in `i64`, phases
   wrapped.

9. **One clock.** The cycle position is `t · cps`, where `t` is the engine's
   second (the LFO clock). No tempo clock of its own, nothing to drift. The
   LFO matrix may route onto `cps`, a layer's `gain`, `speed` and `density`.

10. **Layers go into the mix before the FX chain**, like a formula does: the
    chain is the point's sound, and an owl through the point's reverb is the
    point. A per-layer `dry` flag sends a layer straight to the master for
    drums that must not phase; the listening pass sets the presets'
    defaults, not this document.

11. **Measure first.** The probes in S0 price the plan before the port:
    bank size against the repository and binary budget, decode and voice
    cost, and — the one that can change the design — what a steady rhythm
    does to the fractality score the scout steers by.

## The model

### The `seq` section of the point

```json
"seq": {
  "cps": 0.5,
  "layers": [
    { "on": true,  "sound": "owl",      "pattern": "<0 ~ ~ 1> ~ ~ ~",
      "gain": 0.5, "speed": 1.0, "density": 1.0, "dry": false, "mode": "hits" },
    { "on": true,  "sound": "crickets", "mode": "bed",
      "gain": 0.3, "speed": 1.0, "density": 1.0, "dry": false, "pattern": "" },
    { "on": true,  "sound": "bd",       "pattern": "0(3,8) , ~ sd ~ sd , hh*8?0.3",
      "gain": 0.6, "speed": 1.0, "density": 0.8, "dry": true,  "mode": "hits" },
    { "on": false, "sound": "woodblock", "pattern": "", "gain": 0.5, "speed": 1.0,
      "density": 1.0, "dry": false, "mode": "hits" }
  ]
}
```

- Four layer slots, like the four LFOs — a fixed count, so the genome is
  fixed-size. `cps` is cycles per second (Strudel's default 0.5 = one 4-beat
  bar at 120 bpm).
- A token in a pattern is `~` (rest), a **number** (that variant of the
  layer's `sound`: `owl:1`), or a **full name** (`sd`, `woodblock:2`) that
  overrides the layer's sound for that step. The number form is what makes
  a layer evolvable: the `sound` gene changes what the rhythm is played on
  without touching the rhythm.
- `mode: "bed"` loops the sample with a 1 s crossfade at `gain`, ignoring
  the pattern — crickets, fire, rain under everything; `density` is unused
  for a bed. `mode: "hits"` plays the pattern.
- `density` is Strudel's `degradeBy(1 − density)`: the fraction of events
  kept, decided by the hash of decision 8.
- `speed` is playback rate (pitch); `dry` is decision 10.
- An absent or empty section serializes to nothing (`skip_serializing_if`),
  so canonical JSON, point ids and the `fixtures/points.json` cases are
  untouched for every point that has no layers.

### Mini-notation — the subset

Tidal/Strudel syntax, the subset that covers the documented examples we test
against:

```
sequence  := step+                     "bd sd hh"      steps share the cycle equally
step      := '~' | token | group | alt | poly
            ( '*' num | '/' num | '!' int | '?' num? | '(' k ',' n (',' r)? ')' | '@' num )*
group     := '[' stack ']'             "[hh hh]"       a subsequence in one step
alt       := '<' stack '>'             "<0 1 2>"       one child per cycle, in turn
poly      := '{' stack '}' ('%' int)?  "{0 1 2}%4"     polymeter (phase S6)
stack     := sequence (',' sequence)*  "bd , hh*4"     layered, same span
choice    := sequence ('|' sequence)*  "bd | sd"       one per cycle, by hash
token     := name (':' int)? | int     "owl:1" "2"
```

`*n` speeds a step up, `/n` slows it, `!` repeats, `?` drops with
probability 0.5 (or the given one), `(3,8)` is a Euclidean rhythm, `@3`
weights a step. `.` as a group separator and `_` elongation come with
polymeter in S6 if a preset wants them. Everything parses to a `Pattern`
tree when the point changes; nothing is interpreted at render time.

### The pattern engine

```
Pattern (tree)  --query(span, &mut Events)-->  events {onset, dur, sound, n, gain}
     ^                                                      |
 mini parser                                   engine: fire inside this block
```

- `query` writes the events whose onset lies in `[a, b)` (cycles) into a
  fixed array, recursing through the tree; fractions are exact (`i64`
  numerator / denominator) so a step boundary is never missed by rounding —
  Strudel uses `Fraction` for the same reason.
- The engine queries once per block for the span the block covers, then
  starts each event's voice at its sample offset inside the block:
  sample-accurate, with no lookahead to keep in step.
- The tests are Strudel's own documented examples, transcribed as expected
  event lists (`"bd sd"` → two events at 0 and ½; `"bd(3,8)"` → 0, ⅜, ¾;
  `"<a b>"` alternates; `"[a b]*2"`; `"a?"` with a fixed seed), plus
  invariants: an event never leaves its step, `fast(2)` ≡ `*2`, a query
  split in two spans equals the one query.

### Voices

- **Sampler voice**: a bank slice, a position, rate = `speed · bank_sr / sr`
  (so the 22 050 Hz scout render and the 48 000 Hz live render play the same
  pitch), linear interpolation, 2 ms attack, 2 ms release when stolen or
  cut, per-event gain. `cut` groups (an open hat silenced by the closed one)
  in S6.
- **Bed voice**: two sampler voices crossfading over the loop point.
- **Synth drums** (`bd sd hh oh cp`): closed-form envelopes and one
  oscillator or noise source each, driven by the same `speed` and `gain`;
  deterministic, with a golden take of each committed once so the sound
  cannot drift (the `golden/` rule applies to them from then on).
- **Mix**: layer sum → the FX chain input with the formulas (or → master
  when `dry`); the limiter already protects the output.

### The bank

```
bank.bin = magic "SYNB" | u32 version | u32 manifest_len | manifest JSON (UTF-8)
         | u32 sample_rate | i16 PCM mono, every sample back to back
manifest = { "sounds": { "owl": [ {"offset":0,"len":96000,"src":"https://…","author":"…",
             "license":"CC0","trim":"0.4-2.4s","gain_db":-3.0}, … ], "crickets": [...] } }
```

`scripts/build-bank.py` in the samples repository: trims, normalizes to a
peak (beds to an RMS), resamples to 48 kHz mono, writes the blob and a
`provenance.md` from the same manifest. 16-bit PCM needs no decoder here
(`wav.rs` gets a reader); FLAC (`claxon`) is the fallback if S0's size
number breaks the budget, and would be a dependency decision.

### Genes and schema

Appended to `assets/schema.json` (edited here, by hand, with its tests):

| gene | kind | range | gated by |
|---|---|---|---|
| `seq.cps` | cont, exp | 0.25–2 | — |
| `seq.<i>.on` | bool | | — |
| `seq.<i>.sound` | choice | index into `seqSounds` | `seq.<i>.on` |
| `seq.<i>.rhythm` | choice | index into `seqRhythms`; 0 = "custom" keeps the string | `seq.<i>.on` |
| `seq.<i>.gain` | cont | 0–1 | `seq.<i>.on` |
| `seq.<i>.speed` | cont, exp | 0.5–2 | `seq.<i>.on` |
| `seq.<i>.density` | cont | 0–1 | `seq.<i>.on` |
| `seq.<i>.dry` | bool | | `seq.<i>.on` |

4 × 7 + 1 = **29 genes → 279**; mod targets `seq.cps` and per layer `gain`,
`speed`, `density` → **13 → 146**. `seqRhythms` is a built-in list of ~16
patterns (`"0 ~ ~ ~"`, `"0(3,8)"`, `"0*4?0.5"`, `"<0 ~> ~ 1 ~"`, …) so a
structural move can swap a rhythm; a hand-written pattern encodes as
"custom" and survives decode unchanged. `evolve.rs` gets three structural
moves — toggle a layer, rechoose its sound, rechoose its rhythm — weighted
like the formula toggles. The morph (`lerp_genome`) treats the new genes as
it treats the old: continuous ones glide, choices and bools switch where
they switch today. `sanitize` learns the section: clamp the numbers, drop an
unknown sound or mode, keep an unparsable pattern as `""`.

## Architecture — where the code goes

```
syn-core/src/seq/
  mini.rs        the parser → Pattern
  pattern.rs     Pattern tree, Fraction, query(span, &mut Events)
  bank.rs        SampleBank from bytes; the manifest; the in-memory test bank
  voice.rs       sampler, bed, the synth drums
  sequencer.rs   layers → voices per block; the hash of decision 8
syn-core/src/engine.rs      owns a Sequencer; render: formulas + layers → FX
syn-core/src/state.rs       SeqState / Layer, serde(default), skipped when empty
syn-core/src/point.rs       sanitize for seq (fixtures unchanged)
syn-core/src/schema.rs      seqSounds, seqRhythms, the genes, the mod targets
syn-core/src/genome/        codec (encode/decode seq), evolve (3 moves)
syn-core/src/settings_page.rs  the Layers section
syn-player/src/lib.rs       Command::SetBank(Arc<SampleBank>), retired like states
syn-wasm, syn-ffi           loadBank(bytes) — S7, when the apps bump
syn-bench                   --seq: events/s, voices, overflow count, score with/without
golden/                     + one take per synth drum
assets/schema.json          + seqSounds, seqRhythms, 29 genes, 13 mod targets
assets/presets.json         + presets that use layers (S5)

synesthesia-samples (new)   samples/<name>/<nn>.wav · manifest.json ·
                            scripts/build-bank.py · provenance.md
synesthesia-rust (console)  C0: onto the core; then include_bytes!(bank.bin),
                            the TUI `seq` line, config, the listening pass
```

## Phases

Each phase ends green on `./scripts/check.sh` and with its numbers written
into this file.

- **S0 — probes (throwaway).** Before any port:
  - a 40–60 sample pick from the table above, trimmed: **bytes** as 16-bit
    PCM 48 kHz, as 22 kHz, as FLAC → decides decision 6's format and
    whether the console embeds or loads the bank;
  - bank parse cost against the apps' startup budgets (the console's is
    300 ms to first sound; the worklet compiles the module first, then
    would parse the bank);
  - one sampler voice and 16 voices in a tight loop: **ns per sample** next
    to the generators' worst (2.4 µs for Fractal garden), native and wasm;
  - **the score probe**: `syn-bench` on *Fractal garden* and *Bell spots*
    alone, with crickets under them, and with `bd(3,8)` — the fractality
    band is what the scout steers by, and a steady beat may push every
    candidate with a rhythm out of it (or into it). The number decides
    whether layers are scored as part of the candidate, left out of the
    scout's render, or get a term of their own.
- **S1 — the pattern engine** (`seq/mini.rs`, `pattern.rs`): the subset
  above, exact fractions, the fixed-array query, the Strudel examples as
  tests, a number for one query per block.
- **S2 — voices and the bank** (`bank.rs`, `voice.rs`, `sequencer.rs`,
  `engine.rs`, `syn-player`): the in-memory test bank, the sampler and bed
  voices, the five synth drums with their golden takes, the sequencer in the
  engine, `SetBank`, no allocation in `render` (a counting allocator in the
  test build), `render_offline` bit-identical for a point without `seq`
  (the golden and fixture tests say so).
- **S3 — the point**: `SeqState` in `AppState`, `sanitize`, the genes and
  mod targets appended, codec, the three structural moves, the Layers
  section of the Settings page, the count assertions updated (`250 → 279`,
  `133 → 146`), `fixtures/genomes.json` still matching as a prefix. The
  test: a point without `seq` encodes to the same 250 leading genes and
  the same canonical JSON as before.
- **S4 — the samples repository**: the curated pick (listened to), the
  manifest with provenance, `build-bank.py`, `provenance.md`; here, the
  catalogue (`seqSounds`) and the test that a bank covers it. Candidates:
  owls (tawny, barn, long-eared), great tit, crickets ×3, fire ×2, creaks
  ×4, bottle blow and clink, a cork, nightingale, blackbird, robin; VCSL
  woodblock, claves, shaker, cajon, bongo, frame drum, cowbell, triangle,
  finger cymbals, hand bells, kalimba, ocean drum, wine glass.
- **S5 — presets and the listening pass** (`new-preset` and `sound-check`
  skills): 3–4 presets that use layers — "Night meadow" (a crickets bed,
  an owl now and then, a slow bowl), "Workshop" (creaks, bottles, a
  woodblock over a drone), a drum one over *Molten Polivoks* — each passing
  `tests/presets.rs` and the sanitize fixed point; `dry` defaults and gains
  set by ear on the console; this file gets the numbers.
- **S6 — the second round, by need**: polymeter `{}`, `cut` groups,
  `begin/end`, `rhai` if someone wants functions. Each is a decision when a
  preset asks for it, not before.
- **S7 — hosts**: `loadBank` in `syn-wasm` (bytes in, like a point) and
  `syn-ffi`; the web and Android apps bump their revision and fetch or
  bundle `bank.bin`.

On the console side (synesthesia-rust), in parallel with S1–S3: **C0**,
onto the core — drop its local `syn-core`, `assets/`, `golden/`; depend by
revision; `syn-audio` wraps or yields to `syn-player`; its `check.sh`,
`bench`, `bench --sim` and one `soak.py` run green with PLAN.md's table
re-taken. Then **C1**: bump to S3's revision, embed the bank (or load it
from `$XDG_DATA_HOME` if S0 says the binary gets too fat), `--no-samples`,
the TUI `seq` line, `samples = true` in config, the `verify` ladder with the
heaviest layered preset — 0 xruns in 30 min is the bar.

## Budget (thresholds; checked by the bench, filled in as measured)

| metric | target | measured |
|---|---|---|
| bank, 40–60 samples | ≤ 20 MB in the samples repository, ≤ 12 MB embedded in the console binary | S0 |
| bank parse | ≤ 50 ms native; the worklet's load gate unchanged | S0 |
| one sampler voice | ≤ 50 ns a sample native (a `sin` is 24.6 ns on the console's A76) | S0 |
| 16 voices + a 4-layer pattern query | ≤ 3 % of one core on top of the point, native; inside PLAN-CORE's 35 % quantum gate on the phones | S2 |
| allocations in `render` | 0 | S2 (test) |
| event array overflow | 0 on every preset at 48 kHz and 22.05 kHz | S2 (bench counter) |
| offline render of a layered preset | ≥ 8× realtime on the console | C1 |
| xruns, drum preset, 48 kHz / 1024 frames | 0 in 30 min | C1 (`soak.py`) |
| genes | 279; the first 250 unchanged for a point without `seq`; `fixtures/points.json` unchanged | S3 (tests) |
| fractality of the presets with layers | inside the presets' band like the others, or the scout's rule is amended by S0's number | S0 / S5 |

## Risks and open questions

- **The scout's metric.** A steady beat is the opposite of 1/f; S0's score
  probe comes first for a reason. If the score drops for every rhythm, the
  options are: score the candidate without its layers (they are the user's
  choice, not the search's), or add a rhythm-aware term — chosen by the
  number, not here.
- **Licences are per file, not per site.** BigSoundBank states CC0 on each
  page and Freesound's filter is reliable, but the manifest records what
  the page said on the day, and the curator opens each page.
- **Four apps, one shape.** Appending genes and an optional section is the
  compatible way, but the Android and web apps will carry a `seq`-less
  point until they bump; a point saved from them loses its layers. Known
  and accepted — the same happened with `delay_shimmer`.
- **The bank in wasm memory.** 10–20 MB of f32 in the worklet grows the
  module's memory once at load; the worklet remakes its view then (it
  already handles growth). The web app may want the 22 kHz bed variants
  from S0.
- **The console is behind the core.** C0 is real work (`Player` moved into
  `syn-player`, `preset_name`, two generators, the session), and it is the
  prerequisite for hearing any of this on the machine with the speakers.
- **Pretty samples are a taste, not a test.** The listening pass is a phase
  with a deliverable (the presets and their gains), so it does not become
  folklore.

## Don'ts (in addition to AGENTS.md)

- Never put a file of unstated licence in the samples repository, whatever
  it sounds like.
- Never allocate in `render` for an event, a voice or a bank swap.
- Never give the sequencer its own clock; cycles are `t · cps`.
- Never change the order of the first 250 genes, of `seqSounds` or of
  `seqRhythms`; append.
- Never interpret a pattern string at render time; it is parsed once into a
  `Pattern` when the point changes.
- Never let a point without `seq` serialize differently than before: the
  section is skipped when empty, and the fixtures prove it.
