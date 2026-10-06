//! ⚙ Settings for the web app's main thread (synesthesia PLAN-CORE.md
//! phase 6, C13): the page's data and its rules (syn_core::settings_page),
//! the schema's formulas and cards, and the effect presets. The page keeps
//! its look and edits a plain point; whatever it shows and every rule it
//! keeps comes from here. Points cross as AppState v1 JSON.

use syn_core::fx_presets::{apply_fx_preset, fx_presets};
use syn_core::modmatrix::ModRoute;
use syn_core::settings_page::{self as page, Domain};
use syn_core::state::{AppState, FxState};
use wasm_bindgen::prelude::*;

fn point(json: &str) -> Result<AppState, JsError> {
    serde_json::from_str(json).map_err(|e| JsError::new(&format!("not a point: {e}")))
}

fn domain(d: &str) -> Domain {
    if d == "picture" {
        Domain::Picture
    } else {
        Domain::Sound
    }
}

/// The page's data: FX modules, filter rows per type, vowels, couplings,
/// scales, LFO shape labels, route target groups per side.
#[wasm_bindgen(js_name = settingsPageJson)]
pub fn settings_page_json() -> String {
    page::SETTINGS_PAGE_JSON.to_string()
}

/// The parameter schema: formulas and cards with their sliders and selects,
/// the limits, the gene list.
#[wasm_bindgen(js_name = schemaJson)]
pub fn schema_json() -> String {
    syn_core::schema::SCHEMA_JSON.to_string()
}

#[wasm_bindgen(js_name = canEnableFormula)]
pub fn can_enable_formula(point_json: &str, id: &str) -> Result<bool, JsError> {
    Ok(page::can_enable_formula(&point(point_json)?, id))
}

#[wasm_bindgen(js_name = isTargetOn)]
pub fn is_target_on(point_json: &str, target: &str) -> Result<bool, JsError> {
    Ok(page::is_target_on(&point(point_json)?, target))
}

#[wasm_bindgen(js_name = canAddRoute)]
pub fn can_add_route(point_json: &str) -> Result<bool, JsError> {
    Ok(page::can_add_route(&point(point_json)?))
}

/// A fresh route for "sound" or "picture", as ModRoute JSON.
#[wasm_bindgen(js_name = newRoute)]
pub fn new_route(point_json: &str, side: &str) -> Result<String, JsError> {
    Ok(serde_json::to_string(&page::new_route(&point(point_json)?, domain(side)))?)
}

#[wasm_bindgen(js_name = routeDomain)]
pub fn route_domain(target: &str) -> String {
    match page::route_domain(target) {
        Domain::Sound => "sound".into(),
        Domain::Picture => "picture".into(),
    }
}

#[wasm_bindgen(js_name = targetExp)]
pub fn target_exp(target: &str, param: &str) -> bool {
    page::target_exp(target, param)
}

/// `target.param` of every route that moves something, JSON array.
#[wasm_bindgen(js_name = modulatedKeys)]
pub fn modulated_keys(routes_json: &str) -> Result<String, JsError> {
    let routes: Vec<ModRoute> = serde_json::from_str(routes_json)?;
    Ok(serde_json::to_string(&page::modulated_keys(&routes))?)
}

#[wasm_bindgen(js_name = vowelLabel)]
pub fn vowel_label(v: f64) -> String {
    page::vowel_label(v).to_string()
}

/// Same point in everything a control can set (volume and name aside).
#[wasm_bindgen(js_name = samePoint)]
pub fn same_point(a_json: &str, b_json: &str) -> Result<bool, JsError> {
    Ok(page::same_point(&point(a_json)?, &point(b_json)?))
}

/// The effect presets, `[{name, group, fx}]`.
#[wasm_bindgen(js_name = fxPresetsJson)]
pub fn fx_presets_json() -> String {
    let list: Vec<_> = fx_presets()
        .iter()
        .map(|p| serde_json::json!({ "name": p.name, "group": p.group, "fx": p.fx }))
        .collect();
    serde_json::Value::Array(list).to_string()
}

/// `fx` (FxState JSON) with preset `index` laid over it.
#[wasm_bindgen(js_name = applyFxPreset)]
pub fn apply_fx_preset_json(fx_json: &str, index: u32) -> Result<String, JsError> {
    let fx: FxState = serde_json::from_str(fx_json)?;
    let preset = fx_presets().get(index as usize).ok_or_else(|| JsError::new("no such effect preset"))?;
    Ok(serde_json::to_string(&apply_fx_preset(&fx, preset))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_and_its_rules_cross_as_json() {
        let p = serde_json::to_string(&AppState::new()).unwrap();
        assert!(settings_page_json().contains("fxModules"));
        assert!(schema_json().contains("formulas"));
        assert!(can_add_route(&p).unwrap());
        assert!(is_target_on(&p, "fx").unwrap());
        let r: ModRoute = serde_json::from_str(&new_route(&p, "picture").unwrap()).unwrap();
        assert_eq!(r.target, "reaction");
        assert_eq!(route_domain("palette"), "picture");
        assert_eq!(vowel_label(1.0), "U");
        assert!(same_point(&p, &p).unwrap());
        let fx = serde_json::to_string(&AppState::new().audio.fx).unwrap();
        let out: FxState = serde_json::from_str(&apply_fx_preset_json(&fx, 0).unwrap()).unwrap();
        assert!(out.filter_on);
        assert!(fx_presets_json().starts_with('['));
    }
}
