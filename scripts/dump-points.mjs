// Freezes the web app's point handling into fixtures/points.json (and the
// status line's "what changed" into fixtures/changes.json)
// (synesthesia PLAN-CORE.md phase 1, C5): for a corpus of inputs — the
// presets, random genomes, mutated and broken JSON, old point shapes — what
// the TypeScript's sanitizeState + stateToAppState make of each, its
// canonical JSON and its point id (SHA-256 → 10 base62). The core's
// `point` module must reproduce all three, so old links keep their ids once
// the TypeScript is gone.
//
//   node scripts/dump-points.mjs [--extra points.json]
//
// --extra: a read-only export of the points Worker's D1 (wrangler's --json
// output of SELECT id, body FROM points): each stored body joins the corpus
// as kind "d1", and must keep every value it was stored with (C5: old links
// keep working — the Worker serves a stored body by its id).
// Needs ../synesthesia (its dev server and Playwright are reused), and runs
// only while its TypeScript model still exists — after the swap the
// fixture is frozen.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { startServer, launchBrowser, openApp } from '../../synesthesia/scripts/lib.mjs';

const argv = process.argv.slice(2);
const extraAt = argv.indexOf('--extra');
// `wrangler d1 execute … --json --command "SELECT id, body FROM points"`'s
// output as it is: [{ results: [{ id, body }] }]
const stored = extraAt >= 0 ? JSON.parse(readFileSync(argv[extraAt + 1], 'utf8')).flatMap((r) => r.results ?? []) : [];
const extra = stored.map((r) => JSON.parse(r.body));

const server = await startServer(false);
const browser = await launchBrowser();
try {
  const page = await (await browser.newContext()).newPage();
  await openApp(page, `${server.BASE}/?preset=0&res=128`);
  const cases = await page.evaluate(async (extraInputs) => {
    const { PRESETS } = await import('/src/presets.ts');
    const { sanitizeState, stateToAppState, defaultAppState } = await import('/src/state/schema.ts');
    const { canonicalJson, presetIdOf } = await import('/src/state/canonical.ts');
    const { decodeGenome, genomeLength } = await import('/src/genome/codec.ts');

    // mulberry32, so the corpus is the same on every run
    let seed = 20261005;
    const rnd = () => {
      seed = (seed + 0x6d2b79f5) | 0;
      let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
    const pick = (xs) => xs[Math.floor(rnd() * xs.length)];
    const clone = (x) => JSON.parse(JSON.stringify(x));

    const inputs = [];
    const add = (kind, input) => inputs.push({ kind, input });

    for (const p of PRESETS) add('preset', clone(p.state));
    add('default', defaultAppState());
    for (let i = 0; i < 40; i++) {
      add('genome', decodeGenome(Array.from({ length: genomeLength() }, () => rnd())));
    }

    // Values a hostile or old point may carry where a number, a flag or a
    // name belongs. JSON only: what JSON.parse can produce.
    const odd = [null, true, false, 'x', '', [], {}, [1], -1, 0, -0, 1e21, 1e-7, 1.5e300, -1e300,
      0.1 + 0.2, 123456789.123456789, 5e-324, 2 ** 53 + 1, 'sine', 'lowpass', 'Ünïcødé — "q" \\ \n\t😀'];
    // Every path to a leaf in an object, as key lists.
    const leaves = (o, path = [], out = []) => {
      if (o !== null && typeof o === 'object') {
        for (const k of Object.keys(o)) leaves(o[k], [...path, k], out);
      } else out.push(path);
      return out;
    };
    const setAt = (o, path, v) => {
      let x = o;
      for (const k of path.slice(0, -1)) x = x[k];
      x[path[path.length - 1]] = v;
    };
    const delAt = (o, path) => {
      let x = o;
      for (const k of path.slice(0, -1)) x = x[k];
      if (Array.isArray(x)) x.splice(Number(path[path.length - 1]), 1);
      else delete x[path[path.length - 1]];
    };
    const bases = [...PRESETS.map((p) => p.state), defaultAppState()];
    for (let i = 0; i < 160; i++) {
      const s = clone(pick(bases));
      const n = 1 + Math.floor(rnd() * 6);
      for (let j = 0; j < n; j++) {
        const ls = leaves(s);
        const path = pick(ls);
        const r = rnd();
        if (r < 0.55) setAt(s, path, pick(odd));
        else if (r < 0.7) delAt(s, path);
        else if (r < 0.8) setAt(s, path, (rnd() - 0.3) * 10 ** Math.floor(rnd() * 8 - 2));
        else if (r < 0.9 && path.length > 1) {
          // replace a whole branch
          setAt(s, path.slice(0, -1), pick(odd));
        } else {
          // an unknown key beside a known one
          setAt(s, [...path.slice(0, -1), pick(['zz', 'formula99', 'fooOn', '__proto__x', 'v2'])], pick(odd));
        }
      }
      add('mutated', s);
    }

    // Routes and LFOs, which have their own rules (index-stable LFOs,
    // routes dropped one by one, depth clamped).
    for (let i = 0; i < 30; i++) {
      const s = clone(pick(bases));
      s.mod = s.mod ?? { lfos: [], routes: [] };
      const r = rnd();
      if (r < 0.25) s.mod.lfos = s.mod.lfos.slice(0, Math.floor(rnd() * 6)).concat(rnd() < 0.5 ? [pick(odd)] : []);
      else if (r < 0.5) s.mod.lfos = [...s.mod.lfos, { shape: 'sine', rate: 1, phase: 0 }, { shape: 'nope', rate: 1, phase: 0 }];
      s.mod.routes = [
        ...s.mod.routes,
        { src: Math.floor(rnd() * 7) - 1, target: 'fx', param: 'filterFreq', depth: (rnd() - 0.5) * 4 },
        { src: 0, target: pick(['fx', 'additive', 'reaction', 'palette', 'nope']), param: pick(['filterFreq', 'fund', 'N', 'feed', 'shift', 'gain', 'x']), depth: rnd(), exp: pick([true, false, 'yes', 1]) },
        { src: 1.5, target: 'fx', param: 'reverbMix', depth: 0.3 },
        pick(odd),
      ];
      add('mod', s);
    }

    // Old shapes: before shimmer, before the explicit couplings, before mod,
    // the console's preset_name, and a name that needs escaping.
    for (const p of PRESETS.slice(0, 6)) {
      const s = clone(p.state);
      delete s.audio.fx.delayShimmer;
      add('old', clone(s));
      for (const k of ['loudToPulse', 'onsetToFlash', 'onsetToSeed', 'spectrumToTint']) delete s.coupling[k];
      add('old', clone(s));
      delete s.mod;
      delete s.coupling;
      add('old', clone(s));
      const named = clone(p.state);
      delete named.presetName;
      named.preset_name = 'from the console';
      add('old', named);
    }
    add('old', { ...clone(PRESETS[0].state), presetName: 'Tab\tquote" backslash\\ \u0001   émoji 🎛' });

    // Not points at all.
    for (const x of [null, 0, 'str', [], [1, 2], {}, { audio: [] }, { audio: { formulas: [] } },
      { visual: null }, { mod: { lfos: {}, routes: [] } }, { coupling: [0.5] }, { v: 2 }]) add('broken', x);

    for (const x of extraInputs) add('d1', x);

    const out = [];
    for (const { kind, input: raw } of inputs) {
      // what a point is on the wire: JSON (drops non-index keys on arrays,
      // -0 → 0), so the fixture's input is exactly what was sanitized
      const input = JSON.parse(JSON.stringify(raw));
      const partial = sanitizeState(input);
      const state = partial ? stateToAppState(partial) : null;
      const canonical = state ? canonicalJson(state) : null;
      out.push({ kind, input, canonical, id: canonical ? await presetIdOf(canonical) : null });
    }
    return out;
  }, extra);

  // What the status line says changed between two points: pairs of genomes
  // (random ones, presets, and mutations of both, as 👍/👎/🎲 make them),
  // the TypeScript's diffSummary, and main.ts's describeChange line.
  const changes = await page.evaluate(async () => {
    const { PRESETS } = await import('/src/presets.ts');
    const { encodeGenome } = await import('/src/genome/codec.ts');
    const { mutate, randomGenome, diffSummary } = await import('/src/genome/evolve.ts');
    const { mulberry32 } = await import('/src/dsp/rng.ts');
    const rng = mulberry32(77);
    // main.ts describeChange(), verbatim
    const describe = (from, to) => {
      const changes = diffSummary(from, to);
      if (changes.length === 0) return 'nothing changed';
      const arrow = { up: '↑', down: '↓', on: 'on', off: 'off', switch: '⇄' };
      const top = changes.slice(0, 5).map((c) => `${c.label} ${arrow[c.dir]}`);
      const more = changes.length > 5 ? ` +${changes.length - 5} more` : '';
      return top.join(' · ') + more;
    };
    const starts = [...PRESETS.map((p) => encodeGenome(p.state)), ...Array.from({ length: 15 }, () => randomGenome(rng))];
    const out = [];
    for (const a of starts) {
      const pairs = [
        a,
        mutate(a, rng, { k: 3, sigma: 0.08, structuralProb: 0 }),
        mutate(a, rng, { k: 8, sigma: 0.25, structuralProb: 1 }),
        randomGenome(rng),
      ];
      for (const b of pairs) {
        out.push({ a, b, changes: diffSummary(a, b).map((c) => ({ id: c.id, dir: c.dir })), line: describe(a, b) });
      }
    }
    return out;
  });

  mkdirSync(new URL('../fixtures/', import.meta.url), { recursive: true });
  writeFileSync(new URL('../fixtures/changes.json', import.meta.url),
    `{"note":"Frozen from the web app (scripts/dump-points.mjs): genome pairs → diffSummary → main.ts's describeChange. Never hand-edit.","cases":[\n${changes.map((c) => JSON.stringify(c)).join(',\n')}\n]}\n`);
  console.log(`${changes.length} change lines`);
  const note = 'Frozen from the web app (scripts/dump-points.mjs): input → sanitizeState + stateToAppState → canonical JSON → id. Never hand-edit.';
  // one case per line: a diff names the case that moved
  writeFileSync(new URL('../fixtures/points.json', import.meta.url),
    `{"note":${JSON.stringify(note)},"cases":[\n${cases.map((c) => JSON.stringify(c)).join(',\n')}\n]}\n`);
  // A stored point must come back with every value it was stored with. The
  // schema has grown since some were stored (new formulas, delayShimmer), so
  // re-sanitizing may ADD fields at their defaults — then re-sharing it gives
  // a new id, while the stored link keeps working (the Worker serves the
  // stored body by its id and never recomputes it).
  const leaves = (o, p = '', out = {}) => {
    if (o !== null && typeof o === 'object') for (const k of Object.keys(o)) leaves(o[k], `${p}/${k}`, out);
    else out[p] = o;
    return out;
  };
  const d1 = cases.filter((c) => c.kind === 'd1');
  const changed = [];
  let grown = 0;
  d1.forEach((c, i) => {
    const before = leaves(JSON.parse(stored[i].body));
    const after = leaves(JSON.parse(c.canonical ?? 'null'));
    const lost = Object.keys(before).filter((k) => after[k] !== before[k]);
    if (lost.length) changed.push(`${stored[i].id}: ${lost.slice(0, 3).join(', ')}`);
    if (c.id !== stored[i].id) grown++;
  });
  if (changed.length) {
    console.error(`${changed.length} of ${d1.length} stored points change a value when re-sanitized:\n  ${changed.join('\n  ')}`);
    process.exitCode = 1;
  } else if (d1.length) {
    console.log(`${d1.length} stored points keep every value; ${grown} gain fields the schema grew since (a re-share gets a new id)`);
  }
  const kinds = {};
  for (const c of cases) kinds[c.kind] = (kinds[c.kind] ?? 0) + 1;
  console.log(`${cases.length} cases`, kinds, `${cases.filter((c) => !c.id).length} refused`);
} finally {
  await browser.close();
  server.stop();
}
