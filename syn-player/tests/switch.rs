//! A preset switch as the web app makes it (src/audio/coreEngine.ts: fade
//! out, 100 ms, `switch_to`, fade in) must not click — the web app's
//! `analyze.mjs --switch` bench, as a test (synesthesia PLAN-CORE.md phase 3).
//!
//! The click detector cannot tell a bell's attack from a click, so this
//! compares like with like, by the detector's own measure (the RMS of the
//! second difference per 5 ms frame): the fade-out is never brighter than
//! the old point simply playing on; the switch instant itself is silent;
//! and after it, the new point is no brighter than the same point playing
//! at the same LFO time (the switch keeps the LFO clock).

use syn_core::state::presets;
use syn_player::Player;

const SR: f64 = 22050.0;
const BEFORE: f64 = 4.0;
const GAP: f64 = 0.1;

fn render(p: &Player, secs: f64, out: &mut Vec<f32>) {
    let mut buf = Vec::new();
    p.render_into((secs * SR).round() as usize, &mut buf);
    out.extend_from_slice(&buf);
}

fn play_switch(from: usize, to: usize) -> Vec<f32> {
    let p = Player::new(SR, &presets()[from].state);
    p.fade_in();
    let mut out = Vec::new();
    render(&p, BEFORE, &mut out);
    p.fade_out();
    render(&p, GAP, &mut out);
    p.switch_to(presets()[to].state.clone());
    p.fade_in();
    render(&p, 1.0, &mut out);
    out
}

fn plain(index: usize, secs: f64) -> Vec<f32> {
    let p = Player::new(SR, &presets()[index].state);
    p.fade_in();
    let mut out = Vec::new();
    render(&p, secs, &mut out);
    out
}

/// The loudest 5 ms frame of high-frequency energy in [from, to) seconds.
fn hf_peak(x: &[f32], from: f64, to: f64) -> f64 {
    let frame = (SR * 0.005) as usize;
    let (a, b) = ((from * SR) as usize / frame, (to * SR) as usize / frame);
    (a.max(1)..b.min(x.len() / frame))
        .map(|f| {
            let e: f64 = (0..frame)
                .map(|i| {
                    let j = f * frame + i;
                    let d = f64::from(x[j]) - 2.0 * f64::from(x[j - 1]) + f64::from(x[j - 2]);
                    d * d
                })
                .sum();
            (e / frame as f64).sqrt()
        })
        .fold(0.0, f64::max)
}

#[test]
fn switching_between_any_two_neighbouring_presets_does_not_click() {
    let n = presets().len();
    let at = BEFORE + GAP;
    for from in 0..n {
        let to = (from + 1) % n;
        let name = format!("{} → {}", presets()[from].name, presets()[to].name);
        let x = play_switch(from, to);

        let fading = hf_peak(&x, BEFORE, at);
        let old_on = hf_peak(&plain(from, at + 0.05), BEFORE, at);
        assert!(fading <= old_on * 1.05 + 1e-6, "{name}: the fade-out {fading:.5} vs playing on {old_on:.5}");

        let instant = hf_peak(&x, at - 0.005, at + 0.01);
        assert!(instant < 2e-4, "{name}: {instant:.5} at the switch itself");

        let after = hf_peak(&x, at, at + 0.4);
        let same_time = hf_peak(&plain(to, at + 0.45), at, at + 0.4);
        assert!(
            after <= same_time * 1.5 + 2e-4,
            "{name}: after the switch {after:.5} vs the point at that time {same_time:.5}"
        );
    }
}
