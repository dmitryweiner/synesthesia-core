//! The sound engine: generators → gate → mix (+ the sequencer's layers) →
//! FX → (+ its dry layers) → master.
//!
//! Pure and blocking-free — it owns no device and no thread. `syn-audio` calls
//! [`Engine::render`] from the audio callback; the scout calls
//! [`render_offline`] from a worker thread.

use crate::dsp::gate::{apply_gate, gate_is_silent};
use crate::dsp::generator::FormulaGenerator;
use crate::dsp::rng::{Mulberry32, Rng};
use crate::features::{Analyser, AudioFeatures, FeatureTracker, OnsetDetector, FPS};
use crate::fx::FxChain;
use crate::modmatrix::{effective_param, lfo_value, LfoDef, ModRoute};
use crate::schema::{formula_ranges, schema};
use crate::seq::sequencer::SeqLive;
use crate::seq::{SampleBank, SeqProgram, SeqState, Sequencer, LAYERS};
use crate::state::{AppState, FxState};
use crate::{FormulaId, BLOCK};

/// Generator on/off smoothing, seconds (web: `GAIN_SMOOTH`).
const GAIN_SMOOTH: f64 = 0.02;
/// Master gain smoothing, seconds (web: `PARAM_SMOOTH`).
const MASTER_SMOOTH: f64 = 0.05;
/// Bands in the spectrum the engine publishes for the UI to draw.
pub const SPECTRUM_BANDS: usize = 64;

struct Slot {
    id: &'static str,
    gen: FormulaGenerator,
    enabled: bool,
    fade: f32,
    /// Smoothed mixer gain. The browser applies the formula's `gain` twice —
    /// once inside the worklet and once on the node feeding the mix bus — so
    /// the port does the same; halving it here would make every preset quiet.
    gain: f64,
}

pub struct Engine {
    sr: f64,
    slots: Vec<Slot>,
    fx: FxChain,
    state: AppState,
    master: f64,
    /// Absolute time in seconds — the one clock the LFOs and the visualizers
    /// read (PLAN.md decision 7).
    t: f64,
    scratch: Vec<f32>,
    /// The FX state with the LFOs laid over it, rewritten every block.
    fx_eff: FxState,
    /// The sample and drum layers (PLAN-SAMPLES-SCRIPTS.md), and the bus of
    /// the ones that skip the FX chain.
    seq: Sequencer,
    dry: Vec<f32>,

    // The feature bus (PLAN.md decision 5): the same numbers the browser's
    // analyser produced, published for whatever draws them.
    analyser: Analyser,
    tracker: FeatureTracker,
    onsets: OnsetDetector,
    features: AudioFeatures,
    spectrum: [u8; SPECTRUM_BANDS],
    hits: u64,
}

impl Engine {
    pub fn new(sr: f64, state: &AppState, seed: u32) -> Self {
        let mut e = Engine {
            sr,
            slots: Vec::new(),
            fx: FxChain::new(sr),
            state: state.clone(),
            master: state.audio.master_gain,
            t: 0.0,
            scratch: vec![0.0; BLOCK],
            fx_eff: state.audio.fx.clone(),
            seq: Sequencer::new(sr, seed),
            dry: vec![0.0; BLOCK],
            analyser: Analyser::new(sr),
            tracker: FeatureTracker::new(sr),
            onsets: OnsetDetector::default(),
            features: AudioFeatures::default(),
            spectrum: [0; SPECTRUM_BANDS],
            hits: 0,
        };
        let mut seed_n = seed;
        for id in &schema().formula_ids {
            let fid = FormulaId::parse(id).expect("schema id is a formula");
            let snap = state.audio.formulas.get(id);
            let params = snap.map(|s| s.params.clone()).unwrap_or_default();
            let enabled = snap.is_some_and(|s| s.enabled);
            seed_n = seed_n.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let mut gen = FormulaGenerator::new(
                fid,
                sr,
                params,
                Box::new(Mulberry32::new(seed_n)) as Box<dyn Rng + Send>,
            );
            let routes = routes_for(state, id);
            gen.set_mod(&state.modulation.lfos, &routes, &formula_ranges(id));
            e.slots.push(Slot {
                id: fid.as_str(),
                gen,
                enabled,
                fade: if enabled { 1.0 } else { 0.0 },
                gain: if enabled { snap.map_or(0.0, gain_of) } else { 0.0 },
            });
        }
        e.fx.reset(&state.audio.fx);
        // Built here, so the first block does not build it.
        let _ = seq_ranges();
        e
    }

    pub fn sample_rate(&self) -> f64 {
        self.sr
    }

    /// Seconds since the engine started — the LFO clock.
    pub fn time(&self) -> f64 {
        self.t
    }

    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// New target point: parameters glide, nothing is rebuilt. Use it for the
    /// genome morph, where every value moves continuously.
    pub fn set_state(&mut self, state: &AppState) {
        self.state = state.clone();
        for slot in &mut self.slots {
            if let Some(snap) = self.state.audio.formulas.get(slot.id) {
                slot.gen.set(&snap.params);
                slot.enabled = snap.enabled;
            }
            let routes = routes_for(&self.state, slot.id);
            slot.gen.set_mod(&self.state.modulation.lfos, &routes, &formula_ranges(slot.id));
        }
    }

    /// Plays `program`'s layers from the next block (built on the control
    /// side: patterns parsed, samples decoded). Returns the program it
    /// replaces, for the caller to drop off the audio thread.
    pub fn set_seq(&mut self, program: Box<SeqProgram>) -> Box<SeqProgram> {
        self.seq.set_program(program)
    }

    /// The sequencer: its program, cycle and counters.
    pub fn sequencer(&self) -> &Sequencer {
        &self.seq
    }

    /// Hard switch to another point: drops every tail so the old reverb and
    /// delay cannot bleed into the new sound. The caller ducks around it.
    pub fn switch_to(&mut self, state: &AppState) {
        self.set_state(state);
        self.seq.reset(self.t, self.state.seq.cps);
        self.fx.reset(&state.audio.fx);
        self.master = state.audio.master_gain;
        for slot in &mut self.slots {
            slot.gen.reset();
            slot.fade = if slot.enabled { 1.0 } else { 0.0 };
            slot.gain =
                if slot.enabled { self.state.audio.formulas.get(slot.id).map_or(0.0, gain_of) } else { 0.0 };
        }
    }

    /// Renders one block (mono). `out.len()` should be [`crate::BLOCK`].
    pub fn render(&mut self, out: &mut [f32]) {
        let n = out.len();
        if self.scratch.len() < n {
            self.scratch.resize(n, 0.0);
            self.dry.resize(n, 0.0);
        }
        out.fill(0.0);

        let dt = n as f64 / self.sr;
        let gain_coef = 1.0 - (-dt / GAIN_SMOOTH).exp();
        let master_coef = 1.0 - (-dt / MASTER_SMOOTH).exp();

        for slot in &mut self.slots {
            let target =
                if slot.enabled { self.state.audio.formulas.get(slot.id).map_or(0.0, gain_of) } else { 0.0 };
            slot.gain += gain_coef * (target - slot.gain);

            if gate_is_silent(slot.fade, slot.enabled) {
                continue;
            }
            let buf = &mut self.scratch[..n];
            slot.gen.fill(buf);
            slot.fade = apply_gate(buf, slot.fade, slot.enabled);
            let g = slot.gain as f32;
            for (o, s) in out.iter_mut().zip(buf.iter()) {
                *o += *s * g;
            }
        }

        // A point with no layers skips the sequencer, so its sound is bit for
        // bit what it was before there was one.
        let seq_live = !self.seq.is_idle();
        let live =
            modulate_seq(&self.state.seq, &self.state.modulation.routes, &self.state.modulation.lfos, self.t);
        if seq_live {
            let dry = &mut self.dry[..n];
            dry.fill(0.0);
            self.seq.render(&live, out, dry);
        } else {
            self.seq.advance(n, live.cps);
        }

        // FX parameters follow the LFOs at block rate, as they do in the
        // browser (there at ~40 Hz from a timer; here exactly per block).
        modulate_fx_into(
            &self.state.audio.fx,
            &self.state.modulation.routes,
            &self.state.modulation.lfos,
            self.t,
            &mut self.fx_eff,
        );
        self.fx.set(&self.fx_eff);
        self.fx.process(out);
        if seq_live {
            for (o, d) in out.iter_mut().zip(&self.dry[..n]) {
                *o += *d;
            }
        }

        let master_target = self.state.audio.master_gain;
        self.master += master_coef * (master_target - self.master);
        let master = self.master as f32;
        for s in out.iter_mut() {
            *s *= master;
        }

        self.t += dt;

        if self.analyser.push(out) {
            self.features = self.tracker.update(self.analyser.rms, &self.analyser.bytes, 1.0 / FPS);
            self.update_spectrum();
            if self.onsets.update(self.features.onset, self.t) {
                self.hits += 1;
            }
        }
    }

    pub fn limiter_reduction_db(&self) -> f64 {
        self.fx.limiter_reduction_db()
    }

    /// The latest features — loudness, swell, brightness, bands, onset.
    pub fn features(&self) -> AudioFeatures {
        self.features
    }

    /// A log-spaced summary of the spectrum, 0..255 per band.
    pub fn spectrum(&self) -> &[u8; SPECTRUM_BANDS] {
        &self.spectrum
    }

    /// Onset hits since the engine started.
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// Folds the analyser's linear bins into log-spaced bands, the way an ear
    /// (and a spectrum display) groups them.
    fn update_spectrum(&mut self) {
        let bins = &self.analyser.bytes;
        let hz_per_bin = self.sr / 2.0 / bins.len() as f64;
        let (lo_hz, hi_hz) = (40.0, 16000.0f64.min(self.sr / 2.0 * 0.95));
        for (b, out) in self.spectrum.iter_mut().enumerate() {
            let lo = lo_hz * (hi_hz / lo_hz).powf(b as f64 / SPECTRUM_BANDS as f64);
            let hi = lo_hz * (hi_hz / lo_hz).powf((b + 1) as f64 / SPECTRUM_BANDS as f64);
            let i0 = ((lo / hz_per_bin) as usize).min(bins.len() - 1);
            let i1 = (((hi / hz_per_bin).ceil() as usize).max(i0 + 1)).min(bins.len());
            *out = bins[i0..i1].iter().copied().max().unwrap_or(0);
        }
    }
}

fn gain_of(s: &crate::state::FormulaSnapshot) -> f64 {
    s.params.get("gain").copied().unwrap_or(0.0)
}

fn routes_for(state: &AppState, target: &str) -> Vec<ModRoute> {
    state.modulation.routes.iter().filter(|r| r.target == target).cloned().collect()
}

/// Effective FX state at time `t`: the base with every allowlisted field that
/// has a route laid over it.
pub fn modulate_fx(base: &FxState, routes: &[ModRoute], lfos: &[LfoDef], t: f64) -> FxState {
    let mut eff = base.clone();
    modulate_fx_into(base, routes, lfos, t, &mut eff);
    eff
}

/// [`modulate_fx`] into a state kept between blocks, so the render path
/// allocates nothing.
pub fn modulate_fx_into(base: &FxState, routes: &[ModRoute], lfos: &[LfoDef], t: f64, eff: &mut FxState) {
    let ranges = &schema().fx_param_ranges;
    eff.copy_from(base);
    for r in routes.iter().filter(|r| r.target == "fx") {
        let (Some(lfo), Some(range), Some(v)) = (lfos.get(r.src), ranges.get(&r.param), base.get(&r.param))
        else {
            continue;
        };
        eff.set(&r.param, effective_param(v, lfo_value(lfo, t), r.depth, *range, r.exp));
    }
}

/// The ranges the LFOs move the layers' numbers in: their genes'.
fn seq_ranges() -> &'static [[f64; 2]; 4] {
    static R: std::sync::OnceLock<[[f64; 2]; 4]> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        let range = |id: &str| {
            let g = crate::genome::genes::gene_by_id(id).expect("a seq gene");
            [g.min, g.max]
        };
        [range("seq.cps"), range("seq.0.gain"), range("seq.0.speed"), range("seq.0.density")]
    })
}

/// The layers' numbers at time `t`: the point's, with every route aimed at
/// them laid over (`seq` · cps, `seq.<i>` · gain, speed, density).
/// Allocates nothing — it runs every block.
pub fn modulate_seq(seq: &SeqState, routes: &[ModRoute], lfos: &[LfoDef], t: f64) -> SeqLive {
    let mut live = SeqLive::of(seq);
    let [cps, gain, speed, density] = *seq_ranges();
    for r in routes {
        let Some(rest) = r.target.strip_prefix("seq") else { continue };
        let Some(lfo) = lfos.get(r.src) else { continue };
        let at = |base: f64, range: [f64; 2]| effective_param(base, lfo_value(lfo, t), r.depth, range, r.exp);
        if rest.is_empty() {
            if r.param == "cps" {
                live.cps = at(seq.cps, cps);
            }
            continue;
        }
        let Some(i) = rest.strip_prefix('.').and_then(|x| x.parse::<usize>().ok()).filter(|i| *i < LAYERS)
        else {
            continue;
        };
        let (Some(base), l) = (seq.layers.get(i), &mut live.layers[i]) else { continue };
        match r.param.as_str() {
            "gain" => l.gain = at(base.gain, gain),
            "speed" => l.speed = at(base.speed, speed),
            "density" => l.density = at(base.density, density),
            _ => {}
        }
    }
    live
}

/// Renders `seconds` of a point offline, faster than realtime: the formulas
/// and the FX, **not** the sample and drum layers — the scout's render (the
/// layers are the user's choice and are not scored, decided with the user
/// 2026-10-15) and the bench's.
pub fn render_offline(state: &AppState, seconds: f64, sr: f64, seed: u32) -> Vec<f32> {
    render_with(state, Box::default(), seconds, sr, seed)
}

/// [`render_offline`] with the point's layers too, played from `bank` (or,
/// without one, only the synth drums) — what the point sounds like live.
pub fn render_offline_with(
    state: &AppState,
    bank: Option<&SampleBank>,
    seconds: f64,
    sr: f64,
    seed: u32,
) -> Vec<f32> {
    render_with(state, Box::new(SeqProgram::new(&state.seq, bank)), seconds, sr, seed)
}

fn render_with(state: &AppState, program: Box<SeqProgram>, seconds: f64, sr: f64, seed: u32) -> Vec<f32> {
    let n = (seconds * sr) as usize;
    let mut engine = Engine::new(sr, state, seed);
    engine.set_seq(program);
    let mut out = vec![0.0f32; n.div_ceil(BLOCK) * BLOCK];
    for block in out.chunks_mut(BLOCK) {
        engine.render(block);
    }
    out.truncate(n);
    out
}

#[cfg(test)]
mod tests {
    use crate::state::presets;

    use super::*;

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len().max(1) as f64).sqrt()
    }

    #[test]
    fn every_preset_makes_audible_sound_and_stays_in_range() {
        for p in presets() {
            let x = render_offline(&p.state, 2.0, 22050.0, 1);
            let peak = x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let level = rms(&x);
            assert!(x.iter().all(|v| v.is_finite()), "{}: not finite", p.name);
            assert!(peak <= 1.5, "{}: peak {peak}", p.name);
            assert!(level > 1e-4, "{}: silent ({level:e})", p.name);
        }
    }

    #[test]
    fn a_point_with_nothing_enabled_is_silent() {
        let state = AppState::new();
        let x = render_offline(&state, 0.5, 22050.0, 1);
        assert!(x.iter().all(|v| v.abs() < 1e-9));
    }

    #[test]
    fn switching_points_does_not_click() {
        let sr = 48000.0;
        let a = &presets()[0].state;
        let b = &presets()[3].state;
        let mut engine = Engine::new(sr, a, 7);
        let mut buf = vec![0.0f32; BLOCK];
        for _ in 0..200 {
            engine.render(&mut buf);
        }
        let before = buf[BLOCK - 1];
        engine.switch_to(b);
        let mut after = vec![0.0f32; BLOCK];
        engine.render(&mut after);
        // A hard switch drops tails, so the first sample after it must be near
        // silence rather than a step away from the last one.
        assert!(after[0].abs() < 0.2, "step of {} → {}", before, after[0]);
    }

    #[test]
    fn it_publishes_features_while_it_plays() {
        let sr = 48000.0;
        let mut engine = Engine::new(sr, &presets()[10].state, 3);
        let mut buf = vec![0.0f32; BLOCK];
        for _ in 0..(sr as usize * 4 / BLOCK) {
            engine.render(&mut buf);
        }
        let f = engine.features();
        assert!(f.loudness > 0.0 && f.loudness <= 1.0, "loudness {}", f.loudness);
        assert!(f.brightness > 0.0, "brightness {}", f.brightness);
        assert!(engine.spectrum().iter().any(|b| *b > 0), "empty spectrum");
        assert!(engine.hits() > 0, "Bell spots struck no onset in 4 s");
    }

    #[test]
    fn layers_play_through_the_engine_and_dry_ones_skip_the_chain() {
        use crate::seq::{Layer, SeqState};
        let quiet = AppState::new();
        let seq = |dry| SeqState {
            cps: 0.5,
            layers: vec![Layer {
                on: true,
                sound: "bd".into(),
                pattern: "0*4".into(),
                gain: 0.8,
                dry,
                ..Layer::default()
            }],
        };
        let with = |seq: SeqState| AppState { seq, ..quiet.clone() };
        let wet = render_offline_with(&with(seq(false)), None, 2.0, 22050.0, 1);
        let dry = render_offline_with(&with(seq(true)), None, 2.0, 22050.0, 1);
        assert!(rms(&wet) > 0.01 && rms(&dry) > 0.01);
        assert_ne!(wet, dry, "the chain (its reverb, its limiter) colours only the wet one");
        // The scout's render leaves the layers out.
        assert!(render_offline(&with(seq(false)), 2.0, 22050.0, 1).iter().all(|v| *v == 0.0));
        // Every layer off: the formulas' sound alone, bit for bit.
        let p = &presets()[3].state;
        let mut off = seq(false);
        off.layers[0].on = false;
        let a = render_offline(p, 2.0, 22050.0, 1);
        let b = render_offline_with(&AppState { seq: off, ..p.clone() }, None, 2.0, 22050.0, 1);
        assert_eq!(a, b);
    }

    #[test]
    fn an_lfo_moves_a_layer_and_the_tempo() {
        use crate::modmatrix::{LfoShape, ModRoute};
        use crate::seq::{Layer, SeqState};
        let seq = SeqState {
            cps: 1.0,
            layers: vec![Layer { on: true, sound: "hh".into(), pattern: "0*4".into(), ..Layer::default() }],
        };
        let lfos = vec![LfoDef { shape: LfoShape::Square, rate: 0.5, phase: 0.0 }];
        let route = |target: &str, param: &str, exp| ModRoute {
            src: 0,
            target: target.into(),
            param: param.into(),
            depth: 1.0,
            exp,
        };
        let routes =
            vec![route("seq", "cps", true), route("seq.0", "gain", false), route("fx", "delayMix", false)];
        let a = modulate_seq(&seq, &routes, &lfos, 0.1);
        let b = modulate_seq(&seq, &routes, &lfos, 1.1);
        assert_ne!(a.cps, b.cps);
        assert_ne!(a.layers[0].gain, b.layers[0].gain);
        assert_eq!(a.layers[0].speed, 1.0);
        assert!(a.cps >= 0.25 && a.cps <= 2.0 && b.cps >= 0.25 && b.cps <= 2.0);
        // Nothing aimed at the layers: their own numbers.
        assert_eq!(modulate_seq(&seq, &[], &lfos, 0.1), crate::seq::sequencer::SeqLive::of(&seq));
    }

    #[test]
    fn the_lfo_clock_advances_with_the_audio() {
        let mut engine = Engine::new(48000.0, &presets()[0].state, 1);
        let mut buf = vec![0.0f32; BLOCK];
        for _ in 0..375 {
            engine.render(&mut buf);
        }
        assert!((engine.time() - 1.0).abs() < 1e-9, "{}", engine.time());
    }
}
