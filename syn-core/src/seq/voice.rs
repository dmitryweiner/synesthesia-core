//! The voices a layer's events start (PLAN-SAMPLES-SCRIPTS.md, *Voices*):
//! a sampler reading a decoded sample, and the five synth drums. Each is a
//! plain state machine stepped per sample: no allocation, and no
//! transcendental per sample — the drums' sine is a polynomial on a wrapped
//! phase and their envelopes are multiplications (the wasm rules, C12);
//! `exp`, `sin` and `cos` run once, when a voice starts.

use crate::fx::biquad::{Biquad, BiquadType};

use super::bank::Variants;

/// Attack and release ramps, seconds: a voice never starts or stops on a
/// step.
pub const RAMP: f64 = 0.002;

/// The synthesized drums: no licence, no bytes, the same everywhere.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drum {
    /// Bass drum: a sine swept down, and a click.
    Bd,
    /// Snare: a short tone and a burst of filtered noise.
    Sd,
    /// Closed hi-hat: six square waves (the 808's metal), band-passed high.
    Hh,
    /// Open hi-hat: the same, ringing longer.
    Oh,
    /// Clap: three quick bursts of band-passed noise and a tail.
    Cp,
}

impl Drum {
    pub const ALL: [Drum; 5] = [Drum::Bd, Drum::Sd, Drum::Hh, Drum::Oh, Drum::Cp];

    pub fn parse(name: &str) -> Option<Drum> {
        Some(match name {
            "bd" => Drum::Bd,
            "sd" => Drum::Sd,
            "hh" => Drum::Hh,
            "oh" => Drum::Oh,
            "cp" => Drum::Cp,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Drum::Bd => "bd",
            Drum::Sd => "sd",
            Drum::Hh => "hh",
            Drum::Oh => "oh",
            Drum::Cp => "cp",
        }
    }
}

/// sin(2π·p) for a phase `p` in [0, 1): folded onto a quarter wave and a
/// Taylor polynomial to the 9th power (error < 4e-6) — no libm per sample.
#[inline]
fn sin_turns(p: f64) -> f64 {
    let x = if p < 0.25 {
        p
    } else if p < 0.75 {
        0.5 - p
    } else {
        p - 1.0
    };
    let t = std::f64::consts::TAU * x;
    let t2 = t * t;
    t * (1.0 - t2 / 6.0 * (1.0 - t2 / 20.0 * (1.0 - t2 / 42.0 * (1.0 - t2 / 72.0))))
}

/// The per-sample multiplier that decays by 1/e in `tau` seconds.
fn decay(tau: f64, sr: f64) -> f64 {
    (-1.0 / (tau * sr).max(1e-9)).exp()
}

/// Below this, a drum's envelopes have died away.
const SILENT: f64 = 1e-4;

/// The 808's six hi-hat oscillators, Hz.
const METAL: [f64; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

#[derive(Clone, Debug)]
pub(crate) struct DrumVoice {
    /// The tone: phase in turns, its increment heading for `inc_end` as
    /// `inc_off` decays (the bass drum's sweep).
    phase: f64,
    inc_end: f64,
    inc_off: f64,
    k_inc: f64,
    tone: f64,
    k_tone: f64,
    /// The noise or metal path: its level, its decay, the filters.
    noise: f64,
    k_noise: f64,
    rng: u32,
    metal: bool,
    sq_phase: [f64; 6],
    sq_inc: [f64; 6],
    f1: Biquad,
    f2: Biquad,
    /// The clap: samples to the next burst (`None`: no burst to come),
    /// bursts left, and the tail's decay once they are spent.
    burst_in: Option<u32>,
    burst_every: u32,
    bursts: u8,
    k_burst: f64,
    k_tail: f64,
}

impl DrumVoice {
    /// A drum at `speed` (pitch and length together, as a sample's speed
    /// does); `seed` makes its noise — the same event, the same noise.
    pub(crate) fn new(kind: Drum, sr: f64, speed: f64, seed: u32) -> DrumVoice {
        let s = speed.clamp(0.1, 10.0);
        let mut d = DrumVoice {
            phase: 0.0,
            inc_end: 0.0,
            inc_off: 0.0,
            k_inc: 1.0,
            tone: 0.0,
            k_tone: 0.0,
            noise: 0.0,
            k_noise: 0.0,
            rng: seed | 1,
            metal: false,
            sq_phase: [0.0; 6],
            sq_inc: [0.0; 6],
            f1: Biquad::new(),
            f2: Biquad::new(),
            burst_in: None,
            burst_every: 0,
            bursts: 0,
            k_burst: 0.0,
            k_tail: 0.0,
        };
        match kind {
            Drum::Bd => {
                d.inc_end = 50.0 * s / sr;
                d.inc_off = 110.0 * s / sr;
                d.k_inc = decay(0.035 / s, sr);
                d.tone = 1.0;
                d.k_tone = decay(0.28 / s, sr);
                d.noise = 0.3;
                d.k_noise = decay(0.004 / s, sr);
                d.f1.set(BiquadType::Lowpass, 3000.0 * s, 0.0, 0.0, sr);
                d.f2.set(BiquadType::Highpass, 30.0, 0.0, 0.0, sr);
            }
            Drum::Sd => {
                d.inc_end = 185.0 * s / sr;
                d.tone = 0.45;
                d.k_tone = decay(0.07 / s, sr);
                d.noise = 0.8;
                d.k_noise = decay(0.13 / s, sr);
                d.f1.set(BiquadType::Highpass, 1500.0 * s, 0.0, 0.0, sr);
                d.f2.set(BiquadType::Lowpass, 9000.0 * s, 0.0, 0.0, sr);
            }
            Drum::Hh | Drum::Oh => {
                d.metal = true;
                d.noise = 1.6;
                d.k_noise = decay(if kind == Drum::Hh { 0.04 } else { 0.3 } / s, sr);
                for (inc, f) in d.sq_inc.iter_mut().zip(METAL) {
                    *inc = f * s / sr;
                }
                d.f1.set(BiquadType::Bandpass, 10000.0 * s, 1.0, 0.0, sr);
                d.f2.set(BiquadType::Highpass, 7000.0 * s, 0.0, 0.0, sr);
            }
            Drum::Cp => {
                d.noise = 1.0;
                d.burst_every = ((0.011 / s) * sr).round().max(1.0) as u32;
                d.burst_in = Some(d.burst_every);
                d.bursts = 2;
                d.k_burst = decay(0.0035 / s, sr);
                d.k_noise = d.k_burst;
                d.k_tail = decay(0.14 / s, sr);
                d.f1.set(BiquadType::Bandpass, 1200.0 * s, 1.5, 0.0, sr);
                d.f2.set(BiquadType::Highpass, 400.0 * s, 0.0, 0.0, sr);
            }
        }
        d
    }

    /// The next sample, or `None` once the drum has died away.
    #[inline]
    pub(crate) fn next(&mut self) -> Option<f64> {
        match self.burst_in {
            Some(0) if self.bursts > 0 => {
                self.bursts -= 1;
                self.noise = 1.0;
                self.burst_in = Some(self.burst_every);
            }
            Some(0) => {
                // The bursts are spent: the tail rings out.
                self.noise = 0.7;
                self.k_noise = self.k_tail;
                self.burst_in = None;
            }
            Some(n) => self.burst_in = Some(n - 1),
            None if self.tone < SILENT && self.noise < SILENT => return None,
            None => {}
        }

        let mut x = 0.0;
        if self.tone >= SILENT {
            x += sin_turns(self.phase) * self.tone;
            self.phase += self.inc_end + self.inc_off;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
            self.inc_off *= self.k_inc;
            self.tone *= self.k_tone;
        }
        let src = if self.metal {
            let mut m = 0.0;
            for (p, inc) in self.sq_phase.iter_mut().zip(self.sq_inc) {
                m += if *p < 0.5 { 1.0 } else { -1.0 };
                *p += inc;
                if *p >= 1.0 {
                    *p -= 1.0;
                }
            }
            m / 6.0
        } else {
            // xorshift32: deterministic noise, seeded per event.
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 17;
            self.rng ^= self.rng << 5;
            f64::from(self.rng as i32) / 2_147_483_648.0
        };
        x += self.f2.tick(self.f1.tick(src)) * self.noise;
        self.noise *= self.k_noise;
        Some(x)
    }
}

/// One drum hit, whole, as the sequencer plays it at full gain (no ramp):
/// for the golden takes (`golden/drums/`) and the bench.
pub fn render_drum(kind: Drum, sr: f64, speed: f64, seed: u32) -> Vec<f32> {
    let mut d = DrumVoice::new(kind, sr, speed, seed);
    std::iter::from_fn(|| d.next()).take((sr * 10.0) as usize).map(|x| x as f32).collect()
}

/// A sampler: a decoded sample read at `rate` (speed × the sample's rate /
/// the engine's), with linear interpolation.
#[derive(Clone, Debug)]
pub(crate) struct SampleReader {
    pub(crate) v: Variants,
    pub(crate) idx: usize,
    pub(crate) pos: f64,
    pub(crate) rate: f64,
}

impl SampleReader {
    #[inline]
    pub(crate) fn next(&mut self) -> Option<f64> {
        let d = &self.v[self.idx].data;
        let i = self.pos as usize;
        let a = f64::from(*d.get(i)?);
        let b = d.get(i + 1).map_or(0.0, |&v| f64::from(v));
        let x = (a + (b - a) * (self.pos - i as f64)) * (1.0 / 32768.0);
        self.pos += self.rate;
        Some(x)
    }

    /// The same, wrapping round the end: a bed's loop is baked into the
    /// sample (an equal-power crossfade), so reading it round is seamless.
    #[inline]
    pub(crate) fn next_looped(&mut self) -> f64 {
        let d = &self.v[self.idx].data;
        let n = d.len();
        if n == 0 {
            return 0.0;
        }
        let i = (self.pos as usize).min(n - 1);
        let a = f64::from(d[i]);
        let b = f64::from(d[if i + 1 == n { 0 } else { i + 1 }]);
        let x = (a + (b - a) * (self.pos - i as f64)) * (1.0 / 32768.0);
        self.pos += self.rate;
        while self.pos >= n as f64 {
            self.pos -= n as f64;
        }
        x
    }
}

/// Not boxed: a voice starts inside `render`, which must not allocate.
#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Source {
    Idle,
    Sample(SampleReader),
    Drum(DrumVoice),
}

/// One slot of the pool.
#[derive(Clone, Debug)]
pub(crate) struct Voice {
    pub(crate) src: Source,
    gain: f64,
    env: f64,
    step: f64,
    pub(crate) releasing: bool,
    /// Start order: the oldest is stolen first.
    pub(crate) id: u64,
    pub(crate) dry: bool,
    /// Samples into the current block before it starts.
    delay: usize,
}

impl Voice {
    pub(crate) fn idle() -> Voice {
        Voice {
            src: Source::Idle,
            gain: 0.0,
            env: 0.0,
            step: 0.0,
            releasing: false,
            id: 0,
            dry: false,
            delay: 0,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        !matches!(self.src, Source::Idle)
    }

    /// Starts `src` `delay` samples into the next block rendered.
    pub(crate) fn start(&mut self, src: Source, gain: f64, dry: bool, delay: usize, id: u64, sr: f64) {
        *self = Voice { src, gain, env: 0.0, step: 1.0 / (RAMP * sr), releasing: false, id, dry, delay };
    }

    /// Fades out over [`RAMP`] (a stolen voice).
    pub(crate) fn release(&mut self, sr: f64) {
        self.releasing = true;
        self.step = -1.0 / (RAMP * sr);
    }

    /// Adds the voice into `out`.
    pub(crate) fn render(&mut self, out: &mut [f32]) {
        let from = self.delay.min(out.len());
        self.delay = 0;
        let (mut env, step, gain) = (self.env, self.step, self.gain);
        let mut ended = false;
        // The ramp: up to 1 and held, or down to 0 and done.
        let ramp = |env: &mut f64| {
            *env += step;
            if *env >= 1.0 {
                *env = 1.0;
            }
            *env > 0.0
        };
        match &mut self.src {
            Source::Idle => return,
            Source::Sample(r) => {
                for o in &mut out[from..] {
                    let (Some(x), true) = (r.next(), ramp(&mut env)) else {
                        ended = true;
                        break;
                    };
                    *o += (x * env * gain) as f32;
                }
            }
            Source::Drum(d) => {
                for o in &mut out[from..] {
                    let (Some(x), true) = (d.next(), ramp(&mut env)) else {
                        ended = true;
                        break;
                    };
                    *o += (x * env * gain) as f32;
                }
            }
        }
        self.env = env;
        if ended {
            self.src = Source::Idle;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_polynomial_sine_is_a_sine() {
        let worst = (0..10_000)
            .map(|i| {
                let p = i as f64 / 10_000.0;
                (sin_turns(p) - (std::f64::consts::TAU * p).sin()).abs()
            })
            .fold(0.0, f64::max);
        assert!(worst < 4e-6, "{worst}");
    }

    fn render(kind: Drum, sr: f64, speed: f64) -> Vec<f64> {
        let mut d = DrumVoice::new(kind, sr, speed, 12345);
        std::iter::from_fn(|| d.next()).take(sr as usize * 4).collect()
    }

    #[test]
    fn every_drum_sounds_ends_and_stays_in_range() {
        for kind in Drum::ALL {
            let x = render(kind, 48000.0, 1.0);
            let secs = x.len() as f64 / 48000.0;
            let peak = x.iter().fold(0.0f64, |m, v| m.max(v.abs()));
            assert!(peak > 0.2 && peak < 1.5, "{}: peak {peak}", kind.name());
            assert!(secs > 0.02 && secs < 3.9, "{}: lasts {secs} s", kind.name());
            assert_eq!(Drum::parse(kind.name()), Some(kind));
        }
        // Faster is shorter (and higher).
        assert!(render(Drum::Bd, 48000.0, 2.0).len() < render(Drum::Bd, 48000.0, 1.0).len());
        assert!(render(Drum::Oh, 48000.0, 1.0).len() > 4 * render(Drum::Hh, 48000.0, 1.0).len());
    }

    #[test]
    fn a_drum_lasts_as_long_at_any_rate() {
        for kind in Drum::ALL {
            let a = render(kind, 48000.0, 1.0).len() as f64 / 48000.0;
            let b = render(kind, 22050.0, 1.0).len() as f64 / 22050.0;
            assert!((a - b).abs() < 0.01, "{}: {a} s at 48 kHz, {b} s at 22.05", kind.name());
        }
    }

    #[test]
    fn the_same_event_makes_the_same_noise() {
        assert_eq!(render(Drum::Sd, 48000.0, 1.0), render(Drum::Sd, 48000.0, 1.0));
        let other: Vec<f64> = {
            let mut d = DrumVoice::new(Drum::Sd, 48000.0, 1.0, 999);
            std::iter::from_fn(|| d.next()).take(2000).collect()
        };
        assert_ne!(&render(Drum::Sd, 48000.0, 1.0)[..2000], &other[..]);
    }
}
