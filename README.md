# Synesthesia — core

The portable model of [Synesthesia](https://github.com/dmitryweiner/synesthesia):
one point in a ~500-gene space makes sound (21 formula generators, an FX
chain, 4 shared LFOs) and a picture (Gray–Scott reaction-diffusion driven by
the sound), and 👍/👎 steer the search through it.

It is Rust with no I/O and no threads of its own, and it is shared by every
native app:

| app | how it uses the core |
|---|---|
| [synesthesia-android](https://github.com/dmitryweiner/synesthesia-android) | through `syn-ffi` (UniFFI → Kotlin), pinned by git revision |
| [synesthesia-rust](https://github.com/dmitryweiner/synesthesia-rust) (console) | still its own copy of `syn-core`, where it was born; switching to this repository is planned ([TODO.md](TODO.md)) |
| iOS (planned) | through `syn-ffi` (UniFFI → Swift) |

The web app stays the specification: ranges, defaults, presets and the gene
list are **dumped** from it (`assets/`), and the generators are checked
against its output sample by sample (`golden/`).

## Layout

```
syn-core/   the model: generators, modulation, FX, engine, features,
            analysis, the point, the genome, the search, the scout, the
            CPU picture (`sim/`). No I/O, no threads, deterministic.
syn-player/ the render side every app shares: the engine behind a command
            queue, PCM out in any chunk size, fades, feature frames on the
            played clock
syn-ffi/    the foreign interface (UniFFI): records, functions and the live
            player an app calls; the Kotlin and Swift bindings are
            generated from it
assets/     presets.json, schema.json, genomes.json — dumped from the web app
golden/     63 reference takes of the 21 generators, rendered by the web app
scripts/    check.sh; dump-presets.mjs and dump-golden.mjs (need ../synesthesia)
TODO.md     agreed follow-ups that are not done yet
```

## Checking it

```bash
rustup update stable    # CI uses the latest stable; an older clippy misses its lints
./scripts/check.sh      # fmt --check, clippy -D warnings, every test
```

- **The 21 generators are diffed sample by sample** against the 63 golden
  takes; all match within 1e-6.
- **The genome is the web app's genome**: every preset encodes to the same
  237 genes to 1.1e-16.
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

## History

`syn-core`, `assets/` and `golden/` were moved here from synesthesia-rust
with their history (`git filter-repo`); the decisions behind them are in
that repository's PLAN.md and GRAPHICS.md. The decisions behind this split
are in synesthesia-android's PLAN.md (decisions 1 and 2).

## License

GPL-3.0, see [LICENSE](LICENSE).
