//! What the stereo engine costs in wasm (PLAN-BINAURAL.md B0/B1 budget), the
//! same cases as `syn-bench --stereo` natively. Not part of `check.sh` — it
//! times, it does not test:
//!
//! ```text
//! cargo test -p syn-wasm --release --target wasm32-unknown-unknown --test stereo_cost -- --include-ignored --nocapture
//! ```
#![cfg(target_arch = "wasm32")]

use syn_core::fx::hilbert::{rotation, Shifter};
use syn_core::spatial::HeadModel;
use syn_core::state::{presets, AppState, BinState, AZ};
use syn_core::{Engine, BLOCK};
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance)]
    fn now() -> f64;
}

/// What `syn-bench --stereo` calls a spread point: voices around the head,
/// width 0.8, a 6 Hz beat.
fn spread(s: &AppState) -> AppState {
    let mut s = s.clone();
    let on: Vec<String> = s.enabled_formulas().iter().map(|id| id.to_string()).collect();
    for (k, id) in on.iter().enumerate() {
        let az = if on.len() == 1 { 40.0 } else { -70.0 + 140.0 * k as f64 / (on.len() - 1) as f64 };
        s.audio.formulas.get_mut(id).unwrap().params.insert(AZ.into(), az);
    }
    s.bin = BinState { on: true, beat: 6.0, width: 0.8 };
    s
}

#[wasm_bindgen_test]
#[ignore]
fn stereo_cost_in_wasm() {
    let sr = 48000.0;
    let secs = 10.0;
    let blocks = (secs * sr) as usize / BLOCK;
    let per = |t0: f64| (now() - t0) * 1e6 / (blocks * BLOCK) as f64;
    let time = |state: &AppState, two: bool| {
        let mut e = if two { Engine::new_stereo(sr, state, 1) } else { Engine::new(sr, state, 1) };
        let (mut l, mut r) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
        let t0 = now();
        for _ in 0..blocks {
            if two {
                e.render_stereo(&mut l, &mut r);
            } else {
                e.render(&mut l);
            }
        }
        per(t0)
    };
    for (i, p) in presets().iter().enumerate().take(15) {
        let (mono, neutral, wide) =
            (time(&p.state, false), time(&p.state, true), time(&spread(&p.state), true));
        console_log!(
            "{i:2} {:18} mono {mono:7.1}  stereo {neutral:7.1}  spread {wide:7.1} ns/sample  {:4.2}×  {:5.2} % of a core",
            p.name,
            wide / mono,
            wide * sr / 1e7
        );
    }
    let x = [0.1f32; BLOCK];
    let (mut l, mut r) = ([0.0f32; BLOCK], [0.0f32; BLOCK]);
    for sources in [1usize, 5] {
        let mut heads: Vec<HeadModel> = (0..sources).map(|_| HeadModel::new(sr)).collect();
        let t0 = now();
        for b in 0..blocks {
            for (k, h) in heads.iter_mut().enumerate() {
                h.process(&x, 0.3 + 0.001 * ((b + k) % 1000) as f64, &mut l, &mut r);
            }
        }
        console_log!("head model, {sources} source(s): {:7.1} ns/sample", per(t0));
    }
    let (mut sl, mut sr_) = (Shifter::new(sr), Shifter::new(sr));
    let (rl, rr) = (rotation(-3.0, sr), rotation(3.0, sr));
    let t0 = now();
    for _ in 0..blocks {
        for (a, b) in l.iter_mut().zip(r.iter_mut()) {
            *a = sl.tick(f64::from(*a), rl) as f32 * 0.5;
            *b = sr_.tick(f64::from(*b), rr) as f32 * 0.5;
        }
        sl.renormalize();
        sr_.renormalize();
    }
    console_log!("shifter, both ears: {:7.1} ns/sample", per(t0));
}
