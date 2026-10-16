//! Space around the head (PLAN-BINAURAL.md decision 4): each source of the
//! point — a formula slot, a sample layer — put at an azimuth by a structural
//! head model (Brown & Duda 1998), in the stereo engine only.
//!
//! - **ITD**: Woodworth's ray-traced delay, τ(θ) = (a/c)(|θ| + sin |θ|), at
//!   most 0.66 ms at ±90°. The near ear hears the source at once, the far
//!   ear τ later, through a fractional delay (linear interpolation).
//! - **ILD**: the head-shadow filter H(ω, θ) = (1 + jαω/2ω₀)/(1 + jω/2ω₀),
//!   ω₀ = c/a, α(θ) = 1.05 + 0.95·cos(θ − θ_ear), θ_ear = ±100°: a shelf
//!   that lifts the top on the near side and shadows it on the far side.
//!   Here it is taken **relative to straight ahead**, H(θ)/H(0), so a source
//!   at 0° passes untouched (the stereo engine at width 0 is the mono one).
//!   That ratio is 1 + g·HP(ω) with HP a fixed one-pole high-pass and
//!   g = α/α₀ − 1: one filter whose state never depends on θ, and a gain
//!   that is exactly 0 in the centre.
//!
//! Both are taken from θ once a block (one `sin`, one `cos`) and glided
//! linearly across it, so a source an LFO sweeps does not click. Front/back
//! and elevation are not resolved (a known limit; phase B6 is the path out).

/// Head radius, m.
const HEAD_RADIUS: f64 = 0.0875;
/// Speed of sound, m/s.
const SPEED_OF_SOUND: f64 = 343.0;
/// Where the ears are, degrees from straight ahead.
const EAR_DEG: f64 = 100.0;
/// The largest ITD, seconds (θ = 90°), with a margin for the ring.
const MAX_ITD: f64 = HEAD_RADIUS / SPEED_OF_SOUND * (std::f64::consts::FRAC_PI_2 + 1.0);

/// Woodworth's ITD for azimuth `theta` (radians, |θ| ≤ π/2), seconds.
pub fn itd(theta: f64) -> f64 {
    let t = theta.abs().min(std::f64::consts::FRAC_PI_2);
    HEAD_RADIUS / SPEED_OF_SOUND * (t + t.sin())
}

/// α of the head-shadow filter for an ear at `ear` (radians), from the
/// source's `cos θ` and `sin θ`.
fn alpha(cos_t: f64, sin_t: f64, ear: f64) -> f64 {
    // cos(θ − ear) = cos θ cos ear + sin θ sin ear
    1.05 + 0.95 * (cos_t * ear.cos() + sin_t * ear.sin())
}

/// Where a source is, as one block's parameters.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Place {
    /// Delay of each ear, samples.
    delay: [f64; 2],
    /// High-pass gain of each ear (0 in the centre).
    shade: [f64; 2],
}

/// One ear's path: a short fractional delay line and the shadow's high-pass.
#[derive(Clone, Debug)]
struct Ear {
    buf: Vec<f64>,
    /// The high-pass's last input and output.
    x1: f64,
    y1: f64,
}

/// The structural head model for one source.
#[derive(Clone, Debug)]
pub struct HeadModel {
    ears: [Ear; 2],
    /// Where the ring writes next (shared by both ears).
    w: usize,
    /// The high-pass: y = b·(x − x₁) + p·y₁.
    hp_b: f64,
    hp_p: f64,
    ear_angle: [f64; 2],
    alpha0: f64,
    sr: f64,
    /// Where the source was at the end of the last block.
    at: Place,
    /// Nothing has moved it off centre since the last [`HeadModel::clear`]
    /// (or since it came back and settled): it can be skipped.
    centred: bool,
}

impl HeadModel {
    pub fn new(sr: f64) -> HeadModel {
        let len = (MAX_ITD * sr).ceil() as usize + 3;
        let ear = Ear { buf: vec![0.0; len], x1: 0.0, y1: 0.0 };
        let ear_angle = [-EAR_DEG.to_radians(), EAR_DEG.to_radians()];
        let alpha0 = alpha(1.0, 0.0, ear_angle[1]);
        // The shadow's pole, 1 + s·α₀/(2ω₀), by the bilinear transform: the
        // high-pass s·τ/(1 + s·τ), τ = α₀/(2ω₀).
        let tau = alpha0 * HEAD_RADIUS / (2.0 * SPEED_OF_SOUND);
        let k = 2.0 * sr * tau;
        HeadModel {
            ears: [ear.clone(), ear],
            w: 0,
            hp_b: k / (1.0 + k),
            hp_p: (k - 1.0) / (k + 1.0),
            ear_angle,
            alpha0,
            sr,
            at: Place::default(),
            centred: true,
        }
    }

    /// Drops what is in the delay lines and filters, and recentres.
    pub fn clear(&mut self) {
        for e in &mut self.ears {
            e.buf.fill(0.0);
            e.x1 = 0.0;
            e.y1 = 0.0;
        }
        self.at = Place::default();
        self.centred = true;
    }

    /// Sitting in the centre, settled: [`HeadModel::process`] only adds.
    pub fn is_centred(&self) -> bool {
        self.centred
    }

    fn place(&self, theta: f64) -> Place {
        let t = theta.clamp(-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2);
        if t == 0.0 {
            return Place::default();
        }
        let (sin_t, cos_t) = t.sin_cos();
        let d = itd(t) * self.sr;
        // A source on the right (θ > 0) reaches the left ear late.
        let delay = if t > 0.0 { [d, 0.0] } else { [0.0, d] };
        let shade = self.ear_angle.map(|ear| alpha(cos_t, sin_t, ear) / self.alpha0 - 1.0);
        Place { delay, shade }
    }

    /// Adds `x` (one block) into `l` and `r` from azimuth `theta` (radians,
    /// positive to the right), glided from where the last block left it. At
    /// 0, and settled there, it adds `x` to both unchanged — bit for bit.
    pub fn process(&mut self, x: &[f32], theta: f64, l: &mut [f32], r: &mut [f32]) {
        let to = self.place(theta);
        if to == Place::default() && self.centred {
            for ((a, b), v) in l.iter_mut().zip(r.iter_mut()).zip(x) {
                *a += *v;
                *b += *v;
            }
            return;
        }
        if self.centred {
            // Leaving the centre: the lines and the filter have not been
            // fed while it sat there. Start them from silence; the glide
            // starts at 0 delay and 0 shade, so the first sample is `x`.
            for e in &mut self.ears {
                e.buf.fill(0.0);
                e.x1 = 0.0;
                e.y1 = 0.0;
            }
            self.centred = false;
        }
        let from = self.at;
        let n = x.len();
        let step = 1.0 / n as f64;
        let len = self.ears[0].buf.len();
        let (hp_b, hp_p) = (self.hp_b, self.hp_p);
        for (k, v) in x.iter().enumerate() {
            let s = f64::from(*v);
            let f = (k + 1) as f64 * step;
            self.w = if self.w + 1 == len { 0 } else { self.w + 1 };
            let w = self.w;
            for (ei, (e, o)) in self.ears.iter_mut().zip([&mut l[k], &mut r[k]]).enumerate() {
                e.buf[w] = s;
                let d = from.delay[ei] + (to.delay[ei] - from.delay[ei]) * f;
                let di = d as usize;
                let frac = d - di as f64;
                let i0 = (w + len - di) % len;
                let i1 = if i0 == 0 { len - 1 } else { i0 - 1 };
                let y = e.buf[i0] * (1.0 - frac) + e.buf[i1] * frac;
                let hp = hp_b * (y - e.x1) + hp_p * e.y1;
                e.x1 = y;
                e.y1 = hp;
                let g = from.shade[ei] + (to.shade[ei] - from.shade[ei]) * f;
                *o += (y + g * hp) as f32;
            }
        }
        self.at = to;
        // Back in the centre and the glide done: skip from the next block.
        if to == Place::default() {
            self.centred = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f64 = 48000.0;
    const N: usize = 128;

    fn run(theta: f64, x: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut h = HeadModel::new(SR);
        let (mut l, mut r) = (vec![0.0f32; x.len()], vec![0.0f32; x.len()]);
        // Settle at θ first (a block of silence), so the impulse sees no glide.
        let (mut sl, mut sr) = ([0.0f32; N], [0.0f32; N]);
        h.process(&[0.0; N], theta, &mut sl, &mut sr);
        for (k, chunk) in x.chunks(N).enumerate() {
            let o = k * N;
            h.process(chunk, theta, &mut l[o..o + chunk.len()], &mut r[o..o + chunk.len()]);
        }
        (l, r)
    }

    fn arrival(x: &[f32]) -> f64 {
        // The centre of mass of the first lobe: a fractional delay splits
        // the impulse between two samples.
        let peak = x.iter().map(|v| v.abs()).fold(0.0f32, f32::max);
        let first = x.iter().position(|v| v.abs() > 0.05 * peak).unwrap_or(0);
        let (mut m, mut w) = (0.0, 0.0);
        for (i, v) in x.iter().enumerate().skip(first).take(3) {
            m += i as f64 * f64::from(v.abs());
            w += f64::from(v.abs());
        }
        m / w
    }

    fn band_level(x: &[f32], hz: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (n, v) in x.iter().enumerate() {
            let a = std::f64::consts::TAU * hz * n as f64 / SR;
            re += f64::from(*v) * a.cos();
            im += f64::from(*v) * a.sin();
        }
        (re * re + im * im).sqrt()
    }

    #[test]
    fn a_source_on_the_right_reaches_the_right_ear_first_by_woodworths_delay() {
        let mut x = vec![0.0f32; 1024];
        x[100] = 1.0;
        let (l, r) = run(std::f64::consts::FRAC_PI_2, &x);
        let tau = itd(std::f64::consts::FRAC_PI_2);
        assert!((tau - 0.000_656).abs() < 0.000_002, "τ(90°) = {tau}");
        let d = arrival(&l) - arrival(&r);
        assert!((d - tau * SR).abs() <= 1.0, "left {d:.2} samples late, Woodworth says {:.2}", tau * SR);
        // Louder above 2 kHz on the near side, darker on the far side.
        for hz in [3000.0, 8000.0] {
            assert!(band_level(&r, hz) > 1.5 * band_level(&l, hz), "{hz} Hz");
        }
        // Below the shadow, both ears hear it about equally.
        let low = band_level(&r, 100.0) / band_level(&l, 100.0);
        assert!((low - 1.0).abs() < 0.1, "100 Hz: {low}");
        // And the mirror image on the left.
        let (l2, r2) = run(-std::f64::consts::FRAC_PI_2, &x);
        assert_eq!(l2, r, "left is right mirrored");
        assert_eq!(r2, l);
    }

    #[test]
    fn in_the_centre_both_ears_get_the_source_bit_for_bit() {
        let x: Vec<f32> = (0..1024).map(|i| ((i as f32) * 0.37).sin()).collect();
        let (l, r) = run(0.0, &x);
        assert_eq!(l, x);
        assert_eq!(r, x);
    }

    #[test]
    fn a_source_swept_round_the_head_does_not_click() {
        // An LFO's sweep at block rate: −90° … 90° and back, twice a second,
        // a 1 kHz tone. The largest sample-to-sample step stays that of the
        // tone itself (2π·1000/48000 ≈ 0.13 at full scale), plus the shade.
        let mut h = HeadModel::new(SR);
        let (mut l, mut r) = ([0.0f32; N], [0.0f32; N]);
        let (mut last_l, mut last_r, mut worst) = (0.0f32, 0.0f32, 0.0f32);
        let mut phase = 0.0f64;
        for b in 0..(SR as usize * 2 / N) {
            let t = (b * N) as f64 / SR;
            let theta = std::f64::consts::FRAC_PI_2 * (std::f64::consts::TAU * 2.0 * t).sin();
            let x: [f32; N] = std::array::from_fn(|_| {
                phase += std::f64::consts::TAU * 1000.0 / SR;
                (0.5 * phase.sin()) as f32
            });
            l.fill(0.0);
            r.fill(0.0);
            h.process(&x, theta, &mut l, &mut r);
            for (a, c) in l.iter().zip(&r) {
                worst = worst.max((a - last_l).abs()).max((c - last_r).abs());
                last_l = *a;
                last_r = *c;
            }
        }
        assert!(worst < 0.2, "a step of {worst}");
    }

    #[test]
    fn coming_back_to_the_centre_ends_in_the_bypass() {
        let mut h = HeadModel::new(SR);
        let (mut l, mut r) = ([0.0f32; N], [0.0f32; N]);
        h.process(&[0.1; N], 0.5, &mut l, &mut r);
        assert!(!h.centred);
        h.process(&[0.1; N], 0.0, &mut l, &mut r);
        assert!(h.centred);
    }
}
