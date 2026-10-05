//! Click detection, ported from the web app's `src/analysis/clicks.ts`
//! (after formula-synth's rec.mjs): a click is a short frame whose
//! high-frequency energy (RMS of the second difference ≈ a high-pass) jumps
//! far above the signal's typical level. Relative to the median, so steady
//! noise or bright timbres don't count — only sudden discontinuities do.
//!
//! It cannot tell a pluck from a click (a plucked string over a dark sustain
//! reads as a burst); a log-frequency waterfall
//! ([`super::spectrogram`]) can.

const FRAME_SECONDS: f64 = 0.005;
/// Frame HF energy vs the median frame.
const RATIO: f64 = 6.0;
/// Ignore anything quieter than this.
const ABS_FLOOR: f64 = 0.002;
/// One click per 40 ms at most.
const MIN_GAP: f64 = 0.04;

/// Times (s, rounded to ms) of detected clicks.
pub fn detect_clicks(x: &[f32], sr: f64) -> Vec<f64> {
    let frame = ((sr * FRAME_SECONDS).round() as usize).max(4);
    let nf = x.len() / frame;
    if nf < 3 {
        return Vec::new();
    }
    let dd = |j: usize| -> f64 {
        let d = f64::from(x[j]) - 2.0 * f64::from(x[j - 1]) + f64::from(x[j - 2]);
        d * d
    };
    let hf: Vec<f64> = (0..nf)
        .map(|f| {
            let mut e: f64 = (2..frame).map(|i| dd(f * frame + i)).sum();
            // also the two samples straddling the previous frame boundary
            if f > 0 {
                e += dd(f * frame) + dd(f * frame + 1);
            }
            (e / frame as f64).sqrt()
        })
        .collect();
    let mut sorted = hf.clone();
    sorted.sort_by(f64::total_cmp);
    let med = Some(sorted[nf / 2]).filter(|m| *m != 0.0).unwrap_or(1e-12);
    let mut times = Vec::new();
    let mut last = f64::NEG_INFINITY;
    for (f, h) in hf.iter().enumerate().skip(1) {
        let t = (f * frame) as f64 / sr;
        if *h > med * RATIO && *h > ABS_FLOOR && t - last > MIN_GAP {
            times.push((t * 1000.0).round() / 1000.0);
            last = t;
        }
    }
    times
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_in_a_sine_is_one_click_and_the_sine_none() {
        let sr = 22050.0;
        let mut x: Vec<f32> = (0..sr as usize).map(|i| (0.3 * (i as f64 * 0.05).sin()) as f32).collect();
        assert!(detect_clicks(&x, sr).is_empty());
        for v in &mut x[11025..] {
            *v += 0.2;
        }
        assert_eq!(detect_clicks(&x, sr), vec![0.499], "the frame the step falls in");
    }
}
