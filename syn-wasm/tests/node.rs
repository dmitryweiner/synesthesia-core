//! The same surface, run as wasm under node (`wasm-bindgen-test-runner`):
//! what the host tests check, in the module the web app loads.
#![cfg(target_arch = "wasm32")]

use syn_wasm::{preset_names, preset_state_json, AudioCore, FRAME_LEN};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn every_preset_renders_a_quantum_in_wasm() {
    for i in 0..preset_names().len() as u32 {
        let json = preset_state_json(i).unwrap();
        let mut core = AudioCore::create(48000.0, json.as_bytes(), 128).expect("a point");
        core.fade_in();
        for _ in 0..200 {
            core.render(128);
        }
        assert!(core.output().iter().all(|v| v.is_finite()), "preset {i}");
        assert!(core.output().iter().any(|v| *v != 0.0), "preset {i} sounds");
        assert_ne!(core.latest_frame(), 0);
        assert_eq!(core.frame().len(), FRAME_LEN);
    }
}
