# AGENTS.md — project map and working rules

(Claude Code reads CLAUDE.md, which points here.)

The shared model of Synesthesia, used by the Android app, and meant for
the console app (which still has its own copy — TODO.md) and an iOS app. Docs, UI strings and code comments are in English;
the user talks to agents in Russian.

## Commands

```bash
rustup update stable    # first: CI uses the latest stable, and a newer clippy finds more
./scripts/check.sh      # after every change: fmt, clippy -D warnings, tests
```

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
- **Never hand-edit `assets/` or `golden/`** — re-dump them from the web app
  (`scripts/dump-*.mjs`, needs `../synesthesia`).
- **Golden failures are real**: a generator stopped being bit-exact with the
  TypeScript. Never loosen the tolerance.
- **Never write an oscillator as `sin(2π·f·t)` with absolute `t`** —
  accumulate phase.
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
- Measure first: a performance claim comes with a number from a bench.

## Module map

```
syn-core/src/dsp/         21 generators (bit-exact with the TS), gate, mulberry32
syn-core/src/modmatrix.rs LFOs as pure functions of time; routes onto fx,
                          formulas and visual cards
syn-core/src/fx/          the DSP chain (biquads, comb, chorus, phaser, delay,
                          FDN reverb, limiter)
syn-core/src/engine.rs    Engine: render blocks, features, onset hits, time
syn-core/src/features.rs  AnalyserNode emulation → AudioFeatures, onsets
syn-core/src/genome/      codec, evolve, explorer, scout
syn-core/src/sim/         the CPU picture: field, noise, fields, advect,
                          palette, coupling, display, frame, picture
syn-core/src/state.rs     AppState v1 and the 12 presets
syn-core/src/schema.rs    the dumped schema
syn-core/src/share.rs     `#s=` tokens
syn-player/src/lib.rs     Player: commands in, whole-block rendering, PCM out in
                          any chunk size, fades, frames stamped on the engine
                          clock and looked up by the played time
syn-session/src/lib.rs    Session: the explorer and the 2 s morph, the scout's
                          scheduling and its pool, the name and step count,
                          the status line. Pure; host tests pin main.ts's feel
syn-ffi/src/lib.rs        the UniFFI surface: presets, schema, SoundPlayer
syn-ffi/src/session.rs    … and the session: effects, view, the scout job
```
