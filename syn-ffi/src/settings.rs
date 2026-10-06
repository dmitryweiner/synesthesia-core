//! The Settings page through the FFI: the controls the core derives from the
//! schema, and the point being edited.
//!
//! The page is generated, not written (synesthesia-android PLAN.md, decision
//! 3): an app lays out whatever [`settings_page`] returns, in the order it
//! returns it, and sets values by the ids it carries. When the web app grows
//! a parameter — as it just grew the tanpura's and the delay's shimmer — the
//! page grows with it and no app changes.

use std::sync::{Arc, Mutex, MutexGuard};

use syn_core::schema::GeneKind;
use syn_core::settings::{self, Edit};

use crate::{parse_point, CoreError};

/// What a control looks like on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ControlKind {
    /// A continuous value between `min` and `max`.
    Slider,
    /// On or off: `value` is 0 or 1.
    Switch,
    /// One of `options`: `value` is its index.
    Choice,
}

/// One control. Everything here is the schema's: the label, the range, the
/// step, whether the scale is logarithmic, and which switch gates it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Control {
    /// What [`PointEdit::value`] and [`PointEdit::set_value`] take.
    pub id: String,
    /// The label as the schema writes it ("Filter cutoff").
    pub label: String,
    /// The same, without the section's name ("Feed" under "Reaction").
    pub short_label: String,
    pub kind: ControlKind,
    pub min: f64,
    pub max: f64,
    /// The schema's step, or 0 for a continuous slider with none.
    pub step: f64,
    /// Frequency-like: the slider should move in octaves.
    pub exp: bool,
    /// The switch that must be on for this control to do anything.
    pub active_if: Option<String>,
    /// A choice's options, in value order; empty otherwise.
    pub options: Vec<String>,
}

/// Which tab a section belongs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SettingsTab {
    Sound,
    Picture,
}

/// A group of controls under one heading.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Section {
    pub id: String,
    pub title: String,
    /// What this thing is, in the schema's own words — a formula's or a
    /// card's one-line description. Empty where the schema has none.
    pub description: String,
    /// Further reading: the English Wikipedia article about the thing itself,
    /// where there is one.
    pub article: Option<String>,
    pub tab: SettingsTab,
    /// The switch that turns the whole section on, when it has one.
    pub toggle: Option<Control>,
    pub controls: Vec<Control>,
}

fn control(section: &settings::Section, gene: &syn_core::schema::GeneDef) -> Control {
    Control {
        id: gene.id.clone(),
        label: gene.label.clone(),
        short_label: settings::short_label(section, gene),
        kind: match gene.kind {
            GeneKind::Cont => ControlKind::Slider,
            GeneKind::Bool => ControlKind::Switch,
            GeneKind::Choice => ControlKind::Choice,
        },
        min: gene.min,
        max: gene.max,
        step: gene.step.unwrap_or(0.0),
        exp: gene.exp,
        active_if: gene.active_if.clone(),
        options: settings::options(gene),
    }
}

/// The whole page, in the order it is shown.
#[uniffi::export]
pub fn settings_page() -> Vec<Section> {
    settings::sections()
        .iter()
        .map(|s| Section {
            id: s.id.clone(),
            title: s.title.clone(),
            description: s.description.clone(),
            article: settings::article(&s.id),
            tab: match s.tab {
                settings::Tab::Sound => SettingsTab::Sound,
                settings::Tab::Picture => SettingsTab::Picture,
            },
            toggle: s.toggle.map(|g| control(s, g)),
            controls: s.controls.iter().map(|g| control(s, g)).collect(),
        })
        .collect()
}

/// A point being edited by hand.
///
/// The page reads and writes values by control id while it is open, hands the
/// point to the sound as it goes (so an edit is heard as it is made), and on
/// closing gives it to `Session.close_settings`, which commits it as one
/// undoable step.
#[derive(uniffi::Object)]
pub struct PointEdit {
    inner: Mutex<Edit>,
}

#[uniffi::export]
impl PointEdit {
    /// Starts from a point — `Session.point_json`, the point the page opened on.
    #[uniffi::constructor]
    pub fn new(point_json: String) -> Result<Arc<Self>, CoreError> {
        Ok(Arc::new(PointEdit { inner: Mutex::new(Edit::new(&parse_point(&point_json)?)) }))
    }

    /// The control's value in its own units, or a choice's index.
    pub fn value(&self, id: String) -> f64 {
        self.locked().value(&id)
    }

    pub fn set_value(&self, id: String, value: f64) {
        self.locked().set_value(&id, value);
    }

    /// False when the control's switch is off: the value is still there, it
    /// simply does nothing, so the page can grey it out.
    pub fn is_active(&self, id: String) -> bool {
        self.locked().active(&id)
    }

    /// The user's volume: not a gene, so no press and no edit elsewhere
    /// changes it.
    pub fn master_gain(&self) -> f64 {
        self.locked().master_gain()
    }

    pub fn set_master_gain(&self, value: f64) {
        self.locked().set_master_gain(value);
    }

    /// Everything on one tab back to where a new point starts — ⚙'s "begin
    /// again from nothing", for the sound and the picture apart. The other
    /// tab keeps what it had, and the page is one undoable step either way,
    /// so a reset is taken back by ↩ like any edit.
    pub fn reset(&self, tab: SettingsTab) {
        self.locked().reset(match tab {
            SettingsTab::Sound => settings::Tab::Sound,
            SettingsTab::Picture => settings::Tab::Picture,
        });
    }

    /// The point as edited — for the sound now, and for the session on close.
    pub fn point_json(&self) -> String {
        serde_json::to_string(&self.locked().point()).unwrap_or_default()
    }
}

impl PointEdit {
    fn locked(&self) -> MutexGuard<'_, Edit> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset_state_json;

    #[test]
    fn the_page_is_the_schemas_own_and_covers_every_control() {
        let page = settings_page();
        assert!(!page.is_empty());
        let sound = page.iter().filter(|s| s.tab == SettingsTab::Sound).count();
        let picture = page.iter().filter(|s| s.tab == SettingsTab::Picture).count();
        assert!(sound > 0 && picture > 0, "two tabs");

        let filter = page.iter().find(|s| s.id == "filterOn").expect("the filter");
        assert_eq!(filter.title, "Filter");
        assert_eq!(filter.toggle.as_ref().map(|c| c.kind), Some(ControlKind::Switch));
        let cutoff = filter.controls.iter().find(|c| c.id == "fx.filterFreq").expect("the cutoff");
        assert_eq!(cutoff.kind, ControlKind::Slider);
        assert!(cutoff.exp, "a frequency moves in octaves");
        assert_eq!(cutoff.active_if.as_deref(), Some("fx.filterOn"));
        let kind = filter.controls.iter().find(|c| c.id == "fx.filterType").expect("the type");
        assert_eq!(kind.kind, ControlKind::Choice);
        assert_eq!(kind.options.len(), (kind.max - kind.min) as usize + 1);

        // The newest parameters are on the page with nothing added for them.
        let delay = page.iter().find(|s| s.id == "delayOn").expect("the delay");
        assert!(delay.controls.iter().any(|c| c.id == "fx.delayShimmer"));
        let tanpura = page.iter().find(|s| s.id == "a.tanpura").expect("the tanpura");
        assert_eq!(tanpura.title, "Tanpura");
        assert!(!tanpura.description.is_empty(), "and it says what it is");
        assert!(
            tanpura.article.as_ref().is_some_and(|a| a.contains("wikipedia.org")),
            "and where to read more",
        );
        assert!(filter.article.is_none(), "there is nothing to read about an on-switch");
        assert!(
            tanpura.controls.iter().any(|c| c.short_label == "Jawari"),
            "{:?}",
            tanpura.controls.iter().map(|c| c.short_label.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn an_edit_moves_one_control_and_the_point_comes_back_playable() {
        let edit = PointEdit::new(preset_state_json(0).unwrap()).expect("preset 0");
        let before = edit.value("fx.filterFreq".into());
        edit.set_value("fx.filterFreq".into(), 440.0);
        assert!((edit.value("fx.filterFreq".into()) - 440.0).abs() < 1.0);
        assert_ne!(before, edit.value("fx.filterFreq".into()));

        edit.set_master_gain(0.4);
        assert_eq!(edit.master_gain(), 0.4);

        let json = edit.point_json();
        assert!(crate::SoundPlayer::new(22050, json.clone()).is_ok(), "the sound can play it");
        // And the session commits it as one undoable step.
        let session =
            crate::session::Session::on_preset(0, crate::session::default_session_config()).unwrap();
        session.open_settings(0.0);
        let fx = session.close_settings(0.1, json).expect("the edited point");
        assert!(!fx.is_empty());
        assert!(session.view().can_undo, "one step, and it can be taken back");
    }

    #[test]
    fn a_reset_clears_one_tab_and_the_point_still_plays() {
        let edit = PointEdit::new(preset_state_json(0).unwrap()).expect("preset 0");
        let page = settings_page();
        let a_sound = page
            .iter()
            .find(|s| s.tab == SettingsTab::Sound && s.toggle.is_some())
            .and_then(|s| s.toggle.clone())
            .expect("a sound section with a switch");
        let a_picture = page
            .iter()
            .find(|s| s.tab == SettingsTab::Picture && s.toggle.is_some())
            .and_then(|s| s.toggle.clone())
            .expect("a picture section with a switch");
        edit.set_value(a_sound.id.clone(), 1.0);
        edit.set_value(a_picture.id.clone(), 1.0);
        edit.set_master_gain(0.42);

        edit.reset(SettingsTab::Picture);
        assert_eq!(edit.value(a_sound.id.clone()), 1.0, "the sound is left alone");
        assert_eq!(edit.value(a_picture.id.clone()), 0.0, "the picture starts again");
        assert_eq!(edit.master_gain(), 0.42, "the volume is the sound's");

        edit.reset(SettingsTab::Sound);
        assert_eq!(edit.value(a_sound.id.clone()), 0.0);
        assert_ne!(edit.master_gain(), 0.42);
        assert!(
            crate::SoundPlayer::new(22050, edit.point_json()).is_ok(),
            "a point with nothing switched on is still a point the sound can take",
        );
    }

    #[test]
    fn a_switch_greys_out_what_it_gates() {
        let edit = PointEdit::new(preset_state_json(0).unwrap()).unwrap();
        edit.set_value("fx.filterOn".into(), 1.0);
        assert!(edit.is_active("fx.filterFreq".into()));
        edit.set_value("fx.filterOn".into(), 0.0);
        assert!(!edit.is_active("fx.filterFreq".into()));
        assert!(matches!(PointEdit::new("{}".into()), Err(CoreError::InvalidPoint { .. })));
    }
}
