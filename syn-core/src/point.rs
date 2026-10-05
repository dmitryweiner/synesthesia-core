//! A point from outside — a link, a file, the points Worker — made safe, and
//! its content address (synesthesia PLAN-CORE.md C5).
//!
//! [`sanitize`] is the web app's `sanitizeState` followed by
//! `stateToAppState`: anything JSON in, a complete [`AppState`] out, every
//! number clamped to its range, unknown keys and malformed parts dropped
//! (never the whole point, unless it is not an object at all).
//! [`canonical_json`] and [`point_id`] are its `canonicalJson` and
//! `presetIdOf`: the points Worker stores a point under the first 10 base62
//! digits of the SHA-256 of that JSON, so they must agree with the
//! TypeScript **byte for byte**, or a shared link would get another id.
//! `fixtures/points.json` (dumped from the TypeScript) pins all three.

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::modmatrix::LfoShape;
use crate::schema::{schema, CardDef, FormulaDef};
use crate::state::AppState;

/// `typeof u === 'object' && u !== null` — which an array is too; reading a
/// named key of one gives nothing, as in JavaScript.
fn is_record(v: &Value) -> bool {
    v.is_object() || v.is_array()
}

/// `Object.entries`: an array's entries are its indices, which never name a
/// formula, a card or a parameter here, so it has none worth keeping.
fn entries(v: &Value) -> Option<&Map<String, Value>> {
    v.as_object()
}

/// A finite number (serde_json never holds NaN or ±∞).
fn finite(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite())
}

fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    // Math.max(lo, Math.min(hi, x))
    lo.max(hi.min(x))
}

fn formula_def(id: &str) -> Option<&'static FormulaDef> {
    schema().formulas.iter().find(|f| f.id == id)
}

fn card_def(id: &str) -> Option<&'static CardDef> {
    schema().cards.iter().find(|c| c.id == id)
}

const FX_BOOL_KEYS: [&str; 6] = ["filterOn", "chorusOn", "reverbOn", "limiterOn", "delayOn", "phaserOn"];
const FX_NUM_KEYS: [&str; 22] = [
    "filterFreq",
    "filterQ",
    "filterGain",
    "filterVowel",
    "filterCombFb",
    "chorusRate",
    "chorusDepth",
    "chorusMix",
    "chorusFb",
    "reverbDecay",
    "reverbMix",
    "limiterThr",
    "limiterRel",
    "delayTime",
    "delayFb",
    "delayMix",
    "delayShimmer",
    "phaserRate",
    "phaserDepth",
    "phaserStages",
    "phaserFb",
    "phaserMix",
];

fn is_fx_mod_param(k: &str) -> bool {
    schema().fx_mod_params.iter().any(|p| p == k)
}

fn is_mod_target(target: &str, param: &str) -> bool {
    if target == "fx" {
        return is_fx_mod_param(param);
    }
    if schema().formula_ids.iter().any(|id| id == target) {
        return formula_def(target).is_some_and(|f| f.sliders.iter().any(|s| s.k == param));
    }
    card_def(target).is_some_and(|c| c.sliders.iter().any(|s| s.k == param))
}

fn sanitize_lfo(v: Option<&Value>) -> Option<Value> {
    let v = v.filter(|v| is_record(v))?;
    let shape = v.get("shape").filter(|s| s.is_string())?;
    serde_json::from_value::<LfoShape>(shape.clone()).ok()?;
    let rate = finite(v.get("rate"))?;
    let phase = finite(v.get("phase"))?;
    Some(serde_json::json!({ "shape": shape, "rate": rate, "phase": phase }))
}

fn sanitize_route(v: &Value, lfo_count: usize) -> Option<Value> {
    if !is_record(v) {
        return None;
    }
    let src = v.get("src").and_then(Value::as_f64)?;
    if src.fract() != 0.0 || src < 0.0 || src >= lfo_count as f64 {
        return None;
    }
    let target = v.get("target").and_then(Value::as_str)?;
    let param = v.get("param").and_then(Value::as_str)?;
    if !is_mod_target(target, param) {
        return None;
    }
    let depth = finite(v.get("depth"))?;
    let mut route = serde_json::json!({
        "src": src as u64, "target": target, "param": param, "depth": clamp(depth, -1.0, 1.0),
    });
    if v.get("exp") == Some(&Value::Bool(true)) {
        route["exp"] = Value::Bool(true);
    }
    Some(route)
}

/// The web app's `sanitizeState` + `stateToAppState`: `None` only when the
/// input is not an object (or array) at all.
pub fn sanitize(u: &Value) -> Option<AppState> {
    if !is_record(u) {
        return None;
    }
    let s = schema();
    let mut state = s.default_state.clone();

    if let Some(audio) = u.get("audio").filter(|a| is_record(a)) {
        if let Some(g) = finite(audio.get("masterGain")) {
            state["audio"]["masterGain"] = clamp(g, 0.0, 1.0).into();
        }
        if let Some(fx) = audio.get("fx").filter(|f| is_record(f)) {
            let out = &mut state["audio"]["fx"];
            for k in FX_BOOL_KEYS {
                if let Some(b) = fx.get(k).and_then(Value::as_bool) {
                    out[k] = b.into();
                }
            }
            for k in FX_NUM_KEYS {
                if let Some(x) = finite(fx.get(k)) {
                    out[k] = x.into();
                }
            }
            if let Some(t) = fx.get("filterType").and_then(Value::as_str) {
                if s.filter_types.iter().any(|f| f == t) {
                    out["filterType"] = t.into();
                }
            }
            if let Some(m) = fx.get("chorusMode").and_then(Value::as_str) {
                if m == "chorus" || m == "flanger" {
                    out["chorusMode"] = m.into();
                }
            }
            // clampFx
            for (k, [lo, hi]) in &s.fx_param_ranges {
                if is_fx_mod_param(k) {
                    if let Some(x) = out[k.as_str()].as_f64() {
                        out[k.as_str()] = clamp(x, *lo, *hi).into();
                    }
                }
            }
            let [lo, hi] = s.reverb_decay_range;
            if let Some(x) = out["reverbDecay"].as_f64() {
                out["reverbDecay"] = clamp(x, lo, hi).into();
            }
            let stages = out["phaserStages"].as_f64();
            if !stages.is_some_and(|x| s.phaser_stages.contains(&x)) {
                out["phaserStages"] = s.default_state["audio"]["fx"]["phaserStages"].clone();
            }
        }
        if let Some(formulas) = audio.get("formulas").filter(|f| is_record(f)).and_then(entries) {
            for (id, snap) in formulas {
                let (Some(def), true) = (formula_def(id), is_record(snap)) else { continue };
                if !s.formula_ids.iter().any(|f| f == id) {
                    continue;
                }
                let out = &mut state["audio"]["formulas"][id.as_str()];
                if let Some(b) = snap.get("enabled").and_then(Value::as_bool) {
                    out["enabled"] = b.into();
                }
                if let Some(params) = snap.get("params").filter(|p| is_record(p)) {
                    for sl in &def.sliders {
                        if let Some(x) = finite(params.get(&sl.k)) {
                            out["params"][sl.k.as_str()] = clamp(x, sl.min, sl.max).into();
                        }
                    }
                }
            }
        }
    }

    if let Some(cards) = u.get("visual").filter(|v| is_record(v)).and_then(|v| v.get("cards")) {
        if let Some(cards) = Some(cards).filter(|c| is_record(c)).and_then(entries) {
            for (id, snap) in cards {
                let (Some(card), true) = (card_def(id), is_record(snap)) else { continue };
                let out = &mut state["visual"]["cards"][id.as_str()];
                if let Some(b) = snap.get("on").and_then(Value::as_bool) {
                    out["on"] = b.into();
                }
                if let Some(params) = snap.get("params").filter(|p| is_record(p)) {
                    for sl in &card.sliders {
                        if let Some(x) = finite(params.get(&sl.k)) {
                            out["params"][sl.k.as_str()] = clamp(x, sl.min, sl.max).into();
                        }
                    }
                    for sel in &card.selects {
                        if let Some(x) = finite(params.get(&sel.k)) {
                            if sel.options.iter().any(|o| o.v == x) {
                                out["params"][sel.k.as_str()] = x.into();
                            }
                        }
                    }
                }
            }
        }
    }

    if let Some(m) = u.get("mod").filter(|m| is_record(m)) {
        if let (Some(lfos), Some(routes)) =
            (m.get("lfos").and_then(Value::as_array), m.get("routes").and_then(Value::as_array))
        {
            // A malformed LFO becomes the default rather than being dropped:
            // routes point at LFOs by index.
            let default_lfo = serde_json::to_value(s.default_lfo).expect("an LfoDef serializes");
            let lfos: Vec<Value> = (0..s.lfo_count)
                .map(|i| sanitize_lfo(lfos.get(i)).unwrap_or_else(|| default_lfo.clone()))
                .collect();
            let routes: Vec<Value> = routes.iter().filter_map(|r| sanitize_route(r, lfos.len())).collect();
            state["mod"] = serde_json::json!({ "lfos": lfos, "routes": routes });
        }
    }

    if let Some(c) = u.get("coupling").filter(|c| is_record(c)) {
        for k in &s.coupling_keys {
            if let (Some(x), Some([lo, hi])) = (finite(c.get(k)), s.coupling_ranges.get(k)) {
                state["coupling"][k.as_str()] = clamp(x, *lo, *hi).into();
            }
        }
    }

    // `if (partial.presetName)`: an empty name is no name.
    if let Some(name) = u.get("presetName").and_then(Value::as_str).filter(|n| !n.is_empty()) {
        state["presetName"] = name.into();
    }

    serde_json::from_value(state).ok()
}

/// The web app's `canonicalJson` of a point: keys sorted at every depth,
/// numbers and strings written exactly as `JSON.stringify` writes them.
pub fn canonical_json(state: &AppState) -> String {
    let v = serde_json::to_value(state).expect("an AppState serializes");
    let mut out = String::new();
    write_canonical(&v, &mut out);
    out
}

fn write_canonical(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else {
                out.push_str(&js_number(n.as_f64().unwrap_or(0.0)));
            }
        }
        Value::String(s) => write_js_string(s, out),
        Value::Array(xs) => {
            out.push('[');
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(x, out);
            }
            out.push(']');
        }
        Value::Object(m) => {
            // JavaScript compares strings by UTF-16 code units.
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_js_string(k, out);
                out.push(':');
                write_canonical(&m[k], out);
            }
            out.push('}');
        }
    }
}

/// `JSON.stringify` of a string.
fn write_js_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// ECMAScript's Number::toString for a finite number: the shortest digits
/// that round-trip (which Rust's `{:e}` also gives), laid out by the
/// specification's rules — `55`, `0.1`, `1e-7`, `1e+21`, and `0` for -0.
pub fn js_number(x: f64) -> String {
    if x == 0.0 {
        return "0".to_string();
    }
    if x < 0.0 {
        return format!("-{}", js_number(-x));
    }
    let sci = format!("{x:e}");
    let (mantissa, exp) = sci.split_once('e').expect("{:e} has an exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let k = digits.len() as i64;
    let n = exp.parse::<i64>().expect("an integer exponent") + 1;
    if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e >= 0 { '+' } else { '-' };
        if k == 1 {
            format!("{digits}e{sign}{}", e.abs())
        } else {
            format!("{}.{}e{sign}{}", &digits[..1], &digits[1..], e.abs())
        }
    }
}

const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// The points Worker's id of a canonical JSON string: the first 8 bytes of
/// its SHA-256 as a big-endian integer, its lowest 10 base62 digits.
pub fn point_id(canonical: &str) -> String {
    let digest = Sha256::digest(canonical.as_bytes());
    let mut n = u64::from_be_bytes(digest[..8].try_into().expect("8 bytes"));
    let mut out = [0u8; 10];
    for slot in out.iter_mut().rev() {
        *slot = BASE62[(n % 62) as usize];
        n /= 62;
    }
    String::from_utf8(out.to_vec()).expect("base62 is ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_written_as_javascript_writes_them() {
        for (x, js) in [
            (55.0, "55"),
            (0.1, "0.1"),
            (0.1 + 0.2, "0.30000000000000004"),
            (-0.0, "0"),
            (-2.5, "-2.5"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"),
            (123456789.12345679, "123456789.12345679"),
            (1.5e300, "1.5e+300"),
            (5e-324, "5e-324"),
        ] {
            assert_eq!(js_number(x), js, "{x:e}");
        }
    }

    #[test]
    fn strings_are_escaped_as_javascript_escapes_them() {
        let mut s = String::new();
        write_js_string("q\"b\\\n\t\u{1}\u{2028}é😀/", &mut s);
        assert_eq!(s, "\"q\\\"b\\\\\\n\\t\\u0001\u{2028}é😀/\"");
    }

    #[test]
    fn a_non_object_is_not_a_point() {
        for v in [Value::Null, 0.into(), "x".into(), true.into()] {
            assert!(sanitize(&v).is_none());
        }
        // an array is a record in JavaScript: a point with nothing in it
        assert_eq!(sanitize(&serde_json::json!([])), sanitize(&serde_json::json!({})));
    }
}
