# TODO

Agreed follow-ups that are not done yet. Remove an item when it is done.

## One spelling of the point's name: the console

**Decided with the user (2026-10-03):** the point's name is `preset_name`
in every app. Done in the core on 2026-10-06 (synesthesia PLAN-CORE.md
phase 10): every point is written with `preset_name`; `presetName` is still
read, **for good** — old `#s=` links and the points Worker's rows carry it
and cannot be rewritten. The Worker's id is hashed over the old spelling
(`point::point_id`), so a point keeps the id it always had (decided with
the user 2026-10-06). The web app and the Android app run on this.

Left:

1. **Console**: synesthesia-rust depends on this repository instead of its
   own copy of `syn-core`.
