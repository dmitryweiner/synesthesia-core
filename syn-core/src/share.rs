//! Point tokens: the `#s=` payload of the web app's long share links, which is
//! base64url of the point's JSON. There is no server here (PLAN.md decision
//! 9) — the token is just a way to carry a point between the two apps by
//! copy and paste.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

use crate::state::AppState;

/// Encodes a point as the token a web link would carry.
pub fn encode_token(state: &AppState) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_string(state).unwrap_or_default())
}

/// Decodes a token, or the whole link it was pasted from.
pub fn decode_token(token: &str) -> Option<AppState> {
    let raw = token.trim();
    // Accept a bare token, `#s=…`, or a full URL with `#s=` in it.
    let payload = match raw.split_once("#s=") {
        Some((_, rest)) => rest,
        None => raw.strip_prefix("s=").unwrap_or(raw),
    };
    let payload = payload.split(['&', ' ']).next()?;
    // Padded or not: a token copied with its `=` still opens.
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    // As tolerant as the web app was (sanitizeState + stateToAppState): an
    // old link carries an old, partial point, and opens as one with the
    // defaults filled in (synesthesia PLAN-CORE.md phase 6).
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    crate::point::sanitize(&value)
}

/// What a link asks to open — the web app's `parseLaunch`, in its order:
/// a stored point beats a token, a token beats a preset number.
#[derive(Clone, Debug, PartialEq)]
pub enum Launch {
    /// A point carried whole, in a `#s=` token.
    Point(Box<AppState>),
    /// `?preset=N`: one of the built-in points.
    Preset(usize),
    /// `?presetId=…`: a point kept on the web app's server. A native app has
    /// no way to fetch it (PLAN.md decision 8: no network), so it says so
    /// rather than opening the wrong thing.
    Stored(String),
    /// Nothing in the link names a point.
    Nothing,
}

/// The web app's share ids: ten letters and digits.
fn is_stored_id(s: &str) -> bool {
    s.len() == 10 && s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then_some(v)
    })
}

/// What a link opens. Takes anything a browser or an intent may hand over —
/// a full URL, or just its query and fragment.
pub fn parse_launch(href: &str) -> Launch {
    let href = href.trim();
    let (before_hash, fragment) = href.split_once('#').unwrap_or((href, ""));
    let query = before_hash.split_once('?').map(|(_, q)| q).unwrap_or("");

    if let Some(id) = param(query, "presetId").filter(|id| is_stored_id(id)) {
        return Launch::Stored(id.to_string());
    }
    if fragment.starts_with("s=") {
        if let Some(state) = decode_token(fragment) {
            return Launch::Point(Box::new(state));
        }
    }
    if let Some(index) = param(query, "preset").and_then(|n| n.parse().ok()) {
        return Launch::Preset(index);
    }
    Launch::Nothing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::presets;

    #[test]
    fn a_point_survives_the_round_trip() {
        for p in presets() {
            let token = encode_token(&p.state);
            let back = decode_token(&token).expect("decodes");
            assert_eq!(back, p.state, "{}", p.name);
        }
    }

    #[test]
    fn an_old_partial_point_opens_with_the_defaults_filled_in() {
        let token = URL_SAFE_NO_PAD
            .encode(r#"{"presetName":"Old long link","audio":{"formulas":{"fm":{"enabled":true}}}}"#);
        let state = decode_token(&token).expect("an old link still opens");
        assert_eq!(state.preset_name.as_deref(), Some("Old long link"));
        assert!(state.audio.formulas["fm"].enabled);
        assert!(decode_token(&URL_SAFE_NO_PAD.encode("42")).is_none(), "a number is no point");
    }

    #[test]
    fn a_whole_link_is_accepted_too() {
        let state = &presets()[3].state;
        let token = encode_token(state);
        let link = format!("https://dmitryweiner.github.io/synesthesia/#s={token}");
        assert_eq!(decode_token(&link).as_ref(), Some(state));
        assert_eq!(decode_token(&format!("  #s={token}  ")).as_ref(), Some(state));
    }

    #[test]
    fn nonsense_is_rejected_rather_than_guessed() {
        assert!(decode_token("").is_none());
        assert!(decode_token("not a token").is_none());
        assert!(decode_token("###").is_none());
    }

    #[test]
    fn a_link_says_what_it_opens_in_the_web_apps_order() {
        let state = &presets()[6].state;
        let token = encode_token(state);
        let site = "https://dmitryweiner.github.io/synesthesia/";

        assert_eq!(parse_launch(&format!("{site}#s={token}")), Launch::Point(Box::new(state.clone())));
        assert_eq!(parse_launch(&format!("{site}?preset=7")), Launch::Preset(7));
        assert_eq!(parse_launch(&format!("{site}?res=512&preset=3")), Launch::Preset(3));
        assert_eq!(parse_launch(&format!("{site}?presetId=aB3dE6gH9j")), Launch::Stored("aB3dE6gH9j".into()));
        // A stored id beats a token, as the web app's order has it.
        let both = format!("{site}?presetId=aB3dE6gH9j#s={token}");
        assert_eq!(parse_launch(&both), Launch::Stored("aB3dE6gH9j".into()));

        assert_eq!(parse_launch(site), Launch::Nothing);
        assert_eq!(parse_launch(&format!("{site}?presetId=too-short")), Launch::Nothing);
        assert_eq!(parse_launch(&format!("{site}#s=not-a-token")), Launch::Nothing);
        assert_eq!(parse_launch(&format!("{site}?preset=x")), Launch::Nothing);
        assert_eq!(parse_launch(""), Launch::Nothing);
    }
}
