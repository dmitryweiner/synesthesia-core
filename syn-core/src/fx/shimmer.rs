//! Shimmer: an octave-up pitch shifter in the delay's feedback loop, so every
//! echo comes back an octave higher and blooms into a halo above the drone —
//! the Eno/Lanois sound. Ported from `../synesthesia/src/dsp/shimmer.ts`.
//!
//! Two taps read a delay line at twice the write speed, half a grain apart,
//! each under a sin² window. Since sin² + cos² = 1, the shifted signal is a
//! convex mix of past input and never exceeds the input's peak: the loop is
//! bounded by its feedback gain alone, exactly as the plain echo is. On top of
//! that the shifted part is low-passed before it is read (reading at double
//! speed folds whatever sits above sr/4, and each pass climbs an octave
//! further into the low-pass, so the tail dies out) and soft-limited to ±1.
//!
//! Amount 0 passes the input through bit for bit: a point made before shimmer
//! existed sounds exactly as it did.

const GRAIN_SECONDS: f64 = 0.08;
const LP_HZ: f64 = 4500.0;
const LIMIT_KNEE: f64 = 0.5;

fn soft_limit(v: f64) -> f64 {
    let a = v.abs();
    if a <= LIMIT_KNEE {
        return v;
    }
    let over = LIMIT_KNEE + (1.0 - LIMIT_KNEE) * ((a - LIMIT_KNEE) / (1.0 - LIMIT_KNEE)).tanh();
    if v < 0.0 {
        -over
    } else {
        over
    }
}

pub struct OctaveShimmer {
    grain: usize,
    /// `sin²` over the grain, as the browser builds it (`Float32Array`).
    window: Vec<f32>,
    buf: Vec<f32>,
    mask: usize,
    lp_a: f64,
    write: usize,
    /// Position within the grain, 0..grain-1.
    count: usize,
    lp1: f64,
    lp2: f64,
}

impl OctaveShimmer {
    pub fn new(sr: f64) -> Self {
        let grain = ((GRAIN_SECONDS * sr).round() as usize).max(64);
        let window = (0..grain)
            .map(|c| {
                let s = (std::f64::consts::PI * c as f64 / grain as f64).sin();
                (s * s) as f32
            })
            .collect();
        let mut size = 1usize;
        while size < 2 * grain + 2 {
            size <<= 1;
        }
        OctaveShimmer {
            grain,
            window,
            buf: vec![0.0; size],
            mask: size - 1,
            lp_a: 1.0 - (-2.0 * std::f64::consts::PI * LP_HZ.min(0.2 * sr) / sr).exp(),
            write: 0,
            count: 0,
            lp1: 0.0,
            lp2: 0.0,
        }
    }

    /// Drops the tail, for a hard switch between points.
    pub fn clear(&mut self) {
        self.buf.fill(0.0);
        self.write = 0;
        self.count = 0;
        self.lp1 = 0.0;
        self.lp2 = 0.0;
    }

    /// One sample in, one out. `amount` 0..1 is how much of the loop is
    /// shifted; at 0 the input passes through untouched, while the line goes
    /// on recording, so turning shimmer up finds a full delay line.
    pub fn tick(&mut self, x: f64, amount: f64) -> f64 {
        let half = self.grain >> 1;
        // Two one-pole low-passes on the shifter's own input.
        self.lp1 += self.lp_a * (x - self.lp1);
        self.lp2 += self.lp_a * (self.lp1 - self.lp2);
        self.buf[self.write] = self.lp2 as f32;
        let out = if amount > 1e-6 {
            // The read delay falls by one sample per sample, so the taps read
            // at twice the write speed.
            let c2 = if self.count + half < self.grain {
                self.count + half
            } else {
                self.count + half - self.grain
            };
            let tap = |c: usize, write: usize, buf: &[f32], mask: usize, window: &[f32]| {
                f64::from(window[c]) * f64::from(buf[write.wrapping_sub(self.grain - c) & mask])
            };
            let p = tap(self.count, self.write, &self.buf, self.mask, &self.window)
                + tap(c2, self.write, &self.buf, self.mask, &self.window);
            x + amount * (soft_limit(p) - x)
        } else {
            x
        };
        self.write = (self.write + 1) & self.mask;
        self.count = if self.count + 1 < self.grain { self.count + 1 } else { 0 };
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;

    fn sine(hz: f64, n: usize) -> Vec<f64> {
        (0..n).map(|i| (std::f64::consts::TAU * hz * i as f64 / SR).sin()).collect()
    }

    /// The strongest frequency in `x`, by a plain DFT over a few candidates.
    fn strongest(x: &[f64], candidates: &[f64]) -> f64 {
        let mut best = (0.0, 0.0f64);
        for &hz in candidates {
            let (mut re, mut im) = (0.0, 0.0);
            for (i, v) in x.iter().enumerate() {
                let w = std::f64::consts::TAU * hz * i as f64 / SR;
                re += v * w.cos();
                im += v * w.sin();
            }
            let power = re * re + im * im;
            if power > best.1 {
                best = (hz, power);
            }
        }
        best.0
    }

    #[test]
    fn at_zero_it_passes_the_input_through_bit_for_bit() {
        let mut s = OctaveShimmer::new(SR);
        let x = sine(220.0, 8192);
        let out: Vec<f64> = x.iter().map(|v| s.tick(*v, 0.0)).collect();
        assert_eq!(out, x, "a point made before shimmer sounds as it did");
    }

    #[test]
    fn it_puts_the_echo_an_octave_up() {
        let mut s = OctaveShimmer::new(SR);
        let x = sine(220.0, SR as usize);
        // Fully shifted, so the fundamental is the shifted one.
        let out: Vec<f64> = x.iter().map(|v| s.tick(*v, 1.0)).collect();
        let tail = &out[out.len() / 2..];
        assert_eq!(strongest(tail, &[110.0, 220.0, 440.0, 660.0]), 440.0, "an octave above 220 Hz");
    }

    #[test]
    fn it_stays_bounded_in_a_feedback_loop() {
        // What the delay does with it: the output fed back at 0.95, the
        // highest the chain allows, for ten seconds.
        let mut s = OctaveShimmer::new(SR);
        let mut feedback = 0.0;
        let mut peak = 0.0f64;
        for i in 0..(SR as usize * 10) {
            let input =
                if i < SR as usize { (std::f64::consts::TAU * 110.0 * i as f64 / SR).sin() } else { 0.0 };
            feedback = s.tick(input + 0.95 * feedback, 1.0);
            peak = peak.max(feedback.abs());
            assert!(feedback.is_finite(), "sample {i} is not finite");
        }
        assert!(peak <= 2.0, "the loop is bounded by its gain: peak {peak}");
    }

    #[test]
    fn a_cleared_shimmer_starts_over() {
        let mut s = OctaveShimmer::new(SR);
        let x = sine(330.0, 4096);
        let first: Vec<f64> = x.iter().map(|v| s.tick(*v, 0.8)).collect();
        s.clear();
        let again: Vec<f64> = x.iter().map(|v| s.tick(*v, 0.8)).collect();
        assert_eq!(first, again);
    }
}
