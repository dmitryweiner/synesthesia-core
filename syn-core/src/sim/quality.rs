//! How big to render, measured rather than guessed — ported from the web
//! app's `src/sim/quality.ts` so that every platform asks the device the same
//! question (synesthesia-android PLAN.md, decision 4).
//!
//! Two independent costs decide a frame: the simulation grid (`res`, paid
//! once per reaction substep) and the surface the display pass covers (which
//! does **not** shrink when `res` does). The numbers behind the ladder, from
//! a software rasterizer at 1920×1080 on preset 0 (16 substeps a frame), ms
//! per pass:
//!
//! ```text
//!   res 1024 / grid 1024x576   fields 120   react x16 1406   display 238
//!   res  512 / grid  512x288   fields  38   react x16  373   display 248
//!   res  256 / grid  256x144   fields  13   react x16  101   display 248
//! ```
//!
//! The reaction is 80% of the first row and the noise fields are 7%; with
//! the grid made negligible, the surface alone costs 262 ms at 1920×1031 and
//! 46 ms at 640×271. Both axes have to come down together, which is what the
//! ladder does.
//!
//! The web app's lesson, kept: a device test (`min(width, height) < 700`) is
//! not a measurement. A 1080p board with no GPU passes it and then renders
//! at 0.57 fps.

/// The grid's smallest side, and the floor for the half-size fields.
const MIN_SIDE: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualityRung {
    /// Cap on the longest side of the drawing surface, in device pixels.
    /// 0 = uncapped.
    pub max_side: usize,
    /// The simulation grid's long side; [`grid_size`] fits the short one to
    /// the surface's aspect.
    pub res: usize,
}

/// About 2× the work per step between neighbours (surface area ~1.5×, grid
/// area ~1.8–2.2×), so a rung that misses the budget misses it by about a
/// factor of two — never by the 28× that separates the ends.
pub const QUALITY_LADDER: [QualityRung; 6] = [
    QualityRung { max_side: 640, res: 192 },
    QualityRung { max_side: 840, res: 256 },
    QualityRung { max_side: 1080, res: 384 },
    QualityRung { max_side: 1280, res: 512 },
    QualityRung { max_side: 1600, res: 768 },
    QualityRung { max_side: 0, res: 1024 },
];

pub const TOP_RUNG: usize = QUALITY_LADDER.len() - 1;

/// A rung is accepted at 20 fps, not at the 15 fps floor actually wanted:
/// the probe runs before any sound does, and the audio thread, the features
/// and the scout all take their cut afterwards.
pub const PROBE_BUDGET_MS: f64 = 50.0;

/// The drawing surface for a view of `w`×`h` device pixels: the long side
/// capped by the rung. The view keeps its own size either way — the upscale
/// is a blit, and a blit is the one thing a device with no GPU is good at.
pub fn backing_store(max_side: usize, w: usize, h: usize) -> (usize, usize) {
    let (w, h) = (w.max(1), h.max(1));
    let long = w.max(h);
    if max_side == 0 || long <= max_side {
        return (w, h);
    }
    let k = max_side as f64 / long as f64;
    (((w as f64 * k).round() as usize).max(1), ((h as f64 * k).round() as usize).max(1))
}

/// The simulation grid for a surface of `w`×`h`: long side `res`, short side
/// scaled by the aspect, so texels are square on screen (`gridSize`). A
/// square grid on a tall phone surface would stretch every blob vertically.
pub fn grid_size(res: usize, w: usize, h: usize) -> (usize, usize) {
    if w == 0 || h == 0 {
        return (res, res);
    }
    let aspect = w as f64 / h as f64;
    if aspect >= 1.0 {
        (res, ((res as f64 / aspect).round() as usize).max(MIN_SIDE))
    } else {
        (((res as f64 * aspect).round() as usize).max(MIN_SIDE), res)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeOptions {
    /// A rung is kept if its median frame is no slower than this.
    pub budget_ms: f64,
    /// Frames dropped after a rung is applied (the first one reallocates
    /// every texture).
    pub warmup: usize,
    /// Frames the median is taken over.
    pub samples: usize,
    /// One frame this many times over budget fails the rung without waiting
    /// for a full sample — but never the first frame measured at a rung, so
    /// a boot hiccup cannot pin a fast device at the bottom of the ladder.
    pub abort_factor: f64,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self { budget_ms: PROBE_BUDGET_MS, warmup: 2, samples: 5, abort_factor: 3.0 }
    }
}

/// Picks a rung by rendering at it.
///
/// It walks **up** from the cheapest rung: the cost of guessing wrong is then
/// one frame of the rung above the device's ceiling (~2× the budget), where
/// walking down from the top would cost one frame at full quality — 2.6 s on
/// the machine the web app's version was written for. The picture is visibly
/// coarse for the first few hundred milliseconds and then sharpens, which
/// beats a blank screen.
///
/// Boot-time only (agreed with the user): once [`QualityProbe::done`], the
/// rung never moves again, so the picture cannot degrade under the viewer
/// mid-listen.
#[derive(Clone, Debug)]
pub struct QualityProbe {
    rung: usize,
    done: bool,
    opts: ProbeOptions,
    /// The best rung that fit, or `None` while none has.
    best: Option<usize>,
    seen: usize,
    times: Vec<f64>,
}

impl Default for QualityProbe {
    fn default() -> Self {
        Self::new(ProbeOptions::default())
    }
}

impl QualityProbe {
    pub fn new(opts: ProbeOptions) -> Self {
        Self { rung: 0, done: false, opts, best: None, seen: 0, times: Vec::new() }
    }

    /// A probe that measures nothing: the rung it is given is the rung it
    /// keeps (a test, or a device the user has settled by hand).
    pub fn fixed(rung: usize) -> Self {
        Self { rung: rung.min(TOP_RUNG), done: true, ..Self::default() }
    }

    pub fn rung(&self) -> usize {
        self.rung
    }

    pub fn done(&self) -> bool {
        self.done
    }

    pub fn quality(&self) -> QualityRung {
        QUALITY_LADDER[self.rung.min(TOP_RUNG)]
    }

    /// Feeds one frame's duration, unclamped: a 2 s frame must read as 2 s.
    /// Returns true when the rung changed and the caller must apply it
    /// (resize the surface and the grid).
    pub fn frame(&mut self, ms: f64) -> bool {
        if self.done {
            return false;
        }
        self.seen += 1;
        if self.seen <= self.opts.warmup {
            return false;
        }
        self.times.push(ms);
        if self.times.len() > 1 && ms > self.opts.budget_ms * self.opts.abort_factor {
            return self.settle();
        }
        if self.times.len() < self.opts.samples {
            return false;
        }
        let mut sorted = self.times.clone();
        sorted.sort_by(f64::total_cmp);
        if sorted[sorted.len() / 2] > self.opts.budget_ms {
            return self.settle();
        }
        self.best = Some(self.rung);
        if self.rung == TOP_RUNG {
            return self.settle();
        }
        self.rung += 1;
        self.seen = 0;
        self.times.clear();
        true
    }

    /// Stops at the best rung that fit — or the cheapest one, if none did.
    fn settle(&mut self) -> bool {
        self.done = true;
        let keep = self.best.unwrap_or(0);
        if keep == self.rung {
            return false;
        }
        self.rung = keep;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ladder_rises_in_both_costs_and_ends_uncapped() {
        for pair in QUALITY_LADDER.windows(2) {
            assert!(pair[1].res > pair[0].res, "the grid grows");
            assert!(pair[1].max_side > pair[0].max_side || pair[1].max_side == 0, "so does the surface");
        }
        assert_eq!(QUALITY_LADDER[TOP_RUNG].max_side, 0, "the top rung is what every device got before");
    }

    #[test]
    fn a_surface_is_capped_on_its_long_side_and_keeps_its_shape() {
        // A 1080x2400 phone at the 640 rung.
        let (w, h) = backing_store(640, 1080, 2400);
        assert_eq!((w, h), (288, 640));
        assert!((w as f64 / h as f64 - 1080.0 / 2400.0).abs() < 0.01, "the aspect survives");
        assert_eq!(backing_store(640, 400, 800), (320, 640));
        assert_eq!(backing_store(1600, 1080, 2400), (720, 1600));
        assert_eq!(backing_store(0, 1080, 2400), (1080, 2400), "uncapped");
        assert_eq!(backing_store(640, 320, 480), (320, 480), "already small enough");
        assert_eq!(backing_store(640, 0, 0), (1, 1), "a surface with no size yet");
    }

    #[test]
    fn the_grid_follows_the_surfaces_aspect() {
        assert_eq!(grid_size(512, 1000, 500), (512, 256));
        assert_eq!(grid_size(512, 500, 1000), (256, 512));
        assert_eq!(grid_size(512, 800, 800), (512, 512));
        assert_eq!(grid_size(64, 4000, 100), (64, MIN_SIDE), "never thinner than the floor");
        assert_eq!(grid_size(256, 0, 0), (256, 256));
    }

    #[test]
    fn the_probe_walks_up_while_the_frames_fit_and_stays_at_the_top() {
        let mut p = QualityProbe::default();
        assert_eq!(p.rung(), 0);
        let mut moves = 0;
        // Every frame comfortably inside the budget: it should climb to the top.
        for _ in 0..200 {
            if p.frame(1.0) {
                moves += 1;
            }
            if p.done() {
                break;
            }
        }
        assert!(p.done());
        assert_eq!(p.rung(), TOP_RUNG);
        assert_eq!(moves, TOP_RUNG, "one move per rung, and none off the end");
        assert!(!p.frame(10_000.0), "a settled probe never moves again");
        assert_eq!(p.rung(), TOP_RUNG);
    }

    #[test]
    fn a_device_that_misses_the_budget_keeps_the_rung_below() {
        let mut p = QualityProbe::default();
        let o = ProbeOptions::default();
        // Rung 0 fits, rung 1 does not.
        for _ in 0..o.warmup + o.samples {
            p.frame(10.0);
        }
        assert_eq!(p.rung(), 1);
        for _ in 0..o.warmup + o.samples {
            p.frame(o.budget_ms * 1.5);
        }
        assert!(p.done());
        assert_eq!(p.rung(), 0, "back to the one that fit");
    }

    #[test]
    fn a_device_that_cannot_hold_the_cheapest_rung_stays_on_it() {
        let mut p = QualityProbe::default();
        for _ in 0..40 {
            p.frame(900.0);
        }
        assert!(p.done());
        assert_eq!(p.rung(), 0, "the cheapest rung is the floor");
    }

    #[test]
    fn one_very_slow_frame_fails_a_rung_without_waiting_but_never_the_first() {
        let o = ProbeOptions::default();
        let mut p = QualityProbe::default();
        for _ in 0..o.warmup {
            p.frame(1.0);
        }
        // The first measured frame at a rung is a boot hiccup, not a verdict.
        assert!(!p.frame(o.budget_ms * 10.0));
        assert!(!p.done());
        // The second one is.
        p.frame(o.budget_ms * 10.0);
        assert!(p.done());
        assert_eq!(p.rung(), 0);
    }

    #[test]
    fn a_fixed_probe_keeps_what_it_was_given() {
        let p = QualityProbe::fixed(2);
        assert!(p.done());
        assert_eq!(p.rung(), 2);
        assert_eq!(p.quality(), QUALITY_LADDER[2]);
        assert_eq!(QualityProbe::fixed(99).rung(), TOP_RUNG);
    }
}
