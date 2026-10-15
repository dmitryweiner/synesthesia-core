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
| Wikimedia Commons — NOAA, NPS, FWS recordings (added 2026-10-15) | public domain (US Government works) or CC0, per file page | whales (blue and fin sped up 10× by NOAA, humpback, minke, beluga, sperm-whale clicks), bearded seal (PD-self), elk, kookaburra | the Commons API; licence read from each file's page |
| [Yellowstone Sound Library](https://www.nps.gov/yell/learn/photosmultimedia/soundlibrary.htm) (NPS) | public domain, stated on the library page | elk bugles, the elk chorus, a calf | direct MP3 download |
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
   repository with its tests and `syn-bench`; synesthesia-rust — on the
   core since 2026-10-15 (`syn-core`, `syn-player`, `syn-session` at one
   `rev`; its PLAN.md decisions 11 and 12) — bumps, loads the bank, shows
   the layers and does the listening pass. `syn-wasm` and `syn-ffi` get a
   `loadBank(bytes)` when their apps bump.

6. **The core takes samples as bytes, never as files.** *No I/O in
   `syn-core`* holds. The host hands over one blob — `bank.bin`: header +
   manifest JSON + one FLAC stream per sample (v2; amended by S0's numbers,
   see *The bank*) — and `syn_core::seq::bank` parses it into a
   `SampleBank`. The console loads it from a file (17.6 MB is too fat to
   embed); the web app fetches it next to the wasm package; Android reads
   it from assets. A host with no bank still plays the synth
   drums; a sample name the bank lacks is silent, never an error — a point
   must open everywhere.

7. **The samples live in their own repository, `synesthesia-samples`.** The
   web app clones the pinned core on every `npm run core`, so 17 MB of
   audio here would be paid on every build. This repository holds the
   *catalogue* — the ordered list of sound names (`seqSounds` in
   `assets/schema.json`) the choice gene indexes — and a test that a bank's
   manifest covers it. [synesthesia-samples](https://github.com/dmitryweiner/synesthesia-samples)
   (made 2026-10-15) holds the FLACs, `manifest.json`, `provenance.md`, the
   pick (`picks.json`) and the tools that made them (`tools/cut.py`,
   `listen.py`, `build_bank.py`); apps pin it by revision the way they pin
   the core, and `build_bank.py` rebuilds `bank.bin` byte for byte from a
   revision. Tests here need no real audio: the sampler's tests
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
weights a step. The third Euclidean number rotates **right**, as Strudel's
mini-notation does (it calls `euclidRot`, `rotate(b, −r)`): `(3,8,2)` is
`x.x..x..`; Tidal's `(3,8,2)` rotates left, so a Tidal pattern with a
rotation sounds different here — as it does in Strudel. One group mixes
`,` or `|`, not both. Limits, each an error at parse: 512 bytes, brackets
16 deep, factors and step counts ≤ 128, and time rescaled ≤ 2²⁴ root to
leaf (with the engine's 2⁻²⁰-cycle grid that keeps every fraction inside
`i64`). `.` as a group separator and `_` elongation come with
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

- **Sampler voice**: a decoded sample, a position, rate = `speed ·
  entry_sr / sr` — each sample has its own rate (24, 32 or 48 kHz) — so the
  22 050 Hz scout render and the 48 000 Hz live render play the same pitch, linear interpolation, 2 ms attack, 2 ms release when stolen or
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
bank.bin = "SYNB" | u32 version = 2 | u32 manifest_len | manifest JSON (UTF-8)
         | the FLAC streams (mono, 16-bit), back to back      — integers little-endian
manifest = { "version": 2, "sounds": { "owl": [
             { "file": "samples/owl/00.flac", "sr": 24000, "frames": 51234,
               "offset": 0, "bytes": 61234, "sha256": "…16 hex…", "mode": "hits",
               "src": "bigsoundbank", "page": "https://…", "author": "…",
               "license": "CC0", "title": "…", "trim": "1.20-3.30s", "cand": "…" }, … ],
             "crickets": [ … ] } }
```

`offset` counts from the first byte after the manifest. Names are sorted;
a name's variants are in the order the curator's pick listed them (`owl:1`
is the second entry). `mode` is what the sample was cut for — `hits` (one
event, peak-normalized to −1 dBFS, 2 ms in / 30 ms out) or `bed` (a loop
with its 1 s equal-power crossfade already baked in, RMS −20 dBFS); a layer
may still play a bed sample as a hit and the other way round.

The samples repository's `tools/cut.py` cuts candidates from the raw
downloads, `tools/listen.py` makes the listening page, and
`tools/build_bank.py` turns the pick into `samples/<name>/<nn>.flac`, the
manifest, `provenance.md` and `bank.bin`, choosing each sample's rate by
its spectrum (S0 below). Decoding needs `claxon` in `syn-core`.

The 42 names of the first bank (S4), which `seqSounds` starts from:
`bell blackbird bongo bottle-blow bottle-clink canary chimes cicada conga
cowbell creak crickets crow darbuka dove elk fire frame-drum frog goldfinch
great-tit hand-bell kookaburra lyrebird mark-trees night nightingale oriole
owl pendulum pigeon potoo rain robin shaker sleigh-bells slit-drum stream
triangle whale wings woodblock`. None collides with the synth drums
(`bd sd hh oh cp`).

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
  bank.rs        SampleBank from bytes (v2: FLAC per sample, claxon), decoded
                 per sound on demand; the manifest; the in-memory test bank
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

synesthesia-samples         samples/<name>/<nn>.flac · manifest.json · provenance.md ·
                            picks.json · sources/ · tools/{cut,listen,build_bank}.py
synesthesia-rust (console)  bump the rev; bank.bin loaded from a file, the TUI
                            `seq` line, config, the listening pass
```

## Phases

Each phase ends green on `./scripts/check.sh` and with its numbers written
into this file.

- **S0 — probes (throwaway).** Before any port:
  - ✅ the pick, trimmed: **bytes** as 16-bit PCM 48 kHz, as 22 kHz, as
    FLAC → decided decision 6's format and that the console loads the bank
    (*S0, measured* below);
  - ✅ bank parse cost against the apps' startup budgets (the console's is
    300 ms to first sound; the worklet compiles the module first, then
    would parse the bank) → decode per sound, on demand;
  - open: one sampler voice and 16 voices in a tight loop: **ns per sample** next
    to the generators' worst (2.4 µs for Fractal garden), native and wasm;
  - **the score probe**: `syn-bench` on *Fractal garden* and *Bell spots*
    alone, with crickets under them, and with `bd(3,8)` — the fractality
    band is what the scout steers by, and a steady beat may push every
    candidate with a rhythm out of it (or into it). The number decides
    whether layers are scored as part of the candidate, left out of the
    scout's render, or get a term of their own.
- **S1 — the pattern engine** (`seq/mini.rs`, `pattern.rs`): ✅ 2026-10-15.
  The subset above except `{ }` (S6), exact fractions (`i64`, `i128` only
  inside an operation), the tree as an arena (a Euclidean rhythm shares its
  step), the fixed-array query (64 events, overflow counted), `?` and `|`
  by `unit_hash(time, salt, seed)`, `Pattern::fast/slow/degrade_by`. The
  tests: Strudel's documented examples as event lists, `fast(2)` ≡ `*2`,
  and a query cut into random grid-aligned pieces equals the whole, every
  event inside its span. `syn-bench --patterns` (*S1, measured* below).
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
- **S4 — the samples repository**: ✅ in synesthesia-samples `78a6508` (2026-10-15):
  the curated pick (446 candidates listened to, 165 kept under the 42 names
  in *The bank*), the manifest with provenance, the tools, `provenance.md`.
  Left for this repository: the catalogue (`seqSounds`, the 42 names in
  that order, appended to from then on) and the test that a bank's manifest
  covers it.
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

On the console side (synesthesia-rust, on the core since 2026-10-15 with
`syn-audio` left as the device): **C1** — bump to S3's revision, load the
bank from `$XDG_DATA_HOME` (S0 says it is too fat to embed), `--no-samples`,
the TUI `seq` line, `samples = true` in config, the `verify` ladder with the
heaviest layered preset — 0 xruns in 30 min is the bar (`soak.py`, or
`soak.py --null` on a machine without PipeWire).

## Budget (thresholds; checked by the bench, filled in as measured)

| metric | target | measured |
|---|---|---|
| bank | ≤ 20 MB in the samples repository, ≤ 12 MB embedded in the console binary | 165 samples picked (468 s): **17.6 MB** as FLAC at a measured rate per sample — fits the repository, not the console embed (S0 below) |
| bank parse | ≤ 50 ms native; the worklet's load gate unchanged | **148 ms** to decode all of it with `claxon` on an M1 Max — 3× over; decode lazily (S0 below) |
| one sampler voice | ≤ 50 ns a sample native (a `sin` is 24.6 ns on the console's A76) | S0 (open; needs S2's voice) |
| 16 voices + a 4-layer pattern query | ≤ 3 % of one core on top of the point, native; inside PLAN-CORE's 35 % quantum gate on the phones | S2 |
| allocations in `render` | 0 | S2 (test) |
| event array overflow | 0 on every preset at 48 kHz and 22.05 kHz | S2 (bench counter) |
| offline render of a layered preset | ≥ 8× realtime on the console | C1 |
| xruns, drum preset, 48 kHz / 1024 frames | 0 in 30 min | C1 (`soak.py`) |
| genes | 279; the first 250 unchanged for a point without `seq`; `fixtures/points.json` unchanged | S3 (tests) |
| fractality of the presets with layers | inside the presets' band like the others, or the scout's rule is amended by S0's number | S0 / S5 |

### S0, measured (2026-10-15)

The pick was made by ear over 446 cut candidates (synesthesia-samples
`picks.json`): **165 samples under 42 names, 468 s** — 296 s of hits, 172 s
of beds (18 loops of 10 s). That is ~2.5× the 40–60 the plan assumed, so
the format numbers decided the format:

| bank as | MB |
|---|---|
| 16-bit PCM, 48 kHz (decision 6 as written) | 44.7 |
| 16-bit PCM, 24 kHz | 22.4 |
| FLAC, 48 kHz | 22.4 |
| FLAC, 24 kHz | 12.6 |
| **FLAC, rate per sample by measurement (chosen)** | **17.6** |

- **The format changes (amends decision 6, with the user's choice).**
  `bank.bin` v2 carries one FLAC stream per sample, and each manifest
  entry has its own `sr`, `frames`, `offset`, `bytes`. A sample's rate is
  measured, not set by hand: more than −30 dB of its energy above 15.5 kHz
  → 48 kHz, above 11 kHz → 32 kHz, else 24 kHz (112 × 24, 24 × 32, 29 × 48
  kHz: bells, sleigh bells, cicada, fire, rain keep their air; birds,
  whales, elk do not pay for it). The voice's rate is `speed · entry_sr /
  sr`, per entry. Decoding needs `claxon` (pure Rust, Apache-2.0) in
  `syn-core` — the dependency decision this file left open.
- **Noise does not compress.** Fire, crickets and rain are near noise;
  the beds are ~37 % of the seconds and most of the bytes. Rate tiers took
  the bank from 22.4 to 17.6 MB; reaching 12 MB would take shorter beds
  (6 s loops ≈ −3 MB) or fewer of them. So the console **loads the bank
  from a file** (the plan's fallback), and the web app may want a smaller
  bank later.
- **Decode cost: 148 ms** for the whole bank (15.8 M frames) with
  `claxon`'s block API on an M1 Max (the per-sample iterator: 179 ms); the
  console's A76 will be slower. Decoded, the bank is 31.5 MB as `i16`
  (63 MB as `f32`). So the core must not decode the whole bank up front:
  decode a sound when a point that uses it is set (on the control side,
  never in `render`), keep it as `i16`, and let the scout's render share
  the decoded sounds. That is S2's design input.
- Not measured yet: one sampler voice and 16 voices (S0 item 3), the
  score probe (item 4). Both need the core's sampler, which is S1–S2 work.

### S1, measured (2026-10-15)

`syn-bench --patterns`: one query per 128-frame block over an hour of
cycles, the span put on the grid as the engine will, M1 Max, native:

| pattern | ns / query, cps 0.5 · 2 | events/s at cps 2 |
|---|---|---|
| `0` | 45 · 21 | 2 |
| `bd sd [hh hh] ~` | 275 · 248 | 8 |
| `0(3,8) , ~ sd ~ sd , hh*8?0.3` | 390 · 369 | 21 |
| `[[0 [1 2]] <3 4 [5 6]>]*2 , 0(7,16) , hh*16?` | 1037 · 1045 | 47 |

The heaviest is ~1 µs of a 2.67 ms block: four such layers are 0.15 % of
one core at 48 kHz (at 22.05 kHz, 1.2 µs of 5.8 ms). No overflow at either
rate. Not measured: wasm, where the `i128` products are libcalls — S2's
voice probe measures both on the worklet build.

## Risks and open questions

- **The scout's metric.** A steady beat is the opposite of 1/f; S0's score
  probe comes first for a reason. If the score drops for every rhythm, the
  options are: score the candidate without its layers (they are the user's
  choice, not the search's), or add a rhythm-aware term — chosen by the
  number, not here.
- **Licences are per file, not per site.** BigSoundBank states CC0 on each
  page and Freesound's filter is reliable, but the manifest records what
  the page said on the day, and each page was opened. Two traps met in S4:
  the same Yellowstone elk recordings are public domain at nps.gov and
  CC BY-NC at the Acoustic Atlas (taken from nps.gov), and Commons'
  `En-humpback whale.ogg` is a spoken article, not a whale.
- **Four apps, one shape.** Appending genes and an optional section is the
  compatible way, but the Android and web apps will carry a `seq`-less
  point until they bump; a point saved from them loses its layers. Known
  and accepted — the same happened with `delay_shimmer`.
- **The bank in wasm memory.** The whole bank decoded is 31.5 MB as `i16`
  (63 MB as `f32`), so the worklet decodes only the sounds the point uses
  and keeps them as `i16`; its memory grows when a new sound is decoded and
  the worklet remakes its view then (it already handles growth). The web
  app may still want a smaller bank (shorter beds, fewer variants) — a
  second build of the same samples repository, not a fork.
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
