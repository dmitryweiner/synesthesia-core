---
name: new-preset
description: Create a new built-in Synesthesia preset (sound + picture), or retune an existing one, the way Overtone steppe was made — profile the presets people liked, pick one new idea, render drafts side by side with syn-bench, check the picture over minutes in the web app, verify, hand WAVs to the user. Use when the user asks for a new preset, a variation of one, or to change how a preset sounds or looks ("the bass is too rough", "make it brighter", "a darker picture").
---

# Create or retune a preset

The presets live here, in `assets/presets.json` — the core is the
specification (synesthesia PLAN-CORE.md C2), so a preset is made here and
every app (web, Android, console) gets it by bumping its pin. There are no
speakers on this machine: the user's ears are the judge, and the numbers are
how you get to something worth their time. Load `sound-check` too.

## 0. What people like (start here)

The liked family is formula-synth's FX-mod presets: *Fractal garden* (0),
*Molten Polivoks* (3), *Silver maze* (5), *Space breccia* (6), *Loom &
copper* (8); the user calls the style drone / ambient. Profile it with
`syn-bench --character --ref 0,3,5,6,8 --secs 60`: a band that never
breaks (dropout 3.6–6 dB), heavy low end (0.73–0.96 below 200 Hz), one
harmonic grid (0.5–0.9), slow change; four LFOs at mutually irrational
rates (0.017–0.06 Hz sine/triangle plus S&H ~0.03 Hz); a chain of chorus,
phaser, delay, reverb and a limiter. Fractality alone does not describe it.

The user's taste (memory `sound-taste` in the web app's project): bright,
resonant overtone detail on top is loved; the bass round, fat and melodic,
never rough (a strong smooth fundamental under a saw's harmonics, e.g. `dist`
at α ≈ 0.9); attacks sparse and lyrical; slow breathing layers underneath;
a calm piece still needs high, never-repeating iridescence.

## 1. One new idea, and a picture that shares it

Take what the family has and add **one** move it does not make yet; write
it into the preset's name/comment in the commit. Already used: comb moiré,
a peaking glow, a resonant LP sweep, Shepard grids, logistic bifurcations,
FM fans, a Q-30 overtone whistle, the pink LFO, the tanpura, delay shimmer,
the singing bowl. A move that needs a new generator, LFO shape or FX
parameter needs the model first (AGENTS.md) — agree it with the user.

The picture carries the idea through a **shared LFO**: the one that moves
the sound's main gesture also drives a visual parameter.

Hard limits (`syn-core/tests/presets.rs` holds them): ≤ 5 formulas, ≤ 12
routes, every value inside its range, explicit couplings ≥ the floor (1.2),
a visual route or coupling. Two routes on one parameter add up.

## 2. Drafts, rendered side by side

Write drafts as a JSON file (`[{ "name": …, "state": AppState }]`, e.g.
start from a preset in `assets/presets.json`) — nothing in `assets/`
changes until one is chosen:

```bash
cargo run --release -p syn-bench -- --preset 12 --points drafts.json --secs 60 --character --wav shots/wav --png shots/png
```

- **Look at it:** `--png` draws each render's log-frequency waterfall over
  its loudness curve; a whistle, a pluck, a dropout or a build-up is
  obvious there and slow to prove with a metric.
- A draft whose envβ sits near 2 is on the steep side of the preference
  curve: fix the sound, not the measurement.

## 3. Changing one part? Prove the rest stayed

```bash
node .claude/skills/new-preset/bands.mjs shots/wav --f0 55 --keep 600-2400
```

Bass roughness, fundamental share, body, grit, wobble, and the `--keep`
band's Δ dB and correlation against the first file: **0.0 / 1.00 means the
band you promised not to touch did not move.** Renders are deterministic,
so two WAVs of the same length and rate compare exactly.

## 4. Check the idea itself, not only the scores

Test it with an on/off pair (a draft with the idea and one without): the
simplest paired difference beats a clever metric.

## 5. The picture, over minutes (in the web app)

Put the draft into `assets/presets.json` here, then in `../synesthesia`:

```bash
SYN_CORE_DIR=../synesthesia-core npm run core      # its package from this working tree
for i in 12 15; do node scripts/snap.mjs --out shots/p$i.png --preset $i --res 256 --sound --wait 120000 & done; wait
node scripts/analyze.mjs --picture --preset 15 --minutes 5
```

Always `--sound` (the couplings change the colours). It must not die out or
freeze, and it must fill the canvas.

## 6. Final checks

```bash
cargo run --release -p syn-bench -- --character --ref 0,3,5,6,8 --preset 0,3,5,6,8,15   # distance ≲ 1.3
cargo run --release -p syn-bench -- --onsets --preset 15,0      # set loudToPulse so the exposure stays ~0.75–1.4
cargo run --release -p syn-bench -- --switch 11,15,0,15 --at 30 # no click at a switch
./scripts/check.sh                                               # presets.rs, the golden takes, everything
```

The `fixtures/genomes.json` codec check does not move with a new preset
(its states are frozen). Update the preset count where the docs give one.

## 7. Hand it to the user

- Listening WAVs at 44.1 kHz into `shots/<preset>/`: `1-before.wav`,
  `3-after.wav`… The user listens, you can't.
- Once they accept: commit and push here (check.sh green), bump the pin in
  the web app (`package.json` `synesthesiaCore.rev`) and in Android
  (`core/rust/Cargo.toml`), run their checks; record the decision where the
  user keeps them (the web app's PLAN.md), and taste facts in `sound-taste`.
