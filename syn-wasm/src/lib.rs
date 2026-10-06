//! The foreign interface of the core for JavaScript (synesthesia
//! PLAN-CORE.md, decision C3): the web app reaches the model through here,
//! as the Kotlin and Swift apps do through `syn-ffi`.
//!
//! Rules for this crate (`syn-ffi`'s, plus what a browser asks for):
//! - Thin. It converts types; logic goes into `syn-core`, `syn-player` or
//!   `syn-session`.
//! - **The audio side speaks numbers and byte arrays only.** An
//!   AudioWorklet's scope lacks `TextDecoder`/`TextEncoder` in some browsers,
//!   so [`AudioCore`] takes a point as UTF-8 bytes (decoded here) and hands
//!   samples and frames back as pointers into the module's memory, which the
//!   worklet reads through views it made once — nothing crosses as a string,
//!   and nothing is allocated per quantum on either side.
//! - Points cross as the web app's `AppState` v1 JSON; a malformed one is a
//!   `None` / `false`, never a panic.

pub mod picture;
pub mod session;
pub mod settings;

use syn_core::engine::SPECTRUM_BANDS;
use syn_core::state::AppState;
use syn_player::{Frame, Player};
use wasm_bindgen::prelude::*;

/// Numbers in one flattened frame (see [`AudioCore::frame_at`]): time,
/// peak, rms, limiter dB, loudness, swell, brightness, onset, low, mid,
/// high, hits, then the spectrum's bands (0..255 each).
pub const FRAME_LEN: usize = 12 + SPECTRUM_BANDS;

/// The core's version, so the page can say which model it runs.
#[wasm_bindgen(js_name = coreVersion)]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The built-in presets' names, in the web app's order.
#[wasm_bindgen(js_name = presetNames)]
pub fn preset_names() -> Vec<String> {
    syn_core::state::presets().iter().map(|p| p.name.clone()).collect()
}

/// A built-in preset's point as `AppState` v1 JSON, or `undefined` past the
/// end of the list.
#[wasm_bindgen(js_name = presetStateJson)]
pub fn preset_state_json(index: u32) -> Option<String> {
    let preset = syn_core::state::presets().get(index as usize)?;
    serde_json::to_string(&preset.state).ok()
}

/// [`FRAME_LEN`], for the JavaScript side's views.
#[wasm_bindgen(js_name = frameLen)]
pub fn frame_len() -> usize {
    FRAME_LEN
}

fn parse_point(utf8: &[u8]) -> Option<AppState> {
    serde_json::from_slice(utf8).ok()
}

/// The live sound, for one AudioWorklet: `syn_player::Player` with its
/// output and its frames kept in buffers the worklet views in place.
#[wasm_bindgen]
pub struct AudioCore {
    player: Player,
    out: Vec<f32>,
    frame: Vec<f64>,
}

#[wasm_bindgen]
impl AudioCore {
    /// A player for a point (UTF-8 JSON) at `sample_rate`, silent until
    /// [`AudioCore::fade_in`]; `undefined` when the bytes are not a point.
    /// `max_frames` is the largest render the caller will ask for (a Web
    /// Audio quantum is 128): the output buffer never grows past it.
    pub fn create(sample_rate: f64, point_utf8: &[u8], max_frames: usize) -> Option<AudioCore> {
        let state = parse_point(point_utf8)?;
        Some(AudioCore {
            player: Player::new(sample_rate, &state),
            out: Vec::with_capacity(max_frames),
            frame: vec![0.0; FRAME_LEN],
        })
    }

    /// Renders the next `frames` mono samples and returns where they are: a
    /// byte offset into the module's memory, `frames` × f32. Pending point
    /// changes and fades are applied first. The offset stays the same while
    /// `frames` ≤ `max_frames`; the view onto it must be remade only if the
    /// memory grew (its `buffer` changed).
    pub fn render(&mut self, frames: usize) -> usize {
        self.player.render_into(frames, &mut self.out);
        self.out.as_ptr() as usize
    }

    /// Glide to a point; `false` (and nothing changes) when the bytes are not one.
    #[wasm_bindgen(js_name = setPoint)]
    pub fn set_point(&self, point_utf8: &[u8]) -> bool {
        parse_point(point_utf8).map(|s| self.player.set_state(s)).is_some()
    }

    /// Switch to a point hard (the old one's tails are dropped); `false`
    /// when the bytes are not one.
    #[wasm_bindgen(js_name = switchTo)]
    pub fn switch_to(&self, point_utf8: &[u8]) -> bool {
        parse_point(point_utf8).map(|s| self.player.switch_to(s)).is_some()
    }

    #[wasm_bindgen(js_name = fadeIn)]
    pub fn fade_in(&self) {
        self.player.fade_in();
    }

    #[wasm_bindgen(js_name = fadeOut)]
    pub fn fade_out(&self) {
        self.player.fade_out();
    }

    /// True once a fade out is complete and handed out.
    #[wasm_bindgen(js_name = isSilent)]
    pub fn is_silent(&self) -> bool {
        self.player.is_silent()
    }

    /// Seconds of audio handed out so far — the LFO clock.
    pub fn time(&self) -> f64 {
        self.player.time()
    }

    /// The frame for what is heard at `time`, flattened ([`FRAME_LEN`] ×
    /// f64) into a buffer that lives as long as this object; returns its
    /// byte offset, or 0 before the first frame is published.
    #[wasm_bindgen(js_name = frameAt)]
    pub fn frame_at(&mut self, time: f64) -> usize {
        let f = self.player.frame_at(time);
        self.write_frame(f)
    }

    /// The newest frame, like [`AudioCore::frame_at`].
    #[wasm_bindgen(js_name = latestFrame)]
    pub fn latest_frame(&mut self) -> usize {
        let f = self.player.latest_frame();
        self.write_frame(f)
    }

    fn write_frame(&mut self, f: Option<Frame>) -> usize {
        let Some(f) = f else { return 0 };
        let a = f.features;
        let head = [
            f.time,
            f64::from(f.peak),
            f64::from(f.rms),
            f64::from(f.limiter_db),
            a.loudness,
            a.swell,
            a.brightness,
            a.onset,
            a.low,
            a.mid,
            a.high,
            f.hits as f64,
        ];
        self.frame[..head.len()].copy_from_slice(&head);
        for (d, s) in self.frame[head.len()..].iter_mut().zip(f.spectrum.iter()) {
            *d = f64::from(*s);
        }
        self.frame.as_ptr() as usize
    }
}

/// The output of the last render as a slice — for tests, which run where the
/// returned offsets are not addresses in a JavaScript-visible memory.
impl AudioCore {
    pub fn output(&self) -> &[f32] {
        &self.out
    }

    pub fn frame(&self) -> &[f64] {
        &self.frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(i: u32) -> Vec<u8> {
        preset_state_json(i).expect("a preset").into_bytes()
    }

    #[test]
    fn presets_are_listed_in_the_cores_order() {
        let names = preset_names();
        assert_eq!(names.len(), syn_core::state::presets().len());
        assert!(preset_state_json(names.len() as u32).is_none());
    }

    #[test]
    fn a_quantum_renders_in_place_without_growing_the_buffer() {
        let mut core = AudioCore::create(48000.0, &preset(0), 128).expect("a point");
        core.fade_in();
        let first = core.render(128);
        for _ in 0..400 {
            assert_eq!(core.render(128), first, "the output does not move");
        }
        assert_eq!(core.output().len(), 128);
        assert!(core.output().iter().any(|v| *v != 0.0), "the preset sounds");
        assert!((core.time() - 401.0 * 128.0 / 48000.0).abs() < 1e-12);
    }

    #[test]
    fn a_frame_is_flattened_in_the_documented_order() {
        let mut core = AudioCore::create(48000.0, &preset(0), 128).unwrap();
        assert_eq!(core.latest_frame(), 0, "no frame before the first render");
        core.fade_in();
        for _ in 0..400 {
            core.render(128);
        }
        assert_ne!(core.frame_at(0.5), 0);
        let f = core.frame();
        assert_eq!(f.len(), FRAME_LEN);
        assert!(f[0] <= 0.5 && f[0] > 0.47, "time {}", f[0]);
        assert!(f[1] >= f[2] && f[2] > 0.0, "peak ≥ rms > 0");
        assert!(f[12..].iter().all(|b| (0.0..=255.0).contains(b)));
    }

    #[test]
    fn a_malformed_point_is_refused_not_a_panic() {
        assert!(AudioCore::create(48000.0, b"{}", 128).is_none());
        assert!(AudioCore::create(48000.0, b"\xff", 128).is_none());
        let core = AudioCore::create(48000.0, &preset(0), 128).unwrap();
        assert!(!core.set_point(b"[]"));
        assert!(!core.switch_to(b"nope"));
        assert!(core.switch_to(&preset(5)));
    }
}
