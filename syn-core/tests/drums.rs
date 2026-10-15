//! The synth drums (`bd sd hh oh cp`, PLAN-SAMPLES-SCRIPTS.md decision 4)
//! against their golden takes in `golden/drums/`. The takes were rendered
//! once, by this test, when the drums were made (2026-10-15); from then on
//! the `golden/` rule holds for them: never re-render or hand-edit a take —
//! a failure here is a drum that changed its sound. The one exception is a
//! drum the user has listened to and asked to sound different: then its
//! takes are deleted and written again, in the same commit as the change.
//!
//!   a  48 kHz, speed 1, seed 1
//!   b  22.05 kHz, speed 0.7, seed 2  (the scout's rate, slowed: pitch and length)
//!
//! A take is a hit's first `LEN` samples (0.34 s at 48 kHz): every voice is
//! a recurrence, so a change shows there; the length of the whole hit is
//! checked against the one recorded below.
//!
//! `SYN_WRITE_DRUM_TAKES=1` writes a take that is missing, never one that
//! exists.

use std::path::{Path, PathBuf};

use syn_core::seq::voice::render_drum;
use syn_core::seq::Drum;

const TOL: f32 = 1e-6;
const LEN: usize = 16384;
/// The length of each whole hit, in samples, take a then take b.
const LENGTHS: [(&str, usize, usize); 5] = [
    ("bd", 123787, 81236),
    ("sd", 56081, 36803),
    ("hh", 18587, 12198),
    ("oh", 139397, 91480),
    ("cp", 61083, 40088),
];
const TAKES: [(&str, f64, f64, u32); 2] = [("a", 48000.0, 1.0, 1), ("b", 22050.0, 0.7, 2)];

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../golden/drums")
}

#[test]
fn every_drum_matches_its_golden_take() {
    let write = std::env::var_os("SYN_WRITE_DRUM_TAKES").is_some();
    let mut failures = Vec::new();
    for kind in Drum::ALL {
        for (take, sr, speed, seed) in TAKES {
            let whole = render_drum(kind, sr, speed, seed);
            let (_, la, lb) = LENGTHS.iter().find(|l| l.0 == kind.name()).expect("a length per drum");
            let want_len = if take == "a" { *la } else { *lb };
            if whole.len() != want_len {
                failures.push(format!(
                    "{}.{take}: lasts {} samples, not {want_len}",
                    kind.name(),
                    whole.len()
                ));
            }
            let got = &whole[..whole.len().min(LEN)];
            let path = dir().join(format!("{}.{take}.f32", kind.name()));
            let Ok(bytes) = std::fs::read(&path) else {
                assert!(write, "{} is missing (SYN_WRITE_DRUM_TAKES=1 writes it, once)", path.display());
                std::fs::create_dir_all(dir()).unwrap();
                std::fs::write(&path, got.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>()).unwrap();
                continue;
            };
            let want: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
            if got.len() != want.len() {
                failures.push(format!(
                    "{}.{take}: {} samples, the take has {}",
                    kind.name(),
                    got.len(),
                    want.len()
                ));
                continue;
            }
            if let Some(i) = got.iter().zip(&want).position(|(g, w)| (g - w).abs() > TOL) {
                failures.push(format!(
                    "{}.{take}: first diff at {i}: {} vs {}",
                    kind.name(),
                    got[i],
                    want[i]
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
