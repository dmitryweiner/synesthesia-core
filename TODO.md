# TODO

Agreed follow-ups that are not done yet. Remove an item when it is done.

- **Samples and pattern scripts** — PLAN-SAMPLES-SCRIPTS.md (agreed
  2026-10-14): recorded CC0 sounds and Strudel-style rhythms as a `seq`
  section of the point with its genes appended. Done: S0–S4 (the
  pattern engine, bank, voices, sequencer, the point with 283 genes,
  synesthesia-samples' 165 samples), S5 (six presets with layers, chosen
  by ear over eight listening rounds; drums that breathe, `humanize`),
  S7 (the web app and Android carry the bank and edit the layers in ⚙
  Settings; both on core `6c7840f`), the flash guard (`sim::calm`).
  Next: C1 on the console (the bank from a file, the TUI `seq` line);
  S8, a metric for the layers fitted to the user's better/worse verdicts;
  pictures of their own for the six layered presets; `humanize` as a
  gene, if the search should vary it; decoding a sound per variant if the
  A76 makes a bed's 8 variants (39 ms on an M1) felt.
  **First:** synesthesia-android's `SearchTest` flakes on the API 35
  emulator since the bank (2 of 5 CI runs; its PLAN.md "Open"): if the
  cause is `syn_player`'s layers lock held while a program is built (a
  decode), the fix is here — build the program outside the lock.
- **Binaural sound** — PLAN-BINAURAL.md (agreed 2026-10-15): a stereo
  mode beside mono, sources around the head (structural head model),
  binaural beats by single-sideband shift, all as genes. Done: B0–B4 in
  the core (313 genes; `Player::new_stereo`, `renderStereo`,
  `SoundPlayer.newStereo`; `syn-bench --stereo`). Next: the apps bump —
  the console (`channels = 2`, the `verify` ladder), the worklet
  (`outputChannelCount: [2]`), the first phone measurement of the stereo
  engine (Android carries it since 2026-10-17: stereo on headphones, mono
  on the speaker; its PLAN.md "Binaural"); the web Settings page's
  Binaural section (`settings-page.json` · `binaural`); B5, the presets'
  azimuths, widths and a beat or two by ear on headphones.
