// Takes the web app's ⚙ Settings page model into assets/settings-page.json
// (synesthesia PLAN-CORE.md phase 6, C13): the FX modules as the page shows
// them (titles, tags, slider names and steps, choices with their labels),
// which filter rows each filter type uses, the sound → image controls, the
// LFO and route scales, the LFO shapes' labels, the vowels, and what a route
// on each side can aim at. Dumped once, while the TypeScript still holds it;
// from then on the asset is the source, edited here with its tests
// (syn-core/tests/settings_page.rs).
//
//   node scripts/dump-settings.mjs
import { writeFileSync } from 'node:fs';
import { startServer, launchBrowser, openApp } from '../../synesthesia/scripts/lib.mjs';

const server = await startServer(false);
const browser = await launchBrowser();
try {
  const page = await (await browser.newContext()).newPage();
  await openApp(page, `${server.BASE}/?preset=0&res=128`);
  const model = await page.evaluate(async () => {
    const m = await import('/src/ui/settingsModel.ts');
    const audio = await import('/src/schema/audio.ts');
    const state = await import('/src/state/schema.ts');
    const plain = (x) => JSON.parse(JSON.stringify(x));
    const s = state.defaultAppState();
    return {
      fxModules: plain(m.FX_CARDS),
      filterControls: Object.fromEntries(audio.FILTER_TYPES.map((t) => [t, m.filterControls(t)])),
      vowels: [0, 0.25, 0.5, 0.75, 1].map(m.vowelLabel),
      couplingControls: plain(m.COUPLING_CONTROLS),
      lfoShapeLabels: plain(m.LFO_SHAPE_LABELS),
      lfoRate: plain(m.LFO_RATE_SCALE),
      lfoPhase: plain(m.LFO_PHASE_SCALE),
      routeDepth: plain(m.ROUTE_DEPTH_SCALE),
      newRouteDepth: m.newRoute(s, 'picture').depth,
      targetGroups: { sound: plain(m.targetGroups('sound')), picture: plain(m.targetGroups('picture')) },
    };
  });
  writeFileSync(new URL('../assets/settings-page.json', import.meta.url), `${JSON.stringify(model, null, 1)}\n`);
  console.log(`${model.fxModules.length} FX modules, ${model.couplingControls.length} couplings, ${model.targetGroups.sound.length}+${model.targetGroups.picture.length} target groups`);
} finally {
  await browser.close();
  server.stop();
}
