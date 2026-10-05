# The picture's shaders

GLSL ES 3.00 (`#version 300 es`, `texelFetch`, float targets): WebGL2 in
the web app and OpenGL ES 3.0 on Android run them verbatim. They live here,
next to `syn-core/src/sim/driver.rs`, which sets their uniforms
(synesthesia PLAN-CORE.md C10). An app takes them from its pinned revision:
the web app gets them inside the `syn-wasm` package its
`scripts/build-core.mjs` builds; Android copies them with its
`scripts/sync-shaders.sh`.

Until the web app's swap (PLAN-CORE.md phase 9) its `src/sim/shaders/` is a
second copy, and its `tests/coreShaders.test.ts` fails if the two differ.
Change a shader here first.
