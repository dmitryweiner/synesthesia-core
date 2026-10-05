//! Effect-module presets: ⚙ Settings' "Effects preset" menu (the web app's
//! `fxPresets.ts`, ported from formula-synth; first dumped into
//! `assets/fx-presets.json`). Each sets only its own module's FX fields over
//! the current ones — formulas, modulation and the picture are untouched.

use serde::Deserialize;
use serde_json::{Map, Value};

use crate::state::FxState;

#[derive(Clone, Debug, Deserialize)]
pub struct FxPreset {
    pub name: String,
    /// The menu's group: "Filter", "Flanger" or "Phaser".
    pub group: String,
    /// The `FxState` fields it sets, by their web names.
    pub fx: Map<String, Value>,
}

/// The presets, in the menu's order.
pub fn fx_presets() -> &'static [FxPreset] {
    static PRESETS: std::sync::OnceLock<Vec<FxPreset>> = std::sync::OnceLock::new();
    PRESETS.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/fx-presets.json")).expect("assets/fx-presets.json")
    })
}

/// `fx` with the preset's fields laid over it.
pub fn apply_fx_preset(fx: &FxState, preset: &FxPreset) -> FxState {
    let mut v = serde_json::to_value(fx).expect("an FxState serializes");
    for (k, x) in &preset.fx {
        v[k.as_str()] = x.clone();
    }
    serde_json::from_value(v).expect("a preset sets FxState fields only")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_applies_and_touches_only_its_fields() {
        let base = FxState::default();
        let base_v = serde_json::to_value(&base).unwrap();
        assert_eq!(fx_presets().len(), 15);
        for p in fx_presets() {
            let got = serde_json::to_value(apply_fx_preset(&base, p)).unwrap();
            for (k, v) in got.as_object().unwrap() {
                let want = p.fx.get(k).unwrap_or(&base_v[k]);
                if let (Some(a), Some(b)) = (v.as_f64(), want.as_f64()) {
                    assert_eq!(a, b, "{}: {k}", p.name);
                } else {
                    assert_eq!(v, want, "{}: {k}", p.name);
                }
            }
        }
    }
}
