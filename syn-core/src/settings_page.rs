//! The web app's ⚙ Settings page, as a model (synesthesia PLAN-CORE.md
//! phase 6, decision C13): the page keeps its own look — effect presets, a
//! filter that shows only the rows its type uses, at most five formulas,
//! routes listed per side with "add" — and takes everything it shows and
//! every rule it keeps from here. [`crate::settings`] is the other view of
//! the same point, generated from the genes (the Android page).
//!
//! The data — titles, slider names and steps, choices and their labels,
//! which filter rows a type uses, the scales — is `assets/settings-page.json`
//! (first dumped from the web app, edited here since). The rules are the
//! functions below. Both apps can use either view; a value a control can set
//! always survives the genome (tests/settings_page.rs).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::genome::codec::encode_genome;
use crate::genome::evolve::same_genome;
use crate::modmatrix::ModRoute;
use crate::schema::schema;
use crate::state::AppState;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Control {
    pub k: String,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exp: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChoiceOption {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Choice {
    pub k: String,
    pub name: String,
    pub options: Vec<ChoiceOption>,
}

/// One effect module as the page shows it, in the order the chain runs.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FxModule {
    /// Its on-switch, an `FxState` key (`filterOn`).
    pub on: String,
    pub title: String,
    pub tag: String,
    pub choices: Vec<Choice>,
    pub sliders: Vec<Control>,
}

/// Which filter rows a type uses, and what its frequency and Q mean.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterControls {
    pub q: bool,
    pub gain: bool,
    pub vowel: bool,
    pub comb: bool,
    pub freq_label: String,
    pub q_label: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CouplingControl {
    pub k: String,
    pub name: String,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    /// "offset": a signed nudge to one card param; "effect": a display effect.
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Scale {
    pub min: f64,
    pub max: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exp: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub k: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TargetParam {
    pub k: String,
    pub name: String,
    pub exp: bool,
}

/// What a route on one side can aim at: a formula, a card, or the effects.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TargetGroup {
    pub id: String,
    pub title: String,
    pub params: Vec<TargetParam>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TargetGroups {
    pub sound: Vec<TargetGroup>,
    pub picture: Vec<TargetGroup>,
}

/// The page's data (`assets/settings-page.json`).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPage {
    pub fx_modules: Vec<FxModule>,
    pub filter_controls: std::collections::BTreeMap<String, FilterControls>,
    pub vowels: Vec<String>,
    pub coupling_controls: Vec<CouplingControl>,
    pub lfo_shape_labels: std::collections::BTreeMap<String, String>,
    pub lfo_rate: Scale,
    pub lfo_phase: Scale,
    pub route_depth: Scale,
    pub new_route_depth: f64,
    pub target_groups: TargetGroups,
}

pub const SETTINGS_PAGE_JSON: &str = include_str!("../../assets/settings-page.json");

pub fn page() -> &'static SettingsPage {
    static PAGE: std::sync::OnceLock<SettingsPage> = std::sync::OnceLock::new();
    PAGE.get_or_init(|| serde_json::from_str(SETTINGS_PAGE_JSON).expect("assets/settings-page.json"))
}

/// The sound's side, or the picture's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Domain {
    Sound,
    Picture,
}

pub const FX_TARGET: &str = "fx";

pub fn filter_controls(filter_type: &str) -> Option<&'static FilterControls> {
    page().filter_controls.get(filter_type)
}

/// A vowel position 0..1 as its letter.
pub fn vowel_label(v: f64) -> &'static str {
    let vowels = &page().vowels;
    let i = (v * (vowels.len() - 1) as f64).round().clamp(0.0, (vowels.len() - 1) as f64) as usize;
    &vowels[i]
}

pub fn enabled_formulas(point: &AppState) -> usize {
    point.audio.formulas.values().filter(|f| f.enabled).count()
}

/// More than the schema's maximum turns into mush: evolution keeps to it,
/// and so does the page. A formula that is on can always be switched.
pub fn can_enable_formula(point: &AppState, id: &str) -> bool {
    point.audio.formulas.get(id).is_some_and(|f| f.enabled)
        || enabled_formulas(point) < schema().max_enabled_formulas
}

pub fn route_domain(target: &str) -> Domain {
    if schema().cards.iter().any(|c| c.id == target) {
        Domain::Picture
    } else {
        Domain::Sound
    }
}

pub fn target_groups(domain: Domain) -> &'static [TargetGroup] {
    match domain {
        Domain::Sound => &page().target_groups.sound,
        Domain::Picture => &page().target_groups.picture,
    }
}

/// Whether a route's target is audible or visible now: its formula or card
/// is on; the effects, and the cards that are always on, always are.
pub fn is_target_on(point: &AppState, target: &str) -> bool {
    if target == FX_TARGET || schema().always_on_card_ids.iter().any(|id| id == target) {
        return true;
    }
    if let Some(f) = point.audio.formulas.get(target) {
        return f.enabled;
    }
    point.visual.cards.get(target).is_some_and(|c| c.on)
}

pub fn can_add_route(point: &AppState) -> bool {
    point.modulation.routes.len() < schema().route_slots
}

/// A fresh route for one side: LFO 1 on the first thing that is on there.
pub fn new_route(point: &AppState, domain: Domain) -> ModRoute {
    let groups = target_groups(domain);
    let group = groups
        .iter()
        .find(|g| g.id != FX_TARGET && is_target_on(point, &g.id))
        .or(groups.last())
        .expect("a side has targets");
    let param = &group.params[0];
    ModRoute {
        src: 0,
        target: group.id.clone(),
        param: param.k.clone(),
        depth: page().new_route_depth,
        exp: param.exp,
    }
}

/// The octave flag the schema gives a target (frequency-like params move in octaves).
pub fn target_exp(target: &str, param: &str) -> bool {
    schema().mod_targets.iter().any(|t| t.target == target && t.param == param && t.exp)
}

/// `target.param` of every route that moves something — those controls get a ∿.
pub fn modulated_keys(routes: &[ModRoute]) -> Vec<String> {
    let mut keys: Vec<String> =
        routes.iter().filter(|r| r.depth != 0.0).map(|r| format!("{}.{}", r.target, r.param)).collect();
    keys.sort();
    keys.dedup();
    keys
}

/// Same point in everything a control can set, compared value by value to
/// nine significant digits (the codec moves the fifteenth); the volume and
/// the name do not count — what closing the page asks ("was anything
/// changed?"), and what the round-trip guarantee is stated in. Not a genome
/// comparison: that would call any two points that encode alike the same,
/// which is the very thing to check.
pub fn same_point(a: &AppState, b: &AppState) -> bool {
    key(a) == key(b)
}

fn key(s: &AppState) -> Value {
    let routes: Vec<Value> = s
        .modulation
        .routes
        .iter()
        .map(
            |r| json!({ "src": r.src, "target": r.target, "param": r.param, "depth": r.depth, "exp": r.exp }),
        )
        .collect();
    let mut v = json!({
        "fx": s.audio.fx, "formulas": s.audio.formulas, "visual": s.visual, "coupling": s.coupling,
        "lfos": s.modulation.lfos, "routes": routes,
    });
    round(&mut v);
    v
}

fn round(v: &mut Value) {
    match v {
        Value::Number(n) => {
            if let Some(x) = n.as_f64() {
                // toPrecision(9)
                let r: f64 = format!("{x:.8e}").parse().unwrap_or(x);
                *v = json!(r);
            }
        }
        Value::Array(xs) => xs.iter_mut().for_each(round),
        Value::Object(m) => m.values_mut().for_each(round),
        _ => {}
    }
}

/// Whether the genes differ, up to the codec's noise — the session's own
/// question when a point comes back from the page.
pub fn same_genes(a: &AppState, b: &AppState) -> bool {
    same_genome(&encode_genome(a), &encode_genome(b))
}
