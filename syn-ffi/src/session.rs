//! The control logic through the FFI: `syn-session` as an object an app can
//! hold, and its effects as something Kotlin and Swift can switch on.
//!
//! Thin, like the rest of this crate. It converts types, and it does the one
//! thing a pure session cannot: it keeps the scout job the session hands out
//! until a thread comes for it ([`Session::run_scout`]), and runs it without
//! holding the session, so a render of seconds never makes a press wait.

use std::sync::{Arc, Mutex, MutexGuard};

use syn_session::{Effect, ScoutPool, ScoutRequest, SessionOptions};

use crate::{parse_point, CoreError};

/// How a session is set up. Take [`default_session_config`] and change what
/// the user has settled — never re-type a default (synesthesia-android
/// PLAN.md: the ranges and defaults are the core's).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct SessionConfig {
    /// Render and score candidates in the background while the user listens.
    pub scout: bool,
    /// Candidates per direction.
    pub scout_candidates: u32,
    /// Seconds of audio rendered per candidate, and at what rate: what the
    /// search costs in battery.
    pub scout_seconds: f64,
    pub scout_sample_rate: f64,
    /// Seconds between a settled point and the first render.
    pub scout_settle: f64,
    /// Threads to render on; 0 means every core but two, so the sound keeps one.
    pub scout_threads: u32,
    pub morph_seconds: f64,
    pub undo_morph_seconds: f64,
    /// How often a morph re-sends the point to the sound.
    pub push_interval: f64,
    /// The session's randomness. The same seed replays the same session.
    pub seed: u32,
}

/// Every value at what the core holds for it.
#[uniffi::export]
pub fn default_session_config() -> SessionConfig {
    let o = SessionOptions::default();
    SessionConfig {
        scout: o.scout.enabled,
        scout_candidates: o.scout.candidates as u32,
        scout_seconds: o.scout.seconds,
        scout_sample_rate: o.scout.sample_rate,
        scout_settle: o.scout.settle,
        scout_threads: 0,
        morph_seconds: o.morph_seconds,
        undo_morph_seconds: o.undo_morph_seconds,
        push_interval: o.push_interval,
        seed: o.seed,
    }
}

impl From<SessionConfig> for SessionOptions {
    fn from(c: SessionConfig) -> Self {
        let d = SessionOptions::default();
        SessionOptions {
            morph_seconds: c.morph_seconds,
            undo_morph_seconds: c.undo_morph_seconds,
            push_interval: c.push_interval,
            scout: syn_session::ScoutConfig {
                enabled: c.scout,
                candidates: c.scout_candidates as usize,
                seconds: c.scout_seconds,
                sample_rate: c.scout_sample_rate,
                settle: c.scout_settle,
            },
            explorer: d.explorer,
            seed: c.seed,
        }
    }
}

/// What the app must do after a session call, in the order it came.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SessionEffect {
    /// Glide the live sound to this point (`SoundPlayer.set_point`).
    SetPoint { point_json: String },
    /// Switch the sound to it hard, dropping the old point's tails
    /// (`SoundPlayer.switch_to`).
    SwitchTo { point_json: String },
    /// The picture starts over from a fresh seed.
    Reseed,
    /// Keep this point as the one to come back to.
    SaveLastPoint { point_json: String },
    /// A scout job is waiting: call [`Session::run_scout`] on a background
    /// thread, and apply the effects it returns on the UI thread.
    StartScout,
    /// A line for the user: what the last press did, or what the scout found.
    /// Two lines, in fact — what happened, and what changed.
    Status { text: String },
}

/// What a screen shows about the session, as one snapshot.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SessionView {
    /// The point's name with its step count — what a title shows.
    pub name: String,
    /// The name without the steps.
    pub point_name: String,
    pub steps: u32,
    pub status: String,
    pub can_undo: bool,
    pub undo_depth: u32,
    /// How far the search is stepping now.
    pub sigma: f64,
    /// A change is still arriving.
    pub morphing: bool,
    pub playing: bool,
    /// Candidates scored and waiting for a press, per direction.
    pub scouted_like: u32,
    pub scouted_dislike: u32,
    pub scout_busy: bool,
}

impl From<syn_session::View> for SessionView {
    fn from(v: syn_session::View) -> Self {
        SessionView {
            name: v.name,
            point_name: v.point_name,
            steps: v.steps,
            status: v.status,
            can_undo: v.can_undo,
            undo_depth: v.undo_depth,
            sigma: v.sigma,
            morphing: v.morphing,
            playing: v.playing,
            scouted_like: v.scouted_like,
            scouted_dislike: v.scouted_dislike,
            scout_busy: v.scout_busy,
        }
    }
}

/// One listening session: 👍 👎 🎲 ↩, the morph, the scout, the status line.
///
/// Every call takes `now` — monotonic seconds from the app's own clock — and
/// returns what the app must do. The UI thread makes the calls; one
/// background thread may be in [`Session::run_scout`] at the same time.
#[derive(uniffi::Object)]
pub struct Session {
    inner: Mutex<syn_session::Session>,
    /// The job the last `StartScout` announced, waiting for a thread.
    pending: Mutex<Option<ScoutRequest>>,
    pool: ScoutPool,
}

#[uniffi::export]
impl Session {
    /// A session on a point, named as the app knows it (empty: the point's
    /// own `presetName`, if it has one).
    #[uniffi::constructor]
    pub fn new(name: String, point_json: String, config: SessionConfig) -> Result<Arc<Self>, CoreError> {
        let point = parse_point(&point_json)?;
        Ok(Session::around(syn_session::Session::new(&name, &point, config.into()), config))
    }

    /// A session on a point that was left mid-search and is being opened
    /// again: `name` says where the point came from and the point stays
    /// nobody's, as it was.
    #[uniffi::constructor]
    pub fn restored(name: String, point_json: String, config: SessionConfig) -> Result<Arc<Self>, CoreError> {
        let point = parse_point(&point_json)?;
        Ok(Session::around(syn_session::Session::restored(&name, &point, config.into()), config))
    }

    /// A session on a built-in point, under the name the core holds for it.
    #[uniffi::constructor]
    pub fn on_preset(index: u32, config: SessionConfig) -> Result<Arc<Self>, CoreError> {
        let preset =
            syn_core::state::presets().get(index as usize).ok_or(CoreError::NoSuchPreset { index })?;
        Ok(Session::around(syn_session::Session::new(&preset.name, &preset.state, config.into()), config))
    }

    /// Moves the session on to `now`: a morph a step further, a scout started
    /// once the sound has settled. Call it while [`Session::wants_tick`].
    pub fn tick(&self, now: f64) -> Vec<SessionEffect> {
        let fx = self.locked().tick(now);
        self.convert(fx)
    }

    /// 👍 more of this.
    pub fn like(&self, now: f64) -> Vec<SessionEffect> {
        let fx = self.locked().like(now);
        self.convert(fx)
    }

    /// 👎 not this.
    pub fn dislike(&self, now: f64) -> Vec<SessionEffect> {
        let fx = self.locked().dislike(now);
        self.convert(fx)
    }

    /// 🎲 somewhere else entirely, near a built-in point.
    pub fn surprise(&self, now: f64) -> Vec<SessionEffect> {
        let fx = self.locked().surprise(now);
        self.convert(fx)
    }

    /// ↩ back one step.
    pub fn undo(&self, now: f64) -> Vec<SessionEffect> {
        let fx = self.locked().undo(now);
        self.convert(fx)
    }

    /// A whole point arrives: a fresh search, a hard switch, a new picture.
    pub fn load(&self, now: f64, name: String, point_json: String) -> Result<Vec<SessionEffect>, CoreError> {
        let point = parse_point(&point_json)?;
        let fx = self.locked().load(now, &name, &point);
        Ok(self.convert(fx))
    }

    /// The built-in point at `index`, under the name the core holds for it.
    pub fn load_preset(&self, now: f64, index: u32) -> Result<Vec<SessionEffect>, CoreError> {
        let fx = self.locked().load_preset(now, index as usize).ok_or(CoreError::NoSuchPreset { index })?;
        Ok(self.convert(fx))
    }

    /// The sound started or stopped. The session only morphs what can be
    /// heard, and only scouts for a point someone is listening to.
    pub fn set_playing(&self, now: f64, playing: bool) -> Vec<SessionEffect> {
        let fx = self.locked().set_playing(now, playing);
        self.convert(fx)
    }

    /// 💾 The point was kept under this name: the title says so and the point
    /// carries it from now on, while the search is untouched — ↩ still walks
    /// back through the steps that led here.
    pub fn kept_as(&self, name: String) -> Vec<SessionEffect> {
        let fx = self.locked().kept_as(&name);
        self.convert(fx)
    }

    /// The user's volume — not a gene, so no press changes it.
    pub fn set_master_gain(&self, now: f64, gain: f64) -> Vec<SessionEffect> {
        let fx = self.locked().set_master_gain(now, gain);
        self.convert(fx)
    }

    /// ⚙ Settings opens: a morph in flight lands, and the scout stops.
    pub fn open_settings(&self, now: f64) -> Vec<SessionEffect> {
        let fx = self.locked().open_settings(now);
        self.convert(fx)
    }

    /// ⚙ Settings closes on this point: one undoable step and a jump, or
    /// nothing at all if it was left as it was.
    pub fn close_settings(&self, now: f64, point_json: String) -> Result<Vec<SessionEffect>, CoreError> {
        let point = parse_point(&point_json)?;
        let fx = self.locked().close_settings(now, &point);
        Ok(self.convert(fx))
    }

    /// Runs the job the last [`SessionEffect::StartScout`] announced: it
    /// renders and scores several points, which takes seconds, so call it
    /// from a background thread. The effects it returns are the UI thread's,
    /// like every other. Empty when no job was waiting.
    pub fn run_scout(&self) -> Vec<SessionEffect> {
        let Some(request) = self.take_pending() else { return Vec::new() };
        // The session is not held while this renders: the user goes on
        // pressing, and a result for a point they have left is dropped.
        let result = self.pool.run(&request);
        let fx = self.locked().scout_finished(result);
        self.convert(fx)
    }

    /// True while something is due on the clock: a morph in flight, or a
    /// scout waiting for the sound to settle. An app that stops ticking in
    /// between saves a wake-up per frame with the screen off.
    pub fn wants_tick(&self) -> bool {
        self.locked().wants_tick()
    }

    pub fn view(&self) -> SessionView {
        self.locked().view().into()
    }

    /// The point the search is at — what a save or a token uses. Mid-morph
    /// this is where the change is heading, not what is audible.
    pub fn point_json(&self) -> String {
        point_json(&self.locked().point())
    }

    /// The point that is audible now. A sound that is just starting plays this.
    pub fn live_point_json(&self) -> String {
        point_json(&self.locked().live_point())
    }

    /// Threads the scout renders on.
    pub fn scout_threads(&self) -> u32 {
        self.pool.threads() as u32
    }
}

impl Session {
    fn around(session: syn_session::Session, config: SessionConfig) -> Arc<Self> {
        Arc::new(Session {
            inner: Mutex::new(session),
            pending: Mutex::new(None),
            pool: ScoutPool::new(config.scout_threads as usize),
        })
    }

    /// A session is never left half-changed by a panic, so a poisoned lock is
    /// still the session: take it rather than refuse every later call.
    fn locked(&self) -> MutexGuard<'_, syn_session::Session> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn take_pending(&self) -> Option<ScoutRequest> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    /// The session's effects as the app's, keeping the scout job on this side
    /// of the FFI: a job is genomes and render settings, nothing a UI wants.
    fn convert(&self, fx: Vec<Effect>) -> Vec<SessionEffect> {
        fx.into_iter()
            .map(|e| match e {
                Effect::SetPoint(s) => SessionEffect::SetPoint { point_json: point_json(&s) },
                Effect::SwitchTo(s) => SessionEffect::SwitchTo { point_json: point_json(&s) },
                Effect::Reseed => SessionEffect::Reseed,
                Effect::SaveLastPoint(s) => SessionEffect::SaveLastPoint { point_json: point_json(&s) },
                Effect::StartScout(request) => {
                    *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(*request);
                    SessionEffect::StartScout
                }
                Effect::Status(text) => SessionEffect::Status { text },
            })
            .collect()
    }
}

/// A point as the web app's JSON. `AppState` always serializes; an empty
/// string could only come of a bug, and reaches the app as an invalid point
/// rather than as a panic across the FFI.
fn point_json(state: &syn_core::AppState) -> String {
    serde_json::to_string(state).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> SessionConfig {
        SessionConfig { scout: false, ..default_session_config() }
    }

    fn effects(fx: &[SessionEffect]) -> Vec<&str> {
        fx.iter()
            .map(|e| match e {
                SessionEffect::SetPoint { .. } => "set",
                SessionEffect::SwitchTo { .. } => "switch",
                SessionEffect::Reseed => "reseed",
                SessionEffect::SaveLastPoint { .. } => "save",
                SessionEffect::StartScout => "scout",
                SessionEffect::Status { .. } => "status",
            })
            .collect()
    }

    #[test]
    fn a_session_starts_on_a_built_in_point_and_presses_morph_it() {
        let s = Session::on_preset(0, config()).expect("preset 0");
        assert_eq!(s.view().name, "Fractal garden");
        s.set_playing(0.0, true);
        assert_eq!(effects(&s.like(0.0)), ["status"]);
        assert!(s.view().morphing);
        assert!(s.wants_tick());

        let mid = s.tick(1.0);
        assert_eq!(effects(&mid), ["set"]);
        let end = s.tick(2.0);
        assert_eq!(effects(&end), ["set", "save"]);
        assert!(!s.wants_tick());
        // The point that comes out is the point that goes into the player.
        let json = s.point_json();
        assert!(crate::SoundPlayer::new(22050, json).is_ok());
    }

    #[test]
    fn a_load_switches_hard_and_reseeds_and_an_unknown_preset_is_an_error() {
        let s = Session::on_preset(0, config()).expect("preset 0");
        let fx = s.load_preset(0.0, 5).expect("preset 5");
        assert_eq!(effects(&fx), ["switch", "reseed", "status", "save"]);
        let past_the_end = crate::presets().len() as u32;
        assert!(matches!(s.load_preset(0.0, past_the_end), Err(CoreError::NoSuchPreset { .. })));
        assert!(matches!(Session::on_preset(99, config()), Err(CoreError::NoSuchPreset { .. })));
        assert!(matches!(s.load(0.0, "x".into(), "{}".into()), Err(CoreError::InvalidPoint { .. })));
    }

    #[test]
    fn a_scout_job_is_run_on_a_thread_and_its_result_comes_back_as_a_line() {
        let config = SessionConfig {
            scout_candidates: 1,
            scout_seconds: 1.0,
            scout_threads: 2,
            ..default_session_config()
        };
        let s = Session::on_preset(0, config).expect("preset 0");
        assert_eq!(s.scout_threads(), 2);
        s.set_playing(0.0, true);
        assert_eq!(effects(&s.tick(config.scout_settle + 0.01)), ["scout"]);
        assert!(s.view().scout_busy);
        assert!(s.run_scout().iter().any(|e| matches!(e, SessionEffect::Status { .. })));
        assert_eq!(s.view().scouted_like, 1);
        assert!(s.run_scout().is_empty(), "no job is waiting now");
    }

    #[test]
    fn a_restored_point_shows_where_it_came_from() {
        let session = Session::on_preset(0, config()).expect("preset 0");
        session.like(0.0);
        session.tick(2.0);
        let left_on = session.point_json();
        let shown = session.view().point_name;

        let back = Session::restored(shown.clone(), left_on, config()).expect("the point");
        assert_eq!(back.view().name, shown);
        assert!(!back.point_json().contains("presetName"), "it claims no name of its own");
    }

    #[test]
    fn a_kept_point_is_named_and_kept() {
        let s = Session::on_preset(0, config()).expect("preset 0");
        s.like(0.0);
        s.tick(2.0);
        let fx = s.kept_as("Dawn".into());
        assert_eq!(effects(&fx), ["status", "save"]);
        assert_eq!(s.view().name, "Dawn");
        assert_eq!(s.view().steps, 0);
        assert!(s.point_json().contains("\"presetName\":\"Dawn\""));
        assert!(s.view().can_undo);
    }

    #[test]
    fn the_defaults_are_the_cores() {
        let c = default_session_config();
        let o = SessionOptions::from(c);
        assert_eq!(o, SessionOptions::default());
        assert_eq!(c.morph_seconds, syn_session::MORPH_SECONDS);
    }
}
