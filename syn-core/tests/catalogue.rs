//! The layers' catalogue (`seqSounds` in `assets/schema.json`) against a
//! sample bank's manifest (PLAN-SAMPLES-SCRIPTS.md decision 7): every name
//! a `sound` gene can pick is in the bank, unless it is a synth drum, and
//! the bank cut it for what the catalogue says. The bank lives in its own
//! repository; this reads `../synesthesia-samples/manifest.json`, or the
//! file `SYN_SAMPLES_MANIFEST` names, and says so when there is none.

use std::collections::BTreeMap;
use std::path::PathBuf;

use syn_core::schema::schema;
use syn_core::seq::bank::Entry;
use syn_core::seq::Drum;

#[derive(serde::Deserialize)]
struct Manifest {
    version: u32,
    sounds: BTreeMap<String, Vec<Entry>>,
}

fn manifest_path() -> PathBuf {
    std::env::var_os("SYN_SAMPLES_MANIFEST").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../synesthesia-samples/manifest.json"),
        PathBuf::from,
    )
}

#[test]
fn the_bank_covers_the_catalogue() {
    let path = manifest_path();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        eprintln!("skipped: no sample bank manifest at {} (SYN_SAMPLES_MANIFEST)", path.display());
        return;
    };
    let m: Manifest = serde_json::from_str(&raw).expect("a manifest");
    assert_eq!(m.version, 2);
    for s in &schema().seq_sounds {
        if Drum::parse(&s.name).is_some() {
            assert!(!m.sounds.contains_key(&s.name), "{}: the bank would replace the synth drum", s.name);
            continue;
        }
        let entries = m.sounds.get(&s.name).unwrap_or_else(|| panic!("{}: not in the bank", s.name));
        assert!(!entries.is_empty(), "{}", s.name);
        for e in entries {
            assert_eq!(e.mode, s.mode, "{}: cut as {:?}, the catalogue says {:?}", e.file, e.mode, s.mode);
            assert!(
                ["CC0", "Public domain"].contains(&e.license.as_str()),
                "{}: licence {}",
                e.file,
                e.license
            );
        }
    }
    let extra: Vec<&String> =
        m.sounds.keys().filter(|n| !schema().seq_sounds.iter().any(|s| &s.name == *n)).collect();
    assert!(extra.is_empty(), "in the bank, not in the catalogue (append them): {extra:?}");
}
