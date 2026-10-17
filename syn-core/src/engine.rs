//! The sound engine: generators → gate → mix (+ the sequencer's layers) →
//! FX → (+ its dry layers) → master.
//!
//! Pure and blocking-free — it owns no device and no thread. `syn-audio` calls
//! [`Engine::render`] from the audio callback; the scout calls
//! [`render_offline`] from a worker thread.
//!
//! **Stereo** (PLAN-BINAURAL.md): an engine built with [`Engine::new_stereo`]
//! renders two ears with [`Engine::render_stereo`] — every formula slot and
//! every layer through its own head model ([`crate::spatial`]) at its
//! azimuth × the point's width, the chain as two ears ([`FxChain::new_stereo`]),
//! the binaural beat's shift — and analyses (L + R)/2 before the shift. An
//! engine is built in one mode and never computes both; [`Engine::render`]
//! stays exactly today's mono path.

use crate::dsp::gate::{apply_gate, gate_is_silent};
use crate::dsp::generator::FormulaGenerator;
use crate::dsp::rng::{Mulberry32, Rng};
use crate::features::{Analyser, AudioFeatures, FeatureTracker, OnsetDetector, FPS};
use crate::fx::{FxChain, Space};
use crate::modmatrix::{effective_param, lfo_value, LfoDef, ModRoute};
use crate::schema::{formula_ranges, schema};
use crate::seq::sequencer::SeqLive;
use crate::seq::{SampleBank, SeqProgram, SeqState, Sequencer, LAYERS};
use crate::spatial::HeadModel;
use crate::state::{AppState, BinState, FxState, AZ};
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

/// What only a stereo engine has: the right ear, the analysis tap, a head
/// model per source, and the layers' buses.
struct StereoBus {
    right: Vec<f32>,
    /// (L + R)/2 before the beat's shift — what the analysis hears.
    mid: Vec<f32>,
    /// One per formula slot, in slot order.
    heads: Vec<HeadModel>,
    /// Per layer: its wet bus's and its dry bus's.
    layer_heads: Vec<[HeadModel; 2]>,
    /// The layers, one bus each ([`Sequencer::render_buses`]).
    wet: Vec<f32>,
    dry: Vec<f32>,
    dry_l: Vec<f32>,
    dry_r: Vec<f32>,
}

impl StereoBus {
    fn new(sr: f64, slots: usize, n: usize) -> StereoBus {
        StereoBus {
            right: vec![0.0; n],
            mid: vec![0.0; n],
            heads: (0..slots).map(|_| HeadModel::new(sr)).collect(),
            layer_heads: (0..LAYERS).map(|_| [HeadModel::new(sr), HeadModel::new(sr)]).collect(),
            wet: vec![0.0; LAYERS * n],
            dry: vec![0.0; LAYERS * n],
            dry_l: vec![0.0; n],
            dry_r: vec![0.0; n],
        }
    }

    fn fit(&mut self, n: usize) {
        if self.right.len() < n {
            for b in [&mut self.right, &mut self.mid, &mut self.dry_l, &mut self.dry_r] {
                b.resize(n, 0.0);
            }
            self.wet.resize(LAYERS * n, 0.0);
            self.dry.resize(LAYERS * n, 0.0);
        }
    }

    fn clear(&mut self) {
        for h in self.heads.iter_mut().chain(self.layer_heads.iter_mut().flatten()) {
            h.clear();
        }
    }
}

/// Adds `x` from azimuth `theta` into the ears — or, when it is silent, lets
/// the head model rest (its lines would hold stale sound otherwise).
fn place(head: &mut HeadModel, x: &[f32], theta: f64, l: &mut [f32], r: &mut [f32]) {
    if x.iter().all(|v| *v == 0.0) {
        if !head.is_centred() {
            head.clear();
        }
        return;
    }
    head.process(x, theta, l, r);
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
    /// Built by [`Engine::new_stereo`] only.
    stereo: Option<Box<StereoBus>>,
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
            stereo: None,
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

    /// An engine for two ears (PLAN-BINAURAL.md): [`Engine::render_stereo`].
    pub fn new_stereo(sr: f64, state: &AppState, seed: u32) -> Self {
        let mut e = Engine::new(sr, state, seed);
        e.fx = FxChain::new_stereo(sr);
        e.fx.reset(&state.audio.fx);
        e.stereo = Some(Box::new(StereoBus::new(sr, e.slots.len(), BLOCK)));
        e
    }

    pub fn is_stereo(&self) -> bool {
        self.stereo.is_some()
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
        if let Some(st) = &mut self.stereo {
            st.clear();
        }
        self.master = state.audio.master_gain;
        for slot in &mut self.slots {
            slot.gen.reset();
            slot.fade = if slot.enabled { 1.0 } else { 0.0 };
            slot.gain =
                if slot.enabled { self.state.audio.formulas.get(slot.id).map_or(0.0, gain_of) } else { 0.0 };
        }
    }

    /// Renders one block (mono). `out.len()` should be [`crate::BLOCK`].
    ///
    /// A stereo engine renders its two ears and writes their downmix — the
    /// engine never computes both modes; a host that wants mono builds a
    /// mono engine.
    pub fn render(&mut self, out: &mut [f32]) {
        if let Some(st) = &mut self.stereo {
            let n = out.len();
            st.fit(n);
            let mut right = std::mem::take(&mut st.right);
            self.render_stereo(out, &mut right[..n]);
            for (o, r) in out.iter_mut().zip(&right[..n]) {
                *o = 0.5 * (*o + *r);
            }
            if let Some(st) = &mut self.stereo {
                st.right = right;
            }
            return;
        }
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

    /// Renders one block for two ears (PLAN-BINAURAL.md). On a mono engine
    /// it renders mono into both.
    pub fn render_stereo(&mut self, l: &mut [f32], r: &mut [f32]) {
        let Some(mut st) = self.stereo.take() else {
            self.render(l);
            r.copy_from_slice(l);
            return;
        };
        self.render_two(&mut st, l, r);
        self.stereo = Some(st);
    }

    fn render_two(&mut self, st: &mut StereoBus, l: &mut [f32], r: &mut [f32]) {
        let n = l.len();
        if self.scratch.len() < n {
            self.scratch.resize(n, 0.0);
            self.dry.resize(n, 0.0);
        }
        st.fit(n);
        l.fill(0.0);
        r.fill(0.0);

        let dt = n as f64 / self.sr;
        let gain_coef = 1.0 - (-dt / GAIN_SMOOTH).exp();
        let master_coef = 1.0 - (-dt / MASTER_SMOOTH).exp();
        let (routes, lfos) = (&self.state.modulation.routes, &self.state.modulation.lfos);
        let space = modulate_bin(&self.state.bin, routes, lfos, self.t);
        // Degrees of azimuth → radians, scaled by the width.
        let spread = space.width.clamp(0.0, 1.0) * std::f64::consts::PI / 180.0;

        for (slot, head) in self.slots.iter_mut().zip(&mut st.heads) {
            let snap = self.state.audio.formulas.get(slot.id);
            let target = if slot.enabled { snap.map_or(0.0, gain_of) } else { 0.0 };
            slot.gain += gain_coef * (target - slot.gain);

            if gate_is_silent(slot.fade, slot.enabled) {
                if !head.is_centred() {
                    head.clear();
                }
                continue;
            }
            let buf = &mut self.scratch[..n];
            slot.gen.fill(buf);
            slot.fade = apply_gate(buf, slot.fade, slot.enabled);
            let g = slot.gain as f32;
            for s in buf.iter_mut() {
                *s *= g;
            }
            let az = snap.and_then(|s| s.params.get(AZ)).copied().unwrap_or(0.0);
            let az = modulate_az(az, slot.id, routes, lfos, self.t);
            head.process(buf, az * spread, l, r);
        }

        let seq_live = !self.seq.is_idle();
        let live = modulate_seq(&self.state.seq, routes, lfos, self.t);
        if seq_live {
            let (wet, dry) = (&mut st.wet[..LAYERS * n], &mut st.dry[..LAYERS * n]);
            wet.fill(0.0);
            dry.fill(0.0);
            self.seq.render_buses(&live, wet, dry, LAYERS);
            st.dry_l[..n].fill(0.0);
            st.dry_r[..n].fill(0.0);
            for (li, [hw, hd]) in st.layer_heads.iter_mut().enumerate() {
                let theta = live.layers[li].az * spread;
                let bus = li * n..(li + 1) * n;
                place(hw, &st.wet[bus.clone()], theta, l, r);
                place(hd, &st.dry[bus], theta, &mut st.dry_l[..n], &mut st.dry_r[..n]);
            }
        } else {
            self.seq.advance(n, live.cps);
        }

        modulate_fx_into(&self.state.audio.fx, routes, lfos, self.t, &mut self.fx_eff);
        self.fx.set(&self.fx_eff);
        let mid = &mut st.mid[..n];
        self.fx.process_stereo(l, r, mid, space);
        if seq_live {
            for (((a, b), m), (dl, dr)) in
                l.iter_mut().zip(r.iter_mut()).zip(mid.iter_mut()).zip(st.dry_l.iter().zip(&st.dry_r))
            {
                *a += *dl;
                *b += *dr;
                *m += 0.5 * (*dl + *dr);
            }
        }

        let master_target = self.state.audio.master_gain;
        self.master += master_coef * (master_target - self.master);
        let master = self.master as f32;
        for ((a, b), m) in l.iter_mut().zip(r.iter_mut()).zip(mid.iter_mut()) {
            *a *= master;
            *b *= master;
            *m *= master;
        }

        self.t += dt;

        if self.analyser.push(mid) {
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

fn gene_range(id: &str) -> [f64; 2] {
    let g = crate::genome::genes::gene_by_id(id).expect("a gene of the schema");
    [g.min, g.max]
}

/// The ranges the LFOs move the layers' numbers in: their genes'.
fn seq_ranges() -> &'static [[f64; 2]; 5] {
    static R: std::sync::OnceLock<[[f64; 2]; 5]> = std::sync::OnceLock::new();
    R.get_or_init(|| ["seq.cps", "seq.0.gain", "seq.0.speed", "seq.0.density", "seq.0.az"].map(gene_range))
}

/// The ranges of the space's numbers: an azimuth, the width, the beat.
fn bin_ranges() -> &'static [[f64; 2]; 3] {
    static R: std::sync::OnceLock<[[f64; 2]; 3]> = std::sync::OnceLock::new();
    R.get_or_init(|| ["a.additive.az", "bin.width", "bin.beat"].map(gene_range))
}

/// A formula's azimuth at time `t`, degrees: its own, with every route aimed
/// at it laid over. Allocates nothing.
pub fn modulate_az(base: f64, formula: &str, routes: &[ModRoute], lfos: &[LfoDef], t: f64) -> f64 {
    let mut az = base;
    for r in routes.iter().filter(|r| r.param == AZ && r.target == formula) {
        if let Some(lfo) = lfos.get(r.src) {
            az = effective_param(base, lfo_value(lfo, t), r.depth, bin_ranges()[0], r.exp);
        }
    }
    az
}

/// The point's space at time `t` (`bin` · width, beat), with the LFOs laid
/// over it. Allocates nothing — it runs every block.
pub fn modulate_bin(bin: &BinState, routes: &[ModRoute], lfos: &[LfoDef], t: f64) -> Space {
    let [_, width, beat] = *bin_ranges();
    let mut s = Space { width: bin.width, beat_on: bin.on, beat: bin.beat };
    for r in routes.iter().filter(|r| r.target == "bin") {
        let Some(lfo) = lfos.get(r.src) else { continue };
        let at = |base: f64, range: [f64; 2]| effective_param(base, lfo_value(lfo, t), r.depth, range, r.exp);
        match r.param.as_str() {
            "width" => s.width = at(bin.width, width),
            "beat" => s.beat = at(bin.beat, beat),
            _ => {}
        }
    }
    s
}

/// The layers' numbers at time `t`: the point's, with every route aimed at
/// them laid over (`seq` · cps, `seq.<i>` · gain, speed, density).
/// Allocates nothing — it runs every block.
pub fn modulate_seq(seq: &SeqState, routes: &[ModRoute], lfos: &[LfoDef], t: f64) -> SeqLive {
    let mut live = SeqLive::of(seq);
    let [cps, gain, speed, density, az] = *seq_ranges();
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
            "az" => l.az = at(base.az, az),
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

/// [`render_offline_with`] for two ears (PLAN-BINAURAL.md): `(left, right)`.
pub fn render_offline_stereo(
    state: &AppState,
    bank: Option<&SampleBank>,
    seconds: f64,
    sr: f64,
    seed: u32,
) -> (Vec<f32>, Vec<f32>) {
    let n = (seconds * sr) as usize;
    let mut engine = Engine::new_stereo(sr, state, seed);
    engine.set_seq(Box::new(SeqProgram::new(&state.seq, bank)));
    let len = n.div_ceil(BLOCK) * BLOCK;
    let (mut l, mut r) = (vec![0.0f32; len], vec![0.0f32; len]);
    for (a, b) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)) {
        engine.render_stereo(a, b);
    }
    l.truncate(n);
    r.truncate(n);
    (l, r)
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

    /// PLAN-BINAURAL.md decision 5: the stereo engine is a strict superset.
    /// At width 0 with the beat off both ears are the mono render — bit for
    /// bit where the point has no layers, to float precision where it has
    /// (each layer is summed on its own bus) — and so is what it analyses.
    #[test]
    fn at_width_0_both_ears_are_the_mono_render() {
        let sr = 22050.0;
        for p in presets() {
            // Its azimuths kept: at width 0 they must not be heard.
            let state = AppState { bin: BinState::default(), ..p.state.clone() };
            let mono = render_offline_with(&state, None, 3.0, sr, 1);
            let (l, r) = render_offline_stereo(&state, None, 3.0, sr, 1);
            assert_eq!(l, r, "{}: L ≡ R", p.name);
            if p.state.seq.is_silent() {
                assert_eq!(l, mono, "{}: bit for bit", p.name);
            } else {
                let worst = l.iter().zip(&mono).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
                assert!(worst <= 1e-6, "{}: {worst:e}", p.name);
            }
        }
    }

    #[test]
    fn the_stereo_engine_analyses_what_the_mono_one_does() {
        let sr = 48000.0;
        let p = &presets()[10].state;
        let (mut a, mut b) = (Engine::new(sr, p, 3), Engine::new_stereo(sr, p, 3));
        let (mut x, mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
        for _ in 0..(sr as usize * 3 / BLOCK) {
            a.render(&mut x);
            b.render_stereo(&mut l, &mut r);
        }
        assert_eq!(a.features(), b.features());
        assert_eq!(a.spectrum(), b.spectrum());
        assert_eq!(a.hits(), b.hits());
        // The beat moves the ears, not the analysis: it reads the sum before
        // the shift.
        let beating = AppState { bin: BinState { on: true, beat: 7.0, width: 0.0 }, ..p.clone() };
        let mut c = Engine::new_stereo(sr, &beating, 3);
        for _ in 0..(sr as usize * 3 / BLOCK) {
            c.render_stereo(&mut l, &mut r);
        }
        assert_eq!(a.features(), c.features());
        assert_ne!(l, r, "the ears beat");
    }

    fn wide(p: &AppState, width: f64, az: &[(&str, f64)]) -> AppState {
        let mut s = p.clone();
        for (id, a) in az {
            s.audio.formulas.get_mut(*id).expect("a formula").params.insert(AZ.into(), *a);
        }
        s.bin.width = width;
        s
    }

    /// The lag (samples) at which `l` best matches `r`: positive when the
    /// left ear hears it later.
    fn lag(l: &[f32], r: &[f32]) -> i64 {
        (-40i64..=40)
            .max_by(|a, b| {
                let c = |k: i64| -> f64 {
                    (40..l.len() - 40).map(|i| f64::from(l[(i as i64 + k) as usize]) * f64::from(r[i])).sum()
                };
                c(*a).total_cmp(&c(*b))
            })
            .unwrap_or(0)
    }

    /// Whale coral's one low FM voice: below the head's shadow the ears
    /// tell its side by time (the ITD), a little by level. At low
    /// frequencies the shadow filter's phase adds to Woodworth's delay, as a
    /// real head's does (Kuhn: about 1.5× at the bottom); the impulse test in
    /// `spatial` pins τ itself.
    #[test]
    fn a_voice_at_an_azimuth_reaches_the_near_ear_first_and_width_0_takes_it_back() {
        let sr = 22050.0;
        let p = &presets()[2].state;
        let energy = |x: &[f32]| x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>();
        let tau = (crate::spatial::itd(80f64.to_radians()) * sr).round() as i64;
        let (l, r) = render_offline_stereo(&wide(p, 1.0, &[("fm", 80.0)]), None, 3.0, sr, 1);
        let late = lag(&l, &r);
        assert!((tau..=2 * tau).contains(&late), "the left ear {late} samples late, Woodworth {tau}");
        assert!(energy(&r) > 1.1 * energy(&l), "right {} left {}", energy(&r), energy(&l));
        let (l, r) = render_offline_stereo(&wide(p, 1.0, &[("fm", -80.0)]), None, 3.0, sr, 1);
        assert_eq!(lag(&l, &r), -late, "the mirror image");
        assert!(energy(&l) > 1.1 * energy(&r));
        let (l, r) = render_offline_stereo(&wide(p, 0.0, &[("fm", -80.0)]), None, 3.0, sr, 1);
        assert_eq!(l, r);
        assert_eq!(l, render_offline(p, 3.0, sr, 1), "an azimuth at width 0 is the mono point");
    }

    #[test]
    fn an_lfo_on_an_azimuth_and_on_the_width_moves_them_without_clicks() {
        use crate::analysis::clicks::detect_clicks;
        use crate::modmatrix::{LfoShape, ModState};
        let sr = 22050.0;
        let mut s = wide(&presets()[2].state, 0.6, &[("fm", 0.0)]);
        s.modulation.lfos[0] = LfoDef { shape: LfoShape::Sine, rate: 0.7, phase: 0.0 };
        s.modulation.lfos[1] = LfoDef { shape: LfoShape::Triangle, rate: 0.3, phase: 0.0 };
        let route = |src, target: &str, param: &str| ModRoute {
            src,
            target: target.into(),
            param: param.into(),
            depth: 0.5,
            exp: false,
        };
        s.modulation.routes = vec![route(0, "fm", "az"), route(1, "bin", "width")];
        let (l, r) = render_offline_stereo(&s, None, 6.0, sr, 1);
        let unrouted =
            AppState { modulation: ModState { routes: Vec::new(), ..s.modulation.clone() }, ..s.clone() };
        let still = render_offline_stereo(&unrouted, None, 6.0, sr, 1);
        assert_ne!(l, still.0, "the voice moves");
        for (ear, x) in [("left", &l), ("right", &r)] {
            let clicks = detect_clicks(x, sr);
            assert!(clicks.is_empty(), "{ear}: {} clicks", clicks.len());
        }
        // The engine's own numbers, unrouted and routed.
        assert_eq!(modulate_az(30.0, "fm", &[], &s.modulation.lfos, 1.0), 30.0);
        let moved: Vec<f64> = (0..8)
            .map(|k| modulate_az(30.0, "fm", &s.modulation.routes, &s.modulation.lfos, k as f64 * 0.3))
            .collect();
        assert!(moved.iter().all(|a| (-90.0..=90.0).contains(a)) && moved.iter().any(|a| *a != 30.0));
        let w = modulate_bin(&s.bin, &s.modulation.routes, &s.modulation.lfos, 0.8);
        assert!(w.width != 0.6 && (0.0..=1.0).contains(&w.width) && !w.beat_on);
    }

    #[test]
    fn switching_the_beat_on_fades_it_in() {
        use crate::analysis::clicks::detect_clicks;
        let sr = 22050.0;
        let p = presets()[4].state.clone(); // Aurora
        let mut e = Engine::new_stereo(sr, &p, 1);
        let blocks = (sr as usize * 2) / BLOCK;
        let (mut l, mut r) = (vec![0.0f32; blocks * BLOCK], vec![0.0f32; blocks * BLOCK]);
        for (k, (a, b)) in l.chunks_mut(BLOCK).zip(r.chunks_mut(BLOCK)).enumerate() {
            if k == blocks / 2 {
                e.set_state(&AppState { bin: BinState { on: true, beat: 10.0, width: 0.0 }, ..p.clone() });
            }
            e.render_stereo(a, b);
        }
        assert!(detect_clicks(&l, sr).is_empty() && detect_clicks(&r, sr).is_empty());
        let half = blocks / 2 * BLOCK;
        assert_eq!(l[..half], r[..half]);
        assert_ne!(l[half + 4096..], r[half + 4096..]);
    }

    #[test]
    fn a_stereo_engine_asked_for_mono_gives_the_downmix() {
        let sr = 22050.0;
        let s = wide(&presets()[2].state, 1.0, &[("fm", 60.0)]);
        let (l, r) = render_offline_stereo(&s, None, 1.0, sr, 1);
        let mut e = Engine::new_stereo(sr, &s, 1);
        let mut x = vec![0.0f32; l.len().div_ceil(BLOCK) * BLOCK];
        for b in x.chunks_mut(BLOCK) {
            e.render(b);
        }
        for i in 0..l.len() {
            assert_eq!(x[i], 0.5 * (l[i] + r[i]));
        }
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
