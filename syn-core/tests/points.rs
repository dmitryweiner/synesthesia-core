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

fn leaves(v: &Value, path: String, out: &mut Vec<(String, Value)>) {
    match v {
        Value::Object(m) => m.iter().for_each(|(k, x)| leaves(x, format!("{path}/{k}"), out)),
        Value::Array(xs) => xs.iter().enumerate().for_each(|(i, x)| leaves(x, format!("{path}/{i}"), out)),
        x => out.push((path, x.clone())),
    }
}

/// Points the Worker stored (fixtures kind "d1", a read-only export of its
/// D1): sanitizing one must keep every value it was stored with — the schema
/// may have grown fields since, never changed one.
#[test]
fn a_stored_point_keeps_every_value() {
    let f = fixture();
    let stored: Vec<&Case> = f.cases.iter().filter(|c| c.kind == "d1").collect();
    assert!(!stored.is_empty(), "the fixture has the Worker's points");
    for c in stored {
        let after = serde_json::to_value(sanitize(&c.input).expect("a point")).unwrap();
        let mut before = Vec::new();
        leaves(&c.input, String::new(), &mut before);
        for (path, v) in before {
            let got = after.pointer(&path).unwrap_or(&Value::Null);
            let same = match (got.as_f64(), v.as_f64()) {
                (Some(a), Some(b)) => a == b,
                _ => *got == v,
            };
            assert!(same, "{:?}: {path} was {v}, is {got}", c.id);
        }
    }
}

#[test]
fn the_corpus_covers_what_it_says() {
    let f = fixture();
    for kind in ["preset", "default", "genome", "mutated", "mod", "old", "broken"] {
        assert!(f.cases.iter().any(|c| c.kind == kind), "no {kind} case");
    }
    assert!(f.cases.iter().any(|c| c.id.is_none()), "some inputs are refused");
}
