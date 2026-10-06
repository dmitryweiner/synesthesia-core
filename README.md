# Synesthesia — core

The portable model of [Synesthesia](https://github.com/dmitryweiner/synesthesia):
one point in a ~500-gene space makes sound (23 formula generators, an FX
chain, 4 shared LFOs) and a picture (Gray–Scott reaction-diffusion driven by
the sound), and 👍/👎 steer the search through it.

It is Rust with no I/O and no threads of its own, and it is shared by every
native app:

| app | how it uses the core |
|---|---|
| [synesthesia-android](https://github.com/dmitryweiner/synesthesia-android) | through `syn-ffi` (UniFFI → Kotlin), pinned by git revision |
| [synesthesia-rust](https://github.com/dmitryweiner/synesthesia-rust) (console) | still its own copy of `syn-core`, where it was born; switching to this repository is planned ([TODO.md](TODO.md)) |
| iOS (planned) | through `syn-ffi` (UniFFI → Swift) |
| [synesthesia](https://github.com/dmitryweiner/synesthesia) (web) | moving onto the core (its PLAN-CORE.md): through `syn-wasm` (wasm-bindgen), pinned by git revision |

This repository is the **specification** (since 2026-10-05; the web app's
PLAN-CORE.md C2): ranges, defaults, presets and the gene list are written
here (`assets/`), and a new preset or generator is made here first. What
the web app's TypeScript did while it was the specification is frozen:
the generators' takes (`golden/`) and its handling of points, measurements
and status lines (`fixtures/`).

## Layout

```
syn-core/   the model: generators, modulation, FX, engine, features,
            analysis, the point, the genome, the search, the scout, the
            picture (`sim/`: the CPU one, the per-frame driver every
            renderer shares, the quality ladder). No I/O, no threads,
            deterministic.
syn-player/ the render side every app shares: the engine behind a command
            queue, PCM out in any chunk size, fades, feature frames on the
            played clock
syn-session/ the control logic every app shares: 👍 👎 🎲 ↩, the morph, when
            the scout may render, the point's name, the status line. Pure:
            `tick(now)` and commands in, effects out
syn-ffi/    the foreign interface (UniFFI): records, functions, the live
            player, the session and the picture an app calls; the Kotlin
            and Swift bindings are generated from it
syn-wasm/   the same for JavaScript (wasm-bindgen): the web app's
            AudioWorklet, Web Workers and points Worker load it
syn-bench/  the sound bench: render points and measure them (fractality,
            character, onsets, clicks at switches, render configs, PNGs)
assets/     presets.json, schema.json, fx-presets.json — the source, edited here
golden/     63 reference takes of the 21 generators, rendered by the web app
fixtures/   the TypeScript's behaviour, frozen: points (sanitize, canonical
            JSON, ids), status lines, measurements, preset genomes
scripts/    check.sh; dump-points.mjs and dump-analysis.mjs (need ../synesthesia,
            and only until its TypeScript model is deleted)
TODO.md     agreed follow-ups that are not done yet
```

## Checking it

```bash
rustup update stable    # CI uses the latest stable; an older clippy misses its lints
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129   # the version syn-wasm pins
./scripts/check.sh      # fmt --check, clippy -D warnings, every test (syn-wasm's under node)
```

- **The 23 generators are diffed sample by sample** against the 69 golden
  takes; they match to the last bit or two (worst difference 1.6e-16 as of
  2026-10-05, after Additive and Shepard were rewritten for speed).
- **The genome is the web app's genome**: every preset encodes to the same
  250 genes to 1.1e-16.
- **The picture** has no reference samples (the browser seeds it with
  `Math.random()`); its invariants are the contract.

## Using it from an app

An app pins a revision of this repository and builds its own `cdylib` that
links `syn-ffi`; the bindings are generated from that library with the
`uniffi-bindgen` this crate provides (feature `cli`), so the generator and
the scaffolding are always the same UniFFI version:

```toml
[dependencies]
syn-ffi = { git = "https://github.com/dmitryweiner/synesthesia-core", rev = "…" }
```

The Android app's `core/rust/` (`syn-android` + `uniffi-bindgen`) and its
`core/build.gradle.kts` are the worked example.

The web app pins a revision the same way and builds `syn-wasm` with
`wasm-pack build syn-wasm --target web` (its `scripts/build-core.mjs`).

## History

`syn-core`, `assets/` and `golden/` were moved here from synesthesia-rust
with their history (`git filter-repo`); the decisions behind them are in
that repository's PLAN.md and GRAPHICS.md. The decisions behind this split
are in synesthesia-android's PLAN.md (decisions 1 and 2).

## License

GPL-3.0, see [LICENSE](LICENSE).
