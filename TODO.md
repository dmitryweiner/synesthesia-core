# TODO

Agreed follow-ups that are not done yet. Remove an item when it is done.

## One spelling of the point's name in all three apps: `preset_name`

**Decided with the user (2026-10-03):** the point's name is `preset_name`
in every app — web, console, Android — and the web app is the one that
changes. (Noted when deciding: every other key of the point is camelCase
in all three apps — `masterGain`, `filterOn`, `lfos` … — so this key will
be the only snake_case one.)

Where it stands now: the web app writes and reads only `presetName`.
synesthesia-rust wrote `preset_name`, so names were lost between the two.
The core (`syn-core/src/state.rs`) currently **writes `presetName` and
reads both**, so that nothing is lost while the web app still knows only
`presetName`.

To finish, in this order:

1. **Web app**: read both spellings, write `preset_name`. Two things to
   decide there:
   - Old `#s=` tokens and points in the cloud database carry `presetName`
     and cannot be rewritten; the web app probably has to keep *reading*
     `presetName` for them for good.
   - The Worker's short-link id is a hash of the canonical JSON
     (`src/state/canonical.ts`), so the same point would get a new id once
     the key changes — either canonicalize the key before hashing, or
     accept new ids.
2. **Core**: swap the serde attributes — `rename = "preset_name"`,
   `alias = "presetName"` — and turn the test round
   (`the_preset_name_is_spelled_as_the_web_app_spells_it`). Bump the core's
   revision in the Android app.
3. **Console**: synesthesia-rust depends on this repository instead of its
   own copy of `syn-core`, so it gets step 2.
4. **Drop the `presetName` alias** in the core once no file written with it
   is expected (the Android app's and the console's saved points), keeping
   whatever the web app decided about old links in step 1.
