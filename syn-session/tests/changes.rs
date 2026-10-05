//! The status line's "what changed", frozen from the web app into
//! fixtures/changes.json (synesthesia PLAN-CORE.md phase 1): the same genes,
//! the same directions, the same words.

use serde::Deserialize;
use syn_core::genome::evolve::{diff_summary, ChangeDir};
use syn_session::describe_change;

#[derive(Deserialize)]
struct Change {
    id: String,
    dir: String,
}

#[derive(Deserialize)]
struct Case {
    a: Vec<f64>,
    b: Vec<f64>,
    changes: Vec<Change>,
    line: String,
}

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

fn dir_name(d: ChangeDir) -> &'static str {
    match d {
        ChangeDir::Up => "up",
        ChangeDir::Down => "down",
        ChangeDir::On => "on",
        ChangeDir::Off => "off",
        ChangeDir::Switch => "switch",
    }
}

#[test]
fn every_change_is_named_as_the_web_app_names_it() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/changes.json");
    let f: Fixture =
        serde_json::from_str(&std::fs::read_to_string(path).expect("fixtures/changes.json")).unwrap();
    assert!(f.cases.len() >= 100);
    for (i, c) in f.cases.iter().enumerate() {
        let got: Vec<(String, &str)> =
            diff_summary(&c.a, &c.b).into_iter().map(|g| (g.id, dir_name(g.dir))).collect();
        let want: Vec<(String, &str)> = c.changes.iter().map(|w| (w.id.clone(), w.dir.as_str())).collect();
        assert_eq!(got, want, "case {i}");
        assert_eq!(describe_change(&c.a, &c.b), c.line, "case {i}");
    }
}
