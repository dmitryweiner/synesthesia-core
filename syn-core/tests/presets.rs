//! What every built-in preset must be (the rules the web app's
//! tests/presets.test.ts held while it was the specification; synesthesia
//! PLAN-CORE.md C2): a fully valid point that survives sanitize + clamp
//! unchanged, fits the genome, makes sound, and links sound to image.
//! `assets/presets.json` is edited by hand now — these guard it.

use syn_core::dsp::rng::Mulberry32;
use syn_core::genome::codec::{decode_genome, encode_genome, is_valid_genome};
use syn_core::genome::evolve::enabled_formula_count;
use syn_core::genome::genes::genes;
use syn_core::point::{canonical_json, sanitize};
use syn_core::schema::{formula_ranges, schema};
use syn_core::state::presets;
use syn_core::{FormulaGenerator, FormulaId, BLOCK};

const EXPLICIT_COUPLINGS: [&str; 4] = ["loudToPulse", "onsetToFlash", "onsetToSeed", "spectrumToTint"];

#[test]
fn the_catalogue_has_unique_names_each_spelled_in_its_point() {
    let ps = presets();
    assert!(ps.len() >= 8);
    let mut names: Vec<&str> = ps.iter().map(|p| p.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), ps.len(), "unique names");
    for p in ps {
        assert_eq!(p.state.preset_name.as_deref(), Some(p.name.as_str()));
    }
}

#[test]
fn every_preset_survives_sanitize_and_clamp_unchanged() {
    for p in presets() {
        let v = serde_json::to_value(&p.state).unwrap();
        let back = sanitize(&v).expect("a point");
        assert_eq!(canonical_json(&back), canonical_json(&p.state), "{}", p.name);
    }
}

#[test]
fn every_preset_has_one_to_max_formulas_and_every_card() {
    let s = schema();
    let mut card_ids: Vec<&str> = s.cards.iter().map(|c| c.id.as_str()).collect();
    card_ids.sort_unstable();
    for p in presets() {
        let n = p.state.audio.formulas.values().filter(|f| f.enabled).count();
        assert!((1..=s.max_enabled_formulas).contains(&n), "{}: {n} formulas", p.name);
        let cards: Vec<&str> = p.state.visual.cards.keys().map(String::as_str).collect();
        assert_eq!(cards, card_ids, "{}", p.name);
    }
}

#[test]
fn every_route_fits_the_genome_and_points_at_a_real_target() {
    let s = schema();
    for p in presets() {
        let m = &p.state.modulation;
        assert!(m.routes.len() <= s.route_slots, "{}", p.name);
        for r in &m.routes {
            assert!(r.src < m.lfos.len(), "{}: route from LFO {}", p.name, r.src);
            let real = s.mod_targets.iter().any(|t| t.target == r.target && t.param == r.param);
            assert!(real, "{}: {}.{}", p.name, r.target, r.param);
        }
    }
}

#[test]
fn every_preset_links_sound_and_image() {
    let s = schema();
    for p in presets() {
        let visual_route = p.state.modulation.routes.iter().any(|r| s.cards.iter().any(|c| c.id == r.target));
        let coupled = p.state.coupling.values().any(|v| *v != 0.0);
        assert!(visual_route || coupled, "{}", p.name);
        let explicit: f64 = EXPLICIT_COUPLINGS.iter().map(|k| p.state.coupling[*k]).sum();
        assert!(explicit >= s.coupling_floor, "{}: pulse+flash+seeds+tint {explicit}", p.name);
    }
}

#[test]
fn every_preset_round_trips_through_the_genome() {
    for p in presets() {
        let g = encode_genome(&p.state);
        assert!(is_valid_genome(&g), "{}", p.name);
        assert!(enabled_formula_count(&g) >= 1, "{}", p.name);
        let back = decode_genome(&g);
        assert_eq!(back.audio.fx.filter_type, p.state.audio.fx.filter_type, "{}", p.name);
        assert_eq!(back.modulation.routes.len(), p.state.modulation.routes.len(), "{}", p.name);
        for (id, f) in &p.state.audio.formulas {
            assert_eq!(back.audio.formulas[id].enabled, f.enabled, "{}: {id}", p.name);
        }
        let g2 = encode_genome(&back);
        for (i, (a, b)) in g.iter().zip(&g2).enumerate() {
            assert!((a - b).abs() < 1e-9, "{}: gene {}", p.name, genes()[i].id);
        }
    }
}

#[test]
fn every_preset_sounds_finite_audible_and_bounded() {
    let sr = 48000.0;
    for p in presets() {
        let st = &p.state;
        let mut mix = vec![0.0f32; sr as usize];
        let enabled = st.audio.formulas.iter().filter(|(_, f)| f.enabled);
        for (seed, (id, snap)) in (100..).zip(enabled) {
            let fid = FormulaId::parse(id).expect("a formula");
            let mut g = FormulaGenerator::new(fid, sr, snap.params.clone(), Box::new(Mulberry32::new(seed)));
            let routes: Vec<_> = st.modulation.routes.iter().filter(|r| &r.target == id).cloned().collect();
            g.set_mod(&st.modulation.lfos, &routes, &formula_ranges(id));
            let mut buf = vec![0.0f32; BLOCK];
            for block in mix.chunks_mut(BLOCK) {
                g.fill(&mut buf);
                for (m, x) in block.iter_mut().zip(&buf) {
                    *m += x;
                }
            }
        }
        assert!(mix.iter().all(|v| v.is_finite()), "{}", p.name);
        let rms = (mix.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / mix.len() as f64).sqrt();
        let peak = mix.iter().fold(0.0f32, |a, v| a.max(v.abs()));
        assert!(rms > 1e-4, "{}: rms {rms}", p.name);
        assert!(peak <= 8.0, "{}: peak {peak}", p.name);
    }
}
