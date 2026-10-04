//! The picture through the FFI: what one drawn frame needs, and how big to
//! draw it.
//!
//! The seven GLSL passes are the web app's, verbatim, on every platform
//! (synesthesia-android PLAN.md, decision 4), so an app's renderer is
//! uniforms, ping-pong targets and seven draw calls. Everything it would
//! otherwise have to decide for itself — what an onset hit becomes, where a
//! finger's stamps go, which ripples are alive, how far the noise has
//! drifted, what clock the LFOs are on, which rung of the quality ladder this
//! device holds — is [`syn_session`]-shaped work that lives in the core, and
//! comes out of here as plain records named after the uniforms they fill.

use std::sync::{Arc, Mutex};

use syn_core::sim::driver::{Driver, Frame};
use syn_core::sim::field::Seed;
use syn_core::sim::quality::{self, ProbeOptions, QualityProbe, QualityRung, QUALITY_LADDER, TOP_RUNG};
use syn_core::sim::Picture;
use syn_core::sim::{Inject, Ripple};
use syn_core::state::AppState;
use syn_core::AudioFeatures;

use crate::{parse_point, AudioFrame, CoreError};

/// A disc of fresh growth to drop into the field before this frame's passes
/// (`inject.frag`): an onset hit, or a finger.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct InjectDisc {
    /// UV, 0..1.
    pub x: f32,
    pub y: f32,
    /// In height units — `uAspect` keeps the disc round.
    pub radius: f32,
    /// 0..1 of the disc converted.
    pub amount: f32,
}

/// One ring spreading from a hit or a touch, as `uRipples` wants it.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct RippleRing {
    pub x: f32,
    pub y: f32,
    /// Seconds since it started.
    pub age: f32,
    /// 0..1; the display pass fades it over its life.
    pub amp: f32,
}

/// `react.frag`'s uniforms, and how many times to run it this frame.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct ReactionUniforms {
    pub feed: f32,
    pub kill: f32,
    pub diff_u: f32,
    pub diff_v: f32,
    /// Gray–Scott substeps, at least 1.
    pub substeps: u32,
}

/// `paramfield.frag`'s uniforms. When `active` is false the pass is not worth
/// running at all: hand the reaction a zero texture instead.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct FieldVariationUniforms {
    pub feed_amount: f32,
    pub feed_scale: f32,
    pub feed_warp: f32,
    pub kill_amount: f32,
    pub kill_scale: f32,
    pub kill_warp: f32,
    pub active: bool,
}

/// `velocity.frag`'s and `advect.frag`'s uniforms. When `advecting` is false,
/// advection would be an identity copy: skip both passes.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct FlowUniforms {
    pub curl_strength: f32,
    pub curl_scale: f32,
    pub drift_x: f32,
    /// Already negated for a Y-up surface, as `uDrift` expects.
    pub drift_y: f32,
    pub advect_amount: f32,
    pub advecting: bool,
}

/// `display.frag`'s colour uniforms: the cosine gradient with the Palette
/// card's shift and contrast already folded in, and the relief knobs.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PaletteUniforms {
    /// `uPalA`..`uPalD`, three floats each.
    pub a: Vec<f32>,
    pub b: Vec<f32>,
    pub c: Vec<f32>,
    pub d: Vec<f32>,
    pub bands: f32,
    pub relief: f32,
    pub gloss: f32,
    /// `uLightDir`, normalized here — trig on a uniform, per pixel, is not free.
    pub light_dir: Vec<f32>,
}

/// `display.frag`'s sound effects: the swell breathes the exposure, an onset
/// flares the highlights, the bands tint the tones.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct DisplayUniforms {
    pub exposure: f32,
    pub flash: f32,
    /// `uTint`: dark, mid, light.
    pub tint: Vec<f32>,
}

/// One frame of the picture, ready to draw.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PictureFrame {
    /// The LFO clock this frame was built on — the sound's own time while it
    /// plays.
    pub time: f64,
    /// `uEvolveT`: how far the noise fields have drifted.
    pub evolve_t: f32,
    /// Inject these, in order, before the reaction runs.
    pub injects: Vec<InjectDisc>,
    /// Up to `max_ripples()` of them; pad the uniform with zeros.
    pub ripples: Vec<RippleRing>,
    pub reaction: ReactionUniforms,
    pub field_variation: FieldVariationUniforms,
    pub flow: FlowUniforms,
    pub palette: PaletteUniforms,
    pub display: DisplayUniforms,
}

/// A fresh start for the field: `seed.frag`'s spots, or the same ones drawn
/// into a CPU field.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SeedSpots {
    /// `uSpots`, x and y interleaved — at most `max_seed_spots()` pairs.
    pub xy: Vec<f32>,
    /// `uSpotCount`.
    pub count: u32,
    /// `uSpotRadius`, in height units.
    pub radius: f32,
}

/// A size in pixels or cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SizeInt {
    pub width: u32,
    pub height: u32,
}

/// One rung of the quality ladder.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Rung {
    pub index: u32,
    /// Cap on the drawing surface's longest side, device pixels; 0 = uncapped.
    pub max_side: u32,
    /// The simulation grid's long side.
    pub res: u32,
}

fn rung_at(index: usize) -> Rung {
    let QualityRung { max_side, res } = QUALITY_LADDER[index.min(TOP_RUNG)];
    Rung { index: index.min(TOP_RUNG) as u32, max_side: max_side as u32, res: res as u32 }
}

/// The ladder, cheapest rung first — what a device is measured against.
#[uniffi::export]
pub fn quality_ladder() -> Vec<Rung> {
    (0..QUALITY_LADDER.len()).map(rung_at).collect()
}

/// The drawing surface for a view of `width`×`height` device pixels at a
/// rung's cap. The view keeps its own size: the upscale is a blit.
#[uniffi::export]
pub fn backing_store(max_side: u32, width: u32, height: u32) -> SizeInt {
    let (w, h) = quality::backing_store(max_side as usize, width as usize, height as usize);
    SizeInt { width: w as u32, height: h as u32 }
}

/// The simulation grid for a surface: long side `res`, short side fitted to
/// the aspect, so a blob is round on screen.
#[uniffi::export]
pub fn sim_grid(res: u32, width: u32, height: u32) -> SizeInt {
    let (w, h) = quality::grid_size(res as usize, width as usize, height as usize);
    SizeInt { width: w as u32, height: h as u32 }
}

/// The grid for the paramfield and velocity textures: half each side. Both
/// are smooth by construction and read with a linear filter, so the detail
/// is not there to lose.
#[uniffi::export]
pub fn field_grid(width: u32, height: u32) -> SizeInt {
    let (w, h) = syn_core::sim::fields::half_size(width as usize, height as usize);
    SizeInt { width: w as u32, height: h as u32 }
}

/// The length of `seed.frag`'s `uSpots` array.
#[uniffi::export]
pub fn max_seed_spots() -> u32 {
    syn_core::sim::MAX_SPOTS as u32
}

/// The length of `display.frag`'s `uRipples` array.
#[uniffi::export]
pub fn max_ripples() -> u32 {
    syn_core::sim::coupling::MAX_RIPPLES as u32
}

struct Inner {
    driver: Driver,
    /// The point as it sounds now — set from the session's effects.
    point: AppState,
    probe: QualityProbe,
    /// The engine's hit counter as last heard, so a silent stretch does not
    /// read as a burst of hits when the sound comes back.
    hits: u64,
    /// The last frame handed out, so the CPU picture can be drawn from the
    /// very same one the GPU was.
    last: Option<Frame>,
    /// The CPU picture, once an app has asked for it.
    cpu: Option<Picture>,
}

/// The picture's per-frame driver, and the quality rung this device holds.
///
/// One thread — the renderer's — calls [`PictureDriver::frame`]; the UI
/// thread may set the point and report touches at the same time.
#[derive(uniffi::Object)]
pub struct PictureDriver {
    inner: Mutex<Inner>,
}

#[uniffi::export]
impl PictureDriver {
    /// A driver for `point_json`. With `measure`, the first frames pick a
    /// rung of the quality ladder by rendering at it ([`PictureDriver::probe_frame`]);
    /// without it, the top rung is kept and nothing is measured.
    #[uniffi::constructor]
    pub fn new(seed: u32, point_json: String, measure: bool) -> Result<Arc<Self>, CoreError> {
        let point = parse_point(&point_json)?;
        Ok(Arc::new(PictureDriver {
            inner: Mutex::new(Inner {
                driver: Driver::new(seed),
                point,
                probe: if measure {
                    QualityProbe::new(ProbeOptions::default())
                } else {
                    QualityProbe::fixed(TOP_RUNG)
                },
                hits: 0,
                last: None,
                cpu: None,
            }),
        }))
    }

    /// The point the picture is of: the session's `SetPoint` and `SwitchTo`
    /// effects, the same ones the sound follows.
    pub fn set_point(&self, point_json: String) -> Result<(), CoreError> {
        let point = parse_point(&point_json)?;
        self.locked().point = point;
        Ok(())
    }

    /// One frame. `now` is the app's monotonic clock in seconds and `sound`
    /// is the frame being *heard* (`SoundPlayer.frame_at`), or `None` when
    /// nothing plays — the LFOs then carry on from where the sound left them.
    /// `aspect` is the simulation grid's width / height.
    pub fn frame(&self, now: f64, sound: Option<AudioFrame>, aspect: f32) -> PictureFrame {
        let mut inner = self.locked();
        let time = inner.driver.lfo_time(now, sound.as_ref().map(|s| s.time));
        let features = sound.as_ref().map_or_else(AudioFeatures::default, features_of);
        if let Some(s) = &sound {
            inner.hits = s.hits;
        }
        let hits = inner.hits;
        let point = std::mem::replace(&mut inner.point, AppState::new());
        let frame = inner
            .driver
            .frame(&syn_core::visualizer::VizInput { state: &point, features, hits, time }, aspect);
        inner.point = point;
        let out = PictureFrame {
            time: frame.time,
            evolve_t: frame.evolve_t as f32,
            injects: frame.injects.iter().map(disc_of).collect(),
            ripples: frame.ripples.iter().map(ring_of).collect(),
            reaction: ReactionUniforms {
                feed: frame.params.sim.reaction.feed as f32,
                kill: frame.params.sim.reaction.kill as f32,
                diff_u: frame.params.sim.reaction.diff_u as f32,
                diff_v: frame.params.sim.reaction.diff_v as f32,
                substeps: frame.params.sim.reaction.substeps() as u32,
            },
            field_variation: FieldVariationUniforms {
                feed_amount: frame.params.sim.field_variation.feed_var_amount as f32,
                feed_scale: frame.params.sim.field_variation.feed_var_scale as f32,
                feed_warp: frame.params.sim.field_variation.feed_var_warp as f32,
                kill_amount: frame.params.sim.field_variation.kill_var_amount as f32,
                kill_scale: frame.params.sim.field_variation.kill_var_scale as f32,
                kill_warp: frame.params.sim.field_variation.kill_var_warp as f32,
                active: frame.params.sim.field_variation.active(),
            },
            flow: FlowUniforms {
                curl_strength: frame.params.sim.flow.curl_strength as f32,
                curl_scale: frame.params.sim.flow.curl_scale as f32,
                drift_x: frame.params.sim.flow.drift_x as f32,
                // The surface is Y-up, so "down" — what a positive Drift Y
                // should mean — is -Y.
                drift_y: -frame.params.sim.flow.drift_y as f32,
                advect_amount: frame.params.sim.flow.advect_amount as f32,
                advecting: frame.params.sim.flow.advect_active(),
            },
            palette: PaletteUniforms {
                a: frame.params.palette.a.to_vec(),
                b: frame.params.palette.b.to_vec(),
                c: frame.params.palette.c.to_vec(),
                d: frame.params.palette.d.to_vec(),
                bands: frame.params.palette.bands,
                relief: frame.params.palette.relief,
                gloss: frame.params.palette.gloss,
                light_dir: syn_core::sim::display::light_dir(frame.params.palette.light_angle).to_vec(),
            },
            display: DisplayUniforms {
                exposure: frame.params.fx.exposure,
                flash: frame.params.fx.flash,
                tint: frame.params.fx.tint.to_vec(),
            },
        };
        inner.last = Some(frame);
        out
    }

    /// Draws on the CPU from here on, at `width`×`height` pixels — the
    /// fallback for a device that cannot render into a float texture, and the
    /// reference the GPU path is compared against (synesthesia-android
    /// PLAN.md, decision 4). The grid follows the pixels, as the terminal
    /// picture's does.
    pub fn use_cpu_picture(&self, seed: u32, width: u32, height: u32) {
        let mut inner = self.locked();
        match &mut inner.cpu {
            Some(picture) => picture.resize(width as usize, height as usize),
            None => inner.cpu = Some(Picture::new(width as usize, height as usize, seed)),
        }
    }

    /// A fresh start for the CPU picture, from the same spots
    /// [`PictureDriver::reseed`] gave the renderer.
    pub fn cpu_seed(&self, spots: SeedSpots) {
        let seed = Seed {
            spots: spots
                .xy
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| (p[0], p[1]))
                .take(spots.count as usize)
                .collect(),
            radius: spots.radius,
        };
        if let Some(picture) = &mut self.locked().cpu {
            picture.seed_with(&seed);
        }
    }

    /// The CPU picture of the last [`PictureDriver::frame`] — the same
    /// injects, the same params, the same ripples — as RGBA bytes, row 0 at
    /// the top. `None` until [`PictureDriver::use_cpu_picture`] has been
    /// called and a frame has been asked for.
    ///
    /// It costs a whole simulation step on the CPU; an app draws either this
    /// or the seven passes, never both. The test that compares them is the
    /// exception.
    pub fn cpu_frame(&self) -> Option<Vec<u8>> {
        let inner = &mut *self.locked();
        let frame = inner.last.as_ref()?;
        let image = inner.cpu.as_mut()?.render_frame(frame);
        let mut out = Vec::with_capacity(image.rgb.len() * 4);
        for px in &image.rgb {
            out.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
        Some(out)
    }

    /// The size of the CPU picture, if there is one.
    pub fn cpu_size(&self) -> Option<SizeInt> {
        let inner = self.locked();
        let image = inner.cpu.as_ref()?.image();
        Some(SizeInt { width: image.w as u32, height: image.h as u32 })
    }

    /// A finger lands on the picture (UV, Y-up as the surface is): it stamps
    /// where it landed and sends a ripple out from there.
    pub fn pointer_down(&self, x: f32, y: f32, now: f64) {
        let mut inner = self.locked();
        let t = inner.driver.lfo_time(now, None);
        inner.driver.pointer_down(x, y, t);
    }

    /// The finger has moved; the stamps are laid down on the next frame, not
    /// per event — each one is a full-grid pass.
    pub fn pointer_moved(&self, x: f32, y: f32) {
        self.locked().driver.pointer_moved(x, y);
    }

    pub fn pointer_up(&self) {
        self.locked().driver.pointer_up();
    }

    /// A fresh start — the session's `Reseed` effect. Draw the spots it
    /// returns with `seed.frag` into both halves of the ping-pong.
    pub fn reseed(&self) -> SeedSpots {
        let seed = self.locked().driver.reseed();
        let mut xy = Vec::with_capacity(seed.spots.len() * 2);
        for (x, y) in &seed.spots {
            xy.push(*x);
            xy.push(*y);
        }
        SeedSpots { xy, count: seed.spots.len() as u32, radius: seed.radius }
    }

    /// Feeds the boot probe one real frame time, unclamped (a 2 s frame must
    /// read as 2 s). True when the rung changed and the caller must apply it:
    /// resize the surface and the grid.
    pub fn probe_frame(&self, ms: f64) -> bool {
        self.locked().probe.frame(ms)
    }

    /// The rung this device is on. Once [`PictureDriver::probe_done`], it
    /// never moves again — the picture must not degrade under the viewer
    /// mid-listen.
    pub fn rung(&self) -> Rung {
        rung_at(self.locked().probe.rung())
    }

    pub fn probe_done(&self) -> bool {
        self.locked().probe.done()
    }
}

impl PictureDriver {
    fn locked(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn features_of(f: &AudioFrame) -> AudioFeatures {
    AudioFeatures {
        loudness: f.loudness,
        swell: f.swell,
        brightness: f.brightness,
        onset: f.onset,
        low: f.low,
        mid: f.mid,
        high: f.high,
    }
}

fn disc_of(i: &Inject) -> InjectDisc {
    InjectDisc { x: i.x, y: i.y, radius: i.radius, amount: i.amount }
}

fn ring_of(r: &Ripple) -> RippleRing {
    RippleRing { x: r.x, y: r.y, age: r.age, amp: r.amp }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset_state_json;

    fn driver() -> Arc<PictureDriver> {
        PictureDriver::new(7, preset_state_json(0).unwrap(), false).expect("preset 0")
    }

    #[test]
    fn a_frame_carries_every_uniform_the_passes_need() {
        let d = driver();
        let f = d.frame(0.0, None, 1.78);
        assert!(f.reaction.substeps >= 1);
        assert!(f.reaction.feed > 0.0 && f.reaction.kill > 0.0);
        assert_eq!(f.palette.a.len(), 3);
        assert_eq!(f.palette.light_dir.len(), 3);
        let len: f32 = f.palette.light_dir.iter().map(|x| x * x).sum();
        assert!((len - 1.0).abs() < 1e-5, "the light is normalized: {len}");
        assert_eq!(f.display.tint.len(), 3);
        assert_eq!(f.display.exposure, 1.0, "silence does not breathe");
        assert!(f.ripples.is_empty() && f.injects.is_empty());
    }

    #[test]
    fn the_sound_drives_the_clock_the_hits_and_the_colours() {
        let d = driver();
        let sound = AudioFrame {
            time: 12.0,
            peak: 0.5,
            rms: 0.3,
            limiter_db: 0.0,
            loudness: 0.8,
            swell: 1.0,
            brightness: 0.7,
            onset: 1.0,
            low: 0.9,
            mid: 0.5,
            high: 0.2,
            hits: 3,
            spectrum: vec![0; 64],
        };
        // The first frame only learns the hit counter: a picture that starts
        // halfway through a listen must not burst with every hit so far.
        assert!(d.frame(99.0, None, 1.0).injects.is_empty());

        let f = d.frame(100.0, Some(sound.clone()), 1.0);
        assert_eq!(f.time, 12.0, "the LFO clock is the sound's own");
        assert!(f.display.exposure > 1.0, "a swell brightens the frame");
        assert!(f.display.flash > 0.0, "an onset flares it");
        assert!(f.display.tint.iter().any(|t| *t > 0.0));
        // Three hits since the last frame: growth, each with a ripple
        // (preset 0 couples onsets to seeding at 0.5).
        assert_eq!(f.injects.len(), 3);
        assert_eq!(f.ripples.len(), 3);
        // And when the sound stops, the LFOs carry on rather than jump.
        let next = d.frame(101.0, None, 1.0);
        assert!((next.time - 13.0).abs() < 1e-9, "{}", next.time);
        assert!(next.injects.is_empty(), "no new hits while nothing plays");
    }

    #[test]
    fn a_finger_paints_and_a_reseed_is_a_list_of_spots() {
        let d = driver();
        d.frame(0.0, None, 1.0);
        d.pointer_down(0.5, 0.5, 0.0);
        let f = d.frame(0.1, None, 1.0);
        assert_eq!(f.injects.len(), 1);
        assert_eq!((f.injects[0].x, f.injects[0].y), (0.5, 0.5));
        assert_eq!(f.ripples.len(), 1);
        d.pointer_up();
        assert!(d.frame(0.2, None, 1.0).injects.is_empty());

        let seed = d.reseed();
        assert_eq!(seed.xy.len(), seed.count as usize * 2);
        assert!(seed.count <= max_seed_spots() && seed.count >= 19);
        assert!(seed.radius > 0.0);
    }

    #[test]
    fn the_point_can_change_under_the_renderer() {
        let d = driver();
        let before = d.frame(0.0, None, 1.0);
        d.set_point(preset_state_json(7).unwrap()).expect("preset 7");
        let after = d.frame(0.1, None, 1.0);
        assert_ne!(before.reaction, after.reaction, "another point reacts differently");
        assert!(matches!(d.set_point("{}".into()), Err(CoreError::InvalidPoint { .. })));
    }

    #[test]
    fn a_measuring_driver_walks_the_ladder_and_one_that_is_told_not_to_does_not() {
        let d = PictureDriver::new(1, preset_state_json(0).unwrap(), true).unwrap();
        assert_eq!(d.rung().index, 0, "it starts at the cheapest rung");
        let mut moved = false;
        for _ in 0..60 {
            moved |= d.probe_frame(1.0);
        }
        assert!(moved && d.probe_done());
        assert_eq!(d.rung().index, quality_ladder().len() as u32 - 1);

        let fixed = driver();
        assert!(fixed.probe_done());
        assert_eq!(fixed.rung().index, quality_ladder().len() as u32 - 1);
        assert!(!fixed.probe_frame(5000.0), "a driver that was told not to measure does not");
    }

    #[test]
    fn the_cpu_picture_draws_the_frame_the_renderer_was_given() {
        let d = driver();
        assert!(d.cpu_frame().is_none(), "nothing to draw yet");
        d.use_cpu_picture(5, 16, 8);
        assert_eq!(d.cpu_size(), Some(SizeInt { width: 16, height: 8 }));
        assert!(d.cpu_frame().is_none(), "and no frame has been asked for");

        d.cpu_seed(d.reseed());
        let mut last = Vec::new();
        for i in 1..=10 {
            d.frame(f64::from(i) / 30.0, None, 2.0);
            last = d.cpu_frame().expect("a picture");
        }
        assert_eq!(last.len(), 16 * 8 * 4);
        assert!(last.as_chunks::<4>().0.iter().all(|px| px[3] == 255), "opaque");
        assert!(last.as_chunks::<4>().0.iter().any(|px| px[0] > 0 || px[1] > 0 || px[2] > 0), "not black");
        // A resize keeps the pattern and the picture follows.
        d.use_cpu_picture(5, 8, 4);
        assert_eq!(d.cpu_size(), Some(SizeInt { width: 8, height: 4 }));
        d.frame(1.0, None, 2.0);
        assert_eq!(d.cpu_frame().expect("a picture").len(), 8 * 4 * 4);
    }

    #[test]
    fn two_pictures_from_one_seed_and_one_frame_are_the_same_picture() {
        // What the GPU-vs-CPU parity test does on a device, on the CPU twice:
        // the same spots and the same frames must give the same picture, or
        // the comparison the plan asks for would be meaningless.
        let (a, b) = (driver(), driver());
        a.use_cpu_picture(5, 24, 12);
        b.use_cpu_picture(5, 24, 12);
        let spots = a.reseed();
        a.cpu_seed(spots.clone());
        b.cpu_seed(spots);
        let mut last = (Vec::new(), Vec::new());
        for i in 1..=5 {
            let t = f64::from(i) / 30.0;
            a.frame(t, None, 2.0);
            b.frame(t, None, 2.0);
            last = (a.cpu_frame().unwrap(), b.cpu_frame().unwrap());
        }
        assert_eq!(last.0, last.1);
        // And a different seed is a different picture, so the test can fail.
        let c = driver();
        c.use_cpu_picture(9, 24, 12);
        c.cpu_seed(c.reseed());
        c.frame(1.0 / 30.0, None, 2.0);
        assert_ne!(c.cpu_frame().unwrap(), last.0);
    }

    #[test]
    fn the_sizes_are_the_cores() {
        assert_eq!(backing_store(640, 1080, 2400), SizeInt { width: 288, height: 640 });
        assert_eq!(sim_grid(512, 1000, 500), SizeInt { width: 512, height: 256 });
        assert_eq!(field_grid(512, 256), SizeInt { width: 256, height: 128 });
        let ladder = quality_ladder();
        assert_eq!(ladder.len(), 6);
        assert_eq!(ladder[0].index, 0);
        assert_eq!(ladder.last().unwrap().max_side, 0);
        assert_eq!(max_seed_spots(), 24);
        assert_eq!(max_ripples(), 4);
    }
}
