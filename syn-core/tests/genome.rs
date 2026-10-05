//! The codec against the web app's own output (PLAN.md decision 2).
//!
//! `fixtures/genomes.json` holds the built-in presets as they were when the
//! web app was still the specification, each with the genome its TypeScript
//! `encodeGenome` produced. If the codec disagrees anywhere, a point would
//! stop meaning the same genome — so this compares every gene of each. The
//! states are frozen with their genomes, so editing a preset in
//! `assets/presets.json` does not touch this check.

use syn_core::genome::codec::{decode_genome, encode_genome, is_valid_genome};
use syn_core::genome::genes::{genes, Genome};
use syn_core::schema::GeneKind;
use syn_core::state::AppState;

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    state: AppState,
    genome: Genome,
}

struct Frozen {
    name: String,
    state: AppState,
}

fn fixture() -> Vec<Case> {
    #[derive(serde::Deserialize)]
    struct File {
        cases: Vec<Case>,
    }
    let raw = include_str!("../../fixtures/genomes.json");
    serde_json::from_str::<File>(raw).expect("fixtures/genomes.json parses").cases
}

/// The frozen presets, in the shape the tests below read them.
fn presets() -> Vec<Frozen> {
    fixture().into_iter().map(|c| Frozen { name: c.name, state: c.state }).collect()
}

fn web_genomes() -> Vec<Genome> {
    fixture().into_iter().map(|c| c.genome).collect()
}

#[test]
fn every_preset_encodes_exactly_as_the_web_app_encodes_it() {
    let want = web_genomes();
    assert_eq!(want.len(), presets().len());
    let mut worst = 0.0f64;
    for (p, expected) in presets().iter().zip(&want) {
        let got = encode_genome(&p.state);
        assert_eq!(got.len(), expected.len(), "{}: gene count", p.name);
        for (i, (a, b)) in got.iter().zip(expected).enumerate() {
            let d = (a - b).abs();
            worst = worst.max(d);
            assert!(d < 1e-12, "{}: gene {} ({}) is {a} here and {b} there", p.name, i, genes()[i].id);
        }
    }
    println!("12 presets × {} genes, worst difference {worst:e}", genes().len());
}

#[test]
fn decoding_and_encoding_again_is_a_fixed_point() {
    for (p, g) in presets().iter().zip(web_genomes()) {
        assert!(is_valid_genome(&g), "{}", p.name);
        let again = encode_genome(&decode_genome(&g));
        for (i, (a, b)) in again.iter().zip(&g).enumerate() {
            assert!((a - b).abs() < 1e-9, "{}: gene {} ({}) drifted {a} vs {b}", p.name, i, genes()[i].id);
        }
    }
}

/// The parts of a point that are not genes must survive decoding too — the
/// formulas' enabled flags, the routes and the visual cards the renderer will
/// need one day.
#[test]
fn a_decoded_point_keeps_what_the_sound_and_the_picture_need() {
    for (p, g) in presets().iter().zip(web_genomes()) {
        let state = decode_genome(&g);
        assert_eq!(
            state.enabled_formulas(),
            p.state.enabled_formulas(),
            "{}: different formulas enabled",
            p.name
        );
        assert_eq!(state.modulation.routes.len(), p.state.modulation.routes.len(), "{}: routes", p.name);
        assert_eq!(state.visual.cards.len(), p.state.visual.cards.len(), "{}: cards", p.name);
        for (id, card) in &p.state.visual.cards {
            assert_eq!(state.visual.cards[id].on, card.on, "{}: card {id}", p.name);
        }
    }
}

#[test]
fn the_gene_kinds_are_what_the_dump_says() {
    let counts = genes().iter().fold((0, 0, 0), |(c, b, ch), g| match g.kind {
        GeneKind::Cont => (c + 1, b, ch),
        GeneKind::Bool => (c, b + 1, ch),
        GeneKind::Choice => (c, b, ch + 1),
    });
    assert_eq!(counts.0 + counts.1 + counts.2, genes().len());
    assert!(counts.0 > 100 && counts.1 > 20 && counts.2 > 5, "{counts:?}");
}
