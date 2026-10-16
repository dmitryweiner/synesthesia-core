//! The chain itself: topology switches, per-block parameter smoothing and the
//! per-sample loop.
//!
//! Every numeric parameter is smoothed with the web app's time constant
//! (30 ms). Values arrive at control rate both from LFO routes and from genome
//! morphs, and stepping them would zipper — the phaser's feedback loop rings on
//! it audibly.
//!
//! **Stereo** (PLAN-BINAURAL.md decision 5): a chain built with
//! [`FxChain::new_stereo`] runs the filter, chorus, phaser and delay as two
//! [`Front`]s with the same parameters — the right one's chorus and phaser
//! LFOs `width`·90° ahead — into one reverb with two outputs, the binaural
//! beat's frequency shifter, and one limiter for both ears. At width 0 with
//! the beat off both ears are the mono chain's output, bit for bit.

use crate::schema::{self, vowel_formants};
use crate::state::FxState;

use super::biquad::{Biquad, BiquadType, Coeffs};
use super::delayline::DelayLine;
use super::hilbert::{rotation, Shifter};
use super::limiter::Limiter;
use super::reverb::Fdn;
use super::shimmer::OctaveShimmer;

/// Smoothing time constant for FX parameters, seconds (web: `FX_SMOOTH_TC`).
const SMOOTH_TC: f64 = 0.03;
const MAX_PHASER_STAGES: usize = 8;
const FORMANT_BANDS: usize = 3;
/// All-pass centre sweep, kept strictly positive: [200, 200 + 3600·depth].
const PHASER_F_LO: f64 = 200.0;
/// One Web Audio render quantum: the shortest delay the browser can put in a
/// feedback loop, and therefore the shortest the presets were tuned with.
const MIN_LOOP_DELAY: f64 = 128.0;
/// How fast the binaural beat fades in and out when it is switched, seconds:
/// the shifted sound replaces the plain one through a crossfade, not a step.
const SHIFT_FADE: f64 = 0.05;

#[derive(Clone, Copy, Debug, Default)]
struct Smooth {
    v: f64,
}

impl Smooth {
    fn jump(&mut self, target: f64) {
        self.v = target;
    }

    #[inline]
    fn glide(&mut self, target: f64, coef: f64) -> f64 {
        self.v += coef * (target - self.v);
        self.v
    }
}

/// Everything the chain smooths, in one place so `process` reads like the
/// signal path rather than like bookkeeping.
#[derive(Clone, Copy, Debug, Default)]
struct Smoothed {
    filter_freq: Smooth,
    filter_q: Smooth,
    filter_gain: Smooth,
    filter_vowel: Smooth,
    comb_fb: Smooth,
    chorus_rate: Smooth,
    chorus_depth: Smooth,
    chorus_mix: Smooth,
    chorus_fb: Smooth,
    phaser_rate: Smooth,
    phaser_depth: Smooth,
    phaser_fb: Smooth,
    phaser_mix: Smooth,
    delay_time: Smooth,
    delay_fb: Smooth,
    delay_mix: Smooth,
    delay_shimmer: Smooth,
    reverb_mix: Smooth,
    limiter_thr: Smooth,
    limiter_rel: Smooth,
}

/// One block's parameters: glided once, shared by both ears.
#[derive(Clone, Copy, Debug)]
struct Block {
    filter_on: bool,
    chorus_on: bool,
    phaser_on: bool,
    delay_on: bool,
    reverb_on: bool,
    limiter_on: bool,
    mode: FilterMode,
    filter: Coeffs,
    formants: [Coeffs; FORMANT_BANDS],
    formant_amp: [f64; FORMANT_BANDS],
    comb_delay: f64,
    comb_fb: f64,
    chorus_base_ms: f64,
    chorus_depth: f64,
    chorus_mix: f64,
    chorus_fb: f64,
    chorus_inc: f64,
    phaser_inc: f64,
    phaser_fb: f64,
    phaser_mix: f64,
    stages: usize,
    p_centre: f64,
    p_half: f64,
    delay_samples: f64,
    delay_fb: f64,
    delay_mix: f64,
    delay_shimmer: f64,
    reverb_mix: f64,
    sr: f64,
}

/// The chain before the reverb — filter, chorus, phaser, delay — for one
/// ear: everything that has state of its own.
struct Front {
    filter: Biquad,
    formants: [Biquad; FORMANT_BANDS],
    comb: DelayLine,
    chorus: DelayLine,
    chorus_phase: f64,
    phaser: [Biquad; MAX_PHASER_STAGES],
    phaser_phase: f64,
    phaser_fb_state: f64,
    delay: DelayLine,
    /// In the delay's feedback loop, as the web app's graph has it.
    shimmer: OctaveShimmer,
}

impl Front {
    fn new(sr: f64) -> Front {
        Front {
            filter: Biquad::new(),
            formants: [Biquad::new(); FORMANT_BANDS],
            comb: DelayLine::new((sr * 0.05) as usize + 4),
            chorus: DelayLine::new((sr * 0.2) as usize + 4),
            chorus_phase: 0.0,
            phaser: [Biquad::new(); MAX_PHASER_STAGES],
            phaser_phase: 0.0,
            phaser_fb_state: 0.0,
            delay: DelayLine::new((sr * 3.0) as usize + 4),
            shimmer: OctaveShimmer::new(sr),
        }
    }

    fn clear(&mut self) {
        self.filter.reset();
        for f in &mut self.formants {
            f.reset();
        }
        for f in &mut self.phaser {
            f.reset();
        }
        self.comb.clear();
        self.chorus.clear();
        self.delay.clear();
        self.shimmer.clear();
        self.chorus_phase = 0.0;
        self.phaser_phase = 0.0;
        self.phaser_fb_state = 0.0;
    }

    fn set(&mut self, b: &Block) {
        match b.mode {
            FilterMode::Biquad => self.filter.set_coeffs(b.filter),
            FilterMode::Formant => {
                for (f, c) in self.formants.iter_mut().zip(b.formants) {
                    f.set_coeffs(c);
                }
            }
            FilterMode::Comb => {}
        }
    }

    /// One sample through the filter, chorus, phaser and delay. `lfo_ahead`
    /// is how far this ear's chorus and phaser LFOs run ahead, radians.
    #[inline]
    fn tick(&mut self, mut x: f64, b: &Block, lfo_ahead: f64) -> f64 {
        if b.filter_on {
            x = match b.mode {
                FilterMode::Biquad => self.filter.tick(x),
                FilterMode::Formant => {
                    let mut sum = 0.0;
                    for i in 0..FORMANT_BANDS {
                        sum += self.formants[i].tick(x) * b.formant_amp[i];
                    }
                    sum
                }
                FilterMode::Comb => {
                    let delayed = self.comb.read(b.comb_delay);
                    self.comb.write(x + delayed * b.comb_fb);
                    delayed
                }
            };
        }

        if b.chorus_on {
            let lfo = (self.chorus_phase + lfo_ahead).sin();
            self.chorus_phase += b.chorus_inc;
            let d = ((b.chorus_base_ms + b.chorus_depth * lfo) * 1e-3 * b.sr + MIN_LOOP_DELAY).max(1.0);
            let wet = self.chorus.read(d);
            self.chorus.write(x + wet * b.chorus_fb);
            x = x * (1.0 - b.chorus_mix) + wet * b.chorus_mix;
        }

        if b.phaser_on {
            let lfo = (self.phaser_phase + lfo_ahead).sin();
            self.phaser_phase += b.phaser_inc;
            let f = (b.p_centre + b.p_half * lfo).clamp(20.0, b.sr * 0.45);
            let coeffs = Coeffs::new(BiquadType::Allpass, f, 0.5, 0.0, b.sr);
            let mut y = x + self.phaser_fb_state * b.phaser_fb;
            for stage in self.phaser.iter_mut().take(b.stages) {
                stage.set_coeffs(coeffs);
                y = stage.tick(y);
            }
            self.phaser_fb_state = y;
            x = x * (1.0 - b.phaser_mix) + y * b.phaser_mix;
        }

        if b.delay_on {
            let wet = self.delay.read(b.delay_samples);
            // The feedback goes round through the shimmer, as the web
            // app's graph does it: delay → fb → shimmer → delay.
            self.delay.write(x + self.shimmer.tick(wet * b.delay_fb, b.delay_shimmer));
            x = x * (1.0 - b.delay_mix) + wet * b.delay_mix;
        }
        x
    }

    fn wrap_phases(&mut self) {
        self.chorus_phase %= std::f64::consts::TAU;
        self.phaser_phase %= std::f64::consts::TAU;
    }
}

/// What the stereo chain is asked for, each block (the point's `bin`, with
/// the LFOs laid over it).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Space {
    /// 0–1: the reverb's and the chorus's spread (the sources' azimuths are
    /// scaled by it before the chain).
    pub width: f64,
    /// The binaural beat is on.
    pub beat_on: bool,
    /// The beat, Hz: the left ear moves down by half of it, the right up.
    pub beat: f64,
}

/// What only a stereo chain has.
struct Stereo {
    right: Front,
    /// The beat's shifters, left and right.
    shift: [Shifter; 2],
    width: Smooth,
    beat: Smooth,
    /// How much of the shifted sound each ear hears, 0–1 (a crossfade).
    shift_mix: f64,
    /// A hard switch happened: the next block starts at its targets.
    jump: bool,
}

pub struct FxChain {
    sr: f64,
    fx: FxState,
    s: Smoothed,
    /// The mono chain's front, or the left ear's.
    front: Front,
    stereo: Option<Box<Stereo>>,
    reverb: Fdn,
    limiter: Limiter,
    last_reverb_decay: f64,
}

impl FxChain {
    pub fn new(sr: f64) -> Self {
        let mut chain = Self {
            sr,
            fx: FxState::default(),
            s: Smoothed::default(),
            front: Front::new(sr),
            stereo: None,
            reverb: Fdn::new(sr),
            limiter: Limiter::new(sr),
            last_reverb_decay: f64::NAN,
        };
        let fx = FxState::default();
        chain.reset(&fx);
        chain
    }

    /// A chain for two ears ([`FxChain::process_stereo`]).
    pub fn new_stereo(sr: f64) -> Self {
        let mut chain = FxChain::new(sr);
        chain.stereo = Some(Box::new(Stereo {
            right: Front::new(sr),
            shift: [Shifter::new(sr), Shifter::new(sr)],
            width: Smooth::default(),
            beat: Smooth::default(),
            shift_mix: 0.0,
            jump: true,
        }));
        chain
    }

    pub fn is_stereo(&self) -> bool {
        self.stereo.is_some()
    }

    /// New target values; the chain glides to them.
    pub fn set(&mut self, fx: &FxState) {
        self.fx.copy_from(fx);
    }

    /// Hard switch (a new point): jump every parameter and drop every tail, so
    /// the old reverb and delay cannot bleed into the new sound.
    pub fn reset(&mut self, fx: &FxState) {
        self.fx = fx.clone();
        self.s.filter_freq.jump(fx.filter_freq);
        self.s.filter_q.jump(fx.filter_q);
        self.s.filter_gain.jump(fx.filter_gain);
        self.s.filter_vowel.jump(fx.filter_vowel);
        self.s.comb_fb.jump(fx.filter_comb_fb);
        self.s.chorus_rate.jump(fx.chorus_rate);
        self.s.chorus_depth.jump(fx.chorus_depth);
        self.s.chorus_mix.jump(fx.chorus_mix);
        self.s.chorus_fb.jump(fx.chorus_fb);
        self.s.phaser_rate.jump(fx.phaser_rate);
        self.s.phaser_depth.jump(fx.phaser_depth);
        self.s.phaser_fb.jump(fx.phaser_fb);
        self.s.phaser_mix.jump(fx.phaser_mix);
        self.s.delay_time.jump(fx.delay_time);
        self.s.delay_fb.jump(fx.delay_fb);
        self.s.delay_mix.jump(fx.delay_mix);
        self.s.delay_shimmer.jump(fx.delay_shimmer);
        self.s.reverb_mix.jump(fx.reverb_mix);
        self.s.limiter_thr.jump(fx.limiter_thr);
        self.s.limiter_rel.jump(fx.limiter_rel);

        self.front.clear();
        if let Some(st) = &mut self.stereo {
            st.right.clear();
            for s in &mut st.shift {
                s.clear();
            }
            st.jump = true;
        }
        self.reverb.clear();
        self.limiter.clear();
        self.last_reverb_decay = f64::NAN;
    }

    pub fn limiter_reduction_db(&self) -> f64 {
        self.limiter.reduction_db()
    }

    /// Glides the parameters one block of `n` samples on and derives what
    /// the per-sample loop needs.
    fn block(&mut self, n: usize) -> Block {
        let sr = self.sr;
        let dt = n as f64 / sr;
        let coef = 1.0 - (-dt / SMOOTH_TC).exp();
        // Borrowed, not cloned: a clone would allocate the strings on the
        // audio thread. Only other fields are mutated below.
        let fx = &self.fx;

        let filter_freq = self.s.filter_freq.glide(fx.filter_freq, coef);
        let filter_q = self.s.filter_q.glide(fx.filter_q, coef);
        let filter_gain = self.s.filter_gain.glide(fx.filter_gain, coef);
        let filter_vowel = self.s.filter_vowel.glide(fx.filter_vowel, coef);
        let comb_fb = self.s.comb_fb.glide(fx.filter_comb_fb, coef).clamp(0.0, 0.95);
        let chorus_rate = self.s.chorus_rate.glide(fx.chorus_rate, coef);
        let chorus_depth = self.s.chorus_depth.glide(fx.chorus_depth, coef);
        let chorus_mix = self.s.chorus_mix.glide(fx.chorus_mix, coef).clamp(0.0, 1.0);
        let chorus_fb = self.s.chorus_fb.glide(fx.chorus_fb, coef).clamp(0.0, 0.95);
        let phaser_rate = self.s.phaser_rate.glide(fx.phaser_rate, coef);
        let phaser_depth = self.s.phaser_depth.glide(fx.phaser_depth, coef);
        let phaser_fb = self.s.phaser_fb.glide(fx.phaser_fb, coef).clamp(0.0, 0.9);
        let phaser_mix = self.s.phaser_mix.glide(fx.phaser_mix, coef).clamp(0.0, 1.0);
        let delay_time = self.s.delay_time.glide(fx.delay_time, coef).clamp(0.001, 3.0);
        let delay_fb = self.s.delay_fb.glide(fx.delay_fb, coef).clamp(0.0, 0.95);
        let delay_mix = self.s.delay_mix.glide(fx.delay_mix, coef).clamp(0.0, 1.0);
        let delay_shimmer = self.s.delay_shimmer.glide(fx.delay_shimmer, coef).clamp(0.0, 1.0);
        let reverb_mix = self.s.reverb_mix.glide(fx.reverb_mix, coef).clamp(0.0, 1.0);
        let limiter_thr = self.s.limiter_thr.glide(fx.limiter_thr, coef);
        let limiter_rel = self.s.limiter_rel.glide(fx.limiter_rel, coef);

        let mode = filter_mode(&fx.filter_type);
        let mut filter = Coeffs::default();
        let mut formants = [Coeffs::default(); FORMANT_BANDS];
        let mut formant_amp = [0.0; FORMANT_BANDS];
        match mode {
            FilterMode::Biquad => {
                let kind = BiquadType::parse(&fx.filter_type).unwrap_or(BiquadType::Lowpass);
                filter = Coeffs::new(kind, filter_freq.clamp(20.0, 20000.0), filter_q, filter_gain, sr);
            }
            FilterMode::Formant => {
                let v = vowel_formants(filter_vowel);
                let scale = filter_freq.clamp(200.0, 4000.0) / 1000.0;
                let q = (filter_q * 6.0).clamp(4.0, 28.0);
                for i in 0..FORMANT_BANDS {
                    let f = (v.f[i] * scale).clamp(50.0, 8000.0);
                    formants[i] = Coeffs::new(BiquadType::Bandpass, f, q, 0.0, sr);
                    formant_amp[i] = v.a[i];
                }
            }
            FilterMode::Comb => {}
        }

        // A feedback loop in Web Audio is broken once per render quantum, so
        // every loop carries 128 samples of latency on top of its delay time.
        // The presets were tuned through that, and it is audible: without the
        // extra quantum the comb ran +3.4 dB hot and resonated on the wrong
        // partials. Measured, with it: 0.0–0.1 dB (scripts/parity.mjs).
        let comb_delay = (sr / filter_freq.clamp(20.0, 2000.0) + MIN_LOOP_DELAY)
            .clamp(2.0, self.front.comb.capacity() as f64 - 2.0);
        let chorus_base_ms = if fx.chorus_mode == "flanger" { 2.0 } else { 12.0 };
        let chorus_inc = std::f64::consts::TAU * chorus_rate / sr;
        let phaser_inc = std::f64::consts::TAU * phaser_rate / sr;
        let stages = (fx.phaser_stages.floor() as usize).clamp(1, MAX_PHASER_STAGES);
        let p_hi = PHASER_F_LO + 3600.0 * phaser_depth;
        let p_centre = 0.5 * (PHASER_F_LO + p_hi);
        let p_half = 0.5 * (p_hi - PHASER_F_LO);
        // Twice the quantum: the delay node's own latency, and the shimmer
        // worklet's, which sits in the loop whenever the delay is on — amount
        // zero or not (the browser's graph is wired that way, and the presets
        // were tuned through it).
        let delay_samples = delay_time * sr + 2.0 * MIN_LOOP_DELAY;

        let decay_moved = !matches!(
            (fx.reverb_decay - self.last_reverb_decay).abs().partial_cmp(&0.05),
            Some(std::cmp::Ordering::Less)
        );
        if fx.reverb_on && decay_moved {
            self.last_reverb_decay = fx.reverb_decay;
            self.reverb.set_decay(fx.reverb_decay);
        }
        self.limiter.set(limiter_thr, limiter_rel);

        let b = Block {
            filter_on: fx.filter_on,
            chorus_on: fx.chorus_on,
            phaser_on: fx.phaser_on,
            delay_on: fx.delay_on,
            reverb_on: fx.reverb_on,
            limiter_on: fx.limiter_on,
            mode,
            filter,
            formants,
            formant_amp,
            comb_delay,
            comb_fb,
            chorus_base_ms,
            chorus_depth,
            chorus_mix,
            chorus_fb,
            chorus_inc,
            phaser_inc,
            phaser_fb,
            phaser_mix,
            stages,
            p_centre,
            p_half,
            delay_samples,
            delay_fb,
            delay_mix,
            delay_shimmer,
            reverb_mix,
            sr,
        };
        self.front.set(&b);
        if let Some(st) = &mut self.stereo {
            st.right.set(&b);
        }
        b
    }

    /// Runs one block in place (mono).
    pub fn process(&mut self, buf: &mut [f32]) {
        let b = self.block(buf.len());
        for sample in buf.iter_mut() {
            let mut x = self.front.tick(f64::from(*sample), &b, 0.0);

            if b.reverb_on {
                let wet = self.reverb.tick(x);
                x = x * (1.0 - b.reverb_mix) + wet * b.reverb_mix;
            }

            if b.limiter_on {
                x = self.limiter.tick(x);
            }

            *sample = x as f32;
        }
        self.front.wrap_phases();
    }

    /// Runs one block of both ears in place, and writes into `mid` what the
    /// analysis hears: (L + R)/2 before the beat's shift (a shifted pair
    /// summed would tremble at the beat), through the same limiter.
    ///
    /// A mono chain (built with [`FxChain::new`]) processes `l` and copies
    /// it — the engine never asks that of it.
    pub fn process_stereo(&mut self, l: &mut [f32], r: &mut [f32], mid: &mut [f32], space: Space) {
        let n = l.len();
        if self.stereo.is_none() {
            self.process(l);
            r.copy_from_slice(l);
            mid.copy_from_slice(l);
            return;
        }
        let b = self.block(n);
        let sr = self.sr;
        let Some(st) = self.stereo.as_deref_mut() else { return };

        let (beat_lo, beat_hi) = crate::state::bin_range("bin.beat");
        let width_target = if space.width.is_finite() { space.width.clamp(0.0, 1.0) } else { 0.0 };
        let beat_target = if space.beat.is_finite() { space.beat.clamp(beat_lo, beat_hi) } else { beat_lo };
        let shift_target = if space.beat_on { 1.0 } else { 0.0 };
        let coef = 1.0 - (-(n as f64 / sr) / SMOOTH_TC).exp();
        let (width, beat) = if st.jump {
            st.jump = false;
            st.width.jump(width_target);
            st.beat.jump(beat_target);
            st.shift_mix = shift_target;
            (width_target, beat_target)
        } else {
            (st.width.glide(width_target, coef), st.beat.glide(beat_target, coef))
        };
        let lfo_ahead = width * std::f64::consts::FRAC_PI_2;

        // The beat fades in and out over SHIFT_FADE, linearly per sample.
        let from_mix = st.shift_mix;
        let step = (1.0 / (SHIFT_FADE * sr)).min(1.0);
        let to_mix = if shift_target > from_mix {
            (from_mix + step * n as f64).min(1.0)
        } else {
            (from_mix - step * n as f64).max(0.0)
        };
        let shifting = from_mix > 0.0 || to_mix > 0.0;
        if shifting && from_mix == 0.0 {
            // Coming in from silence: no stale state in the all-passes.
            for s in &mut st.shift {
                s.clear();
            }
        }
        let rot = [rotation(-0.5 * beat, sr), rotation(0.5 * beat, sr)];
        let mix_step = (to_mix - from_mix) / n as f64;

        for k in 0..n {
            let mut xl = self.front.tick(f64::from(l[k]), &b, 0.0);
            let mut xr = st.right.tick(f64::from(r[k]), &b, lfo_ahead);

            if b.reverb_on {
                // One room: what both ears send into it, heard back on the
                // even lines in the left ear and the odd ones in the right,
                // as far apart as the width says.
                let (wet, side) = self.reverb.tick_stereo(0.5 * (xl + xr));
                let (wl, wr) = (wet + width * side, wet - width * side);
                xl = xl * (1.0 - b.reverb_mix) + wl * b.reverb_mix;
                xr = xr * (1.0 - b.reverb_mix) + wr * b.reverb_mix;
            }

            let mut m = 0.5 * (xl + xr);

            if shifting {
                let mix = from_mix + mix_step * (k + 1) as f64;
                let sl = st.shift[0].tick(xl, rot[0]);
                let sr_ = st.shift[1].tick(xr, rot[1]);
                xl += mix * (sl - xl);
                xr += mix * (sr_ - xr);
            }

            if b.limiter_on {
                (xl, xr, m) = self.limiter.tick_linked(xl, xr, m);
            }

            l[k] = xl as f32;
            r[k] = xr as f32;
            mid[k] = m as f32;
        }
        st.shift_mix = to_mix;
        if shifting {
            for s in &mut st.shift {
                s.renormalize();
            }
        }
        self.front.wrap_phases();
        st.right.wrap_phases();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FilterMode {
    Biquad,
    Formant,
    Comb,
}

fn filter_mode(t: &str) -> FilterMode {
    match t {
        "formant" => FilterMode::Formant,
        "comb" => FilterMode::Comb,
        _ => FilterMode::Biquad,
    }
}

/// Every filter type the schema allows must map to a mode we implement.
pub fn filter_types_covered() -> bool {
    schema::schema().filter_types.iter().all(|t| match filter_mode(t) {
        FilterMode::Biquad => BiquadType::parse(t).is_some(),
        _ => true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len() as f64).sqrt()
    }

    fn tone(n: usize, sr: f64, freq: f64, amp: f64) -> Vec<f32> {
        (0..n).map(|i| (amp * (std::f64::consts::TAU * freq * i as f64 / sr).sin()) as f32).collect()
    }

    #[test]
    fn every_filter_type_in_the_schema_is_implemented() {
        assert!(filter_types_covered());
    }

    #[test]
    fn a_silent_input_stays_silent_with_everything_on() {
        let sr = 48000.0;
        let fx = FxState {
            filter_on: true,
            chorus_on: true,
            phaser_on: true,
            delay_on: true,
            reverb_on: true,
            limiter_on: true,
            ..FxState::default()
        };
        let mut chain = FxChain::new(sr);
        chain.reset(&fx);
        let mut buf = vec![0.0f32; 4096];
        for _ in 0..20 {
            chain.process(&mut buf);
        }
        assert!(buf.iter().all(|v| v.abs() < 1e-9), "silence grew a tail");
    }

    #[test]
    fn the_chain_is_stable_with_every_module_at_full_feedback() {
        let sr = 48000.0;
        let fx = FxState {
            filter_on: true,
            filter_type: "comb".into(),
            filter_comb_fb: 0.95,
            chorus_on: true,
            chorus_fb: 0.95,
            phaser_on: true,
            phaser_fb: 0.9,
            delay_on: true,
            delay_fb: 0.9,
            reverb_on: true,
            reverb_decay: 8.0,
            limiter_on: true,
            ..FxState::default()
        };
        let mut chain = FxChain::new(sr);
        chain.reset(&fx);
        let mut worst: f64 = 0.0;
        for block in 0..200 {
            let mut buf = if block < 50 { tone(1024, sr, 220.0, 0.3) } else { vec![0.0f32; 1024] };
            chain.process(&mut buf);
            worst = worst.max(buf.iter().fold(0.0f64, |m, v| m.max(f64::from(v.abs()))));
        }
        assert!(worst.is_finite() && worst < 2.0, "chain blew up: {worst}");
    }

    #[test]
    fn a_lowpass_takes_the_top_off() {
        let sr = 48000.0;
        let fx = FxState { filter_on: true, filter_freq: 300.0, limiter_on: false, ..FxState::default() };
        let mut chain = FxChain::new(sr);
        chain.reset(&fx);
        let mut high = tone(48000, sr, 6000.0, 0.5);
        chain.process(&mut high);
        let mut low = tone(48000, sr, 100.0, 0.5);
        chain.reset(&fx);
        chain.process(&mut low);
        assert!(rms(&high) < 0.05 * rms(&low), "high {} low {}", rms(&high), rms(&low));
    }

    /// The strongest frequency near `around`, to a thousandth of a hertz.
    fn peak(x: &[f32], sr: f64, around: f64) -> f64 {
        let power = |hz: f64| {
            let (mut re, mut im) = (0.0, 0.0);
            for (n, v) in x.iter().enumerate() {
                let w = 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / x.len() as f64).cos();
                let a = std::f64::consts::TAU * hz * n as f64 / sr;
                re += w * f64::from(*v) * a.cos();
                im += w * f64::from(*v) * a.sin();
            }
            re * re + im * im
        };
        let mut best = around;
        for step in [0.5, 0.05, 0.005] {
            let c = best;
            best = (-20..=20)
                .map(|k| c + f64::from(k) * step)
                .max_by(|a, b| power(*a).total_cmp(&power(*b)))
                .unwrap_or(c);
        }
        best
    }

    /// PLAN-BINAURAL.md B3: a 440 Hz sine comes out at 440 − Δ/2 in the left
    /// ear and 440 + Δ/2 in the right, measured, within 0.05 Hz.
    #[test]
    fn the_beat_moves_each_ear_by_half_of_it() {
        let sr = 48000.0;
        let fx = FxState { limiter_on: false, ..FxState::default() };
        for beat in [0.5, 6.0, 40.0] {
            let mut chain = FxChain::new_stereo(sr);
            chain.reset(&fx);
            let space = Space { width: 0.0, beat_on: true, beat };
            let n = (sr * 6.0) as usize;
            let (mut l, mut r, mut m) = (tone(n, sr, 440.0, 0.5), tone(n, sr, 440.0, 0.5), vec![0.0f32; n]);
            for k in (0..n).step_by(128) {
                let e = (k + 128).min(n);
                chain.process_stereo(&mut l[k..e], &mut r[k..e], &mut m[k..e], space);
            }
            let tail = |x: &[f32]| x[(sr * 2.0) as usize..].to_vec();
            let (fl, fr) = (peak(&tail(&l), sr, 440.0 - beat / 2.0), peak(&tail(&r), sr, 440.0 + beat / 2.0));
            assert!((fl - (440.0 - beat / 2.0)).abs() <= 0.05, "Δ {beat}: left at {fl:.3}");
            assert!((fr - (440.0 + beat / 2.0)).abs() <= 0.05, "Δ {beat}: right at {fr:.3}");
            // The analysis tap is the sum before the shift: the plain tone.
            assert!((peak(&tail(&m), sr, 440.0) - 440.0).abs() <= 0.05);
        }
    }

    #[test]
    fn at_width_0_with_the_beat_off_both_ears_are_the_mono_chain() {
        let sr = 48000.0;
        let fx = FxState {
            filter_on: true,
            filter_type: "comb".into(),
            chorus_on: true,
            phaser_on: true,
            delay_on: true,
            reverb_on: true,
            limiter_on: true,
            ..FxState::default()
        };
        let (mut mono, mut two) = (FxChain::new(sr), FxChain::new_stereo(sr));
        mono.reset(&fx);
        two.reset(&fx);
        for k in 0..100 {
            let x = tone(128, sr, 110.0 + k as f64, 1.5);
            let (mut a, mut l, mut r, mut m) = (x.clone(), x.clone(), x.clone(), vec![0.0f32; 128]);
            mono.process(&mut a);
            two.process_stereo(&mut l, &mut r, &mut m, Space::default());
            assert_eq!((&l, &r, &m), (&a, &a, &a), "block {k}");
        }
    }

    #[test]
    fn width_spreads_the_reverb_and_the_chorus_between_the_ears() {
        let sr = 48000.0;
        let fx = FxState { chorus_on: true, reverb_on: true, limiter_on: false, ..FxState::default() };
        let mut chain = FxChain::new_stereo(sr);
        chain.reset(&fx);
        let n = 48000;
        let (mut l, mut r, mut m) = (tone(n, sr, 330.0, 0.3), tone(n, sr, 330.0, 0.3), vec![0.0f32; n]);
        for k in (0..n).step_by(128) {
            chain.process_stereo(
                &mut l[k..k + 128],
                &mut r[k..k + 128],
                &mut m[k..k + 128],
                Space { width: 1.0, ..Space::default() },
            );
        }
        let diff: f64 = l.iter().zip(&r).map(|(a, b)| f64::from(a - b).powi(2)).sum::<f64>();
        assert!(diff.sqrt() / rms(&l) / (n as f64).sqrt() > 0.1, "the ears differ");
    }

    #[test]
    fn the_delay_repeats_after_its_time() {
        let sr = 48000.0;
        let fx = FxState {
            delay_on: true,
            delay_time: 0.25,
            delay_fb: 0.5,
            delay_mix: 1.0,
            limiter_on: false,
            ..FxState::default()
        };
        let mut chain = FxChain::new(sr);
        chain.reset(&fx);
        let mut buf = vec![0.0f32; (sr * 0.6) as usize];
        buf[0] = 1.0;
        chain.process(&mut buf);
        // Each trip round the loop is two render quanta late, like the
        // browser's (MIN_LOOP_DELAY: the delay node's own latency and the
        // shimmer worklet's), so the nth echo is n times that late and the
        // peak is looked for in a window.
        let echo = |n: usize| {
            let c = (sr * 0.25 * n as f64) as usize + n * 2 * MIN_LOOP_DELAY as usize;
            (c - 200..c + 200).fold(0.0f32, |m, i| m.max(buf[i].abs()))
        };
        assert!(echo(1) > 0.5, "no echo at 250 ms: {}", echo(1));
        assert!(echo(2) > 0.2, "no second echo: {}", echo(2));
    }
}
