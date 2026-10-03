# TODO

Agreed follow-ups that are not done yet. Remove an item when it is done.

## One spelling of the point's name in all three apps

The web app writes and reads a point's name as `presetName`, like every
other key of `AppState` (`masterGain`, `filterOn`, …). synesthesia-rust's
`AppState` spelled it `preset_name`, so the name was dropped when a web
point was read and written under a key the web does not know. Fixed in
`syn-core/src/state.rs` (2026-10-03): it is `presetName` now, and
`preset_name` is still *accepted* through a serde `alias`, so points the
console saved before the fix keep their names.

To finish:

1. synesthesia-rust depends on this repository instead of its own copy of
   `syn-core`, so the console writes `presetName` too.
2. The Android app writes only through this core (already true).
3. Once no file written with the old spelling is expected any more (the
   console's `last-point.json` and `points.json`), remove the `alias` and
   the test that reads the old spelling, so all three apps — web, console,
   Android — have exactly one spelling.
