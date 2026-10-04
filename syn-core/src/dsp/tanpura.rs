//! Tanpura: a drone that breathes in plucks, ported from
//! `../synesthesia/src/dsp/tanpura.ts`.
//!
//! Four Karplus–Strong strings tuned Pa–Sa–Sa–Sa (3/2, 2, 2, 1 × the low Sa),
//! plucked one after another in a slow cycle, so the band never breaks but the
//! picture gets a gentle, regular attack to seed on — a drone otherwise fires
//! one or two onset hits in thirty seconds.
//!
//! **Jawari**, the curved bridge's buzz: a real string slaps onto the bridge at
//! the top of each swing. Each string adds a contact pulse while it is past 60%
//! of its own swing — a smoothed step, `env·σ(8·(y/env − 0.6))`, with ~0.2 ms
//! edges at 110 Hz, so a 1/f spectrum up to ~5 kHz (a hard step would alias).
//! The pulse's height follows the swing ~120 ms behind, so the buzz blooms
//! after each pluck and fades with the string, and it is high-passed by two
//! poles, so it adds "zz" and not a second bass.
//!
//! The pluck is a finger, not a pick: a burst of two-pole low-passed noise,
//! three periods long.
//!
//! The port keeps what the web app's version keeps: the delay line is
//! `Float32Array` (the width is part of the sound), everything else is `f64`,
//! and the randomness comes from the generator's own stream, in the same order
//! — `golden/tanpura.*.f32` is the proof.

use std::f64::consts::PI;

use super::rng::Rng;

/// Pa, Sa, Sa, low Sa.
const RATIOS: [f64; 4] = [1.5, 2.0, 2.0, 1.0];
/// The two Sa beat slowly.
const CENTS: [f64; 4] = [0.0, -1.5, 1.5, 0.0];
/// Where in the cycle each string is plucked; a rest after the low Sa.
const PLUCK_AT: [f64; 4] = [0.0, 0.22, 0.44, 0.66];
const LEVEL: [f64; 4] = [0.85, 0.9, 0.9, 1.0];
const OUT: f64 = 0.5;
/// How long the finger stays on the string, in periods.
const PLUCK_PERIODS: f64 = 3.0;
/// The part of its swing past which a string touches the bridge.
const BRIDGE: f64 = 0.6;
/// Contact edge steepness (the σ gain).
const EDGE: f64 = 8.0;
/// Measured in the web app (share of energy above 1.5 kHz, defaults
/// otherwise): dry 0.033, jawari 0.5 → 0.087, jawari 1 → 0.18.
const JAWARI_GAIN: f64 = 4.0;
const BLOOM_SECONDS: f64 = 0.12;
/// The swing follower's release.
const ENV_SECONDS: f64 = 0.05;
const BUZZ_HP_HZ: f64 = 1000.0;
const MIN_HZ: f64 = 25.0;

struct KsString {
    /// `Float32Array` in the browser.
    buf: Vec<f32>,
    mask: usize,
    write: usize,
    /// Last read, for the two-point average.
    prev: f64,
    /// The swing: a peak follower of `|y|`.
    env: f64,
    /// The buzz level: the swing, ~120 ms behind.
    swell: f64,
    /// Samples of pluck left.
    excite: i32,
    excite_len: i32,
    excite_amp: f64,
    noise_lp: f64,
    noise_lp2: f64,
}

impl KsString {
    fn new(sr: f64) -> Self {
        let mut size = 1usize;
        while (size as f64) < sr / MIN_HZ + 8.0 {
            size <<= 1;
        }
        KsString {
            buf: vec![0.0; size],
            mask: size - 1,
            write: 0,
            prev: 0.0,
            env: 0.0,
            swell: 0.0,
            excite: 0,
            excite_len: 1,
            excite_amp: 0.0,
            noise_lp: 0.0,
            noise_lp2: 0.0,
        }
    }

    fn reset(&mut self) {
        self.buf.fill(0.0);
        self.write = 0;
        self.prev = 0.0;
        self.excite = 0;
        self.noise_lp = 0.0;
        self.noise_lp2 = 0.0;
        self.env = 0.0;
        self.swell = 0.0;
    }
}

pub struct Tanpura {
    sr: f64,
    strings: [KsString; 4],
    /// Cycle phase, 0..1 — just below zero, so the first sample plucks Pa.
    cyc: f64,
    dc_x: f64,
    dc_y: f64,
    buzz_x: f64,
    buzz_y: f64,
    buzz_x2: f64,
    buzz_y2: f64,
}

impl Tanpura {
    pub fn new(sr: f64) -> Self {
        Tanpura {
            sr,
            strings: std::array::from_fn(|_| KsString::new(sr)),
            cyc: -1e-12,
            dc_x: 0.0,
            dc_y: 0.0,
            buzz_x: 0.0,
            buzz_y: 0.0,
            buzz_x2: 0.0,
            buzz_y2: 0.0,
        }
    }

    pub fn reset(&mut self) {
        for s in &mut self.strings {
            s.reset();
        }
        self.cyc = -1e-12;
        self.dc_x = 0.0;
        self.dc_y = 0.0;
        self.buzz_x = 0.0;
        self.buzz_y = 0.0;
        self.buzz_x2 = 0.0;
        self.buzz_y2 = 0.0;
    }

    fn pluck(&mut self, i: usize, sa: f64, rng: &mut dyn Rng) {
        let len = ((PLUCK_PERIODS * self.sr / (sa * RATIOS[i])).round() as i32).max(2);
        let amp = 0.8 + 0.2 * rng.next();
        let s = &mut self.strings[i];
        s.excite_len = len;
        s.excite = len;
        s.excite_amp = amp;
    }

    /// One sample. `sa` is the low Sa in Hz, `cycle` the seconds the four
    /// plucks take, `jawari` 0..1, `sustain` the seconds to fade 60 dB, and
    /// `bright` 0..1 the pluck's tone.
    ///
    /// The randomness is the caller's, as it is in the browser: the generator
    /// and its tanpura draw from one stream.
    pub fn next(
        &mut self,
        rng: &mut dyn Rng,
        sa: f64,
        cycle: f64,
        jawari: f64,
        sustain: f64,
        bright: f64,
    ) -> f64 {
        let sr = self.sr;
        let sa_hz = sa.max(MIN_HZ);
        let mut next = self.cyc + 1.0 / (cycle.max(0.5) * sr);
        if next >= 1.0 {
            next -= 1.0;
            self.pluck(0, sa_hz, rng);
        }
        for (i, at) in PLUCK_AT.iter().enumerate() {
            if self.cyc < *at && next >= *at {
                self.pluck(i, sa_hz, rng);
            }
        }
        self.cyc = next;

        let jaw = jawari.clamp(0.0, 1.0) * JAWARI_GAIN;
        let bloom_a = 1.0 / (BLOOM_SECONDS * sr);
        let env_decay = (-1.0 / (ENV_SECONDS * sr)).exp();
        let t60 = sustain.max(0.2);
        let b = bright.clamp(0.0, 1.0);
        let excite_a = 1.0 - (-2.0 * PI * (500.0 + 5500.0 * b * b) / sr).exp();
        let excite_norm = (0.5 * ((2.0 - excite_a) / excite_a).sqrt() * 3.0f64.sqrt()) / PLUCK_PERIODS.sqrt();
        let mut sum = 0.0;
        let mut contact = 0.0;
        for i in 0..RATIOS.len() {
            let f = sa_hz * RATIOS[i] * 2.0f64.powf(CENTS[i] / 1200.0);
            let period = sr / f;
            // The two-point average below adds half a sample to the loop.
            let delay = (period - 0.5).max(2.0);
            let g = 10.0f64.powf(-3.0 / (t60 * f));
            let s = &mut self.strings[i];
            let pos = s.write as f64 - delay;
            let k = pos.floor();
            let frac = pos - k;
            let ki = k as i64;
            // A negative index wraps exactly as the browser's `&` on an int32
            // does: the low bits of the two's complement.
            let a0 = f64::from(s.buf[(ki as usize) & s.mask]);
            let a1 = f64::from(s.buf[((ki + 1) as usize) & s.mask]);
            let read = a0 + (a1 - a0) * frac;
            let mut v = g * 0.5 * (read + s.prev);
            s.prev = read;
            if s.excite > 0 {
                // A raised-cosine burst of two-pole low-passed noise.
                s.noise_lp += excite_a * (rng.next() * 2.0 - 1.0 - s.noise_lp);
                s.noise_lp2 += excite_a * (s.noise_lp - s.noise_lp2);
                let w = (PI * f64::from(s.excite_len - s.excite) / f64::from(s.excite_len)).sin();
                v += s.excite_amp * w * w * s.noise_lp2 * excite_norm;
                s.excite -= 1;
            }
            s.buf[s.write] = v as f32;
            s.write = (s.write + 1) & s.mask;
            sum += LEVEL[i] * read;
            s.env = read.abs().max(s.env * env_decay);
            s.swell += (s.env - s.swell) * bloom_a;
            let touch = 0.5 + 0.5 * (EDGE * (read / s.env.max(1e-9) - BRIDGE)).tanh();
            contact += s.swell * touch;
        }
        // The buzz, high-passed by two poles: its rectified low end would
        // muddy the bass.
        let hp_a = (-2.0 * PI * BUZZ_HP_HZ / sr).exp();
        self.buzz_y = hp_a * (self.buzz_y + contact - self.buzz_x);
        self.buzz_x = contact;
        self.buzz_y2 = hp_a * (self.buzz_y2 + self.buzz_y - self.buzz_x2);
        self.buzz_x2 = self.buzz_y;
        sum += jaw * self.buzz_y2;
        // DC blocker: a noise burst is not zero-mean, and the loop keeps DC.
        let x = sum * OUT;
        let y = x - self.dc_x + 0.995 * self.dc_y;
        self.dc_x = x;
        self.dc_y = y;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::rng::Mulberry32;

    const SR: f64 = 48000.0;

    /// Defaults from the web app's schema: Sa 55 Hz, a five-second cycle.
    fn run(samples: usize, jawari: f64) -> Vec<f64> {
        let mut t = Tanpura::new(SR);
        let mut rng = Mulberry32::new(12345);
        (0..samples).map(|_| t.next(&mut rng, 55.0, 5.0, jawari, 16.0, 0.45)).collect()
    }

    #[test]
    fn it_drones_and_stays_inside_its_bounds() {
        let x = run(SR as usize * 2, 0.5);
        let loud = x.iter().skip(SR as usize).map(|v| v * v).sum::<f64>() / SR;
        assert!(loud.sqrt() > 1e-3, "a drone has to be audible: {}", loud.sqrt());
        assert!(x.iter().all(|v| v.abs() < 4.0), "and bounded");
        assert!(x.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn the_first_sample_plucks_and_the_cycle_plucks_again() {
        // Four plucks in a five-second cycle: energy arrives in bursts rather
        // than all at once.
        let x = run(SR as usize * 5, 0.5);
        let window = SR as usize / 4;
        let energy: Vec<f64> =
            x.chunks(window).map(|c| c.iter().map(|v| v * v).sum::<f64>() / c.len() as f64).collect();
        assert!(energy[0] > 0.0, "the first sample plucks Pa");
        let peaks = energy.windows(3).filter(|w| w[1] > w[0] && w[1] > w[2]).count();
        assert!(peaks >= 2, "the plucks are heard as attacks: {peaks}");
    }

    #[test]
    fn the_jawari_adds_the_buzz_and_nothing_else_does() {
        let dry = run(SR as usize, 0.0);
        let buzzing = run(SR as usize, 1.0);
        let high = |x: &[f64]| {
            // A crude high-pass: the first difference's energy against the
            // signal's, which is what "more above 1.5 kHz" shows up as.
            let d: f64 = x.windows(2).map(|w| (w[1] - w[0]) * (w[1] - w[0])).sum();
            let e: f64 = x.iter().map(|v| v * v).sum::<f64>().max(1e-12);
            d / e
        };
        assert!(
            high(&buzzing) > 2.0 * high(&dry),
            "the buzz is the high end: {} vs {}",
            high(&buzzing),
            high(&dry)
        );
    }

    #[test]
    fn a_reset_starts_the_same_drone_again() {
        let mut t = Tanpura::new(SR);
        let mut rng = Mulberry32::new(7);
        let first: Vec<f64> = (0..4096).map(|_| t.next(&mut rng, 55.0, 5.0, 0.5, 16.0, 0.45)).collect();
        t.reset();
        let mut rng = Mulberry32::new(7);
        let again: Vec<f64> = (0..4096).map(|_| t.next(&mut rng, 55.0, 5.0, 0.5, 16.0, 0.45)).collect();
        assert_eq!(first, again);
    }
}
