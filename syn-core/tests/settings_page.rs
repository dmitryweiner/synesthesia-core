//! The ⚙ Settings page's model (syn_core::settings_page) — the rules the web
//! app's tests/settings.test.ts held while the page's model was TypeScript
//! (synesthesia PLAN-CORE.md phase 6, C13), including its guarantee: a point
//! set with the page's controls survives the genome unchanged.

use syn_core::fx_presets::{apply_fx_preset, fx_presets};
use syn_core::genome::codec::{decode_genome, encode_genome};
use syn_core::modmatrix::{LfoDef, ModRoute};
use syn_core::point::sanitize;
use syn_core::schema::schema;
use syn_core::settings_page::*;
use syn_core::state::{presets, AppState, FxState};

fn fx_value(fx: &FxState, k: &str) -> serde_json::Value {
    serde_json::to_value(fx).unwrap()[k].clone()
}

#[test]
fn every_effect_has_one_module_in_the_order_the_chain_runs() {
    let on: Vec<&str> = page().fx_modules.iter().map(|m| m.on.as_str()).collect();
    assert_eq!(on, ["filterOn", "chorusOn", "phaserOn", "delayOn", "reverbOn", "limiterOn"]);
    let mut keys = on.clone();
    keys.sort_unstable();
    let mut want: Vec<&str> = schema().fx_on_keys.iter().map(String::as_str).collect();
    want.sort_unstable();
    assert_eq!(keys, want);
}

#[test]
fn every_numeric_fx_field_has_one_slider_on_its_own_module_with_its_gene_range() {
    let s = schema();
    let mut keys = Vec::new();
    for m in &page().fx_modules {
        for c in &m.sliders {
            keys.push(c.k.clone());
            let id = format!("fx.{}", c.k);
            let gene = s.genes.iter().find(|g| g.id == id).unwrap_or_else(|| panic!("no gene {id}"));
            assert_eq!([c.min, c.max], [gene.min, gene.max], "{}", c.k);
            assert_eq!(c.exp == Some(true), gene.exp, "{}", c.k);
            let module = s.fx_param_module.get(&c.k).map_or("reverbOn", String::as_str);
            assert_eq!(m.on, module, "{}", c.k);
            assert!(c.step > 0.0, "{}", c.k);
        }
    }
    keys.sort();
    let mut want: Vec<String> = s.fx_mod_params.clone();
    want.push("reverbDecay".into());
    want.sort();
    assert_eq!(keys, want);
}

#[test]
fn the_choices_offer_exactly_what_the_engine_accepts() {
    let s = schema();
    let opts = |k: &str| -> Vec<String> {
        page()
            .fx_modules
            .iter()
            .flat_map(|m| &m.choices)
            .find(|c| c.k == k)
            .map(|c| c.options.iter().map(|o| o.value.clone()).collect())
            .unwrap_or_default()
    };
    assert_eq!(opts("filterType"), s.filter_types);
    assert_eq!(opts("chorusMode"), s.chorus_modes);
    assert_eq!(opts("phaserStages"), s.phaser_stages.iter().map(|n| n.to_string()).collect::<Vec<_>>());
}

#[test]
fn the_filter_shows_only_the_rows_its_type_uses() {
    let f = |t: &str| filter_controls(t).unwrap_or_else(|| panic!("{t}"));
    let lp = f("lowpass");
    assert!(lp.q && !lp.gain && !lp.vowel && !lp.comb);
    assert_eq!((lp.freq_label.as_str(), lp.q_label.as_str()), ("Cutoff (Hz)", "Q"));
    assert!(f("peaking").gain && f("peaking").q);
    assert_eq!(f("peaking").freq_label, "Frequency (Hz)");
    assert!(f("highshelf").gain);
    let formant = f("formant");
    assert!(formant.q && formant.vowel && !formant.comb);
    assert_eq!((formant.freq_label.as_str(), formant.q_label.as_str()), ("Formant shift (Hz)", "Resonance"));
    let comb = f("comb");
    assert!(!comb.q && !comb.gain && !comb.vowel && comb.comb);
    assert_eq!(comb.freq_label, "Pitch (Hz)");
    for t in &schema().filter_types {
        assert!(filter_controls(t).is_some(), "{t} has no rows");
    }
}

#[test]
fn vowels_run_a_e_i_o_u() {
    let s: String = [0.0, 0.25, 0.5, 0.75, 1.0].iter().map(|v| vowel_label(*v)).collect();
    assert_eq!(s, "AEIOU");
    assert_eq!(vowel_label(0.6), "I");
}

#[test]
fn an_effect_preset_sets_only_its_own_fields_and_every_value_survives_sanitize() {
    let base = FxState::default();
    for p in fx_presets() {
        let fx = apply_fx_preset(&base, p);
        for (k, v) in serde_json::to_value(&base).unwrap().as_object().unwrap() {
            if !p.fx.contains_key(k) {
                assert_eq!(&fx_value(&fx, k), v, "{}: {k}", p.name);
            }
        }
        let mut point = AppState::new();
        point.audio.fx = fx.clone();
        let back = sanitize(&serde_json::to_value(&point).unwrap()).unwrap();
        assert_eq!(back.audio.fx, fx, "{}", p.name);
    }
}

#[test]
fn a_sixth_formula_cannot_be_switched_on() {
    let max = schema().max_enabled_formulas;
    let with = |n: usize| {
        let mut s = AppState::new();
        for id in schema().formula_ids.iter().take(n) {
            s.audio.formulas.get_mut(id).unwrap().enabled = true;
        }
        s
    };
    let ids = &schema().formula_ids;
    assert!(!can_enable_formula(&with(max), &ids[max]));
    assert!(can_enable_formula(&with(max), &ids[0]), "one that is on stays switchable");
    assert!(can_enable_formula(&with(max - 1), &ids[max]));
}

#[test]
fn effects_and_formulas_are_sound_cards_are_picture() {
    assert_eq!(route_domain("fx"), Domain::Sound);
    assert_eq!(route_domain("tanpura"), Domain::Sound);
    assert_eq!(route_domain("palette"), Domain::Picture);
    assert_eq!(route_domain("reaction"), Domain::Picture);
}

#[test]
fn the_two_sides_offer_every_genome_mod_target_once_and_nothing_the_genome_would_drop() {
    let mut offered = Vec::new();
    for domain in [Domain::Sound, Domain::Picture] {
        for g in target_groups(domain) {
            for p in &g.params {
                assert_eq!(route_domain(&g.id), domain);
                let t = schema().mod_targets.iter().find(|t| t.target == g.id && t.param == p.k);
                let t = t.unwrap_or_else(|| panic!("{}.{} is no mod target", g.id, p.k));
                assert_eq!(p.exp, t.exp, "{}.{}", g.id, p.k);
                offered.push(format!("{}.{}", g.id, p.k));
            }
        }
    }
    let n = offered.len();
    offered.sort();
    offered.dedup();
    assert_eq!(offered.len(), n, "offered twice");
    assert_eq!(n, schema().mod_targets.len());
    assert_eq!(target_groups(Domain::Sound).last().unwrap().id, "fx");
    let cards: Vec<&str> = schema().cards.iter().map(|c| c.id.as_str()).collect();
    let picture: Vec<&str> = target_groups(Domain::Picture).iter().map(|g| g.id.as_str()).collect();
    assert_eq!(picture, cards);
}

#[test]
fn a_target_is_on_when_its_formula_or_card_is_and_effects_always_are() {
    let mut s = AppState::new();
    assert!(is_target_on(&s, "fx"));
    assert!(!is_target_on(&s, "fm"));
    s.audio.formulas.get_mut("fm").unwrap().enabled = true;
    assert!(is_target_on(&s, "fm"));
    assert!(is_target_on(&s, "reaction"));
    assert_eq!(is_target_on(&s, "flow"), s.visual.cards["flow"].on);
}

#[test]
fn a_new_route_aims_at_something_that_is_on_with_the_schemas_octave_flag() {
    let mut s = AppState::new();
    let r = new_route(&s, Domain::Sound);
    assert_eq!((r.src, r.target.as_str()), (0, "fx"));
    s.audio.formulas.get_mut("bowl").unwrap().enabled = true;
    let r = new_route(&s, Domain::Sound);
    assert_eq!(r.target, "bowl");
    assert_eq!(r.exp, target_exp(&r.target, &r.param));
    let p = new_route(&s, Domain::Picture);
    assert_eq!(p.target, "reaction");
    assert!(p.depth.abs() > 0.0);
}

#[test]
fn at_most_the_genomes_route_slots_sound_and_picture_together() {
    let mut s = AppState::new();
    for i in 0..schema().route_slots {
        assert!(can_add_route(&s));
        let d = if i % 2 == 1 { Domain::Sound } else { Domain::Picture };
        let r = new_route(&s, d);
        s.modulation.routes.push(r);
    }
    assert!(!can_add_route(&s));
}

#[test]
fn modulated_keys_name_every_route_that_moves_something() {
    let r = |target: &str, param: &str, depth: f64| ModRoute {
        src: 0,
        target: target.into(),
        param: param.into(),
        depth,
        exp: false,
    };
    let keys =
        modulated_keys(&[r("fx", "filterFreq", 0.3), r("palette", "shift", 0.0), r("bowl", "bowlF", -0.1)]);
    assert_eq!(keys, ["bowl.bowlF", "fx.filterFreq"]);
}

#[test]
fn every_lfo_shape_has_a_label_and_the_rate_spans_the_gene_range_in_octaves() {
    let mut labelled: Vec<&String> = page().lfo_shape_labels.keys().collect();
    labelled.sort();
    let mut shapes: Vec<&String> = schema().lfo_shapes.iter().collect();
    shapes.sort();
    assert_eq!(labelled, shapes);
    assert_eq!([page().lfo_rate.min, page().lfo_rate.max], schema().lfo_rate_range);
    assert_eq!(page().lfo_rate.exp, Some(true));
}

#[test]
fn one_coupling_control_per_coupling_with_its_range() {
    let keys: Vec<&str> = page().coupling_controls.iter().map(|c| c.k.as_str()).collect();
    let want: Vec<&str> = schema().coupling_keys.iter().map(String::as_str).collect();
    assert_eq!(keys, want);
    for c in &page().coupling_controls {
        assert_eq!([c.min, c.max], schema().coupling_ranges[&c.k], "{}", c.k);
    }
}

#[test]
fn same_point_ignores_codec_noise_the_volume_and_the_name_and_sees_any_control() {
    let s = presets()[0].state.clone();
    let mut back = decode_genome(&encode_genome(&s));
    assert!(same_point(&s, &back));
    back.audio.master_gain = 0.1;
    back.preset_name = Some("renamed".into());
    assert!(same_point(&s, &back));
    let mut a = s.clone();
    a.audio.fx.delay_mix += 0.01;
    assert!(!same_point(&s, &a));
    let mut b = s.clone();
    b.modulation.lfos[2].shape = if b.modulation.lfos[2].shape == syn_core::modmatrix::LfoShape::Pink {
        syn_core::modmatrix::LfoShape::Sine
    } else {
        syn_core::modmatrix::LfoShape::Pink
    };
    assert!(!same_point(&s, &b));
    let mut c = s.clone();
    let id = c.visual.cards["palette"].params["paletteId"];
    c.visual.cards.get_mut("palette").unwrap().params.insert("paletteId".into(), (id + 1.0) % 5.0);
    assert!(!same_point(&s, &c));
}

// --- the guarantee ----------------------------------------------------------
// A value a slider can produce: the web page's slider (src/ui/settingsModel.ts)
// moves a log scale in 1000 positions and rounds whole-step ones; a linear
// one snaps to its step.

const LOG_STEPS: f64 = 1000.0;

fn at(min: f64, max: f64, step: f64, exp: bool, t: f64) -> f64 {
    if exp && min > 0.0 && max > min {
        let pos = (t * LOG_STEPS).round();
        let v = min * (max / min).powf(pos / LOG_STEPS);
        return if step >= 1.0 { v.round().clamp(min, max) } else { v };
    }
    (min + ((t * (max - min)) / step).round() * step).clamp(min, max)
}

#[test]
fn a_point_set_with_the_pages_controls_survives_the_genome_unchanged() {
    let s = schema();
    for t in [0.0, 0.37, 0.81, 1.0] {
        let mut p = AppState::new();
        for (i, f) in s.formulas.iter().enumerate() {
            let snap = p.audio.formulas.get_mut(&f.id).unwrap();
            snap.enabled = i % 5 == 0;
            for sl in &f.sliders {
                snap.params.insert(sl.k.clone(), at(sl.min, sl.max, sl.step, sl.exp, t));
            }
        }
        let mut fx = serde_json::to_value(&p.audio.fx).unwrap();
        for m in &page().fx_modules {
            fx[m.on.as_str()] = (t > 0.5).into();
            for c in &m.sliders {
                fx[c.k.as_str()] = at(c.min, c.max, c.step, c.exp == Some(true), t).into();
            }
        }
        let types = &s.filter_types;
        fx["filterType"] = types[(t * (types.len() - 1) as f64).round() as usize].clone().into();
        p.audio.fx = serde_json::from_value(fx).unwrap();
        for c in &s.cards {
            let card = p.visual.cards.get_mut(&c.id).unwrap();
            card.on = true;
            for sl in &c.sliders {
                card.params.insert(sl.k.clone(), at(sl.min, sl.max, sl.step, sl.exp, t));
            }
            for sel in &c.selects {
                card.params.insert(
                    sel.k.clone(),
                    sel.options[(t * (sel.options.len() - 1) as f64).round() as usize].v,
                );
            }
        }
        let rate = &page().lfo_rate;
        p.modulation.lfos = (0..p.modulation.lfos.len())
            .map(|i| LfoDef {
                shape: syn_core::settings::lfo_shape((i + 1) as f64),
                rate: at(rate.min, rate.max, 0.0, true, t),
                phase: at(0.0, 1.0, 0.01, false, t),
            })
            .collect();
        let groups: Vec<_> =
            target_groups(Domain::Sound).iter().chain(target_groups(Domain::Picture)).collect();
        p.modulation.routes = (0..s.route_slots)
            .map(|i| {
                let g = groups[(i * 7) % groups.len()];
                let par = &g.params[i % g.params.len()];
                ModRoute {
                    src: i % 4,
                    target: g.id.clone(),
                    param: par.k.clone(),
                    depth: at(-1.0, 1.0, 0.01, false, t),
                    exp: i % 3 == 0,
                }
            })
            .collect();
        for c in &page().coupling_controls {
            p.coupling.insert(c.k.clone(), at(c.min, c.max, c.step, false, t));
        }
        assert!(same_point(&p, &decode_genome(&encode_genome(&p))), "at {t} of the travel");
    }
}
