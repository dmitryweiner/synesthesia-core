//! The "character" of a sound, ported from the web app's
//! `src/analysis/character.ts`: what kind of sound it is, next to
//! [`super::fractal`]'s "how 1/f are its fluctuations". The presets people
//! liked most share a profile these numbers make visible — a band that never
//! breaks, heavy low end, one harmonic grid, slow change — so a new preset
//! can be checked against it instead of by ear alone.
//!
//! - `dropout`: median minus 5th percentile of 400 ms loudness, dB — how
//!   deep the sound falls out (a drone ~3–6, drips/bells 8–12)
//! - `swing`: 95th minus 5th percentile, dB — how far it breathes
//! - `low_share`: energy share below 200 Hz ("weight")
//! - `harmonicity`: share of spectral-peak energy on the best single
//!   harmonic grid (f0 30–400 Hz), mean over frames
//! - `roughness`: Plomp–Levelt sensory dissonance of the peaks (Sethares'
//!   parametrization), level-independent, mean over frames
//! - `motion_1s` / `motion_10s`: RMS change of the 48-band dB spectrum
//!   between frames 1 s / 10 s apart — fast flicker vs slow evolution

use super::fft::fft;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Character {
    pub silent: bool,
    pub dropout: f64,
    pub swing: f64,
    pub low_share: f64,
    pub harmonicity: f64,
    pub roughness: f64,
    pub motion_1s: f64,
    pub motion_10s: f64,
}

const SILENCE_DB: f64 = -60.0;
/// A silent gap reads as 100 dB down, not −240.
const FLOOR_DB: f64 = -100.0;
/// Renders start with a fade-in.
const SKIP_SECONDS: f64 = 1.0;
/// Momentary loudness, like EBU's 400 ms.
const WINDOW_SECONDS: f64 = 0.4;
const LOUD_HOP_SECONDS: f64 = 0.1;
/// ~0.19–0.26 s: resolves the harmonics of a 30 Hz grid.
const FRAME: usize = 4096;
const SPEC_HOP_SECONDS: f64 = 0.25;
const LOW_HZ: f64 = 200.0;
const PEAKS: usize = 40;
/// −40 dB below the frame's strongest bin.
const PEAK_FLOOR: f64 = 0.01;
const PEAK_MAX_HZ: f64 = 5000.0;
/// A peak within 3 % of f0 of a multiple is on the grid.
const GRID_TOLERANCE: f64 = 0.03;
const BANDS: usize = 48;
const BAND_LO_HZ: f64 = 40.0;
/// Spectrogram cells more than this below the max are floored.
const BAND_FLOOR_DB: f64 = 60.0;

fn percentile(xs: &[f64], p: f64) -> f64 {
    let mut s = xs.to_vec();
    s.sort_by(f64::total_cmp);
    if s.is_empty() {
        return f64::NAN;
    }
    s[((p * s.len() as f64).floor().max(0.0) as usize).min(s.len() - 1)]
}

#[derive(Clone, Copy)]
struct Peak {
    f: f64,
    a: f64,
}

/// The strongest spectral peaks of one magnitude frame, parabolically
/// refined, sorted by frequency.
fn peaks_of(mag: &[f64], hz_per_bin: f64) -> Vec<Peak> {
    let max = mag[2..mag.len() - 1].iter().copied().fold(0.0, f64::max);
    if max <= 0.0 {
        return Vec::new();
    }
    let k_max = (mag.len() - 2).min((PEAK_MAX_HZ / hz_per_bin).floor() as usize);
    let k_min = 2usize.max((30.0 / hz_per_bin).floor() as usize);
    let mut out = Vec::new();
    for k in k_min..=k_max {
        if mag[k] > mag[k - 1] && mag[k] >= mag[k + 1] && mag[k] > max * PEAK_FLOOR {
            let a = (mag[k - 1] + 1e-12).ln();
            let b = mag[k].ln();
            let c = (mag[k + 1] + 1e-12).ln();
            let den = a - 2.0 * b + c;
            let d = if den != 0.0 { 0.5 * (a - c) / den } else { 0.0 };
            out.push(Peak { f: (k as f64 + d) * hz_per_bin, a: mag[k] });
        }
    }
    out.sort_by(|p, q| q.a.total_cmp(&p.a));
    out.truncate(PEAKS);
    out.sort_by(|p, q| p.f.total_cmp(&q.f));
    out
}

/// Sethares' dissonance of a peak set, divided by the total amplitude.
fn roughness_of(peaks: &[Peak]) -> f64 {
    let mut r = 0.0;
    let mut total = 0.0;
    for (i, p) in peaks.iter().enumerate() {
        total += p.a;
        let s = 0.24 / (0.0207 * p.f + 18.96);
        for q in &peaks[i + 1..] {
            let x = s * (q.f - p.f);
            if x > 3.0 {
                break; // the curve is ~0 beyond this, and peaks are sorted
            }
            r += p.a.min(q.a) * ((-3.51 * x).exp() - (-5.75 * x).exp());
        }
    }
    if total > 0.0 {
        r / total
    } else {
        0.0
    }
}

/// The largest share of peak energy one harmonic grid (f0 30–400 Hz) explains.
fn harmonicity_of(peaks: &[Peak]) -> f64 {
    let energy: f64 = peaks.iter().map(|p| p.a * p.a).sum();
    if energy <= 0.0 {
        return 0.0;
    }
    let mut best = 0.0;
    let mut f0 = 30.0;
    while f0 <= 400.0 {
        let mut e = 0.0;
        for p in peaks {
            let h = (p.f / f0).round();
            if h >= 1.0 && (p.f - h * f0).abs() < GRID_TOLERANCE * f0 {
                e += p.a * p.a;
            }
        }
        if e > best {
            best = e;
        }
        f0 *= 1.005;
    }
    best / energy
}

pub fn analyze_character(signal: &[f32], sr: f64) -> Character {
    let skip = ((SKIP_SECONDS * sr).floor() as usize).min(signal.len() / 4);
    let sq: f64 = signal[skip..].iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
    let n = signal.len() - skip;
    if n <= FRAME || 10.0 * (sq / n as f64 + 1e-24).log10() < SILENCE_DB {
        let nan = f64::NAN;
        return Character {
            silent: true,
            dropout: nan,
            swing: nan,
            low_share: nan,
            harmonicity: nan,
            roughness: nan,
            motion_1s: nan,
            motion_10s: nan,
        };
    }

    // Loudness contour: 400 ms windows every 100 ms.
    let win = (WINDOW_SECONDS * sr).floor() as usize;
    let loud_hop = ((LOUD_HOP_SECONDS * sr).floor() as usize).max(1);
    let mut loud = Vec::new();
    let mut o = skip;
    while o + win <= signal.len() {
        let s: f64 = signal[o..o + win].iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
        loud.push(FLOOR_DB.max(10.0 * (s / win as f64 + 1e-24).log10()));
        o += loud_hop;
    }
    let p5 = percentile(&loud, 0.05);

    // Spectra: 4096-point frames every 250 ms.
    let hop = ((SPEC_HOP_SECONDS * sr).floor() as usize).max(1);
    let hz_per_bin = sr / FRAME as f64;
    let half = FRAME / 2;
    let hann: Vec<f64> = (0..FRAME)
        .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (FRAME - 1) as f64).cos())
        .collect();
    let f_hi = 8000.0f64.min(sr / 2.0);
    let edges: Vec<usize> = (0..=BANDS)
        .map(|b| {
            let hz = BAND_LO_HZ * (f_hi / BAND_LO_HZ).powf(b as f64 / BANDS as f64);
            ((hz / hz_per_bin).round().max(1.0) as usize).min(half)
        })
        .collect();
    let mut re = vec![0.0; FRAME];
    let mut im = vec![0.0; FRAME];
    let mut mag = vec![0.0; half];
    let mut specs: Vec<[f64; BANDS]> = Vec::new();
    let (mut low_e, mut tot_e, mut rough, mut harm) = (0.0, 0.0, 0.0, 0.0);
    let mut o = skip;
    while o + FRAME <= signal.len() {
        for i in 0..FRAME {
            re[i] = f64::from(signal[o + i]) * hann[i];
            im[i] = 0.0;
        }
        fft(&mut re, &mut im, false);
        for k in 0..half {
            // not hypot(): wasm has no fma (synesthesia PLAN-CORE.md C12)
            mag[k] = (re[k] * re[k] + im[k] * im[k]).sqrt();
        }
        for (k, m) in mag.iter().enumerate().skip(1) {
            let e = m * m;
            tot_e += e;
            if k as f64 * hz_per_bin < LOW_HZ {
                low_e += e;
            }
        }
        let mut band = [0.0; BANDS];
        for (b, slot) in band.iter_mut().enumerate() {
            let k0 = edges[b];
            let k1 = (k0 + 1).max(edges[b + 1]);
            let p: f64 = mag[k0.min(half)..k1.min(half)].iter().map(|m| m * m).sum();
            *slot = 10.0 * (p / (k1 - k0) as f64 + 1e-24).log10();
        }
        specs.push(band);
        let peaks = peaks_of(&mag, hz_per_bin);
        rough += roughness_of(&peaks);
        harm += harmonicity_of(&peaks);
        o += hop;
    }
    let frames = specs.len();

    // Motion: floor quiet cells (they flicker in dB without being heard),
    // then the RMS difference between spectra a fixed time apart.
    let top = specs.iter().flatten().copied().fold(f64::NEG_INFINITY, f64::max);
    for s in &mut specs {
        for v in s.iter_mut() {
            *v = v.max(top - BAND_FLOOR_DB);
        }
    }
    let motion = |seconds: f64| -> f64 {
        let lag = ((seconds / SPEC_HOP_SECONDS).round() as usize).max(1);
        let (mut acc, mut c) = (0.0, 0usize);
        for t in 0..specs.len().saturating_sub(lag) {
            let d: f64 = (0..BANDS).map(|b| (specs[t + lag][b] - specs[t][b]).powi(2)).sum();
            acc += (d / BANDS as f64).sqrt();
            c += 1;
        }
        if c > 0 {
            acc / c as f64
        } else {
            f64::NAN
        }
    };

    let per_frame = |x: f64| if frames > 0 { x / frames as f64 } else { 0.0 };
    Character {
        silent: false,
        dropout: percentile(&loud, 0.5) - p5,
        swing: percentile(&loud, 0.95) - p5,
        low_share: if tot_e > 0.0 { low_e / tot_e } else { 0.0 },
        harmonicity: per_frame(harm),
        roughness: per_frame(rough),
        motion_1s: motion(1.0),
        motion_10s: motion(10.0),
    }
}
