// Freezes the web app's sound measurements into fixtures/analysis.json
// (synesthesia PLAN-CORE.md phase 1, C6): character, clicks and the log
// spectrogram, on signals both sides can make bit for bit — each preset's
// dry generator mix (enabled formulas in key order, chaotic ones left out,
// seeded 1, 2, …, LFO routes applied, summed at the master gain), and the same mix with steps
// added for the click detector. The core's ports must agree.
//
//   node scripts/dump-analysis.mjs
//
// Needs ../synesthesia, like the other dumps.
import { writeFileSync, mkdirSync } from 'node:fs';
import { startServer, launchBrowser, openApp } from '../../synesthesia/scripts/lib.mjs';

const SR = 22050;
// Left out of the mix: a last-bit difference in an LFO grows without bound
// in a chaotic map (logistic diverges by 0.24 after 17 s), and what is
// checked here is the measurement, not the generators (golden/ does those).
const CHAOTIC = ['logistic', 'lorenz', 'rossler'];
const SECONDS = 24;

const server = await startServer(false);
const browser = await launchBrowser();
try {
  const page = await (await browser.newContext()).newPage();
  await openApp(page, `${server.BASE}/?preset=0&res=128`);
  const cases = await page.evaluate(async ({ SR, SECONDS, CHAOTIC }) => {
    const { PRESETS } = await import('/src/presets.ts');
    const { FormulaGenerator, FORMULA_IDS } = await import('/src/dsp/generator.ts');
    const { buildModPayload } = await import('/src/audio/modrouting.ts');
    const { FORMULAS } = await import('/src/schema/audio.ts');
    const { mulberry32 } = await import('/src/dsp/rng.ts');
    const { analyzeCharacter } = await import('/src/analysis/character.ts');
    const { detectClicks } = await import('/src/analysis/clicks.ts');
    const { logSpectrogram } = await import('/src/analysis/spectrogram.ts');

    const dryMix = (state) => {
      const n = Math.round(SR * SECONDS);
      const mix = new Float32Array(Math.ceil(n / 128) * 128);
      let seed = 1;
      for (const id of Object.keys(state.audio.formulas).sort()) {
        const snap = state.audio.formulas[id];
        if (!snap.enabled || CHAOTIC.includes(id)) continue;
        const fid = FORMULA_IDS.find((f) => f === id);
        const gen = new FormulaGenerator(fid, SR, snap.params, mulberry32(seed++));
        const pay = buildModPayload(state.mod, fid, FORMULAS);
        gen.setMod(pay.lfos, pay.routes, pay.ranges);
        const buf = new Float32Array(128);
        for (let i = 0; i < mix.length; i += 128) {
          gen.fill(buf);
          for (let j = 0; j < 128; j++) mix[i + j] += buf[j] * state.audio.masterGain;
        }
      }
      return mix.subarray(0, n);
    };
    const finite = (x) => (Number.isFinite(x) ? x : null);
    return PRESETS.map((p, index) => {
      const x = dryMix(p.state);
      const c = analyzeCharacter(x, SR);
      // a step of +0.25 every 5 s, from 2.5 s: discontinuities to find
      const stepped = Float32Array.from(x, (v, i) => v + 0.25 * Math.floor((i / SR + 2.5) / 5));
      const spec = logSpectrogram(x, SR, { columns: 40, rows: 48 });
      return {
        index,
        name: p.name,
        character: Object.fromEntries(Object.entries(c).map(([k, v]) => [k, typeof v === 'number' ? finite(v) : v])),
        clicks: detectClicks(x, SR),
        clicksStepped: detectClicks(stepped, SR),
        spectrogram: { columns: spec.columns, rows: spec.rows, db: Array.from(spec.db), loudness: Array.from(spec.loudness) },
      };
    });
  }, { SR, SECONDS, CHAOTIC });
  mkdirSync(new URL('../fixtures/', import.meta.url), { recursive: true });
  writeFileSync(new URL('../fixtures/analysis.json', import.meta.url),
    `{"note":"Frozen from the web app (scripts/dump-analysis.mjs): the dry mix (no chaotic formulas) of each preset at ${SR} Hz for ${SECONDS} s → character, clicks (plain and with a +0.25 step every 5 s from 2.5 s), a 40×48 log spectrogram. Never hand-edit.","sr":${SR},"seconds":${SECONDS},"cases":[\n${cases.map((c) => JSON.stringify(c)).join(',\n')}\n]}\n`);
  console.log(`${cases.length} presets measured`);
} finally {
  await browser.close();
  server.stop();
}
