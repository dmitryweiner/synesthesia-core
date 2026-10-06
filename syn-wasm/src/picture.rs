//! The picture for the web app's main thread (synesthesia PLAN-CORE.md
//! phase 5): `syn-ffi`'s `PictureDriver`, for JavaScript. The WebGL2
//! renderer stays the web app's — ping-pong targets and seven draw calls —
//! and everything it would otherwise decide for itself (what an onset hit
//! becomes, where a finger's stamps go, which ripples are alive, how far the
//! noise has drifted, what clock the LFOs are on, which rung of the quality
//! ladder this device holds) comes from the core's `sim::driver` and
//! `sim::quality`, as JSON named after the uniforms it fills.
//!
//! The CPU picture (C8, for a browser without WebGL2 float targets) draws
//! the very frame the driver handed out, into RGBA bytes for a 2D canvas.

use serde_json::{json, Value};
use syn_core::sim::driver::{Driver, Frame};
use syn_core::sim::field::Seed;
use syn_core::sim::quality::{self, ProbeOptions, QualityProbe, QualityRung, QUALITY_LADDER, TOP_RUNG};
use syn_core::sim::Picture;
use syn_core::state::AppState;
use syn_core::visualizer::VizInput;
use syn_core::AudioFeatures;
use wasm_bindgen::prelude::*;

fn parse_point(json: &str) -> Result<AppState, JsError> {
    serde_json::from_str(json).map_err(|e| JsError::new(&format!("not a point: {e}")))
}

fn rung_json(index: usize) -> Value {
    let i = index.min(TOP_RUNG);
    let QualityRung { max_side, res } = QUALITY_LADDER[i];
    json!({ "index": i, "maxSide": max_side, "res": res })
}

fn frame_json(f: &Frame) -> String {
    let p = &f.params;
    let (r, v, fl, pal, fx) = (&p.sim.reaction, &p.sim.field_variation, &p.sim.flow, &p.palette, &p.fx);
    json!({
        "time": f.time,
        "evolveT": f.evolve_t,
        "injects": f.injects.iter().map(|i| [i.x, i.y, i.radius, i.amount]).collect::<Vec<_>>(),
        "ripples": f.ripples.iter().map(|r| [r.x, r.y, r.age, r.amp]).collect::<Vec<_>>(),
        "reaction": {
            "feed": r.feed, "kill": r.kill, "diffU": r.diff_u, "diffV": r.diff_v, "substeps": r.substeps(),
        },
        "fieldVariation": {
            "feedAmount": v.feed_var_amount, "feedScale": v.feed_var_scale, "feedWarp": v.feed_var_warp,
            "killAmount": v.kill_var_amount, "killScale": v.kill_var_scale, "killWarp": v.kill_var_warp,
            "active": v.active(),
        },
        "flow": {
            "curlStrength": fl.curl_strength, "curlScale": fl.curl_scale,
            // the canvas is Y-up, so "down" — a positive Drift Y — is -Y
            "driftX": fl.drift_x, "driftY": -fl.drift_y,
            "advectAmount": fl.advect_amount, "advecting": fl.advect_active(),
        },
        "palette": {
            "a": pal.a, "b": pal.b, "c": pal.c, "d": pal.d,
            "bands": pal.bands, "relief": pal.relief, "gloss": pal.gloss,
            "lightDir": syn_core::sim::display::light_dir(pal.light_angle),
        },
        "display": { "exposure": fx.exposure, "flash": fx.flash, "tint": fx.tint },
    })
    .to_string()
}

/// The picture's per-frame driver, its quality probe, and (on demand) the
/// CPU picture.
#[wasm_bindgen]
pub struct WebPicture {
    driver: Driver,
    point: AppState,
    probe: QualityProbe,
    hits: u64,
    last: Option<Frame>,
    cpu: Option<Picture>,
}

#[wasm_bindgen]
impl WebPicture {
    /// A driver for a point. With `measure`, the first frames pick a rung of
    /// the quality ladder ([`WebPicture::probe_frame`]); without it,
    /// `fixed_rung` is kept and nothing is measured.
    #[wasm_bindgen(constructor)]
    pub fn new(seed: u32, point_json: &str, measure: bool, fixed_rung: u32) -> Result<WebPicture, JsError> {
        Ok(WebPicture {
            driver: Driver::new(seed),
            point: parse_point(point_json)?,
            probe: if measure {
                QualityProbe::new(ProbeOptions::default())
            } else {
                QualityProbe::fixed((fixed_rung as usize).min(TOP_RUNG))
            },
            hits: 0,
            last: None,
            cpu: None,
        })
    }

    /// The point the picture is of — the session's `setPoint` / `switchTo`.
    #[wasm_bindgen(js_name = setPoint)]
    pub fn set_point(&mut self, point_json: &str) -> Result<(), JsError> {
        self.point = parse_point(point_json)?;
        Ok(())
    }

    /// One frame, as JSON: `{time, evolveT, injects: [[x, y, radius,
    /// amount]], ripples: [[x, y, age, amp]], reaction, fieldVariation,
    /// flow, palette, display}`. `now` is the page's clock in seconds;
    /// `sound` the frame being heard — `[time, loudness, swell, brightness,
    /// onset, low, mid, high, hits]` — or empty when nothing plays (the LFOs
    /// then carry on from where the sound left them). `aspect` is the grid's
    /// width / height.
    pub fn frame(&mut self, now: f64, sound: &[f64], aspect: f32) -> String {
        let heard = (sound.len() >= 9).then_some(sound);
        let time = self.driver.lfo_time(now, heard.map(|s| s[0]));
        let features = heard.map_or_else(AudioFeatures::default, |s| AudioFeatures {
            loudness: s[1],
            swell: s[2],
            brightness: s[3],
            onset: s[4],
            low: s[5],
            mid: s[6],
            high: s[7],
        });
        if let Some(s) = heard {
            self.hits = s[8] as u64;
        }
        let input = VizInput { state: &self.point, features, hits: self.hits, time };
        let frame = self.driver.frame(&input, aspect);
        let out = frame_json(&frame);
        self.last = Some(frame);
        out
    }

    /// A fresh start, as `{xy: [x0, y0, x1, y1, …], count, radius}` for
    /// `seed.frag` (both halves of the ping-pong) — the session's `reseed`.
    pub fn reseed(&mut self) -> String {
        let seed = self.driver.reseed();
        let xy: Vec<f32> = seed.spots.iter().flat_map(|(x, y)| [*x, *y]).collect();
        json!({ "xy": xy, "count": seed.spots.len(), "radius": seed.radius }).to_string()
    }

    /// A finger lands (UV, Y-up): it stamps where it landed and sends a
    /// ripple out from there.
    #[wasm_bindgen(js_name = pointerDown)]
    pub fn pointer_down(&mut self, x: f32, y: f32, now: f64) {
        let t = self.driver.lfo_time(now, None);
        self.driver.pointer_down(x, y, t);
    }

    /// The finger moved; the stamps are laid on the next frame.
    #[wasm_bindgen(js_name = pointerMoved)]
    pub fn pointer_moved(&mut self, x: f32, y: f32) {
        self.driver.pointer_moved(x, y);
    }

    #[wasm_bindgen(js_name = pointerUp)]
    pub fn pointer_up(&mut self) {
        self.driver.pointer_up();
    }

    pub fn painting(&self) -> bool {
        self.driver.painting()
    }

    /// One real frame time for the boot probe, unclamped. True when the rung
    /// changed: resize the canvas and the grid.
    #[wasm_bindgen(js_name = probeFrame)]
    pub fn probe_frame(&mut self, ms: f64) -> bool {
        self.probe.frame(ms)
    }

    /// `{index, maxSide, res}` of the rung this device is on.
    pub fn rung(&self) -> String {
        rung_json(self.probe.rung()).to_string()
    }

    #[wasm_bindgen(js_name = probeDone)]
    pub fn probe_done(&self) -> bool {
        self.probe.done()
    }

    // --- the CPU picture (C8) ----------------------------------------------

    /// Draws on the CPU from here on, at `width`×`height` cells.
    #[wasm_bindgen(js_name = useCpu)]
    pub fn use_cpu(&mut self, seed: u32, width: u32, height: u32) {
        match &mut self.cpu {
            Some(p) => p.resize(width as usize, height as usize),
            None => self.cpu = Some(Picture::new(width as usize, height as usize, seed)),
        }
    }

    /// Seeds the CPU field from the spots [`WebPicture::reseed`] returned.
    #[wasm_bindgen(js_name = cpuSeed)]
    pub fn cpu_seed(&mut self, xy: &[f32], radius: f32) {
        let seed = Seed { spots: xy.as_chunks::<2>().0.iter().map(|p| (p[0], p[1])).collect(), radius };
        if let Some(p) = &mut self.cpu {
            p.seed_with(&seed);
        }
    }

    /// The CPU picture of the last frame, RGBA, row 0 at the top; empty
    /// before [`WebPicture::use_cpu`] and a frame.
    #[wasm_bindgen(js_name = cpuFrame)]
    pub fn cpu_frame(&mut self) -> Vec<u8> {
        let (Some(frame), Some(cpu)) = (self.last.as_ref(), self.cpu.as_mut()) else { return Vec::new() };
        let image = cpu.render_frame(frame);
        image.rgb.iter().flat_map(|px| [px[0], px[1], px[2], 255]).collect()
    }

    /// The CPU field's grid, `[width, height]` (it follows the pixels, at
    /// its own density); empty before [`WebPicture::use_cpu`].
    #[wasm_bindgen(js_name = cpuGrid)]
    pub fn cpu_grid(&self) -> Vec<u32> {
        self.cpu.as_ref().map_or_else(Vec::new, |p| {
            let f = p.sim().field();
            vec![f.width() as u32, f.height() as u32]
        })
    }

    /// The CPU field's V channel ("ink"), row by row — what the GPU's
    /// readState() gives, for the test that compares the two.
    #[wasm_bindgen(js_name = cpuInk)]
    pub fn cpu_ink(&self) -> Vec<f32> {
        self.cpu.as_ref().map(|p| p.sim().field().v().to_vec()).unwrap_or_default()
    }
}

/// The ladder, cheapest rung first, as JSON `[{index, maxSide, res}]`.
#[wasm_bindgen(js_name = qualityLadder)]
pub fn quality_ladder() -> String {
    Value::Array((0..QUALITY_LADDER.len()).map(rung_json).collect()).to_string()
}

/// The canvas for a view of `width`×`height` device pixels at a rung's cap.
#[wasm_bindgen(js_name = backingStore)]
pub fn backing_store(max_side: u32, width: u32, height: u32) -> Vec<u32> {
    let (w, h) = quality::backing_store(max_side as usize, width as usize, height as usize);
    vec![w as u32, h as u32]
}

/// The simulation grid for a canvas: long side `res`, the short side fitted.
#[wasm_bindgen(js_name = simGrid)]
pub fn sim_grid(res: u32, width: u32, height: u32) -> Vec<u32> {
    let (w, h) = quality::grid_size(res as usize, width as usize, height as usize);
    vec![w as u32, h as u32]
}

/// The paramfield and velocity textures' grid: half each side.
#[wasm_bindgen(js_name = fieldGrid)]
pub fn field_grid(width: u32, height: u32) -> Vec<u32> {
    let (w, h) = syn_core::sim::fields::half_size(width as usize, height as usize);
    vec![w as u32, h as u32]
}

#[wasm_bindgen(js_name = maxSeedSpots)]
pub fn max_seed_spots() -> u32 {
    syn_core::sim::MAX_SPOTS as u32
}

#[wasm_bindgen(js_name = maxRipples)]
pub fn max_ripples() -> u32 {
    syn_core::sim::coupling::MAX_RIPPLES as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset_json(i: usize) -> String {
        serde_json::to_string(&syn_core::state::presets()[i].state).unwrap()
    }

    #[test]
    fn a_frame_carries_every_uniform_the_passes_need() {
        let mut p = WebPicture::new(3, &preset_json(0), false, 2).unwrap();
        let f: Value = serde_json::from_str(&p.frame(1.0, &[], 1.5)).unwrap();
        for k in [
            "time",
            "evolveT",
            "injects",
            "ripples",
            "reaction",
            "fieldVariation",
            "flow",
            "palette",
            "display",
        ] {
            assert!(f.get(k).is_some(), "{k}");
        }
        assert!(f["reaction"]["substeps"].as_u64().unwrap() >= 1);
        assert_eq!(f["palette"]["lightDir"].as_array().unwrap().len(), 3);
        assert_eq!(serde_json::from_str::<Value>(&p.rung()).unwrap()["index"], 2);
    }

    #[test]
    fn a_hit_heard_seeds_growth_and_a_finger_stamps() {
        let mut p = WebPicture::new(3, &preset_json(10), false, 0).unwrap();
        let heard = |hits: f64, t: f64| vec![t, 0.5, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0, hits];
        p.frame(1.0, &heard(0.0, 1.0), 1.0); // learns the counter
        let f: Value = serde_json::from_str(&p.frame(1.02, &heard(2.0, 1.02), 1.0)).unwrap();
        assert_eq!(f["injects"].as_array().unwrap().len(), 2);
        assert_eq!(f["time"], 1.02, "the LFOs run on the heard clock");
        p.pointer_down(0.5, 0.5, 2.0);
        p.pointer_moved(0.9, 0.5);
        let f: Value = serde_json::from_str(&p.frame(2.02, &[], 1.0)).unwrap();
        assert!(!f["injects"].as_array().unwrap().is_empty());
    }

    #[test]
    fn the_cpu_picture_draws_the_frame_the_driver_handed_out() {
        let mut p = WebPicture::new(5, &preset_json(0), false, 0).unwrap();
        p.use_cpu(9, 48, 32);
        let seed: Value = serde_json::from_str(&p.reseed()).unwrap();
        let xy: Vec<f32> = serde_json::from_value(seed["xy"].clone()).unwrap();
        p.cpu_seed(&xy, seed["radius"].as_f64().unwrap() as f32);
        assert!(p.cpu_frame().is_empty(), "nothing before a frame");
        p.frame(0.0, &[], 1.5);
        assert_eq!(p.cpu_frame().len(), 48 * 32 * 4);
        let g = p.cpu_grid();
        assert_eq!(p.cpu_ink().len(), (g[0] * g[1]) as usize);
    }
}
