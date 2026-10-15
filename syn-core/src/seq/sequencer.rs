//! Layers → voices, per block (PLAN-SAMPLES-SCRIPTS.md decisions 8–10).
//!
//! Two halves, on two sides of the command queue:
//!
//! - [`SeqProgram`] is built on the **control side** when the `seq` section
//!   changes: every pattern parsed, every sound looked up — decoded from the
//!   bank, or a synth drum, or silent. That is where the allocation and the
//!   decoding happen.
//! - [`Sequencer`] lives in the engine and plays the current program inside
//!   `render`: it asks each layer's pattern for the events that start in
//!   this block, starts a voice for each at its sample offset, and mixes
//!   the voices and the beds. It allocates nothing — the voices are a fixed
//!   pool, the events a fixed array — and a program it replaces goes back to
//!   the control side to be dropped there.
//!
//! **The clock.** The cycle position advances by `cps · dt` each block —
//! the engine's own blocks, so it is a pure function of the render, the
//! same offline and live. With a constant `cps` from the start it *is*
//! `t · cps`; a hard switch sets it back to `t · cps`. It is integrated
//! rather than computed as `t · cps` because `cps` may glide (a morph) or be
//! modulated, and `t · cps` would then jump — backwards, an hour in.

use super::bank::{SampleBank, Variants};
use super::pattern::{unit_hash, Events, Frac, Pattern};
use super::voice::{Drum, DrumVoice, SampleReader, Source, Voice};
use super::{mini, split_sound, Mode, SeqState, LAYERS};

/// Voices sounding at once; the oldest is stolen, with a 2 ms release.
pub const VOICES: usize = 16;
/// Slots: the voices, and room for the ones being released.
const SLOTS: usize = VOICES + 4;
/// The salt of `density`'s draw (decision 8), apart from the pattern's own.
const DENSITY_SALT: u64 = 0xd3_15_17_7e;
/// The salt of a drum's noise seed.
const NOISE_SALT: u64 = 0x5eed;
/// How fast a bed's level follows its gain (and fades in and out), seconds.
const BED_SMOOTH: f64 = 0.3;
/// `cps` is kept inside this, whatever the point says.
const CPS: (f64, f64) = (0.01, 16.0);

/// A layer's numbers for one block: the point's, with the LFOs laid over
/// them. The program says what a layer plays; these say how loud, how fast,
/// how dense — so a morph glides them without a new program.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveLayer {
    pub gain: f64,
    pub speed: f64,
    pub density: f64,
    pub dry: bool,
}

/// The layers' numbers for one block, as a fixed array: the engine fills it
/// every block without allocating.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeqLive {
    pub cps: f64,
    pub layers: [LiveLayer; LAYERS],
}

impl SeqLive {
    /// The section's own numbers, unmodulated.
    pub fn of(seq: &SeqState) -> SeqLive {
        let mut layers = [LiveLayer { gain: 0.0, speed: 1.0, density: 1.0, dry: false }; LAYERS];
        for (o, l) in layers.iter_mut().zip(&seq.layers) {
            *o = LiveLayer { gain: l.gain, speed: l.speed, density: l.density, dry: l.dry };
        }
        SeqLive { cps: seq.cps, layers }
    }
}

/// What a name plays.
#[derive(Clone, Debug)]
enum Voicing {
    Silent,
    Samples(Variants),
    Drum(Drum),
}

/// Decision 4: a name in the bank is a sample, even `bd`; otherwise one of
/// the synth drums; otherwise silence — never an error, a point must open
/// everywhere.
fn voicing(name: &str, bank: Option<&SampleBank>) -> Voicing {
    if let Some(v) = bank.and_then(|b| b.sound(name)) {
        return Voicing::Samples(v);
    }
    Drum::parse(name).map_or(Voicing::Silent, Voicing::Drum)
}

#[derive(Debug)]
struct LayerProgram {
    pattern: Pattern,
    /// The layer's own sound, and the variant its `sound` names
    /// (`crickets:3`); a bare number `n` in the pattern plays `own_n + n`.
    own: Voicing,
    own_n: u16,
    /// What each name in the pattern plays (`pattern.names()` order).
    names: Vec<Voicing>,
}

/// A `seq` section made ready to play: built on the control side, played by
/// the engine's [`Sequencer`].
#[derive(Debug, Default)]
pub struct SeqProgram {
    seq: SeqState,
    layers: Vec<LayerProgram>,
}

impl SeqProgram {
    /// Parses the patterns and looks up (decoding if need be) the sounds of
    /// the layers that are on. A pattern that does not parse is silence.
    pub fn new(seq: &SeqState, bank: Option<&SampleBank>) -> SeqProgram {
        let layers = seq
            .layers
            .iter()
            .take(LAYERS)
            .map(|l| {
                if !l.on {
                    return LayerProgram {
                        pattern: Pattern::silence(),
                        own: Voicing::Silent,
                        own_n: 0,
                        names: Vec::new(),
                    };
                }
                let (name, own_n) = split_sound(&l.sound);
                let pattern = match l.mode {
                    Mode::Hits => mini::parse(&l.pattern).unwrap_or_default(),
                    Mode::Bed => Pattern::silence(),
                };
                let names = pattern.names().iter().map(|n| voicing(n, bank)).collect();
                LayerProgram { own: voicing(name, bank), own_n, pattern, names }
            })
            .collect();
        SeqProgram { seq: seq.clone(), layers }
    }

    pub fn seq(&self) -> &SeqState {
        &self.seq
    }

    /// Nothing to play: no layer is on.
    pub fn is_silent(&self) -> bool {
        self.seq.is_silent()
    }

    /// What layer `li` loops as a bed, if it is one.
    fn bed(&self, li: usize) -> Option<(&Variants, usize)> {
        let l = self.seq.layers.get(li)?;
        if !l.on || l.mode != Mode::Bed {
            return None;
        }
        match &self.layers.get(li)?.own {
            Voicing::Samples(v) if !v.is_empty() => {
                let idx = usize::from(self.layers[li].own_n) % v.len();
                (!v[idx].data.is_empty()).then_some((v, idx))
            }
            _ => None,
        }
    }
}

/// One looping reader and its level.
#[derive(Clone, Debug, Default)]
struct Bed {
    r: Option<SampleReader>,
    level: f64,
}

/// The sequencer inside the engine.
pub struct Sequencer {
    sr: f64,
    seed: u64,
    program: Box<SeqProgram>,
    voices: Vec<Voice>,
    next_id: u64,
    /// Per layer: the bed playing, and the one fading out after a change.
    beds: [[Bed; 2]; LAYERS],
    bed_coef: f64,
    /// The cycle position at the start of the next block, and the same on
    /// the query grid — one block's end is exactly the next one's start.
    cycle: f64,
    at: Frac,
    events: Events,
    /// Events dropped because a block had more than the array holds.
    pub overflow: u64,
    /// Voices stolen because all [`VOICES`] were sounding.
    pub stolen: u64,
}

/// splitmix64's finalizer, for the per-layer seeds.
fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl Sequencer {
    /// `seed` is the engine's (the point's): with the layer, it picks every
    /// random choice of the patterns and `density`.
    pub fn new(sr: f64, seed: u32) -> Sequencer {
        Sequencer {
            sr,
            seed: u64::from(seed),
            program: Box::default(),
            voices: vec![Voice::idle(); SLOTS],
            next_id: 1,
            beds: Default::default(),
            bed_coef: 1.0 - (-1.0 / (BED_SMOOTH * sr)).exp(),
            cycle: 0.0,
            at: Frac::ZERO,
            events: Events::default(),
            overflow: 0,
            stolen: 0,
        }
    }

    pub fn program(&self) -> &SeqProgram {
        &self.program
    }

    /// The cycle position the next block starts at.
    pub fn cycle(&self) -> f64 {
        self.cycle
    }

    /// Plays `program` from the next block; returns the one it replaces, to
    /// be dropped on the control side. Voices already sounding ring on; a
    /// bed whose sample changed fades out under the new one.
    pub fn set_program(&mut self, program: Box<SeqProgram>) -> Box<SeqProgram> {
        let old = std::mem::replace(&mut self.program, program);
        for li in 0..LAYERS {
            let same = match (self.program.bed(li), &self.beds[li][0].r) {
                (Some((v, idx)), Some(r)) => std::sync::Arc::ptr_eq(v, &r.v) && idx == r.idx,
                (None, None) => true,
                _ => false,
            };
            if same {
                continue;
            }
            let [cur, fading] = &mut self.beds[li];
            *fading = std::mem::take(cur);
            if let Some((v, idx)) = self.program.bed(li) {
                cur.r = Some(SampleReader { v: v.clone(), idx, pos: 0.0, rate: 0.0 });
            }
        }
        old
    }

    /// A hard switch: every voice and bed stops at once (the caller ducks
    /// around it), and the clock is set to `t · cps`.
    pub fn reset(&mut self, t: f64, cps: f64) {
        for v in &mut self.voices {
            v.src = Source::Idle;
        }
        for bed in self.beds.iter_mut().flatten() {
            *bed = Bed::default();
        }
        self.set_cycle(t * cps_of(cps));
        // The program's beds start again, from silence.
        for li in 0..LAYERS {
            if let Some((v, idx)) = self.program.bed(li) {
                self.beds[li][0].r = Some(SampleReader { v: v.clone(), idx, pos: 0.0, rate: 0.0 });
            }
        }
    }

    fn set_cycle(&mut self, c: f64) {
        self.cycle = c;
        self.at = Frac::approx(c);
    }

    /// Nothing sounds and nothing will this block: the engine may skip the
    /// sequencer and leave its output bit for bit what it was without one.
    pub fn is_idle(&self) -> bool {
        self.program.is_silent()
            && !self.voices.iter().any(Voice::is_active)
            && !self.beds.iter().flatten().any(|b| b.r.is_some())
    }

    /// Moves the clock on by `n` samples without playing (when idle).
    pub fn advance(&mut self, n: usize, cps: f64) {
        let c = self.cycle + cps_of(cps) * n as f64 / self.sr;
        self.set_cycle(c);
    }

    fn layer_seed(&self, li: usize) -> u64 {
        mix(self.seed ^ (li as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15))
    }

    /// One block: layers into `wet` (added: it goes on into the FX chain
    /// with the formulas) and `dry` (added: straight to the master).
    pub fn render(&mut self, live: &SeqLive, wet: &mut [f32], dry: &mut [f32]) {
        let n = wet.len();
        if n == 0 {
            return;
        }
        let c1 = self.cycle + cps_of(live.cps) * n as f64 / self.sr;
        let end = Frac::approx(c1);
        if end > self.at {
            for li in 0..self.program.layers.len().min(LAYERS) {
                self.start_events(li, &live.layers[li], end, c1, n);
            }
        }
        self.render_beds(live, wet, dry);
        for v in &mut self.voices {
            if v.is_active() {
                v.render(if v.dry { &mut *dry } else { &mut *wet });
            }
        }
        self.cycle = c1;
        self.at = end;
    }

    /// Starts layer `li`'s events of `[self.at, end)`.
    fn start_events(&mut self, li: usize, live: &LiveLayer, end: Frac, c1: f64, n: usize) {
        let l = &self.program.seq.layers[li];
        if !l.on || l.mode != Mode::Hits || self.program.layers[li].pattern.is_silent() {
            return;
        }
        let (gain, speed, density, dry) = (live.gain, live.speed, live.density, live.dry);
        let seed = self.layer_seed(li);
        self.events.clear();
        self.program.layers[li].pattern.query(self.at, end, seed, &mut self.events);
        self.overflow += u64::from(self.events.overflow);
        self.events.sort();
        for k in 0..self.events.len() {
            let e = self.events.as_slice()[k];
            if density < 1.0 && unit_hash(e.onset, DENSITY_SALT, seed) >= density {
                continue;
            }
            let lp = &self.program.layers[li];
            let (voicing, var) = match e.sound.name {
                None => (&lp.own, lp.own_n.wrapping_add(e.sound.n)),
                Some(i) => (&lp.names[usize::from(i)], e.sound.n),
            };
            let src = match voicing {
                Voicing::Silent => continue,
                Voicing::Samples(v) => {
                    if v.is_empty() {
                        continue;
                    }
                    let idx = usize::from(var) % v.len();
                    if v[idx].data.is_empty() {
                        continue;
                    }
                    let rate = speed_of(speed) * f64::from(v[idx].sr) / self.sr;
                    Source::Sample(SampleReader { v: v.clone(), idx, pos: 0.0, rate })
                }
                Voicing::Drum(d) => {
                    let noise = (unit_hash(e.onset, NOISE_SALT, seed) * 4_294_967_296.0) as u32;
                    Source::Drum(DrumVoice::new(*d, self.sr, speed_of(speed), noise))
                }
            };
            let into = (e.onset.to_f64() - self.cycle) / (c1 - self.cycle);
            let offset = ((into * n as f64) as usize).min(n - 1);
            self.start_voice(src, gain_of(gain), dry, offset);
        }
    }

    fn start_voice(&mut self, src: Source, gain: f64, dry: bool, offset: usize) {
        let live = self.voices.iter().filter(|v| v.is_active() && !v.releasing).count();
        if live >= VOICES {
            if let Some(v) =
                self.voices.iter_mut().filter(|v| v.is_active() && !v.releasing).min_by_key(|v| v.id)
            {
                v.release(self.sr);
                self.stolen += 1;
            }
        }
        let slot = match self.voices.iter().position(|v| !v.is_active()) {
            Some(i) => i,
            // Every slot busy, most of them releasing: the oldest is cut.
            None => (0..SLOTS).min_by_key(|&i| self.voices[i].id).unwrap_or(0),
        };
        self.voices[slot].start(src, gain, dry, offset, self.next_id, self.sr);
        self.next_id += 1;
    }

    fn render_beds(&mut self, live: &SeqLive, wet: &mut [f32], dry: &mut [f32]) {
        let coef = self.bed_coef;
        for li in 0..LAYERS {
            let l = &live.layers[li];
            let playing = self.program.bed(li).is_some();
            let (gain, speed, to_dry) = (gain_of(l.gain), speed_of(l.speed), l.dry);
            let out: &mut [f32] = if to_dry { &mut *dry } else { &mut *wet };
            for (slot, bed) in self.beds[li].iter_mut().enumerate() {
                let Some(r) = &mut bed.r else { continue };
                let target = if slot == 0 && playing { gain } else { 0.0 };
                r.rate = speed * f64::from(r.v[r.idx].sr) / self.sr;
                let mut level = bed.level;
                for o in out.iter_mut() {
                    level += coef * (target - level);
                    *o += (r.next_looped() * level) as f32;
                }
                bed.level = level;
                if target == 0.0 && level < 1e-5 {
                    bed.r = None;
                    bed.level = 0.0;
                }
            }
        }
    }
}

fn cps_of(c: f64) -> f64 {
    if c.is_finite() {
        c.clamp(CPS.0, CPS.1)
    } else {
        0.5
    }
}

fn gain_of(g: f64) -> f64 {
    if g.is_finite() {
        g.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn speed_of(s: f64) -> f64 {
    if s.is_finite() {
        s.clamp(0.1, 8.0)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seq::bank::Sample;
    use crate::seq::Layer;
    use crate::BLOCK;

    const SR: f64 = 48000.0;

    /// The in-memory bank of decision 7: a 1 kHz blip at 24 kHz (50 ms), a
    /// second, quieter variant, and a 1 s 300 Hz loop at 48 kHz. Cosines, so
    /// a sample's first frame is not zero and an onset is visible on it.
    fn bank() -> SampleBank {
        let sine = |sr: f64, f: f64, n: usize, amp: f64| -> Vec<i16> {
            (0..n)
                .map(|i| (amp * 32767.0 * (std::f64::consts::TAU * f * i as f64 / sr).cos()) as i16)
                .collect()
        };
        SampleBank::from_pcm(vec![
            (
                "blip".into(),
                vec![
                    Sample { sr: 24000, data: sine(24000.0, 1000.0, 1200, 0.9), mode: Mode::Hits },
                    Sample { sr: 24000, data: sine(24000.0, 500.0, 1200, 0.3), mode: Mode::Hits },
                ],
            ),
            (
                "hum".into(),
                vec![Sample { sr: 48000, data: sine(48000.0, 300.0, 48000, 0.5), mode: Mode::Bed }],
            ),
        ])
    }

    fn layer(sound: &str, pattern: &str) -> Layer {
        Layer { on: true, sound: sound.into(), pattern: pattern.into(), gain: 1.0, ..Layer::default() }
    }

    fn seq(cps: f64, layers: Vec<Layer>) -> SeqState {
        SeqState { cps, layers }
    }

    /// `secs` of the sequencer alone: (wet, dry).
    fn play(s: &SeqState, bank: Option<&SampleBank>, secs: f64) -> (Vec<f32>, Vec<f32>, Sequencer) {
        let mut q = Sequencer::new(SR, 1);
        q.set_program(Box::new(SeqProgram::new(s, bank)));
        let n = (secs * SR) as usize / BLOCK * BLOCK;
        let (mut wet, mut dry) = (vec![0.0f32; n], vec![0.0f32; n]);
        let live = SeqLive::of(s);
        for (w, d) in wet.chunks_mut(BLOCK).zip(dry.chunks_mut(BLOCK)) {
            q.render(&live, w, d);
        }
        (wet, dry, q)
    }

    /// Where the sound starts after each silence of at least `gap` samples.
    fn onsets(x: &[f32], gap: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut quiet = gap;
        for (i, v) in x.iter().enumerate() {
            if v.abs() > 1e-6 {
                if quiet >= gap {
                    out.push(i);
                }
                quiet = 0;
            } else {
                quiet += 1;
            }
        }
        out
    }

    #[test]
    fn events_start_on_the_sample() {
        // cps 1: "0 ~ 0 ~" plays at 0 and 0.5 s — 24000 is not a block
        // boundary (187.5 blocks), so this is sample-accurate starting.
        let b = bank();
        let (wet, _, _) = play(&seq(1.0, vec![layer("blip", "0 ~ 0 ~")]), Some(&b), 2.0);
        assert_eq!(onsets(&wet, 1000), [0, 24000, 48000, 72000]);
        // cps 0.75: the step boundaries are not on whole samples; each lands
        // in the sample its time falls in.
        let (wet, _, _) = play(&seq(0.75, vec![layer("blip", "0*3")]), Some(&b), 2.0);
        let want: Vec<usize> = (0..5).map(|k| (k as f64 / 2.25 * SR) as usize).collect();
        assert_eq!(onsets(&wet, 1000), want);
    }

    #[test]
    fn a_sample_plays_at_its_own_rate_and_the_speed() {
        let b = bank();
        // A 24 kHz blip of 1200 frames lasts 2400 samples at 48 kHz.
        let (wet, _, _) = play(&seq(0.5, vec![layer("blip", "0")]), Some(&b), 0.2);
        let last = wet.iter().rposition(|v| v.abs() > 1e-6).unwrap();
        assert!((2395..=2400).contains(&last), "{last}");
        let fast = Layer { speed: 2.0, ..layer("blip", "0") };
        let (wet, _, _) = play(&seq(0.5, vec![fast]), Some(&b), 0.2);
        let last = wet.iter().rposition(|v| v.abs() > 1e-6).unwrap();
        assert!((1195..=1200).contains(&last), "{last}");
    }

    #[test]
    fn numbers_pick_variants_and_names_pick_sounds() {
        let b = bank();
        let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        let (a, _, _) = play(&seq(1.0, vec![layer("blip", "0")]), Some(&b), 0.1);
        let (c, _, _) = play(&seq(1.0, vec![layer("blip", "1")]), Some(&b), 0.1);
        assert!(peak(&a) > 0.8 && peak(&c) < 0.35, "{} {}", peak(&a), peak(&c));
        // Variants wrap, as Strudel's n does: 2 is 0 again.
        let (w, _, _) = play(&seq(1.0, vec![layer("blip", "2")]), Some(&b), 0.1);
        assert_eq!(a, w);
        // "blip:1" names it in the pattern; the layer's sound can carry it too.
        let (n, _, _) = play(&seq(1.0, vec![layer("hum", "blip:1")]), Some(&b), 0.1);
        let (s, _, _) = play(&seq(1.0, vec![layer("blip:1", "0")]), Some(&b), 0.1);
        assert_eq!(n, c);
        assert_eq!(s, c);
    }

    #[test]
    fn synth_drums_play_without_a_bank_and_unknown_names_are_silent() {
        let started = |q: &Sequencer| q.next_id - 1;
        let (wet, _, q) = play(&seq(0.5, vec![layer("bd", "0*4")]), None, 2.0);
        assert_eq!(started(&q), 4);
        assert!(wet.iter().any(|v| v.abs() > 0.2));
        let (_, _, q) = play(&seq(0.5, vec![layer("owl", "0*4 , hh*2 , ~ nothing")]), None, 2.0);
        assert_eq!(started(&q), 2, "only the hats");
        // A pattern that does not parse is silence, not an error.
        let (wet, _, _) = play(&seq(0.5, vec![layer("bd", "[0 0")]), None, 1.0);
        assert!(wet.iter().all(|v| *v == 0.0));
        // The bank wins over the synth: a "bd" sample replaces the drum.
        let b = SampleBank::from_pcm(vec![(
            "bd".into(),
            vec![Sample { sr: 48000, data: vec![10000; 100], mode: Mode::Hits }],
        )]);
        let (wet, _, _) = play(&seq(0.5, vec![layer("bd", "0")]), Some(&b), 0.1);
        assert!(wet[200..].iter().all(|v| *v == 0.0), "the 100-frame sample, not the drum");
    }

    #[test]
    fn dry_layers_skip_the_chain_and_off_layers_are_silent() {
        let b = bank();
        let d = Layer { dry: true, ..layer("blip", "0") };
        let (wet, dry, _) = play(&seq(1.0, vec![d]), Some(&b), 0.2);
        assert!(wet.iter().all(|v| *v == 0.0) && dry.iter().any(|v| *v != 0.0));
        let off = Layer { on: false, ..layer("blip", "0*8") };
        let (wet, dry, q) = play(&seq(1.0, vec![off]), Some(&b), 0.5);
        assert!(wet.iter().chain(&dry).all(|v| *v == 0.0));
        assert!(q.is_idle());
    }

    #[test]
    fn density_keeps_a_fraction_by_hash() {
        let b = bank();
        let count = |density: f64| {
            let l = Layer { density, ..layer("blip", "0*8") };
            let (wet, _, _) = play(&seq(1.0, vec![l]), Some(&b), 8.0);
            onsets(&wet, 1000).len()
        };
        assert_eq!(count(1.0), 64);
        assert_eq!(count(0.0), 0);
        let half = count(0.5);
        assert!((20..=44).contains(&half), "{half}");
        assert_eq!(half, count(0.5), "the same every render");
    }

    #[test]
    fn the_pool_steals_the_oldest_and_never_overflows() {
        // 64 hum hits a second, each 1 s long: far more than 16 voices.
        let b = bank();
        let (wet, _, q) = play(&seq(1.0, vec![layer("hum", "0*64")]), Some(&b), 2.0);
        assert!(q.stolen > 100, "{}", q.stolen);
        assert_eq!(q.overflow, 0);
        assert!(q.voices.iter().filter(|v| v.is_active() && !v.releasing).count() <= VOICES);
        let peak = wet.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 16.0 * 0.5 + 0.01, "{peak}");
    }

    #[test]
    fn a_bed_loops_and_fades() {
        let b = bank();
        let bed = Layer { mode: Mode::Bed, gain: 0.5, pattern: "this is ignored".into(), ..layer("hum", "") };
        let s = seq(0.5, vec![bed]);
        let (wet, _, mut q) = play(&s, Some(&b), 3.0);
        let rms = |x: &[f32]| (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt();
        // Faded in, then steady through the loop point at 1 s and 2 s.
        assert!(rms(&wet[..4800]) < rms(&wet[96000..100800]));
        let steady = 0.5 * 0.5 / 2f64.sqrt();
        for w in [(46000, 50000), (94000, 98000), (120000, 124000)] {
            let r = rms(&wet[w.0..w.1]);
            assert!((r - steady).abs() < 0.01, "{w:?}: {r} vs {steady}");
        }
        // Turned off: it fades out and the sequencer goes idle.
        let mut off = s.clone();
        off.layers[0].on = false;
        q.set_program(Box::new(SeqProgram::new(&off, Some(&b))));
        let mut buf = vec![0.0f32; BLOCK];
        let mut dry = vec![0.0f32; BLOCK];
        let live = SeqLive::of(&off);
        for _ in 0..(SR as usize * 6 / BLOCK) {
            buf.fill(0.0);
            q.render(&live, &mut buf, &mut dry);
        }
        assert!(q.is_idle());
    }

    #[test]
    fn the_clock_is_t_times_cps_and_glides_with_it() {
        let b = bank();
        let (_, _, q) = play(&seq(0.5, vec![layer("blip", "0")]), Some(&b), 2.0);
        let t = (2.0 * SR) as usize / BLOCK * BLOCK;
        assert!((q.cycle() - t as f64 / SR * 0.5).abs() < 1e-9);
        // A new cps carries on from where the cycle is: no jump.
        let mut q = q;
        let before = q.cycle();
        let faster = seq(2.0, vec![layer("blip", "0")]);
        q.set_program(Box::new(SeqProgram::new(&faster, Some(&b))));
        q.render(&SeqLive::of(&faster), &mut [0.0; BLOCK], &mut [0.0; BLOCK]);
        assert!((q.cycle() - (before + 2.0 * BLOCK as f64 / SR)).abs() < 1e-12);
        // A hard switch puts it back on t · cps.
        q.reset(10.0, 2.0);
        assert!((q.cycle() - 20.0).abs() < 1e-12);
        assert!(!q.voices.iter().any(Voice::is_active));
    }

    #[test]
    fn a_render_is_the_same_every_time() {
        let b = bank();
        let s = seq(0.7, vec![layer("blip", "0(3,8) , <1 [0 1]>?"), layer("sd", "~ 0 , hh*8?0.3")]);
        let (a, _, _) = play(&s, Some(&b), 4.0);
        let (c, _, _) = play(&s, Some(&b), 4.0);
        assert_eq!(a, c);
        assert!(a.iter().any(|v| *v != 0.0));
    }
}
