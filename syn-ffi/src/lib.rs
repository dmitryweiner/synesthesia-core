//! The foreign interface of the core (synesthesia-android PLAN.md,
//! decisions 1–2): everything an app reaches in the model goes through here,
//! and the Kotlin and Swift bindings are generated from these declarations.
//!
//! Rules for this crate:
//! - Thin. Logic belongs in `syn-core` (the model) or `syn-session` (the
//!   control logic); this crate converts types and nothing else.
//! - The types it exposes are records and plain values a UI can hold, never
//!   `syn-core`'s internals.
//! - It grows by phase: presets and the schema now; the engine, the feature
//!   frames, the session and the picture as the Android plan reaches them.

uniffi::setup_scaffolding!();

/// One built-in preset as a list shows it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PresetInfo {
    /// Position in the built-in list — the web app's `?preset=N`.
    pub index: u32,
    pub name: String,
}

/// The core's version, so an app can say which model it runs.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The built-in presets, in the web app's order.
#[uniffi::export]
pub fn presets() -> Vec<PresetInfo> {
    syn_core::state::presets()
        .iter()
        .enumerate()
        .map(|(i, p)| PresetInfo { index: i as u32, name: p.name.clone() })
        .collect()
}

/// A built-in preset's point as the web app's `AppState` v1 JSON, or `None`
/// when there is no preset at that index.
#[uniffi::export]
pub fn preset_state_json(index: u32) -> Option<String> {
    let preset = syn_core::state::presets().get(index as usize)?;
    serde_json::to_string(&preset.state).ok()
}

/// The parameter schema (ranges, defaults, labels, the gene list) exactly as
/// it was dumped from the web app. Typed records replace this when the
/// Settings page needs them.
#[uniffi::export]
pub fn schema_json() -> String {
    syn_core::schema::SCHEMA_JSON.to_string()
}

/// Entry point of the bindings generator, for an app's own
/// `uniffi-bindgen` binary (see this crate's `cli` feature).
#[cfg(feature = "cli")]
pub fn uniffi_bindgen_main() {
    uniffi::uniffi_bindgen_main()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_twelve_presets_are_listed_in_order() {
        let list = presets();
        assert_eq!(list.len(), 12);
        for (i, p) in list.iter().enumerate() {
            assert_eq!(p.index as usize, i);
            assert!(!p.name.is_empty());
        }
    }

    #[test]
    fn a_preset_comes_back_as_the_web_apps_json() {
        let json = preset_state_json(0).expect("preset 0");
        let state: syn_core::AppState = serde_json::from_str(&json).expect("parses back");
        assert_eq!(state, syn_core::state::presets()[0].state);
        assert!(preset_state_json(12).is_none());
    }

    #[test]
    fn the_schema_is_the_dumped_one() {
        assert!(schema_json().contains("\"formulaIds\""));
    }
}
