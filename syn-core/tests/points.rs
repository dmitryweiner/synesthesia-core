//! The web app's point handling, frozen into fixtures/points.json by
//! scripts/dump-points.mjs (synesthesia PLAN-CORE.md phase 1, C5): every
//! input must sanitize to the same point, with the same canonical JSON —
//! byte for byte — and so the same id. A failure here means a shared link
//! would open differently, or get another id, than it did in the TypeScript.

use serde::Deserialize;
use serde_json::Value;
use syn_core::point::{canonical_json, point_id, sanitize};

#[derive(Deserialize)]
struct Case {
    kind: String,
    input: Value,
    canonical: Option<String>,
    id: Option<String>,
}

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

fn fixture() -> Fixture {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/points.json");
    serde_json::from_str(&std::fs::read_to_string(path).expect("fixtures/points.json")).expect("parses")
}

#[test]
fn every_point_sanitizes_to_the_same_canonical_json_and_id() {
    let f = fixture();
    let mut failures = Vec::new();
    for (i, c) in f.cases.iter().enumerate() {
        let got = sanitize(&c.input).map(|s| canonical_json(&s));
        if got != c.canonical {
            let (g, w) = (got.unwrap_or_default(), c.canonical.clone().unwrap_or_default());
            let at = g.bytes().zip(w.bytes()).position(|(a, b)| a != b).unwrap_or(g.len().min(w.len()));
            let lo = at.saturating_sub(60);
            failures.push(format!(
                "case {i} ({}): differs at byte {at}\n  got  …{}…\n  want …{}…",
                c.kind,
                g.get(lo..(at + 60).min(g.len())).unwrap_or(""),
                w.get(lo..(at + 60).min(w.len())).unwrap_or("")
            ));
            continue;
        }
        if let (Some(canonical), Some(id)) = (&c.canonical, &c.id) {
            if &point_id(canonical) != id {
                failures.push(format!("case {i} ({}): id {} vs {id}", c.kind, point_id(canonical)));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        f.cases.len(),
        failures.join("\n")
    );
}

#[test]
fn the_corpus_covers_what_it_says() {
    let f = fixture();
    for kind in ["preset", "default", "genome", "mutated", "mod", "old", "broken"] {
        assert!(f.cases.iter().any(|c| c.kind == kind), "no {kind} case");
    }
    assert!(f.cases.iter().any(|c| c.id.is_none()), "some inputs are refused");
}
