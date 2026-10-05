//! The web app's sound measurements, ported (synesthesia PLAN-CORE.md C6)
//! and checked against fixtures/analysis.json, which scripts/dump-analysis.mjs
//! froze from the TypeScript: the same dry mix of every preset, made here
//! bit for bit as the dump made it, must measure the same.

use serde::Deserialize;
use syn_core::analysis::character::analyze_character;
use syn_core::analysis::clicks::detect_clicks;
use syn_core::analysis::spectrogram::{log_spectrogram, SpectrogramOptions};
use syn_core::dsp::rng::Mulberry32;
use syn_core::schema::formula_ranges;
use syn_core::state::{presets, AppState};
use syn_core::{FormulaGenerator, FormulaId, BLOCK};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Character {
    silent: bool,
    dropout: Option<f64>,
    swing: Option<f64>,
    low_share: Option<f64>,
    harmonicity: Option<f64>,
    roughness: Option<f64>,
    motion1s: Option<f64>,
    motion10s: Option<f64>,
}

#[derive(Deserialize)]
struct Spec {
    columns: usize,
    rows: usize,
    db: Vec<f32>,
    loudness: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    index: usize,
    name: String,
    character: Character,
    clicks: Vec<f64>,
    clicks_stepped: Vec<f64>,
    spectrogram: Spec,
}

#[derive(Deserialize)]
struct Fixture {
    sr: f64,
    seconds: f64,
    cases: Vec<Case>,
}

/// Left out, as in the dump: chaos turns a last-bit LFO difference into a
/// different signal (logistic: 0.24 apart after 17 s).
const CHAOTIC: [&str; 3] = ["logistic", "lorenz", "rossler"];

/// The dump's dry mix: enabled formulas in key order, chaotic ones left out,
/// seeded 1, 2, …, LFO routes applied, summed at the master gain, whole
/// blocks, then cut.
fn dry_mix(state: &AppState, sr: f64, seconds: f64) -> Vec<f32> {
    let n = (sr * seconds).round() as usize;
    let mut mix = vec![0.0f32; n.div_ceil(BLOCK) * BLOCK];
    let enabled = state.audio.formulas.iter().filter(|(id, s)| s.enabled && !CHAOTIC.contains(&id.as_str()));
    for (seed, (id, snap)) in (1..).zip(enabled) {
        let fid = FormulaId::parse(id).expect("a formula");
        let mut g = FormulaGenerator::new(fid, sr, snap.params.clone(), Box::new(Mulberry32::new(seed)));
        let routes: Vec<_> = state.modulation.routes.iter().filter(|r| &r.target == id).cloned().collect();
        g.set_mod(&state.modulation.lfos, &routes, &formula_ranges(id));
        let mut buf = vec![0.0f32; BLOCK];
        for block in mix.chunks_mut(BLOCK) {
            g.fill(&mut buf);
            for (m, x) in block.iter_mut().zip(&buf) {
                *m += x * state.audio.master_gain as f32;
            }
        }
    }
    mix.truncate(n);
    mix
}

/// To 1e-6 relative: the mixes agree to the last bit of an f32 or so, and a
/// flipped last bit moves a loudness percentile by ~1e-8 dB.
fn close(got: f64, want: Option<f64>, what: &str) {
    match want {
        None => assert!(got.is_nan(), "{what}: {got} vs NaN"),
        Some(w) => assert!((got - w).abs() <= 1e-6 * w.abs().max(1.0), "{what}: {got} vs {w}"),
    }
}

#[test]
fn every_measurement_agrees_with_the_web_app() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/analysis.json");
    let f: Fixture =
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixtures/analysis.json")).unwrap();
    assert_eq!(f.cases.len(), presets().len());
    for c in &f.cases {
        let p = &presets()[c.index];
        assert_eq!(p.name, c.name);
        let x = dry_mix(&p.state, f.sr, f.seconds);
        let name = &c.name;

        let ch = analyze_character(&x, f.sr);
        assert_eq!(ch.silent, c.character.silent, "{name}");
        close(ch.dropout, c.character.dropout, &format!("{name} dropout"));
        close(ch.swing, c.character.swing, &format!("{name} swing"));
        close(ch.low_share, c.character.low_share, &format!("{name} lowShare"));
        close(ch.harmonicity, c.character.harmonicity, &format!("{name} harmonicity"));
        close(ch.roughness, c.character.roughness, &format!("{name} roughness"));
        close(ch.motion_1s, c.character.motion1s, &format!("{name} motion1s"));
        close(ch.motion_10s, c.character.motion10s, &format!("{name} motion10s"));

        assert_eq!(detect_clicks(&x, f.sr), c.clicks, "{name}: clicks");
        let stepped: Vec<f32> = x
            .iter()
            .enumerate()
            .map(|(i, v)| (f64::from(*v) + 0.25 * ((i as f64 / f.sr + 2.5) / 5.0).floor()) as f32)
            .collect();
        assert_eq!(detect_clicks(&stepped, f.sr), c.clicks_stepped, "{name}: clicks on the steps");

        let s = &c.spectrogram;
        let got = log_spectrogram(
            &x,
            f.sr,
            SpectrogramOptions { columns: s.columns, rows: s.rows, ..Default::default() },
        );
        for (i, (g, w)) in got.db.iter().zip(&s.db).enumerate() {
            // Below −120 dB both are the FFT's own rounding, and down to −90
            // nearly so (the waterfall shows 72 dB): silence is silence there.
            if *g < -120.0 && *w < -120.0 {
                continue;
            }
            let tol = if *w < -90.0 { 0.1 } else { 1e-3 };
            assert!((g - w).abs() <= tol, "{name}: spectrogram cell {i}: {g} vs {w} dB");
        }
        for (i, (g, w)) in got.loudness.iter().zip(&s.loudness).enumerate() {
            assert!((g - w).abs() <= 1e-3, "{name}: loudness {i}: {g} vs {w} dB");
        }
    }
}
