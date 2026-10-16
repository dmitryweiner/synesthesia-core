//! The binaural beat (PLAN-BINAURAL.md decision 3): a single-sideband
//! frequency shifter — every partial of the mix moved by the same number of
//! hertz, −Δ/2 in the left ear and +Δ/2 in the right, so the two ears hear
//! the whole sound Δ apart.
//!
//! **The Hilbert pair.** Two cascades of second-order all-passes in z⁻²,
//! `(c − z⁻²)/(1 − c·z⁻²)`, the second one a sample late, whose outputs stay
//! 90° apart over the band (Niemitalo's structure). The plan named his
//! published 4 + 4 coefficients; measured, they hold 90° only to ±1.2° over
//! 20 Hz – 20 kHz, so the coefficients are designed here instead, for the
//! engine's sample rate, by the elliptic half-band method of Laurent de
//! Soras' HIIR (`PolyphaseIir2Designer`): 7 + 7 sections, ±0.033° at
//! 48 kHz, the band [20 Hz, sr/2 − 20 Hz]. Designed once, at construction.
//!
//! **The shift.** `y = I·cos φ − Q·sin φ`, the phasor `(cos φ, sin φ)`
//! advanced by a rotation each sample (two multiplies and adds) and pulled
//! back to unit length once a block — no `sin` per sample (the wasm rule).

/// Sections in each arm.
const SECTIONS: usize = 7;
/// The band's lower edge, Hz; the upper is as far below Nyquist.
const BAND_LO: f64 = 20.0;

/// The 2·[`SECTIONS`] coefficients for `sr`: the even ones are the first
/// arm's, the odd ones the second's.
fn design(sr: f64) -> [f64; 2 * SECTIONS] {
    let n = 2 * SECTIONS;
    let tbw = (BAND_LO / sr).clamp(1e-5, 0.45);
    // The transition parameter (an elliptic modulus and its nome).
    let k = {
        let t = ((1.0 - 2.0 * tbw) * std::f64::consts::FRAC_PI_4).tan();
        t * t
    };
    let kk = (1.0 - k * k).powf(0.25);
    let e = 0.5 * (1.0 - kk) / (1.0 + kk);
    let e4 = e.powi(4);
    let q = e * (1.0 + e4 * (2.0 + e4 * (15.0 + 150.0 * e4)));
    let order = (2 * n + 1) as f64;
    let mut out = [0.0; 2 * SECTIONS];
    for (idx, c) in out.iter_mut().enumerate() {
        let m = (idx + 1) as f64;
        let mut num = 0.0;
        let mut sign = 1.0;
        for i in 0.. {
            let i = f64::from(i);
            let t = q.powf(i * (i + 1.0)) * ((2.0 * i + 1.0) * m * std::f64::consts::PI / order).sin() * sign;
            num += t;
            sign = -sign;
            if t.abs() < 1e-100 {
                break;
            }
        }
        let mut den = 0.0;
        let mut sign = -1.0;
        for i in 1.. {
            let i = f64::from(i);
            let t = q.powf(i * i) * (2.0 * i * m * std::f64::consts::PI / order).cos() * sign;
            den += t;
            sign = -sign;
            if t.abs() < 1e-100 {
                break;
            }
        }
        let ww = num * q.powf(0.25) / (den + 0.5);
        let w2 = ww * ww;
        let x = ((1.0 - w2 * k) * (1.0 - w2 / k)).sqrt() / (1.0 + w2);
        *c = (1.0 - x) / (1.0 + x);
    }
    out
}

/// One arm: [`SECTIONS`] all-passes in z⁻².
#[derive(Clone, Debug)]
struct Arm {
    c: [f64; SECTIONS],
    /// Per section: its input and output one and two samples back.
    x1: [f64; SECTIONS],
    x2: [f64; SECTIONS],
    y1: [f64; SECTIONS],
    y2: [f64; SECTIONS],
}

impl Arm {
    fn new(c: [f64; SECTIONS]) -> Arm {
        Arm { c, x1: [0.0; SECTIONS], x2: [0.0; SECTIONS], y1: [0.0; SECTIONS], y2: [0.0; SECTIONS] }
    }

    fn clear(&mut self) {
        *self = Arm::new(self.c);
    }

    #[inline]
    fn tick(&mut self, mut x: f64) -> f64 {
        for i in 0..SECTIONS {
            let y = self.c[i] * (x + self.y2[i]) - self.x2[i];
            self.x2[i] = self.x1[i];
            self.x1[i] = x;
            self.y2[i] = self.y1[i];
            self.y1[i] = y;
            x = y;
        }
        x
    }
}

/// The 90° pair: one input, two outputs in quadrature.
#[derive(Clone, Debug)]
pub struct Hilbert {
    a: Arm,
    b: Arm,
    /// The second arm's extra sample of delay.
    b_last: f64,
}

impl Hilbert {
    pub fn new(sr: f64) -> Hilbert {
        let c = design(sr);
        let arm = |off: usize| Arm::new(std::array::from_fn(|i| c[2 * i + off]));
        Hilbert { a: arm(0), b: arm(1), b_last: 0.0 }
    }

    pub fn clear(&mut self) {
        self.a.clear();
        self.b.clear();
        self.b_last = 0.0;
    }

    /// `(i, q)`: `q` lags `i` by 90° at every frequency in the band, so
    /// `i + j·q` holds only the positive frequencies of the input.
    #[inline]
    pub fn tick(&mut self, x: f64) -> (f64, f64) {
        let i = self.a.tick(x);
        let q = self.b_last;
        self.b_last = self.b.tick(x);
        (i, q)
    }
}

/// A frequency shifter: the Hilbert pair and a rotating phasor.
#[derive(Clone, Debug)]
pub struct Shifter {
    h: Hilbert,
    re: f64,
    im: f64,
}

impl Shifter {
    pub fn new(sr: f64) -> Shifter {
        Shifter { h: Hilbert::new(sr), re: 1.0, im: 0.0 }
    }

    pub fn clear(&mut self) {
        self.h.clear();
        self.re = 1.0;
        self.im = 0.0;
    }

    /// One sample shifted by the rotation `(cos ω, sin ω)`, ω = 2π·shift/sr.
    #[inline]
    pub fn tick(&mut self, x: f64, rot: (f64, f64)) -> f64 {
        let (i, q) = self.h.tick(x);
        let y = i * self.re - q * self.im;
        let re = self.re * rot.0 - self.im * rot.1;
        self.im = self.re * rot.1 + self.im * rot.0;
        self.re = re;
        y
    }

    /// Pulls the phasor back to unit length (one Newton step for 1/|z|):
    /// once a block is far more than the rounding drift needs.
    pub fn renormalize(&mut self) {
        let k = 1.5 - 0.5 * (self.re * self.re + self.im * self.im);
        self.re *= k;
        self.im *= k;
    }
}

/// The rotation for a shift of `hz` at `sr` — taken once a block.
pub fn rotation(hz: f64, sr: f64) -> (f64, f64) {
    let w = std::f64::consts::TAU * hz / sr;
    (w.cos(), w.sin())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The phase of `x` at `hz`, by correlation over whole periods.
    fn phase_at(x: &[f64], hz: f64, sr: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (n, v) in x.iter().enumerate() {
            let w = std::f64::consts::TAU * hz * n as f64 / sr;
            re += v * w.cos();
            im -= v * w.sin();
        }
        im.atan2(re)
    }

    fn worst_phase_error(sr: f64, hi: f64) -> f64 {
        let mut worst: f64 = 0.0;
        let points = 40;
        for k in 0..points {
            let hz = BAND_LO * (hi / BAND_LO).powf(f64::from(k) / f64::from(points - 1));
            let mut h = Hilbert::new(sr);
            // Settle (the lowest band edge rings longest), then measure over
            // whole periods of the tone.
            let settle = (sr * 2.0) as usize;
            let periods = (hz * 0.5).ceil().max(4.0);
            let len = (periods * sr / hz).round() as usize;
            let (mut is, mut qs) = (Vec::with_capacity(len), Vec::with_capacity(len));
            for n in 0..settle + len {
                let x = (std::f64::consts::TAU * hz * n as f64 / sr).sin();
                let (i, q) = h.tick(x);
                if n >= settle {
                    is.push(i);
                    qs.push(q);
                }
            }
            let d = (phase_at(&is, hz, sr) - phase_at(&qs, hz, sr)).to_degrees();
            let d = (d + 540.0) % 360.0 - 180.0;
            worst = worst.max((d - 90.0).abs());
        }
        worst
    }

    #[test]
    fn the_pair_stays_90_degrees_apart_over_the_band() {
        // The budget: 0.1° at 48 kHz, 0.2° at 44.1 and 22.05 kHz.
        for (sr, hi, budget) in [(48000.0, 20000.0, 0.1), (44100.0, 20000.0, 0.2), (22050.0, 10000.0, 0.2)] {
            let e = worst_phase_error(sr, hi);
            assert!(e <= budget, "{sr} Hz: {e:.4}° off 90°");
        }
    }

    /// The strongest frequency of `x` near `around`, to a hundredth of a
    /// hertz: a coarse DFT scan, then a fine one.
    fn peak(x: &[f64], sr: f64, around: f64) -> f64 {
        let power = |hz: f64| {
            let (mut re, mut im) = (0.0, 0.0);
            for (n, v) in x.iter().enumerate() {
                // A Hann window, so the neighbours do not pull the peak.
                let w = 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / x.len() as f64).cos();
                let a = std::f64::consts::TAU * hz * n as f64 / sr;
                re += w * v * a.cos();
                im += w * v * a.sin();
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

    #[test]
    fn a_sine_comes_out_shifted_by_exactly_the_shift() {
        let sr = 48000.0;
        for shift in [-3.0, 3.0, -0.25, 20.0] {
            let mut s = Shifter::new(sr);
            let rot = rotation(shift, sr);
            let n = (sr * 6.0) as usize;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let x = (std::f64::consts::TAU * 440.0 * i as f64 / sr).sin();
                out.push(s.tick(x, rot));
                if i % 128 == 127 {
                    s.renormalize();
                }
            }
            let tail = &out[(sr * 2.0) as usize..];
            let f = peak(tail, sr, 440.0 + shift);
            assert!((f - (440.0 + shift)).abs() <= 0.05, "shift {shift}: peak at {f:.3} Hz");
            // The other sideband is gone: what is left is the tone, moved.
            let level = (tail.iter().map(|v| v * v).sum::<f64>() / tail.len() as f64).sqrt();
            assert!((level - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.01, "level {level}");
        }
    }

    #[test]
    fn the_phasor_keeps_its_length() {
        let sr = 48000.0;
        let mut s = Shifter::new(sr);
        let rot = rotation(17.3, sr);
        for i in 0..(sr as usize * 120) {
            s.tick(0.0, rot);
            if i % 128 == 127 {
                s.renormalize();
            }
        }
        assert!(((s.re * s.re + s.im * s.im) - 1.0).abs() < 1e-12);
    }
}
