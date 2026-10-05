//! Does the picture react to a bell, and stay calm on a drone? The web app's
//! tests/onsets.test.ts stated it as behaviour on its dry generators; here it
//! is restated on the core (synesthesia PLAN-CORE.md phase 1):
//!
//! - **the port is exact**: the web test's pipeline (generators summed at the
//!   master gain, analyser frames at round(k·sr/60), tracker, detector) gives
//!   the TypeScript's own hit counts on eight presets;
//! - **the feel**: its thresholds, with frames on block boundaries as a live
//!   AnalyserNode takes them;
//! - **the engine as the apps run it** (generators, FX, analyser, detector).

use syn_core::dsp::rng::Mulberry32;
use syn_core::features::{Analyser, FeatureTracker, OnsetDetector, FPS};
use syn_core::schema::formula_ranges;
use syn_core::state::presets;
use syn_core::{Engine, FormulaGenerator, FormulaId, BLOCK};

struct Run {
    hits: u64,
    swell_max: f64,
    swell_min: f64,
}

/// The web test's pipeline, step for step: each enabled generator (seeded
/// 1, 2, … in the point's order) with its LFO routes, summed at the master
/// gain — no slot gains, no FX — then the analyser, tracker and detector.
/// Frames end on block boundaries, or with `exact_frames` at round(k·sr/FPS)
/// as the web test's simulated AnalyserNode took them.
fn web_test_pipeline(name: &str, seconds: f64, sr: f64, exact_frames: bool) -> Run {
    let p = presets().iter().find(|p| p.name == name).unwrap_or_else(|| panic!("no preset {name}"));
    let st = &p.state;
    let n = (seconds * sr).round() as usize;
    let mut mix = vec![0.0f32; n.div_ceil(BLOCK) * BLOCK];
    for (seed, (id, snap)) in (1..).zip(st.audio.formulas.iter().filter(|(_, s)| s.enabled)) {
        let fid = FormulaId::parse(id).expect("a formula");
        let mut g = FormulaGenerator::new(fid, sr, snap.params.clone(), Box::new(Mulberry32::new(seed)));
        let routes: Vec<_> = st.modulation.routes.iter().filter(|r| &r.target == id).cloned().collect();
        g.set_mod(&st.modulation.lfos, &routes, &formula_ranges(id));
        let mut buf = vec![0.0f32; BLOCK];
        for block in mix.chunks_mut(BLOCK) {
            g.fill(&mut buf);
            for (m, x) in block.iter_mut().zip(&buf) {
                *m += x * st.audio.master_gain as f32;
            }
        }
    }
    let usable = n - n % BLOCK;
    let mut ends: Vec<usize> = if exact_frames {
        (1..).map(|k| (k as f64 * sr / FPS).round() as usize).take_while(|e| *e <= usable).collect()
    } else {
        (1..=usable / BLOCK).map(|i| i * BLOCK).collect()
    };
    ends.dedup();

    let mut an = Analyser::new(sr);
    let mut tr = FeatureTracker::new(sr);
    let mut det = OnsetDetector::default();
    let mut run = Run { hits: 0, swell_max: -1.0, swell_min: 1.0 };
    let mut from = 0;
    for end in ends {
        let fired = an.push(&mix[from..end]);
        from = end;
        if !fired {
            continue;
        }
        let t = end as f64 / sr;
        let f = tr.update(an.rms, &an.bytes, 1.0 / FPS);
        if det.update(f.onset, t) {
            run.hits += 1;
        }
        if t > 2.0 {
            run.swell_max = run.swell_max.max(f.swell);
            run.swell_min = run.swell_min.min(f.swell);
        }
    }
    run
}

/// The engine as an app plays it, from its first block.
fn engine(name: &str, seconds: f64, sr: f64) -> Run {
    let p = presets().iter().find(|p| p.name == name).unwrap_or_else(|| panic!("no preset {name}"));
    let mut e = Engine::new(sr, &p.state, 1);
    let mut buf = vec![0.0f32; BLOCK];
    let mut run = Run { hits: 0, swell_max: -1.0, swell_min: 1.0 };
    for _ in 0..(seconds * sr / BLOCK as f64) as usize {
        e.render(&mut buf);
        if e.time() > 2.0 {
            let f = e.features();
            run.swell_max = run.swell_max.max(f.swell);
            run.swell_min = run.swell_min.min(f.swell);
        }
    }
    run.hits = e.hits();
    run
}

#[test]
fn the_web_tests_pipeline_gives_the_typescripts_own_hit_counts() {
    // The TypeScript's counts, 16 s at 24 kHz (measured 2026-10-05 with the
    // web app's analyserSim + FeatureTracker + OnsetDetector).
    for (name, want) in [
        ("Bell spots", 4),
        ("Cave coral", 22),
        ("Stillness ink", 1),
        ("Wind ash", 1),
        ("Molten Polivoks", 1),
        ("Aurora", 14),
        ("Tanpura halo", 2),
        ("Whale coral", 3),
    ] {
        assert_eq!(web_test_pipeline(name, 16.0, 24000.0, true).hits, want, "{name}");
    }
}

// --- the web test's thresholds, frames on block boundaries ---------------

#[test]
fn a_bell_every_few_seconds_is_about_one_hit_per_strike() {
    let r = web_test_pipeline("Bell spots", 16.0, 24000.0, false);
    assert!((3..=6).contains(&r.hits), "{} hits", r.hits);
    assert!(r.swell_max > 0.5, "swell max {}", r.swell_max);
}

#[test]
fn drops_over_a_noise_bed_are_many_hits() {
    let r = web_test_pipeline("Cave coral", 12.0, 24000.0, false);
    assert!((12..=45).contains(&r.hits), "{} hits", r.hits);
}

#[test]
fn steady_drones_hardly_fire() {
    for name in ["Stillness ink", "Wind ash", "Molten Polivoks"] {
        let r = web_test_pipeline(name, 12.0, 24000.0, false);
        assert!(r.hits <= 4, "{name}: {} hits", r.hits);
    }
}

#[test]
fn a_shimmering_pad_fires_well_under_once_a_second() {
    let r = web_test_pipeline("Aurora", 12.0, 24000.0, false);
    assert!(r.hits <= 8, "{} hits", r.hits);
}

#[test]
fn wind_swells_visibly() {
    let r = web_test_pipeline("Wind ash", 16.0, 24000.0, false);
    assert!(r.swell_max - r.swell_min > 0.6, "swell {}..{}", r.swell_min, r.swell_max);
}

// --- the engine, 48 kHz ---------------------------------------------------
// Its FX are not the browser's (C1), and through them it fires more than the
// browser's graph did on the same 16 s (PLAN-CORE.md phase 1): these pin the
// feel, not the browser's numbers. The listening pass (phase 9) decides
// whether that is too much.

#[test]
fn through_the_fx_a_bell_still_fires_and_a_drone_still_does_not() {
    let bells = engine("Bell spots", 16.0, 48000.0);
    assert!((3..=16).contains(&bells.hits), "Bell spots: {} hits", bells.hits);
    let drops = engine("Cave coral", 16.0, 48000.0);
    assert!(drops.hits >= 12, "Cave coral: {} hits", drops.hits);
    for name in ["Stillness ink", "Wind ash", "Molten Polivoks", "Whale coral", "Tanpura halo"] {
        let r = engine(name, 16.0, 48000.0);
        assert!(r.hits <= 4, "{name}: {} hits", r.hits);
    }
}
