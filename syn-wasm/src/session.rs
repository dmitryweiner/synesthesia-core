//! The session for the web app's main thread (synesthesia PLAN-CORE.md
//! phase 4): `syn-session` as an object, its effects as JSON the page
//! switches on, and the scout split into units a Web Worker runs.
//!
//! The main thread has a `TextDecoder`, so strings cross freely here (unlike
//! the audio side). A scout job goes out as a `startScout` effect; the page
//! runs [`scout_score`] for the parent and each candidate on its worker pool
//! and hands the parts back to [`WebSession::scout_finished`].

use serde_json::{json, Value};
use syn_core::analysis::fractal::SoundAnalysis;
use syn_core::genome::scout::{self, ScoutKind, ScoutSettings};
use syn_core::state::AppState;
use syn_session::{Effect, Session, SessionOptions, View};
use wasm_bindgen::prelude::*;

const SILENT: SoundAnalysis = SoundAnalysis {
    silent: true,
    loudness: f64::NEG_INFINITY,
    env_beta: 0.0,
    centroid_beta: 0.0,
    env_higuchi: 0.0,
    box_dim: 0.0,
    score: 0.0,
};

fn parse_point(json: &str) -> Result<AppState, JsError> {
    serde_json::from_str(json).map_err(|e| JsError::new(&format!("not a point: {e}")))
}

fn analysis_json(a: &SoundAnalysis) -> Value {
    json!({
        "silent": a.silent, "loudness": a.loudness, "envBeta": a.env_beta,
        "centroidBeta": a.centroid_beta, "envHiguchi": a.env_higuchi, "boxDim": a.box_dim, "score": a.score,
    })
}

/// A silent render's numbers are not finite, and JSON writes them as null:
/// null reads back as NaN (the loudness as −∞), a missing field as nothing.
fn analysis_of(v: &Value) -> Option<SoundAnalysis> {
    let n = |k: &str| match v.get(k)? {
        Value::Null => Some(f64::NAN),
        x => x.as_f64(),
    };
    Some(SoundAnalysis {
        silent: v.get("silent")?.as_bool()?,
        loudness: n("loudness").map(|x| if x.is_nan() { f64::NEG_INFINITY } else { x })?,
        env_beta: n("envBeta")?,
        centroid_beta: n("centroidBeta")?,
        env_higuchi: n("envHiguchi")?,
        box_dim: n("boxDim")?,
        score: n("score")?,
    })
}

fn effect_json(e: Effect) -> Value {
    match e {
        Effect::SetPoint(p) => json!({ "type": "setPoint", "point": *p }),
        Effect::SwitchTo(p) => json!({ "type": "switchTo", "point": *p }),
        Effect::Reseed => json!({ "type": "reseed" }),
        Effect::SaveLastPoint(p) => json!({ "type": "saveLastPoint", "point": *p }),
        Effect::Status(text) => json!({ "type": "status", "text": text }),
        Effect::StartScout(req) => json!({
            "type": "startScout",
            "job": {
                "version": req.version,
                "parent": req.parent,
                "likes": req.likes,
                "dislikes": req.dislikes,
                "settings": {
                    "seconds": req.settings.seconds,
                    "sampleRate": req.settings.sample_rate,
                    "masterGain": req.settings.master_gain,
                    "seed": req.settings.seed,
                },
            },
        }),
    }
}

fn effects(fx: Vec<Effect>) -> String {
    Value::Array(fx.into_iter().map(effect_json).collect()).to_string()
}

fn view_json(v: View) -> String {
    json!({
        "name": v.name, "pointName": v.point_name, "steps": v.steps, "status": v.status,
        "canUndo": v.can_undo, "undoDepth": v.undo_depth, "sigma": v.sigma, "morphing": v.morphing,
        "playing": v.playing, "scoutedLike": v.scouted_like, "scoutedDislike": v.scouted_dislike,
        "scoutBusy": v.scout_busy,
    })
    .to_string()
}

/// One listening session. Every call takes `now` — monotonic seconds from
/// the page's clock — and returns the effects to carry out, as a JSON array
/// of `{type, …}`: setPoint / switchTo / saveLastPoint `{point}`, reseed,
/// status `{text}`, startScout `{job}`.
#[wasm_bindgen]
pub struct WebSession {
    inner: Session,
}

#[wasm_bindgen]
impl WebSession {
    /// A session on a point (AppState JSON), named as the page knows it
    /// (empty: the point's own `presetName`). `seed` is the session's
    /// randomness; `scout` turns the background scout on.
    #[wasm_bindgen(constructor)]
    pub fn new(name: &str, point_json: &str, seed: u32, scout: bool) -> Result<WebSession, JsError> {
        let mut opts = SessionOptions { seed, ..SessionOptions::default() };
        opts.scout.enabled = scout;
        Ok(WebSession { inner: Session::new(name, &parse_point(point_json)?, opts) })
    }

    pub fn tick(&mut self, now: f64) -> String {
        effects(self.inner.tick(now))
    }

    #[wasm_bindgen(js_name = wantsTick)]
    pub fn wants_tick(&self) -> bool {
        self.inner.wants_tick()
    }

    #[wasm_bindgen(js_name = setPlaying)]
    pub fn set_playing(&mut self, now: f64, playing: bool) -> String {
        effects(self.inner.set_playing(now, playing))
    }

    pub fn like(&mut self, now: f64) -> String {
        effects(self.inner.like(now))
    }

    pub fn dislike(&mut self, now: f64) -> String {
        effects(self.inner.dislike(now))
    }

    pub fn surprise(&mut self, now: f64) -> String {
        effects(self.inner.surprise(now))
    }

    pub fn undo(&mut self, now: f64) -> String {
        effects(self.inner.undo(now))
    }

    /// A whole point arrives (a preset, a saved point, a link).
    pub fn load(&mut self, now: f64, name: &str, point_json: &str) -> Result<String, JsError> {
        Ok(effects(self.inner.load(now, name, &parse_point(point_json)?)))
    }

    #[wasm_bindgen(js_name = openSettings)]
    pub fn open_settings(&mut self, now: f64) -> String {
        effects(self.inner.open_settings(now))
    }

    #[wasm_bindgen(js_name = closeSettings)]
    pub fn close_settings(&mut self, now: f64, point_json: &str) -> Result<String, JsError> {
        Ok(effects(self.inner.close_settings(now, &parse_point(point_json)?)))
    }

    #[wasm_bindgen(js_name = keptAs)]
    pub fn kept_as(&mut self, name: &str) -> String {
        effects(self.inner.kept_as(name))
    }

    #[wasm_bindgen(js_name = setMasterGain)]
    pub fn set_master_gain(&mut self, now: f64, gain: f64) -> String {
        effects(self.inner.set_master_gain(now, gain))
    }

    /// A scout job came back: `{version, seconds, parent: analysis,
    /// candidates: [{kind: "like"|"dislike", genome, analysis}]}`. A result
    /// for a point the user has moved on from is dropped; so is a malformed
    /// one, which still frees the scout for the next job.
    #[wasm_bindgen(js_name = scoutFinished)]
    pub fn scout_finished(&mut self, result_json: &str) -> String {
        let v: Value = serde_json::from_str(result_json).unwrap_or(Value::Null);
        let parse = || -> Option<scout::ScoutResult> {
            let parent = analysis_of(v.get("parent")?)?;
            let mut scored = Vec::new();
            for c in v.get("candidates")?.as_array()? {
                let kind = match c.get("kind")?.as_str()? {
                    "like" => ScoutKind::Like,
                    "dislike" => ScoutKind::Dislike,
                    _ => return None,
                };
                let genome: Vec<f64> = serde_json::from_value(c.get("genome")?.clone()).ok()?;
                scored.push((kind, genome, analysis_of(c.get("analysis")?)?));
            }
            Some(scout::assemble(v.get("version")?.as_u64()?, parent, scored, v.get("seconds")?.as_f64()?))
        };
        // u64::MAX is never the explorer's version: a failed job is dropped
        // like a stale one, and the session may scout again.
        let result = parse().unwrap_or_else(|| scout::assemble(u64::MAX, SILENT, Vec::new(), 0.0));
        effects(self.inner.scout_finished(result))
    }

    /// The point the search is at (mid-morph: where it is heading), JSON.
    #[wasm_bindgen(js_name = pointJson)]
    pub fn point_json(&self) -> String {
        serde_json::to_string(&self.inner.point()).unwrap_or_default()
    }

    /// The point that is audible and visible now (mid-morph: the blend), JSON.
    #[wasm_bindgen(js_name = livePointJson)]
    pub fn live_point_json(&self) -> String {
        serde_json::to_string(&self.inner.live_point()).unwrap_or_default()
    }

    /// The name, steps, status, undo depth, spread and scout counts, JSON.
    pub fn view(&self) -> String {
        view_json(self.inner.view())
    }

    /// What the scout measured of the current point, JSON, or `undefined`.
    #[wasm_bindgen(js_name = scoutParent)]
    pub fn scout_parent(&self) -> Option<String> {
        self.inner.scout_parent().map(|a| analysis_json(&a).to_string())
    }
}

/// One unit of a scout job, for a Web Worker: render `genome` as the scout
/// hears it and score it. Returns the analysis as JSON.
#[wasm_bindgen(js_name = scoutScore)]
pub fn scout_score(genome: &[f64], seconds: f64, sample_rate: f64, master_gain: f64, seed: u32) -> String {
    let set = ScoutSettings { seconds, sample_rate, master_gain, seed };
    analysis_json(&scout::score(&genome.to_vec(), &set)).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset_json(i: usize) -> String {
        serde_json::to_string(&syn_core::state::presets()[i].state).unwrap()
    }

    fn parse(s: &str) -> Vec<Value> {
        serde_json::from_str::<Vec<Value>>(s).unwrap()
    }

    #[test]
    fn a_press_morphs_and_the_scout_round_trips_through_json() {
        let mut s = WebSession::new("", &preset_json(0), 7, true).unwrap();
        assert!(parse(&s.set_playing(0.0, true)).is_empty());
        // settle, then the scout asks for a job
        let job = (1..40)
            .flat_map(|i| parse(&s.tick(i as f64 * 0.05)))
            .find(|e| e["type"] == "startScout")
            .expect("a scout job")["job"]
            .clone();
        let set = &job["settings"];
        let tiny = |g: &Value| -> Value {
            let g: Vec<f64> = serde_json::from_value(g.clone()).unwrap();
            serde_json::from_str(&scout_score(&g, 1.0, 8000.0, set["masterGain"].as_f64().unwrap(), 1))
                .unwrap()
        };
        let mut candidates = Vec::new();
        for (kind, list) in [("like", &job["likes"]), ("dislike", &job["dislikes"])] {
            for g in list.as_array().unwrap() {
                candidates.push(json!({ "kind": kind, "genome": g, "analysis": tiny(g) }));
            }
        }
        let result = json!({ "version": job["version"], "seconds": 0.5, "parent": tiny(&job["parent"]), "candidates": candidates });
        let fx = parse(&s.scout_finished(&result.to_string()));
        assert!(
            fx.iter().any(|e| e["text"].as_str().is_some_and(|t| t.starts_with("scouted 3 + 3"))),
            "{fx:?}"
        );
        let view: Value = serde_json::from_str(&s.view()).unwrap();
        assert_eq!(view["scoutedLike"], 3);
        assert!(s.scout_parent().is_some());

        let fx = parse(&s.like(3.0));
        assert!(fx
            .iter()
            .any(|e| e["type"] == "status" && e["text"].as_str().unwrap().contains("scouted: best of 3")));
        let view: Value = serde_json::from_str(&s.view()).unwrap();
        assert_eq!(view["morphing"], true);
        assert_eq!(view["canUndo"], true);
    }

    #[test]
    fn a_broken_scout_result_frees_the_scout_and_is_dropped() {
        let mut s = WebSession::new("", &preset_json(1), 3, true).unwrap();
        s.set_playing(0.0, true);
        let fx = parse(&s.scout_finished("{nope"));
        assert!(fx.is_empty());
        let view: Value = serde_json::from_str(&s.view()).unwrap();
        assert_eq!(view["scoutBusy"], false);
    }

    #[test]
    fn a_load_switches_hard_and_reseeds() {
        let mut s = WebSession::new("", &preset_json(0), 1, false).unwrap();
        let fx = parse(&s.load(0.0, "", &preset_json(3)).unwrap());
        let kinds: Vec<&str> = fx.iter().map(|e| e["type"].as_str().unwrap()).collect();
        assert_eq!(&kinds[..2], ["switchTo", "reseed"]);
        assert!(kinds.contains(&"saveLastPoint"));
    }
}
