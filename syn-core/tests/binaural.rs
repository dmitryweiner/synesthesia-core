//! Space and the binaural beat in the point (PLAN-BINAURAL.md B2/B3): the
//! genes appended without moving any earlier one, a neutral point written
//! exactly as before, a wide one surviving every path a point takes — the
//! JSON, `sanitize`, the genome, the Settings page — and the search finding
//! space by itself.

use serde_json::json;
use syn_core::dsp::rng::Mulberry32;
use syn_core::genome::codec::{decode_genome, encode_genome, is_valid_genome};
use syn_core::genome::evolve::{mutate, random_genome, MutateOptions};
use syn_core::genome::genes::{gene_index, gene_value, genes, is_gene_active, read_value};
use syn_core::point::{canonical_json, sanitize};
use syn_core::seq::{Layer, SeqState};
use syn_core::settings::Edit;
use syn_core::settings_page::same_point;
use syn_core::state::{presets, AppState, BinState, AZ};

/// Fractal garden with its voices around the head, a layer on the left, and
/// a theta beat.
fn wide() -> AppState {
    let mut s = presets()[0].state.clone();
    s.audio.formulas.get_mut("fm").unwrap().params.insert(AZ.into(), 35.0);
    s.audio.formulas.get_mut("additive").unwrap().params.insert(AZ.into(), -60.0);
    s.seq = SeqState {
        cps: 0.5,
        // Four slots, as `sanitize` writes them.
        layers: [Layer {
            on: true,
            sound: "bd".into(),
            pattern: "0*4".into(),
            az: -45.0,
            ..Layer::default()
        }]
        .into_iter()
        .chain(std::iter::repeat_n(Layer::default(), 3))
        .collect(),
    };
    s.bin = BinState { on: true, beat: 6.0, width: 0.8 };
    s
}

#[test]
fn a_point_without_space_is_written_as_before() {
    // The presets made before space (Magic forest and Sea shore have it).
    for p in presets().iter().filter(|p| p.state.bin.is_neutral()) {
        let json = canonical_json(&p.state);
        assert!(!json.contains("\"az\"") && !json.contains("\"bin\""), "{}", p.name);
        // Through the genome — which carries every azimuth, at 0 — and back.
        let back = decode_genome(&encode_genome(&p.state));
        let again = canonical_json(&back);
        assert!(!again.contains("\"az\"") && !again.contains("\"bin\""), "{}", p.name);
    }
    // A fresh point: every slider default but the centred azimuths.
    assert!(!canonical_json(&AppState::new()).contains("\"az\""));
    // An azimuth of 0 written out by someone else is no azimuth.
    let mut v = serde_json::to_value(&presets()[2].state).unwrap();
    v["audio"]["formulas"]["fm"]["params"]["az"] = json!(0);
    v["bin"] = json!({ "on": false, "beat": 12, "width": 0 });
    assert_eq!(canonical_json(&sanitize(&v).unwrap()), canonical_json(&presets()[2].state));
}

#[test]
fn a_wide_point_survives_the_json_sanitize_and_the_genome() {
    let s = wide();
    let json = canonical_json(&s);
    assert!(json.contains("\"az\":35") && json.contains("\"az\":-45"), "{json}");
    assert!(json.contains("\"bin\":{\"beat\":6,\"on\":true,\"width\":0.8}"), "{json}");
    let back = canonical_json(&sanitize(&serde_json::to_value(&s).unwrap()).unwrap());
    let at = back.bytes().zip(json.bytes()).position(|(a, b)| a != b).unwrap_or(0);
    assert_eq!(back, json, "from byte {at}: {}", &back[at.saturating_sub(80)..(at + 80).min(back.len())]);
    let g = encode_genome(&s);
    assert!(is_valid_genome(&g));
    let decoded = decode_genome(&g);
    assert_eq!(decoded.audio.formulas["fm"].params[AZ], 35.0);
    assert_eq!(decoded.seq.layers[0].az, -45.0);
    // The beat is a log gene: the codec moves its last bits, as it does a
    // filter's frequency — the same point to nine digits.
    assert!((decoded.bin.beat - 6.0).abs() < 1e-9 && decoded.bin.on && decoded.bin.width == 0.8);
    // And once decoded, the point is a fixed point of the codec.
    let again = decode_genome(&encode_genome(&decoded));
    assert!(same_point(&again, &decoded));
}

#[test]
fn sanitize_keeps_space_in_its_ranges() {
    let mut v = serde_json::to_value(wide()).unwrap();
    v["audio"]["formulas"]["fm"]["params"]["az"] = json!(500);
    v["seq"]["layers"][0]["az"] = json!(-1000);
    v["bin"] = json!({ "on": true, "beat": 1000, "width": 7 });
    let s = sanitize(&v).unwrap();
    assert_eq!(s.audio.formulas["fm"].params[AZ], 90.0);
    assert_eq!(s.seq.layers[0].az, -90.0);
    assert_eq!(s.bin, BinState { on: true, beat: 40.0, width: 1.0 });
    // Nonsense is no section at all.
    v["bin"] = json!("wide");
    assert!(sanitize(&v).unwrap().bin.is_neutral());
    v["bin"] = json!({ "on": "yes", "width": -3 });
    assert!(sanitize(&v).unwrap().bin.is_neutral());
}

#[test]
fn the_space_genes_are_gated_like_their_voices() {
    let s = wide();
    let g = encode_genome(&s);
    assert!(is_gene_active(&g, gene_index("a.fm.az")));
    assert!(!is_gene_active(&g, gene_index("a.bell.az")), "a formula that is off");
    assert!(is_gene_active(&g, gene_index("seq.0.az")));
    assert!(!is_gene_active(&g, gene_index("seq.1.az")));
    assert!(is_gene_active(&g, gene_index("bin.width")));
    assert!(is_gene_active(&g, gene_index("bin.beat")));
    let still = encode_genome(&presets()[0].state);
    assert!(!is_gene_active(&still, gene_index("bin.beat")), "no beat, no rate");
    // The neutral values are the ones a point without space encodes.
    for id in ["a.fm.az", "seq.0.az", "bin.beat", "bin.width"] {
        let i = gene_index(id);
        assert_eq!(genes()[i].neutral, Some(still[i]), "{id}");
    }
}

#[test]
fn a_random_point_is_spread_out_and_the_search_finds_the_beat() {
    let mut rng = Mulberry32::new(9);
    for _ in 0..50 {
        let g = random_genome(&mut rng);
        assert!(is_valid_genome(&g));
        let width = read_value(&g, "bin.width");
        assert!((0.3..=0.8).contains(&width), "{width}");
        assert_eq!(g[gene_index("bin.on")], 0.0, "no beat by chance");
        let s = decode_genome(&g);
        for id in s.enabled_formulas() {
            let az = s.audio.formulas[id].params.get(AZ).copied().unwrap_or(0.0);
            assert!(az.abs() <= 72.0, "{id}: {az}");
        }
    }
    // Structural moves switch the beat on — between theta and alpha — and off.
    let opts = MutateOptions { structural_prob: 1.0, ..MutateOptions::default() };
    let mut g = encode_genome(&presets()[1].state);
    let (mut on, mut off) = (0, 0);
    for _ in 0..2000 {
        let next = mutate(&g, &mut rng, &opts);
        let (was, is) = (g[gene_index("bin.on")], next[gene_index("bin.on")]);
        if was == 0.0 && is == 1.0 {
            on += 1;
            let beat = gene_value(&genes()[gene_index("bin.beat")], next[gene_index("bin.beat")]);
            assert!((4.0..=12.0).contains(&beat), "{beat}");
        } else if was == 1.0 && is == 0.0 {
            off += 1;
        }
        g = next;
    }
    assert!(on > 5 && off > 5, "on {on} off {off}");
}

#[test]
fn the_settings_page_edits_space() {
    let mut e = Edit::new(&presets()[0].state);
    e.set_value("a.fm.az", -30.0);
    e.set_value("seq.0.az", 20.0);
    e.set_value("bin.width", 0.5);
    e.set_value("bin.on", 1.0);
    e.set_value("bin.beat", 10.0);
    let p = e.point();
    assert_eq!(p.audio.formulas["fm"].params[AZ], -30.0);
    assert!(p.bin.on && p.bin.width == 0.5 && (p.bin.beat - 10.0).abs() < 1e-9, "{:?}", p.bin);
    assert_eq!(e.value("a.fm.az"), -30.0);
    let page = syn_core::settings::sections();
    let fm = page.iter().find(|s| s.id == "a.fm").unwrap();
    assert_eq!(fm.controls.last().map(|g| g.id.as_str()), Some("a.fm.az"), "the azimuth is the voice's own");
    let bin = page.iter().find(|s| s.id == "bin").expect("a binaural section");
    assert_eq!(bin.title, "Binaural");
    let ids: Vec<&str> = bin.controls.iter().map(|g| g.id.as_str()).collect();
    assert_eq!(ids, ["bin.on", "bin.beat", "bin.width"]);
}
