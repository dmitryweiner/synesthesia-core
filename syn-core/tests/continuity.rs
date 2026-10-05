//! Phase continuity (the web app's tests/continuity.test.ts, moved here with
//! the sound — synesthesia PLAN-CORE.md phase 3): changing a frequency
//! parameter (an LFO route, a morph step, a preset switch) must bend the
//! pitch, never jump the waveform. An oscillator written as sin(2π·f·t) with
//! absolute t jumps by 2π·Δf·t per change — after a minute, an arbitrary
//! phase step on every block, heard as harsh beating.

use syn_core::analysis::clicks::detect_clicks;
use syn_core::dsp::rng::Mulberry32;
use syn_core::modmatrix::{LfoDef, LfoShape, ModRoute, ParamRanges};
use syn_core::schema::formula_defaults;
use syn_core::{FormulaGenerator, FormulaId, Params, BLOCK};

const SR: f64 = 24000.0;

/// Whole blocks only: a truncated last block fakes a discontinuity.
fn render(g: &mut FormulaGenerator, seconds: f64) -> Vec<f32> {
    let blocks = (SR * seconds / BLOCK as f64).ceil() as usize;
    let mut out = vec![0.0f32; blocks * BLOCK];
    for b in out.chunks_mut(BLOCK) {
        g.fill(b);
    }
    out
}

/// Formula → its frequency-like params (what LFOs and morphs bend), and
/// params that make a struck one strike often enough to hear.
fn tonal() -> Vec<(&'static str, Vec<&'static str>, Vec<(&'static str, f64)>)> {
    vec![
        ("fm", vec!["fc", "fm"], vec![]),
        ("additive", vec!["fund", "move", "N"], vec![]),
        ("pm", vec!["f", "f2pm"], vec![]),
        ("beats", vec!["fbeat", "df"], vec![]),
        ("dist", vec!["fd"], vec![]),
        ("quasi", vec!["fq", "wq"], vec![]),
        ("logistic", vec!["base"], vec![]),
        ("shepard", vec!["shepBase"], vec![]),
        ("bell", vec!["bellF0"], vec![("bellPeriod", 20.0), ("bellDecay", 8.0)]),
        ("risset", vec!["rissF0"], vec![("rissPeriod", 20.0), ("rissDecay", 12.0)]),
        ("bowl", vec!["bowlF", "bowlBeat"], vec![]),
    ]
}

fn base(id: &str, extra: &[(&str, f64)]) -> Params {
    let mut p = formula_defaults(id);
    for (k, v) in extra {
        p.insert((*k).to_string(), *v);
    }
    p.insert("gain".to_string(), 0.3);
    p
}

fn generator(id: &str, params: &Params, seed: u32) -> FormulaGenerator {
    FormulaGenerator::new(
        FormulaId::parse(id).expect("a formula"),
        SR,
        params.clone(),
        Box::new(Mulberry32::new(seed)),
    )
}

/// HF roughness: RMS of the 2nd difference over RMS of the 1st — amplitude-free.
fn roughness(x: &[f32]) -> f64 {
    let (mut d1, mut d2) = (0.0, 0.0);
    for i in 2..x.len() {
        let a = f64::from(x[i]) - f64::from(x[i - 1]);
        let b = f64::from(x[i]) - 2.0 * f64::from(x[i - 1]) + f64::from(x[i - 2]);
        d1 += a * a;
        d2 += b * b;
    }
    (d2 / d1.max(1e-30)).sqrt()
}

#[test]
fn a_slow_shallow_lfo_does_not_roughen_any_oscillator_a_minute_in() {
    for (id, params, extra) in tonal() {
        let b = base(id, &extra);
        let mut plain = generator(id, &b, 7);
        let mut modded = generator(id, &b, 7);
        let ranges: ParamRanges =
            params.iter().map(|p| ((*p).to_string(), [b[*p] * 0.5, b[*p] * 2.0])).collect();
        let routes: Vec<ModRoute> = params
            .iter()
            .map(|p| ModRoute {
                src: 0,
                target: id.to_string(),
                param: (*p).to_string(),
                depth: 0.05,
                exp: true,
            })
            .collect();
        modded.set_mod(&[LfoDef { shape: LfoShape::Sine, rate: 0.5, phase: 0.0 }], &routes, &ranges);
        render(&mut plain, 60.0); // let absolute time grow: the bug scales with t
        render(&mut modded, 60.0);
        let r0 = roughness(&render(&mut plain, 3.0));
        let r1 = roughness(&render(&mut modded, 3.0));
        assert!(r1 / r0 < 1.3, "{id}: roughness ×{:.2} at 60–63 s", r1 / r0);
    }
}

#[test]
fn a_step_in_every_frequency_param_does_not_click() {
    for (id, params, extra) in tonal() {
        if id == "bell" || id == "risset" {
            continue; // struck: covered above
        }
        let b = base(id, &extra);
        let mut g = generator(id, &b, 8);
        render(&mut g, 30.0);
        let mut joined = render(&mut g, 1.0);
        let stepped: Params = params.iter().map(|p| ((*p).to_string(), b[*p] * 1.37)).collect();
        g.set(&stepped);
        joined.extend(render(&mut g, 1.0));
        assert_eq!(detect_clicks(&joined, SR), Vec::<f64>::new(), "{id}: a click at the step");
    }
}

#[test]
fn the_additive_harmonic_count_can_step_without_a_click() {
    let mut b = formula_defaults("additive");
    b.extend([("gain".to_string(), 0.4), ("N".to_string(), 12.0), ("fund".to_string(), 200.0)]);
    let mut g = generator("additive", &b, 9);
    render(&mut g, 5.0);
    let mut joined = render(&mut g, 0.5);
    for n in [13.0, 11.0] {
        g.set(&Params::from([("N".to_string(), n)]));
        joined.extend(render(&mut g, 0.5));
    }
    assert_eq!(detect_clicks(&joined, SR), Vec::<f64>::new());
}
