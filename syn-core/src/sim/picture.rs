//! The whole picture behind one pair of calls: [`Visualizer::step`] on the
//! simulation clock, [`Picture::draw`] on the redraw clock. It does what the
//! web app's frame loop does between the features and the screen — onset
//! hits become growth and ripples, the point becomes this frame's params —
//! and it is what both the live field thread and `render --picture` run.

use super::display::{render, Image};
use super::driver::Driver;
use super::frame::FrameParams;
use super::{grid_for_pixels, Sim};
use crate::visualizer::{Visualizer, VizInput};

pub struct Picture {
    sim: Sim,
    /// What each frame does — the same decisions a GPU renderer makes
    /// ([`Driver`]).
    driver: Driver,
    image: Image,
    frame: Option<FrameParams>,
}

impl Picture {
    /// A picture of `w`×`h` pixels on the grid [`grid_for_pixels`] picks.
    pub fn new(w: usize, h: usize, seed: u32) -> Self {
        let (gw, gh) = grid_for_pixels(w, h);
        Self { sim: Sim::new(gw, gh, seed), driver: Driver::new(seed), image: Image::new(w, h), frame: None }
    }

    pub fn sim(&self) -> &Sim {
        &self.sim
    }

    /// A new size in pixels; the grid follows and the pattern is kept.
    pub fn resize(&mut self, w: usize, h: usize) {
        if (w, h) == (self.image.w, self.image.h) {
            return;
        }
        let (gw, gh) = grid_for_pixels(w, h);
        self.sim.resize(gw, gh);
        self.image = Image::new(w, h);
    }

    /// The picture as of the last step, with the ripples alive at `t` — the
    /// redraw clock, which runs faster than the simulation's.
    pub fn draw(&mut self, t: f64) -> &Image {
        if let Some(f) = self.frame {
            let ripples = self.driver.ripples_at(t);
            render(self.sim.field(), &f.palette, &f.fx, &ripples, &mut self.image);
        }
        &self.image
    }
}

impl Visualizer for Picture {
    /// What the driver asks for — onset hits as fresh growth, a finger's
    /// stamps — goes into the field, and then the field advances under the
    /// point's params as the LFOs and the sound have them.
    fn step(&mut self, input: &VizInput) {
        let frame = self.driver.frame(input, self.sim.field().aspect());
        for i in &frame.injects {
            self.sim.inject(i.x, i.y, i.radius, i.amount);
        }
        self.sim.step(&frame.params.sim);
        self.frame = Some(frame.params);
    }

    fn reseed(&mut self) {
        let seed = self.driver.reseed();
        self.sim.reseed_with(&seed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::AudioFeatures;
    use crate::state::presets;

    #[test]
    fn hits_grow_the_field_and_ripple_the_picture() {
        let state = &presets()[0].state;
        let quiet = AudioFeatures { brightness: 0.5, ..Default::default() };
        let (mut a, mut b) = (Picture::new(24, 12, 4), Picture::new(24, 12, 4));
        let input = |hits, time| VizInput { state, features: quiet, hits, time };
        for (i, p) in [&mut a, &mut b].into_iter().enumerate() {
            p.step(&input(10, 0.0));
            // The first step only learns the counter; b then sees two hits.
            p.step(&input(10 + 2 * i as u64, 1.0 / 30.0));
        }
        assert_ne!(a.sim().field().v(), b.sim().field().v());
        assert_ne!(a.draw(0.05), b.draw(0.05));
    }

    #[test]
    fn a_resize_keeps_the_pattern_and_the_picture_follows() {
        let state = &presets()[5].state;
        let mut p = Picture::new(30, 16, 2);
        for i in 0..20 {
            p.step(&VizInput {
                state,
                features: AudioFeatures::default(),
                hits: 0,
                time: f64::from(i) / 30.0,
            });
        }
        p.resize(60, 32);
        assert_eq!((p.sim().field().width(), p.sim().field().height()), (120, 64));
        let v = p.sim().field().v();
        assert!(v.iter().any(|&x| x > 0.1), "the pattern survived the resize");
        p.step(&VizInput { state, features: AudioFeatures::default(), hits: 0, time: 1.0 });
        let img = p.draw(1.0);
        assert_eq!((img.w, img.h, img.rgb.len()), (60, 32, 60 * 32));
    }

    #[test]
    fn nothing_is_drawn_before_the_first_step() {
        let mut p = Picture::new(8, 4, 1);
        assert!(p.draw(0.0).rgb.iter().all(|&c| c == [0, 0, 0]));
    }
}
