//! The picture's guard against flashing (photosensitivity, WCAG 2.3.1:
//! no more than three flashes in any second). The sound's features move
//! the picture — an onset flares the highlights and turns the light, the
//! swell pulses the exposure, the bands tint, the loudness glosses — and a
//! drum layer strikes several times a second: drawn straight, every hit
//! was a flash of the whole screen. Here the features the picture reads are
//! slewed first: a flare rises over ~150 ms and fades over ~0.6 s and is
//! capped, the rest glide over a fraction of a second, so a run of hits
//! becomes a glow that breathes with the rhythm instead of a strobe. The
//! sound is untouched; only what the picture is told changes.
//! `syn-bench --flashes` counts the flashes per second, the WCAG way.

use crate::features::AudioFeatures;

/// How fast a flare may rise and how slowly it fades, seconds, and how bright
/// it may get (of the gene's full 1).
const ONSET_RISE: f64 = 0.15;
const ONSET_FALL: f64 = 0.6;
const ONSET_MAX: f64 = 0.6;
/// The other features' glide, seconds.
const SWELL: f64 = 0.5;
const BANDS: f64 = 0.25;
const LOUDNESS: f64 = 0.15;
const BRIGHTNESS: f64 = 0.25;
/// A gap longer than this (a stop, a dropped frame) restarts the glide.
const MAX_DT: f64 = 0.25;

#[derive(Clone, Debug, Default)]
pub struct Calm {
    last: Option<f64>,
    f: AudioFeatures,
}

fn toward(x: &mut f64, target: f64, dt: f64, tau: f64) {
    *x += (1.0 - (-dt / tau).exp()) * (target - *x);
}

impl Calm {
    /// The features as the picture may see them at engine time `t`.
    pub fn apply(&mut self, raw: &AudioFeatures, t: f64) -> AudioFeatures {
        let dt = match self.last {
            Some(last) => (t - last).clamp(0.0, MAX_DT),
            // the first frame: start where the sound is, without a flare
            None => {
                self.f = AudioFeatures { onset: 0.0, ..*raw };
                0.0
            }
        };
        self.last = Some(t);
        if dt == 0.0 {
            return self.f;
        }
        let f = &mut self.f;
        let onset = raw.onset.clamp(0.0, ONSET_MAX);
        let tau = if onset > f.onset { ONSET_RISE } else { ONSET_FALL };
        toward(&mut f.onset, onset, dt, tau);
        toward(&mut f.swell, raw.swell, dt, SWELL);
        toward(&mut f.loudness, raw.loudness, dt, LOUDNESS);
        toward(&mut f.brightness, raw.brightness, dt, BRIGHTNESS);
        toward(&mut f.low, raw.low, dt, BANDS);
        toward(&mut f.mid, raw.mid, dt, BANDS);
        toward(&mut f.high, raw.high, dt, BANDS);
        self.f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hits four times a second at 60 frames a second: the onset feature
    /// spikes to 1 and decays; what the picture sees swings far less.
    #[test]
    fn a_run_of_hits_is_a_glow_not_a_strobe() {
        let mut calm = Calm::default();
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for k in 0..600 {
            let t = k as f64 / 60.0;
            let since = t % 0.25;
            let raw = AudioFeatures { onset: (-since / 0.08).exp(), ..Default::default() };
            let seen = calm.apply(&raw, t).onset;
            if t > 3.0 {
                lo = lo.min(seen);
                hi = hi.max(seen);
            }
        }
        assert!(hi <= ONSET_MAX, "{hi}");
        assert!(hi - lo < 0.2, "the flare swings {lo}..{hi} with every hit");
        assert!(lo > 0.05, "but it is there: {lo}");
    }

    #[test]
    fn a_steady_sound_is_seen_as_it_is() {
        let mut calm = Calm::default();
        let raw = AudioFeatures {
            loudness: 0.4,
            brightness: 0.6,
            swell: 0.1,
            low: 0.3,
            mid: 0.2,
            high: 0.1,
            onset: 0.0,
        };
        let mut seen = AudioFeatures::default();
        for k in 0..300 {
            seen = calm.apply(&raw, k as f64 / 60.0);
        }
        assert!(
            (seen.loudness - 0.4).abs() < 1e-6
                && (seen.high - 0.1).abs() < 1e-6
                && (seen.swell - 0.1).abs() < 1e-3
        );
        // the same time twice (a paused clock): nothing moves
        let again = calm.apply(&AudioFeatures { onset: 1.0, ..raw }, 299.0 / 60.0);
        assert_eq!(again.onset, seen.onset);
    }
}
