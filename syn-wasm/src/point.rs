//! Points as the points Worker stores them (synesthesia PLAN-CORE.md C5,
//! phase 7): sanitize, canonical JSON and the content id — the core's
//! syn_core::point, which reproduces the TypeScript byte for byte
//! (fixtures/points.json), so old links keep their ids.

use syn_core::point::{canonical_json, point_id, sanitize};
use wasm_bindgen::prelude::*;

/// The canonical JSON of what `json` sanitizes to, or `undefined` when it is
/// not a point at all (not JSON, or not an object).
#[wasm_bindgen(js_name = sanitizePoint)]
pub fn sanitize_point(json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    sanitize(&value).map(|s| canonical_json(&s))
}

/// The id a canonical JSON string is stored under: 10 base62 characters of
/// its SHA-256.
#[wasm_bindgen(js_name = pointId)]
pub fn point_id_of(canonical: &str) -> String {
    point_id(canonical)
}

/// Whether `s` has the shape of an id.
#[wasm_bindgen(js_name = isPointId)]
pub fn is_point_id(s: &str) -> bool {
    s.len() == 10 && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_point_comes_back_whole_and_canonical() {
        let c = sanitize_point(r#"{"audio":{"formulas":{"fm":{"enabled":true}}}}"#).expect("a point");
        assert!(c.starts_with(r#"{"audio":{"formulas":{"additive""#), "keys sorted: {}", &c[..40]);
        assert!(is_point_id(&point_id_of(&c)));
        assert!(sanitize_point("[1").is_none());
        assert!(sanitize_point("42").is_none());
        assert!(!is_point_id("short"));
    }
}
