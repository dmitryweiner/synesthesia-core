//! What the sequencer costs in wasm (PLAN-SAMPLES-SCRIPTS.md S1/S2 budget),
//! the same cases as `syn-bench --seq` and `--patterns` natively. Not part
//! of `check.sh` — it times, it does not test:
//!
//! ```text
//! cargo test -p syn-wasm --release --target wasm32-unknown-unknown --test seq_cost -- --include-ignored --nocapture
//! ```
#![cfg(target_arch = "wasm32")]

use syn_core::seq::{parse, Events, Frac, Layer, Mode, Sample, SampleBank, SeqProgram, SeqState, Sequencer};
use syn_core::BLOCK;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = performance)]
    fn now() -> f64;
}

#[wasm_bindgen_test]
#[ignore]
fn sequencer_cost_in_wasm() {
    let sr = 48000.0;
    for src in ["bd sd [hh hh] ~", "[[0 [1 2]] <3 4 [5 6]>]*2 , 0(7,16) , hh*16?"] {
        let p = parse(src).unwrap();
        let blocks = 200_000u64;
        let mut out = Events::default();
        let mut begin = Frac::ZERO;
        let t0 = now();
        for k in 1..=blocks {
            let end = Frac::approx((k * BLOCK as u64) as f64 / sr * 2.0);
            out.clear();
            p.query(begin, end, 1, &mut out);
            begin = end;
        }
        console_log!("query {src:52} {:8.1} ns", (now() - t0) * 1e6 / blocks as f64);
    }

    let mut x = 1u32;
    let noise: Vec<i16> = (0..32000 * 30)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 20) as i16 - 2048
        })
        .collect();
    let bank = SampleBank::from_pcm(vec![
        ("long".into(), vec![Sample { sr: 32000, data: noise.clone(), mode: Mode::Hits }]),
        ("loop".into(), vec![Sample { sr: 32000, data: noise, mode: Mode::Bed }]),
    ]);
    let layer = |sound: &str, pattern: &str, mode: Mode| Layer {
        on: true,
        sound: sound.into(),
        pattern: pattern.into(),
        mode,
        ..Layer::default()
    };
    let cases = [
        ("1 sampler voice", SeqState { cps: 1.0 / 120.0, layers: vec![layer("long", "0", Mode::Hits)] }),
        ("16 sampler voices", SeqState { cps: 1.0, layers: vec![layer("long", "0*16", Mode::Hits)] }),
        (
            "synth drums, 4 layers",
            SeqState {
                cps: 0.5,
                layers: vec![
                    layer("bd", "0(3,8)", Mode::Hits),
                    layer("sd", "~ 0 ~ 0", Mode::Hits),
                    layer("hh", "0*16?0.3", Mode::Hits),
                    layer("oh", "<~ 0> ~ cp ~", Mode::Hits),
                ],
            },
        ),
        ("1 bed", SeqState { cps: 0.5, layers: vec![layer("loop", "", Mode::Bed)] }),
        (
            "heaviest",
            SeqState {
                cps: 1.0,
                layers: vec![
                    layer("long", "0*16", Mode::Hits),
                    layer("bd", "0(3,8) , hh*16?0.3", Mode::Hits),
                    layer("sd", "~ 0 , <oh cp>*4", Mode::Hits),
                    layer("loop", "", Mode::Bed),
                ],
            },
        ),
    ];
    for (name, s) in &cases {
        let mut q = Sequencer::new(sr, 1);
        q.set_program(Box::new(SeqProgram::new(s, Some(&bank))));
        let (mut wet, mut dry) = (vec![0.0f32; BLOCK], vec![0.0f32; BLOCK]);
        let blocks = (20.0 * sr) as usize / BLOCK;
        let t0 = now();
        for _ in 0..blocks {
            q.render(&mut wet, &mut dry);
        }
        let ns = (now() - t0) * 1e6 / (blocks * BLOCK) as f64;
        console_log!("{name:24} {ns:8.1} ns/sample  {:6.2} % of a core", ns * sr / 1e7);
    }
}
