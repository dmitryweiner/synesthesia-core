//! The sample bank on the main thread (synesthesia-core
//! PLAN-SAMPLES-SCRIPTS.md S7). The worklet must never decode — a sound's
//! FLAC streams take tens of milliseconds, far over a 2.7 ms quantum — so
//! the page holds the bank here, decodes what a point needs before the
//! point goes to the worklet, and hands each new sound over as bytes
//! ([`SoundBank::packed`] → `AudioCore.addSound`).

use syn_core::seq::bank::pack;
use syn_core::seq::{sounds_of, SampleBank};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct SoundBank {
    bank: SampleBank,
}

#[wasm_bindgen]
impl SoundBank {
    /// The bytes of `bank.bin`; `undefined` when they are not a bank. Nothing
    /// is decoded yet.
    pub fn load(bytes: Vec<u8>) -> Option<SoundBank> {
        SampleBank::from_bytes(bytes).ok().map(|bank| SoundBank { bank })
    }

    /// The bank's sound names, sorted.
    pub fn names(&self) -> Vec<String> {
        self.bank.names().map(String::from).collect()
    }

    /// The names a point's layers play that this bank holds — what must reach
    /// the worklet before (or soon after) the point does. Empty for a point
    /// without layers, or that is not a point.
    #[wasm_bindgen(js_name = soundsFor)]
    pub fn sounds_for(&self, point_json: &str) -> Vec<String> {
        let Ok(state) = serde_json::from_str::<syn_core::state::AppState>(point_json) else {
            return Vec::new();
        };
        sounds_of(&state.seq).into_iter().filter(|n| self.bank.contains(n)).collect()
    }

    /// One sound, decoded (the first time; kept after) and packed for
    /// `AudioCore.addSound`; `undefined` for a name the bank lacks.
    pub fn packed(&self, name: &str) -> Option<Vec<u8>> {
        self.bank.sound(name).map(|v| pack(&v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_not_a_bank_is_refused() {
        assert!(SoundBank::load(b"nope".to_vec()).is_none());
    }
}
