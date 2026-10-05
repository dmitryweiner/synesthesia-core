//! The control logic of one listening session — the layer the web app
//! (`main.ts`) and the console (`syn-app/main.rs`) each wrote by hand, moved
//! here so the next app is a shell around it (synesthesia-android PLAN.md,
//! decision 2).
//!
//! It is what happens between a press and the sound: the explorer's 👍 👎 🎲 ↩,
//! the 2 s morph from what is audible to where the press leads, when the
//! scout may render candidates, which candidate a press takes, the point's
//! name and step count, and the line of text that explains the last change.
//!
//! Pure, like the rest of the model: no threads, no clock, no files, no
//! device. Time comes in as `now` — monotonic seconds, from whatever clock the
//! app has — and everything that has to touch the world goes out as an
//! [`Effect`] for the app to carry out:
//!
//! ```text
//!   like(now) ─┐                        ┌─► SetPoint      the live sound glides
//!   tick(now) ─┼─► Session ─ effects ─► ├─► SwitchTo      … or switches hard
//!   load(…)   ─┘      │                 ├─► Reseed         the picture starts over
//!                     │                 ├─► SaveLastPoint  storage
//!                     │                 ├─► StartScout     a thread, off the UI
//!                     │                 └─► Status         a line for the user
//!                     └─ view() ──────────► name, step, spread, undo depth
//! ```
//!
//! The behaviour is `main.ts`'s, down to the feel: a press mid-morph starts
//! from what is audible, not from where the last one was heading; a load is a
//! hard switch and a fresh picture; closing Settings is one undoable step and
//! a jump, because the sound is already there.

pub mod points;

use std::sync::Arc;

use syn_core::dsp::rng::{Mulberry32, Rng};
use syn_core::genome::evolve::{diff_summary, lerp_genome, same_genome, ChangeDir};
use syn_core::genome::explorer::{Explorer, ExplorerAction, ExplorerOptions};
use syn_core::genome::scout::{self, ScoutKind, ScoutResult, ScoutSettings};
use syn_core::genome::{decode_genome, encode_genome, Genome};
use syn_core::state::{presets, AppState, Preset};

/// How long a change takes to arrive, seconds — the web app's feel.
pub const MORPH_SECONDS: f64 = 2.0;
/// An undo is quicker: it goes back to something that was just heard.
pub const UNDO_MORPH_SECONDS: f64 = 0.8;
/// How often a morph re-sends the point to the sound (the web app's
/// `AUDIO_PUSH_INTERVAL`): 20 times a second is a glide to an ear.
pub const PUSH_INTERVAL: f64 = 0.05;
/// After a morph settles, before the scout starts rendering: the user may
/// press again straight away, and the press must not wait for a render.
pub const SCOUT_SETTLE: f64 = 0.8;
/// Changes named in a status line before it says "+N more".
const CHANGES_SHOWN: usize = 5;

/// What the scout costs, and whether it runs at all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScoutConfig {
    pub enabled: bool,
    /// Candidates per direction.
    pub candidates: usize,
    pub seconds: f64,
    pub sample_rate: f64,
    /// Seconds between a settled point and the first render.
    pub settle: f64,
}

impl Default for ScoutConfig {
    /// The web app's surrogate, which is what a phone can afford: 24 s at
    /// 8 kHz ranks candidates close to a full 30 s / 22 kHz render
    /// (Spearman ρ 0.73, against 0.23 for 8 s at 16 kHz) because the slow
    /// LFOs need the long window and the fractal metrics do not need the high
    /// frequencies. The console renders at full quality instead, on a machine
    /// that is plugged in; the length and the rate are settings either way
    /// (synesthesia-android PLAN.md, open question 1).
    fn default() -> Self {
        Self { enabled: true, candidates: 3, seconds: 24.0, sample_rate: 8000.0, settle: SCOUT_SETTLE }
    }
}

/// Everything a session is set up with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SessionOptions {
    pub morph_seconds: f64,
    pub undo_morph_seconds: f64,
    pub push_interval: f64,
    pub scout: ScoutConfig,
    pub explorer: ExplorerOptions,
    /// The session's own randomness: proposals and 🎲 come from here, so a
    /// seed replays a whole session in a test.
    pub seed: u32,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            morph_seconds: MORPH_SECONDS,
            undo_morph_seconds: UNDO_MORPH_SECONDS,
            push_interval: PUSH_INTERVAL,
            scout: ScoutConfig::default(),
            explorer: ExplorerOptions::default(),
            seed: 1,
        }
    }
}

/// What the app must do. Everything the session cannot do itself, because it
/// has no device, no screen, no files and no threads.
#[derive(Clone, Debug)]
pub enum Effect {
    /// Glide the live sound to this point: parameters move, nothing is
    /// rebuilt. A morph sends these as it goes.
    SetPoint(Box<AppState>),
    /// Switch the sound to this point hard — the previous point's reverb and
    /// delay tails go with it.
    SwitchTo(Box<AppState>),
    /// The picture starts over from a fresh seed.
    Reseed,
    /// Keep this point as the one to come back to.
    SaveLastPoint(Box<AppState>),
    /// Render and score candidates — seconds of work, so not on the thread
    /// that draws. Run it with [`ScoutPool::run`] (or [`run_scout`]) and hand
    /// the result to [`Session::scout_finished`].
    StartScout(Box<ScoutRequest>),
    /// A line for the user: what the last press did, or what the scout found.
    Status(String),
}

/// A scout job, ready to run: the point being listened to and the candidates
/// proposed for either direction.
#[derive(Clone, Debug)]
pub struct ScoutRequest {
    /// The explorer version these candidates belong to. Anything that commits
    /// a change makes them stale, and the session drops them.
    pub version: u64,
    pub parent: Genome,
    pub likes: Vec<Genome>,
    pub dislikes: Vec<Genome>,
    pub settings: ScoutSettings,
}

/// Renders and scores a request on whatever threads the caller has installed.
/// Blocks for seconds: never call it from a thread that has to answer a user.
pub fn run_scout(req: &ScoutRequest) -> ScoutResult {
    scout::run(req.version, &req.parent, &req.likes, &req.dislikes, req.settings)
}

/// The threads the scout renders on: fewer than the machine has, so the sound
/// always keeps a core to play on (synesthesia-android PLAN.md, decision 6 —
/// the console's fix for its xruns).
pub struct ScoutPool {
    pool: Option<Arc<rayon::ThreadPool>>,
    threads: usize,
}

impl ScoutPool {
    /// A pool of `threads` threads; 0 means every core but two, at least one.
    /// A machine that refuses one falls back to rayon's own pool.
    pub fn new(threads: usize) -> Self {
        let cores = std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get);
        let n = if threads == 0 { cores.saturating_sub(2).max(1) } else { threads };
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .thread_name(|i| format!("syn-scout-{i}"))
            .build()
            .ok()
            .map(Arc::new);
        ScoutPool { pool, threads: n }
    }

    pub fn threads(&self) -> usize {
        self.threads
    }

    /// Runs a job on the pool, blocking this thread until it is scored.
    pub fn run(&self, req: &ScoutRequest) -> ScoutResult {
        match &self.pool {
            // Inside the pool, the scout's own par_iter and join stay on its
            // threads rather than borrowing the caller's.
            Some(pool) => pool.install(|| run_scout(req)),
            None => run_scout(req),
        }
    }
}

/// What a screen shows about the session, as one snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    /// The point's name with its step count — what a title shows.
    pub name: String,
    /// The name without the steps.
    pub point_name: String,
    /// Presses since this point was loaded.
    pub steps: u32,
    /// The last thing that happened, in words. Two lines: what, and what changed.
    pub status: String,
    pub can_undo: bool,
    pub undo_depth: u32,
    /// How far the search is currently stepping.
    pub sigma: f64,
    /// A change is still arriving.
    pub morphing: bool,
    pub playing: bool,
    /// Candidates scored and waiting for a press, per direction.
    pub scouted_like: u32,
    pub scouted_dislike: u32,
    /// A scout job is out.
    pub scout_busy: bool,
}

/// One listening session: the point, the search, the morph, the scout.
pub struct Session {
    explorer: Explorer,
    rng: Mulberry32,
    /// The point's name, without the step count.
    base_name: String,
    /// True while the point is still the one that was loaded under that name;
    /// a press makes it the user's own, and the name only says where it came
    /// from (the web app clears `presetName` on every step).
    named: bool,
    steps: u32,
    /// Not a gene, and not the search's business: the user's volume.
    master_gain: f64,
    playing: bool,
    settings_open: bool,
    /// The point that is audible: `morph_from` eased towards `morph_to`.
    live: Genome,
    morph_from: Genome,
    morph_to: Genome,
    morph_start: f64,
    morph_seconds: f64,
    morph_done: bool,
    last_push: f64,
    /// When the scout may start, once the sound has settled.
    scout_at: Option<f64>,
    scout_busy: bool,
    scout_result: Option<ScoutResult>,
    status: String,
    opts: SessionOptions,
}

impl Session {
    /// A session on `point`, named `name` (a preset's name, a saved point's,
    /// or empty for an unnamed one). Nothing is playing yet: the app starts
    /// its sound on [`Session::live_point`] and says so with
    /// [`Session::set_playing`].
    pub fn new(name: &str, point: &AppState, opts: SessionOptions) -> Self {
        let g = encode_genome(point);
        let base_name =
            if name.is_empty() { point.preset_name.clone().unwrap_or_default() } else { name.to_string() };
        Session {
            explorer: Explorer::new(g.clone(), opts.explorer),
            rng: Mulberry32::new(opts.seed),
            named: !base_name.is_empty(),
            base_name,
            steps: 0,
            master_gain: point.audio.master_gain,
            playing: false,
            settings_open: false,
            live: g.clone(),
            morph_from: g.clone(),
            morph_to: g,
            morph_start: 0.0,
            morph_seconds: opts.morph_seconds,
            morph_done: true,
            last_push: f64::NEG_INFINITY,
            scout_at: None,
            scout_busy: false,
            scout_result: None,
            status: String::new(),
            opts,
        }
    }

    /// A session on a point that was left mid-search — the app was closed and
    /// opened again. The name says where the point came from ("Fractal
    /// garden"), and the point is nobody's, exactly as it was: a step away
    /// from that preset and not it.
    pub fn restored(name: &str, point: &AppState, opts: SessionOptions) -> Self {
        let mut session = Self::new(name, point, opts);
        session.named = false;
        session
    }

    // --- what the app reads ------------------------------------------------

    /// The point the search is at — what a save, a token or a scout uses.
    /// Mid-morph this is where the change is heading, not what is audible.
    pub fn point(&self) -> AppState {
        self.state_of(&self.explorer.current)
    }

    /// The point that is audible now (mid-morph: the interpolated one). An
    /// app hands this to a sound that is just starting.
    pub fn live_point(&self) -> AppState {
        self.state_of(&self.live)
    }

    pub fn name(&self) -> String {
        match self.steps {
            0 => self.base_name.clone(),
            1 => format!("{} · 1 step", self.base_name),
            n => format!("{} · {n} steps", self.base_name),
        }
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn master_gain(&self) -> f64 {
        self.master_gain
    }

    /// True while something is due on the clock: a morph in flight, or a
    /// scout waiting for the sound to settle. An app that stops ticking in
    /// between saves itself a wake-up per frame with the screen off.
    pub fn wants_tick(&self) -> bool {
        !self.morph_done || self.scout_at.is_some()
    }

    pub fn view(&self) -> View {
        let ready = |kind| self.ready_for(kind) as u32;
        View {
            name: self.name(),
            point_name: self.base_name.clone(),
            steps: self.steps,
            status: self.status.clone(),
            can_undo: self.explorer.can_undo(),
            undo_depth: self.explorer.undo_depth() as u32,
            sigma: self.explorer.sigma,
            morphing: !self.morph_done,
            playing: self.playing,
            scouted_like: ready(ScoutKind::Like),
            scouted_dislike: ready(ScoutKind::Dislike),
            scout_busy: self.scout_busy,
        }
    }

    // --- the clock ---------------------------------------------------------

    /// Moves the session on to `now`: a morph a step further, a scout started
    /// when the sound has settled.
    pub fn tick(&mut self, now: f64) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.tick_morph(now, &mut fx);
        self.tick_scout(now, &mut fx);
        fx
    }

    /// The sound started or stopped. The session only morphs what can be
    /// heard, and only scouts for a point someone is listening to.
    pub fn set_playing(&mut self, now: f64, playing: bool) -> Vec<Effect> {
        if self.playing == playing {
            return Vec::new();
        }
        self.playing = playing;
        if playing {
            // The sound starts on `live_point`; a morph in flight takes over
            // from the next push.
            self.last_push = now;
            self.schedule_scout(now);
        } else {
            self.stop_scout();
        }
        Vec::new()
    }

    // --- the presses -------------------------------------------------------

    /// 👍 "more of this": the point becomes the anchor and the search carries
    /// on along the step that led here, with a smaller spread.
    pub fn like(&mut self, now: f64) -> Vec<Effect> {
        self.press(now, ScoutKind::Like)
    }

    /// 👎 "not this": back to the anchor, stepping elsewhere, with a larger
    /// spread and the rejected dimensions left alone.
    pub fn dislike(&mut self, now: f64) -> Vec<Effect> {
        self.press(now, ScoutKind::Dislike)
    }

    fn press(&mut self, now: f64, kind: ScoutKind) -> Vec<Effect> {
        let mut fx = Vec::new();
        let prev = self.explorer.current.clone();
        // Taken before the press: a committed change makes the candidates stale.
        let picked = self.picked(kind);
        let proposal = picked.as_ref().map(|p| p.genome.clone());
        match kind {
            ScoutKind::Like => self.explorer.like(proposal, &mut self.rng),
            ScoutKind::Dislike => self.explorer.dislike(proposal, &mut self.rng),
        };
        self.steps += 1;
        self.after_action(now, &prev, self.opts.morph_seconds, picked, &mut fx);
        fx
    }

    /// 🎲 "somewhere else entirely": near a built-in point, and the search
    /// starts over from there.
    pub fn surprise(&mut self, now: f64) -> Vec<Effect> {
        let mut fx = Vec::new();
        let prev = self.explorer.current.clone();
        // Any built-in point but the one we are sitting on (the web app's rule).
        let here = self.named.then_some(self.base_name.as_str());
        let pool: Vec<&Preset> = presets().iter().filter(|p| Some(p.name.as_str()) != here).collect();
        let i = ((self.rng.next() * pool.len() as f64) as usize).min(pool.len() - 1);
        let preset = pool[i];
        let target = encode_genome(&preset.state);
        let name = preset.name.clone();
        self.explorer.surprise(&target, &mut self.rng);
        // A fresh start, as a load is — but heard as a morph, not a switch.
        self.base_name = format!("near {name}");
        self.steps = 0;
        self.after_action(now, &prev, self.opts.morph_seconds, None, &mut fx);
        fx.push(Effect::Reseed);
        fx
    }

    /// ↩ back one step, quicker than a change arrives (it goes back to
    /// something that was just heard).
    pub fn undo(&mut self, now: f64) -> Vec<Effect> {
        let mut fx = Vec::new();
        let prev = self.explorer.current.clone();
        if self.explorer.undo().is_none() {
            self.say(&mut fx, "nothing to undo".to_string());
            return fx;
        }
        self.steps = self.steps.saturating_sub(1);
        self.after_action(now, &prev, self.opts.undo_morph_seconds, None, &mut fx);
        fx
    }

    /// A whole point arrives — a built-in, a saved one, a link: a fresh
    /// search with no history, a hard switch (no tails of the old point) and
    /// a new picture.
    pub fn load(&mut self, now: f64, name: &str, point: &AppState) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.stop_scout();
        self.base_name =
            if name.is_empty() { point.preset_name.clone().unwrap_or_default() } else { name.to_string() };
        self.named = !self.base_name.is_empty();
        self.steps = 0;
        self.master_gain = point.audio.master_gain;
        let g = encode_genome(point);
        self.explorer.load(g.clone());
        self.live = g.clone();
        self.morph_from = g.clone();
        self.morph_to = g;
        self.morph_done = true;
        self.last_push = now;
        fx.push(Effect::SwitchTo(Box::new(self.live_point())));
        fx.push(Effect::Reseed);
        let what = if self.base_name.is_empty() { "unnamed point" } else { &self.base_name };
        let msg = format!("loaded: {what}");
        self.say(&mut fx, msg);
        self.on_settled(now, &mut fx);
        fx
    }

    /// The built-in point at `index`, under the name the core holds for it.
    pub fn load_preset(&mut self, now: f64, index: usize) -> Option<Vec<Effect>> {
        let preset = presets().get(index)?;
        let (name, state) = (preset.name.clone(), preset.state.clone());
        Some(self.load(now, &name, &state))
    }

    // --- Settings ----------------------------------------------------------

    /// ⚙ Settings opens: a morph in flight lands now, because the page edits
    /// the point it was heading to, and the scout stops — its candidates are
    /// about a point being edited away.
    pub fn open_settings(&mut self, now: f64) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.settings_open = true;
        self.stop_scout();
        if !self.morph_done {
            self.live = self.morph_to.clone();
            self.morph_from = self.morph_to.clone();
            self.morph_done = true;
            self.last_push = now;
            fx.push(Effect::SetPoint(Box::new(self.live_point())));
        }
        fx
    }

    /// ⚙ Settings closes on `point`. A change is one undoable step and a
    /// jump, not a morph: the sound was edited as it played, so it is already
    /// there. No change at all only settles what the opening landed.
    pub fn close_settings(&mut self, now: f64, point: &AppState) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.settings_open = false;
        self.master_gain = point.audio.master_gain;
        let prev = self.explorer.current.clone();
        let g = encode_genome(point);
        // Not an exact comparison: the point went out through the codec and
        // comes back through it, which moves the last bits (`same_genome`).
        if same_genome(&g, &prev) {
            self.on_settled(now, &mut fx);
            return fx;
        }
        self.explorer.edit(g);
        self.steps += 1;
        self.named = false;
        self.live = self.explorer.current.clone();
        self.morph_from = self.live.clone();
        self.morph_to = self.live.clone();
        self.morph_done = true;
        self.last_push = now;
        self.stop_scout();
        fx.push(Effect::SetPoint(Box::new(self.live_point())));
        let msg = format!(
            "{} · step {}\n{}",
            action_label(ExplorerAction::Edit),
            self.steps,
            describe_change(&prev, &self.explorer.current)
        );
        self.say(&mut fx, msg);
        self.on_settled(now, &mut fx);
        fx
    }

    /// The point was kept under `name` (💾): it is the user's own named point
    /// from now on, so the title says that and not "the preset, five steps
    /// ago". The search is untouched — undo still walks back through it.
    pub fn kept_as(&mut self, name: &str) -> Vec<Effect> {
        let mut fx = Vec::new();
        if name.is_empty() {
            return fx;
        }
        self.base_name = name.to_string();
        self.named = true;
        self.steps = 0;
        let msg = format!("kept as “{name}”");
        self.say(&mut fx, msg);
        fx.push(Effect::SaveLastPoint(Box::new(self.point())));
        fx
    }

    /// The user's volume: not a gene, so it survives every press, and the
    /// scout judges candidates at the level they would play at.
    pub fn set_master_gain(&mut self, now: f64, gain: f64) -> Vec<Effect> {
        let mut fx = Vec::new();
        if (gain - self.master_gain).abs() < f64::EPSILON {
            return fx;
        }
        self.master_gain = gain;
        if self.playing {
            self.last_push = now;
            fx.push(Effect::SetPoint(Box::new(self.live_point())));
        }
        fx
    }

    // --- the scout ---------------------------------------------------------

    /// A job came back. A result for a point the user has moved on from is
    /// dropped: nobody is listening to it any more.
    pub fn scout_finished(&mut self, result: ScoutResult) -> Vec<Effect> {
        let mut fx = Vec::new();
        self.scout_busy = false;
        if result.version != self.explorer.version {
            return fx;
        }
        let msg = format!(
            "scouted {} + {} candidates in {:.1} s",
            result.ready(ScoutKind::Like),
            result.ready(ScoutKind::Dislike),
            result.seconds
        );
        self.scout_result = Some(result);
        self.say(&mut fx, msg);
        fx
    }

    fn schedule_scout(&mut self, now: f64) {
        let c = self.opts.scout;
        if !c.enabled || c.candidates == 0 || !self.playing || self.settings_open {
            return;
        }
        self.scout_at = Some(now + c.settle);
    }

    /// Drops what was prepared: the point moved, so it is stale. A job
    /// already out keeps rendering — [`Session::scout_finished`] throws its
    /// result away by the version.
    fn stop_scout(&mut self) {
        self.scout_at = None;
        self.scout_result = None;
    }

    fn tick_scout(&mut self, now: f64, fx: &mut Vec<Effect>) {
        let Some(at) = self.scout_at else { return };
        if now < at || !self.morph_done || !self.playing || self.scout_busy || self.scout_result.is_some() {
            return;
        }
        let c = self.opts.scout;
        let mut likes = Vec::with_capacity(c.candidates);
        let mut dislikes = Vec::with_capacity(c.candidates);
        // Interleaved, so both directions get candidates early if the user is
        // quick and a job is cut short.
        for _ in 0..c.candidates {
            likes.push(self.explorer.propose_like(&mut self.rng));
            dislikes.push(self.explorer.propose_dislike(&mut self.rng));
        }
        self.scout_at = None;
        self.scout_busy = true;
        fx.push(Effect::StartScout(Box::new(ScoutRequest {
            version: self.explorer.version,
            parent: self.explorer.current.clone(),
            likes,
            dislikes,
            settings: ScoutSettings {
                seconds: c.seconds,
                sample_rate: c.sample_rate,
                master_gain: self.master_gain,
                // Its own stream, so a candidate's noise does not depend on
                // how many proposals were drawn before it.
                seed: self.opts.seed ^ 0x5f36_1a2b,
            },
        })));
    }

    /// The candidate the scout prepared for this direction, if it is still
    /// about the point the user is listening to.
    fn picked(&self, kind: ScoutKind) -> Option<Pick> {
        let result = self.scout_result.as_ref().filter(|r| r.version == self.explorer.version)?;
        let best = result.best(kind)?;
        Some(Pick { genome: best.genome.clone(), score: best.analysis.score, of: result.ready(kind) })
    }

    fn ready_for(&self, kind: ScoutKind) -> usize {
        self.scout_result.as_ref().filter(|r| r.version == self.explorer.version).map_or(0, |r| r.ready(kind))
    }

    // --- the morph ---------------------------------------------------------

    fn start_morph(&mut self, now: f64, seconds: f64) {
        // From what is audible, not from where the last press was heading:
        // press twice quickly and the second change starts from the sound.
        self.morph_from = self.live.clone();
        self.morph_to = self.explorer.current.clone();
        self.morph_start = now;
        self.morph_seconds = seconds;
        self.morph_done = false;
    }

    fn tick_morph(&mut self, now: f64, fx: &mut Vec<Effect>) {
        if self.morph_done {
            return;
        }
        let p = ((now - self.morph_start) / self.morph_seconds.max(1e-9)).clamp(0.0, 1.0);
        let done = p >= 1.0;
        self.live = lerp_genome(&self.morph_from, &self.morph_to, ease_in_out(p));
        if self.playing && (done || now - self.last_push >= self.opts.push_interval) {
            self.last_push = now;
            fx.push(Effect::SetPoint(Box::new(self.live_point())));
        }
        if done {
            self.morph_done = true;
            self.morph_from = self.morph_to.clone();
            self.on_settled(now, fx);
        }
    }

    /// The sound has arrived where it was going: worth keeping, and worth
    /// scouting from.
    fn on_settled(&mut self, now: f64, fx: &mut Vec<Effect>) {
        fx.push(Effect::SaveLastPoint(Box::new(self.point())));
        self.schedule_scout(now);
    }

    // --- bookkeeping -------------------------------------------------------

    fn after_action(
        &mut self,
        now: f64,
        prev: &Genome,
        seconds: f64,
        picked: Option<Pick>,
        fx: &mut Vec<Effect>,
    ) {
        self.stop_scout();
        // The point is the user's own now, not the one that was loaded.
        self.named = false;
        self.start_morph(now, seconds);
        let scouted = match picked {
            Some(p) => format!(" · scouted: best of {} (fractality {:.2})", p.of, p.score),
            None => String::new(),
        };
        let msg = format!(
            "{} · step {} · spread {:.2}{scouted}\n{}",
            action_label(self.explorer.last_action),
            self.steps,
            self.explorer.sigma,
            describe_change(prev, &self.explorer.current)
        );
        self.say(fx, msg);
    }

    fn say(&mut self, fx: &mut Vec<Effect>, message: String) {
        self.status = message.clone();
        fx.push(Effect::Status(message));
    }

    fn state_of(&self, g: &Genome) -> AppState {
        let mut s = decode_genome(g);
        s.audio.master_gain = self.master_gain;
        s.preset_name = (self.named && !self.base_name.is_empty()).then(|| self.base_name.clone());
        s
    }
}

struct Pick {
    genome: Genome,
    score: f64,
    of: usize,
}

/// Ease-in-out, so a change reads as a glide and not as a jump (the web
/// app's curve; `t = 1` is exact, so a morph ends on its target).
fn ease_in_out(t: f64) -> f64 {
    if t >= 1.0 {
        1.0
    } else if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
    }
}

/// What a press did, in the words the web app uses.
pub fn action_label(action: ExplorerAction) -> &'static str {
    match action {
        ExplorerAction::Like => "👍 continuing this way",
        ExplorerAction::Dislike => "👎 back to the last liked point, trying elsewhere",
        ExplorerAction::Surprise => "🎲 jumped somewhere new",
        ExplorerAction::Undo => "↩ undone",
        ExplorerAction::Edit => "⚙ set by hand in Settings",
        ExplorerAction::Load => "loaded",
    }
}

/// What changed between two points, named: the first few genes with which way
/// they went, and how many more there were.
pub fn describe_change(a: &Genome, b: &Genome) -> String {
    let changes = diff_summary(a, b);
    if changes.is_empty() {
        return "nothing changed".to_string();
    }
    let top: Vec<String> =
        changes.iter().take(CHANGES_SHOWN).map(|c| format!("{} {}", c.label, arrow(c.dir))).collect();
    let mut out = top.join(" · ");
    if changes.len() > CHANGES_SHOWN {
        out.push_str(&format!(" +{} more", changes.len() - CHANGES_SHOWN));
    }
    out
}

fn arrow(dir: ChangeDir) -> &'static str {
    match dir {
        ChangeDir::Up => "↑",
        ChangeDir::Down => "↓",
        ChangeDir::On => "on",
        ChangeDir::Off => "off",
        ChangeDir::Switch => "⇄",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn_core::genome::genes::genes;
    use syn_core::schema::GeneKind;

    /// Without the scout, so a test spends no time rendering candidates.
    fn opts() -> SessionOptions {
        SessionOptions {
            scout: ScoutConfig { enabled: false, ..ScoutConfig::default() },
            ..SessionOptions::default()
        }
    }

    /// A scout small enough for a test, and otherwise as it ships.
    fn scout_opts() -> SessionOptions {
        SessionOptions {
            scout: ScoutConfig { candidates: 2, seconds: 1.5, ..ScoutConfig::default() },
            ..SessionOptions::default()
        }
    }

    fn session_with(opts: SessionOptions) -> Session {
        let p = &presets()[0];
        Session::new(&p.name, &p.state, opts)
    }

    fn session() -> Session {
        session_with(opts())
    }

    fn pushed(fx: &[Effect]) -> Vec<&AppState> {
        fx.iter()
            .filter_map(|e| match e {
                Effect::SetPoint(s) => Some(&**s),
                _ => None,
            })
            .collect()
    }

    fn switched(fx: &[Effect]) -> Option<&AppState> {
        fx.iter().find_map(|e| match e {
            Effect::SwitchTo(s) => Some(&**s),
            _ => None,
        })
    }

    fn saved(fx: &[Effect]) -> Option<&AppState> {
        fx.iter().find_map(|e| match e {
            Effect::SaveLastPoint(s) => Some(&**s),
            _ => None,
        })
    }

    fn said(fx: &[Effect]) -> Option<&str> {
        fx.iter().find_map(|e| match e {
            Effect::Status(t) => Some(t.as_str()),
            _ => None,
        })
    }

    fn reseeded(fx: &[Effect]) -> bool {
        fx.iter().any(|e| matches!(e, Effect::Reseed))
    }

    fn job(fx: &[Effect]) -> Option<&ScoutRequest> {
        fx.iter().find_map(|e| match e {
            Effect::StartScout(r) => Some(&**r),
            _ => None,
        })
    }

    /// Two points are the same point, whatever their name and volume — the
    /// core's own comparison, which the codec's last bits do not fool.
    fn same_point(a: &AppState, b: &AppState) -> bool {
        same_genome(&encode_genome(a), &encode_genome(b))
    }

    fn how_far(a: &AppState, b: &AppState) -> f64 {
        let (x, y) = (encode_genome(a), encode_genome(b));
        x.iter().zip(&y).map(|(p, q)| (p - q) * (p - q)).sum::<f64>().sqrt()
    }

    /// A point as ⚙ Settings would hand it back: one continuous gene moved.
    fn edited(point: &AppState) -> AppState {
        let mut g = encode_genome(point);
        let i = genes().iter().position(|d| d.kind == GeneKind::Cont).expect("a continuous gene");
        g[i] = 1.0 - g[i];
        let mut out = decode_genome(&g);
        out.audio.master_gain = point.audio.master_gain;
        out
    }

    #[test]
    fn a_press_morphs_the_sound_to_where_it_leads_and_lands_exactly_there() {
        let mut s = session();
        s.set_playing(0.0, true);
        let fx = s.like(0.0);
        assert!(said(&fx).expect("a status line").contains("👍"));
        assert!(pushed(&fx).is_empty(), "the press itself changes no sound — the morph does");
        assert!(s.view().morphing);

        let target = s.point();
        let half = s.tick(1.0);
        let mid = *pushed(&half).last().expect("a push half-way");
        assert!(!same_point(mid, &target), "half-way is not there yet");
        assert!(how_far(mid, &target) > 0.0);

        let end = s.tick(2.0);
        let landed = *pushed(&end).last().expect("a last push");
        assert_eq!(landed, &target, "a morph ends exactly on its target");
        assert!(saved(&end).is_some(), "and the point is worth keeping");
        assert!(!s.view().morphing);
        assert!(!s.wants_tick());
    }

    #[test]
    fn a_press_mid_morph_starts_from_what_is_audible() {
        let mut s = session();
        s.set_playing(0.0, true);
        s.like(0.0);
        let half = s.tick(1.0);
        let audible = (*pushed(&half).last().expect("a push")).clone();
        let heading_for = s.point();

        s.dislike(1.0);
        let on = s.tick(1.06);
        let next = (*pushed(&on).last().expect("a push")).clone();
        let moved = how_far(&audible, &next);
        assert!(moved > 0.0, "the new morph has started");
        assert!(
            moved < 0.05 * how_far(&audible, &heading_for),
            "it starts from the sound, not from where the first morph began"
        );
    }

    #[test]
    fn nothing_is_pushed_to_a_sound_that_is_not_playing() {
        let mut s = session();
        let fx = s.like(0.0);
        assert!(pushed(&fx).is_empty());
        assert!(pushed(&s.tick(1.0)).is_empty());
        let end = s.tick(3.0);
        assert!(pushed(&end).is_empty());
        assert!(saved(&end).is_some(), "it still settles");
        assert_eq!(s.live_point(), s.point(), "what ▶ would start is where the press led");
    }

    #[test]
    fn a_load_is_a_hard_switch_a_reseed_and_a_fresh_search() {
        let mut s = session();
        s.set_playing(0.0, true);
        s.like(0.0);
        s.tick(2.0);
        assert!(s.view().can_undo);

        let fx = s.load_preset(3.0, 5).expect("preset 5");
        let point = switched(&fx).expect("a hard switch, not a glide");
        assert!(pushed(&fx).is_empty());
        assert!(same_point(point, &presets()[5].state));
        assert_eq!(point.preset_name.as_deref(), Some(presets()[5].name.as_str()));
        assert!(reseeded(&fx), "a new point gets a new picture");
        assert!(saved(&fx).is_some());

        let v = s.view();
        assert_eq!(v.name, presets()[5].name);
        assert_eq!(v.steps, 0);
        assert!(!v.can_undo, "a load clears the history");
        assert!(!v.morphing);
        assert!(s.load_preset(3.0, presets().len()).is_none(), "past the end of the built-in list");
    }

    #[test]
    fn closing_settings_is_one_undoable_step_and_a_jump() {
        let mut s = session();
        s.set_playing(0.0, true);
        s.like(0.0);

        // Opening lands a morph in flight: the page edits the point the sound
        // was heading to, so that is what is playing from now on.
        let open = s.open_settings(0.5);
        assert_eq!(pushed(&open).len(), 1);
        assert!(!s.view().morphing);
        let landed = s.point();
        assert_eq!(pushed(&open)[0], &landed);

        let hand_set = edited(&landed);
        let fx = s.close_settings(1.0, &hand_set);
        assert_eq!(pushed(&fx).len(), 1, "a jump: the sound was edited as it played");
        assert!(same_point(pushed(&fx)[0], &hand_set));
        assert!(!s.view().morphing);
        assert_eq!(s.view().steps, 2, "the press, then the edit");
        assert!(said(&fx).expect("a status line").contains("Settings"));
        assert!(saved(&fx).is_some());

        let back = s.undo(2.0);
        assert!(said(&back).expect("a status line").contains("undone"));
        assert_eq!(s.view().steps, 1);
        s.tick(2.0 + UNDO_MORPH_SECONDS + 1e-6);
        assert!(same_point(&s.point(), &landed), "one undo takes the whole edit back");
    }

    #[test]
    fn settings_closed_on_the_same_point_changes_nothing() {
        let mut s = session();
        s.set_playing(0.0, true);
        s.open_settings(0.0);
        let same = s.point();
        let fx = s.close_settings(0.1, &same);
        assert!(pushed(&fx).is_empty());
        assert!(said(&fx).is_none());
        assert!(saved(&fx).is_some(), "it settles, and is scouted from here");
        assert_eq!(s.view().steps, 0);
        assert!(!s.view().can_undo);
    }

    #[test]
    fn the_scout_waits_for_the_sound_to_settle_and_a_press_takes_its_best_candidate() {
        let mut s = session_with(scout_opts());
        assert!(job(&s.tick(10.0)).is_none(), "nothing is rendered for a point nobody hears");

        s.set_playing(0.0, true);
        assert!(job(&s.tick(0.5)).is_none(), "not until the sound has settled");
        let fx = s.tick(SCOUT_SETTLE + 0.01);
        let request = job(&fx).expect("a job");
        assert_eq!(request.likes.len(), 2);
        assert_eq!(request.dislikes.len(), 2);
        assert_eq!(request.settings.sample_rate, ScoutConfig::default().sample_rate);
        assert_eq!(request.settings.master_gain, s.master_gain());
        assert!(s.view().scout_busy);
        assert!(job(&s.tick(2.0)).is_none(), "one job at a time");

        let likes = request.likes.clone();
        let told = s.scout_finished(run_scout(request));
        assert!(said(&told).expect("a status line").starts_with("scouted 2 + 2 candidates"));
        let v = s.view();
        assert_eq!((v.scouted_like, v.scouted_dislike), (2, 2));
        assert!(!v.scout_busy);

        let fx = s.like(3.0);
        assert!(said(&fx).expect("a status line").contains("best of 2"));
        assert!(
            likes.iter().any(|g| same_point(&decode_genome(g), &s.point())),
            "the press took a candidate"
        );
        assert_eq!(s.view().scouted_like, 0, "they were about the point before the press");
    }

    #[test]
    fn candidates_for_a_point_the_user_has_left_are_dropped() {
        let mut s = session_with(scout_opts());
        s.set_playing(0.0, true);
        let fx = s.tick(SCOUT_SETTLE + 0.01);
        let request = job(&fx).expect("a job");
        let result = run_scout(request);

        s.like(1.0); // the point moved while it was rendering
        let told = s.scout_finished(result);
        assert!(told.is_empty(), "a stale result says nothing");
        assert_eq!(s.view().scouted_like, 0);
        assert!(!s.view().scout_busy);
    }

    #[test]
    fn a_surprise_lands_near_another_built_in_point_and_reseeds() {
        let mut s = session();
        let fx = s.surprise(0.0);
        assert!(reseeded(&fx));
        assert!(said(&fx).expect("a status line").contains("🎲"));
        let v = s.view();
        assert!(v.point_name.starts_with("near "));
        assert_ne!(v.point_name, format!("near {}", presets()[0].name), "not where we already are");
        assert_eq!(v.steps, 0, "a fresh start, as a load is");
        assert!(v.can_undo, "but one you can take back");
    }

    #[test]
    fn undo_walks_back_and_says_when_there_is_nothing_left() {
        let mut s = session();
        assert_eq!(said(&s.undo(0.0)), Some("nothing to undo"));
        assert!(!s.view().can_undo);

        s.set_playing(0.0, true);
        s.like(0.0);
        s.tick(2.0);
        s.like(2.0);
        s.tick(4.0);
        assert_eq!(s.view().steps, 2);
        let there = s.point();

        let fx = s.undo(4.0);
        assert!(said(&fx).expect("a status line").contains("undone"));
        assert_eq!(s.view().steps, 1);
        assert!(s.view().morphing);
        // An undo arrives quicker than a change: it goes back to something
        // that was just heard.
        s.tick(4.0 + UNDO_MORPH_SECONDS + 1e-6);
        assert!(!s.view().morphing);
        assert!(!same_point(&s.point(), &there));
    }

    #[test]
    fn the_name_carries_the_step_count_and_the_point_stops_being_the_preset() {
        let mut s = session();
        let preset = presets()[0].name.clone();
        assert_eq!(s.view().name, preset);
        assert_eq!(s.point().preset_name.as_deref(), Some(preset.as_str()));

        s.like(0.0);
        assert_eq!(s.view().name, format!("{preset} · 1 step"));
        assert_eq!(s.point().preset_name, None, "it is the user's own point now");
        assert_eq!(s.view().point_name, preset, "the name still says where it came from");

        s.tick(2.0);
        s.like(2.0);
        assert_eq!(s.view().name, format!("{preset} · 2 steps"));
    }

    #[test]
    fn a_status_line_says_what_happened_and_what_changed() {
        let mut s = session();
        let fx = s.like(0.0);
        let (first, second) = said(&fx).expect("a status line").split_once('\n').expect("two lines");
        assert!(first.starts_with(action_label(ExplorerAction::Like)));
        assert!(first.contains("step 1"), "{first}");
        assert!(first.contains("spread 0."), "{first}");
        assert!(!second.is_empty(), "a press that changes nothing is not a press");
        assert_eq!(s.view().status, said(&fx).unwrap(), "the view shows the last line");
    }

    #[test]
    fn a_morph_pushes_about_twenty_times_a_second_not_at_every_tick() {
        let mut s = session();
        s.set_playing(0.0, true);
        s.like(0.0);
        let mut pushes = 0;
        let mut t = 0.0;
        while t < 2.0 {
            t += 0.01;
            pushes += pushed(&s.tick(t)).len();
        }
        let want = (MORPH_SECONDS / PUSH_INTERVAL) as usize;
        assert!(pushes.abs_diff(want) <= 2, "{pushes} pushes, wanted about {want}");
    }

    #[test]
    fn a_point_left_mid_search_keeps_the_name_of_where_it_came_from() {
        // What the app stores and reads back: the point as it was, and the
        // name that was on screen.
        let mut s = session();
        let preset = presets()[0].name.clone();
        s.like(0.0);
        s.tick(2.0);
        let (left_on, shown) = (s.point(), s.view().point_name);
        assert_eq!(left_on.preset_name, None, "a stepped point is nobody's");

        let back = Session::restored(&shown, &left_on, opts());
        assert_eq!(back.view().name, preset, "the title says where it came from");
        assert_eq!(back.point().preset_name, None, "and the point still claims nothing");
        assert_eq!(back.view().steps, 0, "the steps start again: the history is gone");
    }

    #[test]
    fn a_kept_point_takes_the_name_it_was_kept_under() {
        let mut s = session();
        let preset = presets()[0].name.clone();
        s.like(0.0);
        s.tick(2.0);
        assert_eq!(s.view().name, format!("{preset} · 1 step"));
        assert_eq!(s.point().preset_name, None, "a stepped point is nobody's yet");

        let fx = s.kept_as("Dawn");
        assert_eq!(s.view().name, "Dawn", "the title is the name it was kept under");
        assert_eq!(s.view().steps, 0, "and the steps start again from it");
        assert_eq!(s.point().preset_name.as_deref(), Some("Dawn"));
        assert!(said(&fx).expect("a status line").contains("Dawn"));
        assert_eq!(saved(&fx).and_then(|p| p.preset_name.clone()).as_deref(), Some("Dawn"));
        assert!(s.view().can_undo, "the search is untouched: ↩ still walks back");
        assert!(s.kept_as("").is_empty(), "a point is not kept under no name");
    }

    #[test]
    fn the_volume_is_not_a_gene_and_the_scout_judges_at_it() {
        let mut s = session_with(scout_opts());
        s.set_playing(0.0, true);
        let fx = s.set_master_gain(0.1, 0.4);
        assert_eq!(pushed(&fx).len(), 1, "a volume change is heard at once");
        assert_eq!(pushed(&fx)[0].audio.master_gain, 0.4);

        s.like(0.2);
        s.tick(2.2);
        assert_eq!(s.point().audio.master_gain, 0.4, "the search does not touch the volume");
        let fx = s.tick(2.2 + SCOUT_SETTLE + 0.01);
        assert_eq!(job(&fx).expect("a job").settings.master_gain, 0.4);
    }

    #[test]
    fn it_asks_for_a_clock_only_while_something_is_due() {
        let mut s = session();
        assert!(!s.wants_tick(), "a settled point with no scout needs no clock");
        s.like(0.0);
        assert!(s.wants_tick());
        s.tick(1.0);
        assert!(s.wants_tick());
        s.tick(2.0);
        assert!(!s.wants_tick());

        let mut s = session_with(scout_opts());
        s.set_playing(0.0, true);
        assert!(s.wants_tick(), "a scout is due");
        s.tick(SCOUT_SETTLE + 0.01);
        assert!(!s.wants_tick(), "the job is out; its result comes back on a thread");
    }

    #[test]
    fn stopping_the_sound_stops_the_scout() {
        let mut s = session_with(scout_opts());
        s.set_playing(0.0, true);
        s.set_playing(0.1, false);
        assert!(!s.wants_tick());
        assert!(job(&s.tick(10.0)).is_none());
        s.set_playing(10.0, true);
        assert!(job(&s.tick(10.0 + SCOUT_SETTLE + 0.01)).is_some(), "and ▶ starts it again");
    }

    #[test]
    fn the_same_seed_replays_the_same_session() {
        let run = || {
            let mut s = session();
            s.like(0.0);
            s.tick(2.0);
            s.dislike(2.0);
            s.tick(4.0);
            s.surprise(4.0);
            s.tick(6.0);
            (s.point(), s.view().name)
        };
        let (a, name_a) = run();
        let (b, name_b) = run();
        assert_eq!(a, b);
        assert_eq!(name_a, name_b);
    }

    #[test]
    fn a_pool_renders_a_job_on_its_own_threads() {
        let pool = ScoutPool::new(2);
        assert_eq!(pool.threads(), 2);
        let mut s = session_with(scout_opts());
        s.set_playing(0.0, true);
        let fx = s.tick(SCOUT_SETTLE + 0.01);
        let result = pool.run(job(&fx).expect("a job"));
        assert_eq!(result.ready(ScoutKind::Like), 2);
        assert!(result.seconds > 0.0);
        assert!(ScoutPool::new(0).threads() >= 1, "0 means every core but two");
    }
}
