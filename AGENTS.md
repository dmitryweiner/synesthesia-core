# AGENTS.md — project map and working rules

(Claude Code reads CLAUDE.md, which points here.)

The shared model of Synesthesia and its **specification** (synesthesia
PLAN-CORE.md C2, since 2026-10-05): used by the Android app, being moved
under the web app (`syn-wasm`, branch `core` there), and meant for the
console app (which still has its own copy — TODO.md) and an iOS app.
A new preset, generator, LFO shape or FX parameter is made here first. Docs, UI strings and code comments are in English;
the user talks to agents in Russian.

## Commands

```bash
rustup update stable    # first: CI uses the latest stable, and a newer clippy finds more
./scripts/check.sh      # after every change: fmt, clippy -D warnings, tests
cargo run --release -p syn-bench -- …   # the sound bench (below)
```

**The sound bench** (`syn-bench`, synesthesia PLAN-CORE.md C6) measures
the actual sound, as the web app's `analyze.mjs` did: `cargo run --release
-p syn-bench -- [--preset 0,3] [--secs 30 --sr 22050] [--mutants N]
[--random N] [--repeat N] [--character --ref 0,3,5,6,8] [--wav DIR]
[--png DIR] [--json FILE]`, or one of `--onsets`, `--switch 0,3,10 --at 20`,
`--configs 30@22050,24@11025`. The header of `syn-bench/src/main.rs` says
what each prints. An agent cannot hear: read the `--png` waterfall.

## Rules

- **No I/O, no threads, no clocks in `syn-core` or `syn-session`.** The caller
  supplies time, files, devices and thread pools (the scout runs inside
  whatever rayon pool the caller installs — `syn_session::ScoutPool` builds
  one). This is what makes the core portable: `syn-session` takes `now` as an
  argument and hands work back as effects.
- **`syn-ffi` is thin.** It converts types; logic goes into `syn-core`
  (the model), `syn-player` (the live render side) or `syn-session` (the
  control logic). Its one exception is written down where it lives: the
  session's scout job waits in `syn-ffi` for a thread the app brings, and is
  run without the session locked.
  Every change to its surface changes the generated Kotlin and Swift, so
  name things for an app developer, and keep records plain.
- **Never change the `AppState` shape or the gene order** — point files, the
  web app and every native app depend on both.
- **`assets/` is the source** (presets, schema, FX presets): edit it here,
  by hand, with its tests (`syn-core/tests/presets.rs` holds what every
  preset must be). Nothing re-dumps it any more. A preset that uses a new
  generator, LFO shape or FX parameter needs the model to have it first, or
  it loads and plays with that instrument silently missing.
- **`golden/` is frozen**: the 69 takes the TypeScript rendered are what
  "unchanged" means for a generator. Never re-render or hand-edit them.
- **`fixtures/` is the TypeScript's behaviour, frozen** (synesthesia
  PLAN-CORE.md phase 1) by `scripts/dump-points.mjs` / `dump-analysis.mjs`
  while the TypeScript model still exists; after the swap nothing can
  re-dump it. A fixture failure is
  a real change of behaviour (`points.json`: a shared link would open
  differently or get another id). Never hand-edit it.
- **Golden failures are real**: a generator stopped being bit-exact with the
  TypeScript. Never loosen the tolerance.
- **Never write an oscillator as `sin(2π·f·t)` with absolute `t`** —
  accumulate phase.
- **The web runs the render path as wasm, which has no fma and a slow
  big-argument `sin`** (synesthesia PLAN-CORE.md C12): no `hypot` or
  `mul_add` per sample or per bin (libm emulates the fma — `hypot` alone was
  20 % of the web render), no transcendental per partial where a recurrence
  or an identity gives it, and phases wrapped long before |x| ≈ 1.6e6.
  Measure a change in wasm too (`node --cpu-prof` on a `--keep-debug`
  wasm-bindgen build names the functions).
- **Never allocate, lock or log on the render path** (`Engine::render`).
  `syn_player::Player::render_into` takes one uncontended lock (only the
  audio thread uses it) and `try_lock`s the frame ring, so a reader can
  cost a frame, never the sound. Crossing UniFFI allocates the returned
  buffer once per call — the price of the FFI, paid outside the engine.
- An app depends on a pinned revision. After pushing a change here, bump
  the revision in the app deliberately and run its checks
  (synesthesia-android: `rev` in `core/rust/Cargo.toml`, then its
  `scripts/check.sh`; its AGENTS.md shows a local `[patch]` for trying a
  change before pushing).
- **`syn-wasm` is thin too** — `syn-ffi`'s rules, for the web app
  (synesthesia PLAN-CORE.md C3). Its audio side (`AudioCore`) runs in an
  AudioWorklet: numbers and byte arrays only (no strings — some worklet
  scopes have no `TextDecoder`), samples and frames handed back as offsets
  into the module's memory, nothing allocated per quantum. `check.sh`
  clippies it for `wasm32-unknown-unknown` and runs its tests under node.
- Measure first: a performance claim comes with a number from a bench.

## Module map

```
syn-core/src/dsp/         23 generators (bit-exact with the TS), gate, mulberry32,
                          tanpura (four KS strings and the jawari buzz)
syn-core/src/modmatrix.rs LFOs as pure functions of time (sine … S&H, and pink:
                          1/f value noise); routes onto fx, formulas and cards
syn-core/src/fx/          the DSP chain (biquads, comb, chorus, phaser, delay
                          with the octave-up shimmer in its loop, FDN reverb,
                          limiter)
syn-core/src/analysis/    fft, fractal (the scout's score), and the web app's bench
                          metrics: character, clicks, a log spectrogram, the picture's
                          coverage/edges/change — each
                          checked against the TypeScript (fixtures/analysis.json)
syn-core/src/fx_presets.rs ⚙ Settings' effect-module presets (assets/fx-presets.json)
syn-core/src/settings_page.rs the web app's ⚙ Settings as a model (assets/settings-page.json
                          + the rules: formula limit, filter rows per type, route
                          targets and new routes, same_point); settings.rs is the
                          generated, gene-by-gene view of the same point
syn-core/src/engine.rs    Engine: render blocks, features, onset hits, time
syn-core/src/features.rs  AnalyserNode emulation → AudioFeatures, onsets
syn-core/src/genome/      codec, evolve, explorer, scout
syn-core/src/sim/         the picture: field, noise, fields, advect, palette,
                          coupling, display, frame, picture (the CPU one);
                          driver (what each frame does, for a renderer that
                          owns its field — the GPU's and the CPU's alike),
                          quality (the ladder and the boot probe)
syn-core/src/state.rs     AppState v1 and the built-in presets (assets/presets.json)
syn-core/src/point.rs     a point from outside made safe (the web's sanitizeState +
                          stateToAppState), its canonical JSON and id — byte for
                          byte the TypeScript's (fixtures/points.json)
syn-core/src/schema.rs    the schema (assets/schema.json): ranges, defaults, genes
syn-core/src/settings.rs  the Settings page, derived from it: sections, their
                          titles, a choice's options, and the point being
                          edited (Edit). No label is written by hand
syn-core/src/share.rs     `#s=` tokens, and what a link opens (parse_launch)
syn-player/src/lib.rs     Player: commands in, whole-block rendering, PCM out in
                          any chunk size, fades, frames stamped on the engine
                          clock and looked up by the played time
syn-session/src/lib.rs    Session: the explorer and the 2 s morph, the scout's
                          scheduling and its pool, the name and step count,
                          the status line. Pure; host tests pin main.ts's feel
syn-session/src/points.rs the kept points and their file — the console's, so
                          a point file opens in both; the naming rules
syn-ffi/src/lib.rs        the UniFFI surface: presets, schema, SoundPlayer
syn-ffi/src/session.rs    … the session: effects, view, the scout job
syn-ffi/src/picture.rs    … the picture: a frame's uniforms, the seed spots,
                          the quality rung, the sizes
syn-ffi/src/points.rs     … and the points: the kept list, `#s=` tokens, what
                          a link opens
syn-wasm/src/             the wasm-bindgen surface for the web app: AudioCore (the
                          player, for one AudioWorklet), session, picture, settings,
                          point (sanitize, id — the points Worker), share (links)
syn-bench/src/main.rs     the sound bench: fractality, character, onsets, clicks
                          at switches, render configs, WAVs and waterfalls
```
