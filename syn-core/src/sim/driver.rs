//! What the picture does each frame, for a renderer that owns its own field.
//!
//! The web app's frame loop (`loop()` in `main.ts`) decides a handful of
//! things between the sound and the screen, none of which depends on where
//! the field lives: an onset hit becomes fresh growth at a random spot and a
//! ripple spreading from it; a finger becomes the same thing with the finger
//! as its source, stamped along a stroke so a drag is a line and not a dotted
//! trail; the point becomes this frame's simulation params, palette and
//! display effects; and the LFOs keep their phase across a stop, because they
//! run on the sound's clock while there is sound and on a continuation of it
//! when there is not.
//!
//! That is this type. The CPU [`Picture`](super::Picture) drives its
//! [`Sim`](super::Sim) with it, and the Android app drives seven GLSL passes
//! with the same values (synesthesia-android PLAN.md, decision 4) — so the
//! two pictures are the same picture, and a Swift app will not write this a
//! third time.

use super::coupling::{RippleSet, MAX_RIPPLES};
use super::field::Seed;
use super::frame::{frame_params, FrameParams};
use super::{Ripple, EVOLVE_DT};
use crate::dsp::rng::{Mulberry32, Rng};
use crate::visualizer::VizInput;

/// A finger's disc is fixed, not taken from the point's `onsetToSeed`
/// coupling (agreed with the user): people asked for a reaction, and a point
/// that happens to have evolved a weak onset coupling must not read as a
/// broken app.
pub const TOUCH_RADIUS: f32 = 0.035;
pub const TOUCH_AMOUNT: f32 = 0.85;
/// Stamps are this far apart along a drag, and at most this many per frame.
pub const TOUCH_SPACING: f32 = TOUCH_RADIUS * 0.6;
pub const TOUCH_MAX_STAMPS: usize = 8;

/// Below this, the `onsetToSeed` coupling does nothing at all (`seedOnHit`).
const MIN_SEED_COUPLING: f64 = 0.02;

/// A disc of fresh growth to drop into the field before the frame's
/// simulation runs — `inject.frag` on a GPU, [`Sim::inject`](super::Sim::inject) here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inject {
    /// UV, 0..1.
    pub x: f32,
    pub y: f32,
    /// In height units, so the disc is round on any aspect.
    pub radius: f32,
    /// 0..1 of the disc converted.
    pub amount: f32,
}

/// Everything one frame needs from the model.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The LFO clock this frame was built on — the sound's own time while it
    /// plays.
    pub time: f64,
    /// Apply these first, in order.
    pub injects: Vec<Inject>,
    pub params: FrameParams,
    /// Alive at [`Frame::time`], newest last.
    pub ripples: Vec<Ripple>,
    /// How far the noise fields have drifted (`uEvolveT`).
    pub evolve_t: f64,
}

/// Where to stamp between the pointer's last stamped position and where it is
/// now (`strokePoints`). A drag is sampled once per frame — a move event can
/// fire at 120 Hz and each stamp is a full-grid pass — and at 10 fps a finger
/// crosses a third of the screen between two frames.
///
/// `aspect` (grid width / height) makes the spacing a screen distance rather
/// than a UV one, the same correction `inject.frag` applies to keep its disc
/// round. `from` is never returned: the previous frame stamped it already.
pub fn stroke_points(
    from: (f32, f32),
    to: (f32, f32),
    spacing: f32,
    aspect: f32,
    max: usize,
) -> Vec<(f32, f32)> {
    let dx = (to.0 - from.0) * aspect;
    let dy = to.1 - from.1;
    let steps = ((dx.hypot(dy) / spacing).ceil() as usize).clamp(1, max.max(1));
    (1..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
        })
        .collect()
}

pub struct Driver {
    rng: Mulberry32,
    ripples: RippleSet,
    /// The engine's hit counter as last seen; `None` until the first frame.
    hits: Option<u64>,
    evolve_t: f64,
    /// Where the finger is, while one is down.
    pointer: Option<(f32, f32)>,
    /// Where it was last stamped, so a stroke continues from there.
    stamped: Option<(f32, f32)>,
    /// `now` − the sound's clock, which keeps the LFOs' phase when the sound
    /// stops and starts again (`clockOffset`).
    lfo_offset: f64,
}

impl Driver {
    pub fn new(seed: u32) -> Self {
        Self {
            rng: Mulberry32::new(seed),
            ripples: RippleSet::default(),
            hits: None,
            evolve_t: 0.0,
            pointer: None,
            stamped: None,
            lfo_offset: 0.0,
        }
    }

    /// The clock the LFOs run on: the sound's own time while it plays, and a
    /// continuation of it when it does not, so a point does not jump when ▶
    /// is pressed (`lfoTime`). `now` is the app's monotonic clock.
    pub fn lfo_time(&mut self, now: f64, sound_time: Option<f64>) -> f64 {
        match sound_time {
            Some(t) => {
                self.lfo_offset = now - t;
                t
            }
            None => now - self.lfo_offset,
        }
    }

    /// A finger lands: it stamps exactly where it did, with no trail, and
    /// sends a ripple out from there.
    pub fn pointer_down(&mut self, x: f32, y: f32, t: f64) {
        self.pointer = Some((x, y));
        self.stamped = None;
        self.ripples.add(x, y, TOUCH_AMOUNT, t);
    }

    /// The finger has moved. Nothing is stamped until the next frame.
    pub fn pointer_moved(&mut self, x: f32, y: f32) {
        if self.pointer.is_some() {
            self.pointer = Some((x, y));
        }
    }

    pub fn pointer_up(&mut self) {
        self.pointer = None;
        self.stamped = None;
    }

    pub fn painting(&self) -> bool {
        self.pointer.is_some()
    }

    /// The ripples alive at `t`; the dead ones are dropped. A renderer that
    /// draws on its own clock (the CPU picture redraws between steps) asks
    /// again at the moment it draws.
    pub fn ripples_at(&mut self, t: f64) -> Vec<Ripple> {
        self.ripples.active(t)
    }

    /// A fresh start, as data: the caller draws it into whatever holds its
    /// field.
    pub fn reseed(&mut self) -> Seed {
        Seed::roll(&mut self.rng)
    }

    /// One frame. `aspect` is the field's (grid width / height), which sets
    /// how far apart a stroke's stamps are.
    pub fn frame(&mut self, input: &VizInput, aspect: f32) -> Frame {
        let mut injects = Vec::new();
        self.seed_on_hits(input, &mut injects);
        self.paint_stroke(aspect, &mut injects);
        let params = frame_params(input.state, &input.features, input.time);
        // The noise fields drift by this frame's flow rate, as the step they
        // are about to feed will see it.
        self.evolve_t += params.sim.flow.evolve_rate * EVOLVE_DT;
        Frame {
            time: input.time,
            injects,
            ripples: self.ripples.active(input.time),
            evolve_t: self.evolve_t,
            params,
        }
    }

    /// New hits since the last frame — at most as many as there are ripples —
    /// each seeding growth at a random spot away from the edges (`seedOnHit`).
    fn seed_on_hits(&mut self, input: &VizInput, out: &mut Vec<Inject>) {
        let new_hits = self.hits.map_or(0, |seen| input.hits.saturating_sub(seen)).min(MAX_RIPPLES as u64);
        self.hits = Some(input.hits);
        let amount = input.state.coupling.get("onsetToSeed").copied().unwrap_or(0.0);
        if amount < MIN_SEED_COUPLING {
            return;
        }
        for _ in 0..new_hits {
            let x = (0.08 + self.rng.next() * 0.84) as f32;
            let y = (0.08 + self.rng.next() * 0.84) as f32;
            out.push(Inject {
                x,
                y,
                radius: (0.015 + 0.035 * amount) as f32,
                amount: (0.4 + amount).min(1.0) as f32,
            });
            self.ripples.add(x, y, amount as f32, input.time);
        }
    }

    fn paint_stroke(&mut self, aspect: f32, out: &mut Vec<Inject>) {
        let Some(to) = self.pointer else { return };
        let points = match self.stamped {
            Some(from) => stroke_points(from, to, TOUCH_SPACING, aspect, TOUCH_MAX_STAMPS),
            None => vec![to],
        };
        for (x, y) in points {
            out.push(Inject { x, y, radius: TOUCH_RADIUS, amount: TOUCH_AMOUNT });
        }
        self.stamped = Some(to);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::AudioFeatures;
    use crate::sim::field::MAX_SPOTS;
    use crate::state::{presets, AppState};

    fn state_with_seeding(amount: f64) -> AppState {
        let mut s = presets()[0].state.clone();
        s.coupling.insert("onsetToSeed".into(), amount);
        s
    }

    fn input<'a>(state: &'a AppState, hits: u64, time: f64) -> VizInput<'a> {
        VizInput { state, features: AudioFeatures { brightness: 0.5, ..Default::default() }, hits, time }
    }

    #[test]
    fn an_onset_hit_seeds_growth_and_a_ripple_where_it_seeded() {
        let state = state_with_seeding(0.6);
        let mut d = Driver::new(7);
        // The first frame only learns the counter.
        assert!(d.frame(&input(&state, 5, 0.0), 1.0).injects.is_empty());
        let f = d.frame(&input(&state, 7, 0.1), 1.0);
        assert_eq!(f.injects.len(), 2, "two hits, two discs");
        assert_eq!(f.ripples.len(), 2);
        for (i, r) in f.injects.iter().zip(&f.ripples) {
            assert_eq!((i.x, i.y), (r.x, r.y), "the ripple starts where the growth did");
            assert!((0.08..=0.92).contains(&i.x) && (0.08..=0.92).contains(&i.y), "away from the edges");
            assert!(i.radius > 0.015 && i.amount > 0.4);
        }
    }

    #[test]
    fn more_hits_than_ripples_in_one_frame_are_capped() {
        let state = state_with_seeding(0.5);
        let mut d = Driver::new(1);
        d.frame(&input(&state, 0, 0.0), 1.0);
        let f = d.frame(&input(&state, 50, 0.1), 1.0);
        assert_eq!(f.injects.len(), MAX_RIPPLES);
    }

    #[test]
    fn a_weak_onset_coupling_seeds_nothing() {
        let state = state_with_seeding(0.01);
        let mut d = Driver::new(3);
        d.frame(&input(&state, 0, 0.0), 1.0);
        let f = d.frame(&input(&state, 4, 0.1), 1.0);
        assert!(f.injects.is_empty());
        assert!(f.ripples.is_empty());
    }

    #[test]
    fn a_finger_stamps_where_it_lands_then_strokes_as_it_moves() {
        let state = presets()[0].state.clone();
        let mut d = Driver::new(5);
        d.frame(&input(&state, 0, 0.0), 1.0);
        assert!(!d.painting());

        d.pointer_down(0.25, 0.5, 0.0);
        assert!(d.painting());
        let f = d.frame(&input(&state, 0, 0.1), 1.0);
        assert_eq!(f.injects, vec![Inject { x: 0.25, y: 0.5, radius: TOUCH_RADIUS, amount: TOUCH_AMOUNT }]);
        assert_eq!(f.ripples.len(), 1, "a touch ripples too");

        // Far enough to need several stamps, and never more than the cap.
        d.pointer_moved(0.95, 0.5);
        let f = d.frame(&input(&state, 0, 0.2), 1.0);
        assert_eq!(f.injects.len(), TOUCH_MAX_STAMPS);
        assert!((f.injects.last().unwrap().x - 0.95).abs() < 1e-6, "the last stamp is where the finger is");

        // Held still: one stamp a frame, as the web app does.
        let f = d.frame(&input(&state, 0, 0.3), 1.0);
        assert_eq!(f.injects.len(), 1);

        d.pointer_up();
        assert!(d.frame(&input(&state, 0, 0.4), 1.0).injects.is_empty());
    }

    #[test]
    fn a_stroke_is_spaced_in_screen_distance_not_in_uv() {
        // The same UV distance across a wide grid is a longer stroke.
        let narrow = stroke_points((0.2, 0.5), (0.5, 0.5), TOUCH_SPACING, 1.0, 64);
        let wide = stroke_points((0.2, 0.5), (0.5, 0.5), TOUCH_SPACING, 4.0, 64);
        assert!(wide.len() > narrow.len());
        assert_eq!(*narrow.last().unwrap(), (0.5, 0.5), "it ends at the pointer");
        assert_eq!(stroke_points((0.5, 0.5), (0.5, 0.5), TOUCH_SPACING, 1.0, 8).len(), 1);
        assert_eq!(stroke_points((0.0, 0.0), (1.0, 1.0), TOUCH_SPACING, 1.0, 3).len(), 3, "capped");
    }

    #[test]
    fn the_lfo_clock_follows_the_sound_and_carries_on_without_it() {
        let mut d = Driver::new(1);
        // No sound yet: the clock is the app's own.
        assert_eq!(d.lfo_time(10.0, None), 10.0);
        // The sound starts, 4 s into its own life: that is the clock now.
        assert_eq!(d.lfo_time(100.0, Some(4.0)), 4.0);
        // It stops at app-time 100: the LFOs carry on from 4 s, not jump to 101.
        assert_eq!(d.lfo_time(101.0, None), 5.0);
        // And when it comes back, it leads again.
        assert_eq!(d.lfo_time(200.0, Some(4.5)), 4.5);
    }

    #[test]
    fn the_noise_fields_drift_at_the_points_flow_rate() {
        let mut state = presets()[0].state.clone();
        let rate = 0.5;
        if let Some(card) = state.visual.cards.get_mut("flow") {
            card.on = true;
            card.params.insert("evolveRate".into(), rate);
        }
        state.modulation.routes.clear();
        let mut d = Driver::new(2);
        let first = d.frame(&input(&state, 0, 0.0), 1.0);
        let second = d.frame(&input(&state, 0, 0.1), 1.0);
        assert!(second.evolve_t > first.evolve_t, "it drifts");
        let step = second.evolve_t - first.evolve_t;
        assert!((step - rate * EVOLVE_DT).abs() < 1e-12, "{step}");
    }

    #[test]
    fn a_reseed_is_a_value_the_renderer_draws() {
        let mut d = Driver::new(9);
        let a = d.reseed();
        assert!((19..=MAX_SPOTS).contains(&a.spots.len()));
        assert!((0.02..=0.05).contains(&a.radius));
        assert_ne!(a, d.reseed(), "a new start is a new start");
    }
}
