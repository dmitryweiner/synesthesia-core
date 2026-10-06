//! Links for the web app's main thread (synesthesia PLAN-CORE.md phase 6):
//! what a link asks to open (syn_core::share::parse_launch — the web app's
//! own order: a stored point, then a `#s=` token, then `?preset=N`) and the
//! long `#s=` token for a point. Rewriting the address bar stays the page's.

use serde_json::json;
use syn_core::share::{encode_token, parse_launch, Launch};
use wasm_bindgen::prelude::*;

/// `{kind: "presetId", id}` | `{kind: "point", point}` | `{kind: "preset",
/// index}` | `{kind: "none"}`, JSON.
#[wasm_bindgen(js_name = parseLaunch)]
pub fn parse_launch_json(href: &str) -> String {
    match parse_launch(href) {
        Launch::Stored(id) => json!({ "kind": "presetId", "id": id }),
        Launch::Point(p) => json!({ "kind": "point", "point": *p }),
        Launch::Preset(i) => json!({ "kind": "preset", "index": i }),
        Launch::Nothing => json!({ "kind": "none" }),
    }
    .to_string()
}

/// The `#s=` token of a point (AppState JSON) — the long link's payload.
#[wasm_bindgen(js_name = encodeToken)]
pub fn encode_token_json(point_json: &str) -> Result<String, JsError> {
    let state = serde_json::from_str(point_json).map_err(|e| JsError::new(&format!("not a point: {e}")))?;
    Ok(encode_token(&state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_says_what_it_opens() {
        let v = |h: &str| serde_json::from_str::<serde_json::Value>(&parse_launch_json(h)).unwrap();
        assert_eq!(v("https://x/?presetId=AbCdEf1234")["kind"], "presetId");
        assert_eq!(v("https://x/?preset=3")["index"], 3);
        assert_eq!(v("https://x/")["kind"], "none");
        let point = serde_json::to_string(&syn_core::state::presets()[0].state).unwrap();
        let token = encode_token_json(&point).unwrap();
        assert_eq!(
            v(&format!("https://x/?preset=2#s={token}"))["kind"],
            "point",
            "a token beats a preset number"
        );
    }
}
