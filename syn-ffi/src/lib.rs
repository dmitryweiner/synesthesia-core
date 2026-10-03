//! The foreign interface of the core (synesthesia-android PLAN.md,
//! decisions 1–2): everything an app reaches in the model goes through here,
//! and the Kotlin and Swift bindings are generated from these declarations.
//!
//! Rules for this crate:
//! - Thin. Logic belongs in `syn-core` (the model) or `syn-session` (the
//!   control logic); this crate converts types and nothing else.
//! - The types it exposes are records and plain values a UI can hold, never
//!   `syn-core`'s internals.
//! - It grows by phase: presets, the schema and the live player now; the
//!   session and the picture as the Android plan reaches them.
//! - Points cross as the web app's `AppState` JSON; a malformed one is a
//!   [`CoreError`], never a panic.

use std::sync::{Arc, Mutex};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("not a point: {reason}")]
    InvalidPoint { reason: String },
}

fn parse_point(json: &str) -> Result<syn_core::AppState, CoreError> {
    serde_json::from_str(json).map_err(|e| CoreError::InvalidPoint { reason: e.to_string() })
}

/// One built-in preset as a list shows it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PresetInfo {
    /// Position in the built-in list — the web app's `?preset=N`.
    pub index: u32,
    pub name: String,
}

/// The core's version, so an app can say which model it runs.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The built-in presets, in the web app's order.
#[uniffi::export]
pub fn presets() -> Vec<PresetInfo> {
    syn_core::state::presets()
        .iter()
        .enumerate()
        .map(|(i, p)| PresetInfo { index: i as u32, name: p.name.clone() })
        .collect()
}

/// A built-in preset's point as the web app's `AppState` v1 JSON, or `None`
/// when there is no preset at that index.
#[uniffi::export]
pub fn preset_state_json(index: u32) -> Option<String> {
    let preset = syn_core::state::presets().get(index as usize)?;
    serde_json::to_string(&preset.state).ok()
}

/// The parameter schema (ranges, defaults, labels, the gene list) exactly as
/// it was dumped from the web app. Typed records replace this when the
/// Settings page needs them.
#[uniffi::export]
pub fn schema_json() -> String {
    syn_core::schema::SCHEMA_JSON.to_string()
}

/// What the meters and the picture read about the sound at one moment
/// (syn-player's frame, flattened for the app).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AudioFrame {
    /// Engine time, seconds — the LFO clock.
    pub time: f64,
    pub peak: f32,
    pub rms: f32,
    /// How hard the limiter is holding the level down, dB (≤ 0).
    pub limiter_db: f32,
    /// 0..1, smoothed RMS.
    pub loudness: f64,
    /// -1..1, loudness against its ~4 s average.
    pub swell: f64,
    /// 0..1, spectral centroid on a log-frequency scale.
    pub brightness: f64,
    /// 0..1, decaying spike on spectral energy jumps.
    pub onset: f64,
    pub low: f64,
    pub mid: f64,
    pub high: f64,
    /// Onset hits since the player started.
    pub hits: u64,
    /// 64 log-spaced bands, 0..255.
    pub spectrum: Vec<u8>,
}

impl From<syn_player::Frame> for AudioFrame {
    fn from(f: syn_player::Frame) -> Self {
        let a = f.features;
        AudioFrame {
            time: f.time,
            peak: f.peak,
            rms: f.rms,
            limiter_db: f.limiter_db,
            loudness: a.loudness,
            swell: a.swell,
            brightness: a.brightness,
            onset: a.onset,
            low: a.low,
            mid: a.mid,
            high: a.high,
            hits: f.hits,
            spectrum: f.spectrum.to_vec(),
        }
    }
}

/// The live sound of one session. One audio thread calls [`SoundPlayer::render`];
/// any other thread may change the point or read frames at the same time.
#[derive(uniffi::Object)]
pub struct SoundPlayer {
    inner: syn_player::Player,
    scratch: Mutex<Vec<f32>>,
}

#[uniffi::export]
impl SoundPlayer {
    /// A player for a point at `sample_rate`. It starts silent: call
    /// [`SoundPlayer::fade_in`].
    #[uniffi::constructor]
    pub fn new(sample_rate: u32, point_json: String) -> Result<Arc<Self>, CoreError> {
        let state = parse_point(&point_json)?;
        Ok(Arc::new(SoundPlayer {
            inner: syn_player::Player::new(f64::from(sample_rate), &state),
            scratch: Mutex::new(Vec::new()),
        }))
    }

    /// The next `frames` mono samples as 32-bit float, little-endian — the
    /// layout of a float PCM buffer.
    pub fn render(&self, frames: u32) -> Vec<u8> {
        let mut buf = self.scratch.lock().unwrap_or_else(|e| e.into_inner());
        self.inner.render_into(frames as usize, &mut buf);
        let mut bytes = Vec::with_capacity(buf.len() * 4);
        for s in buf.iter() {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        bytes
    }

    /// Glide to a point: parameters move, nothing is rebuilt.
    pub fn set_point(&self, point_json: String) -> Result<(), CoreError> {
        self.inner.set_state(parse_point(&point_json)?);
        Ok(())
    }

    /// Switch to a point hard: the old one's reverb and delay tails are dropped.
    pub fn switch_to(&self, point_json: String) -> Result<(), CoreError> {
        self.inner.switch_to(parse_point(&point_json)?);
        Ok(())
    }

    pub fn fade_in(&self) {
        self.inner.fade_in();
    }

    pub fn fade_out(&self) {
        self.inner.fade_out();
    }

    /// True once a fade out is complete and handed out.
    pub fn is_silent(&self) -> bool {
        self.inner.is_silent()
    }

    /// Seconds of audio handed out so far.
    pub fn time(&self) -> f64 {
        self.inner.time()
    }

    pub fn sample_rate(&self) -> u32 {
        self.inner.sample_rate() as u32
    }

    /// The frame for what is heard at `time` (the player's clock): the app
    /// passes its device's played position, not [`SoundPlayer::time`].
    pub fn frame_at(&self, time: f64) -> Option<AudioFrame> {
        self.inner.frame_at(time).map(AudioFrame::from)
    }

    /// The newest frame, whatever has been heard of it.
    pub fn latest_frame(&self) -> Option<AudioFrame> {
        self.inner.latest_frame().map(AudioFrame::from)
    }
}

/// Loudness of an offline render, for the bench.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RenderStats {
    pub seconds: f64,
    pub rms: f64,
    pub peak: f64,
}

/// Renders `seconds` of a point offline at `sample_rate` — the live player's
/// sound, without a device — and measures its level. The caller times it.
#[uniffi::export]
pub fn render_stats(point_json: String, seconds: f64, sample_rate: u32) -> Result<RenderStats, CoreError> {
    let state = parse_point(&point_json)?;
    let st = syn_player::render_stats(&state, seconds, f64::from(sample_rate));
    Ok(RenderStats { seconds: st.seconds, rms: st.rms, peak: st.peak })
}

/// Entry point of the bindings generator, for an app's own
/// `uniffi-bindgen` binary (see this crate's `cli` feature).
#[cfg(feature = "cli")]
pub fn uniffi_bindgen_main() {
    uniffi::uniffi_bindgen_main()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_twelve_presets_are_listed_in_order() {
        let list = presets();
        assert_eq!(list.len(), 12);
        for (i, p) in list.iter().enumerate() {
            assert_eq!(p.index as usize, i);
            assert!(!p.name.is_empty());
        }
    }

    #[test]
    fn a_preset_comes_back_as_the_web_apps_json() {
        let json = preset_state_json(0).expect("preset 0");
        let state: syn_core::AppState = serde_json::from_str(&json).expect("parses back");
        assert_eq!(state, syn_core::state::presets()[0].state);
        assert!(preset_state_json(12).is_none());
    }

    #[test]
    fn a_player_renders_float_pcm_and_publishes_frames() {
        let p = SoundPlayer::new(48000, preset_state_json(0).unwrap()).unwrap();
        p.fade_in();
        let mut last = Vec::new();
        for _ in 0..20 {
            last = p.render(4800);
        }
        assert_eq!(last.len(), 4800 * 4);
        let samples: Vec<f32> = last.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
        assert!(samples.iter().any(|v| *v != 0.0));
        assert!((p.time() - 2.0).abs() < 1e-9);
        let f = p.frame_at(1.0).expect("a frame");
        assert!(f.time <= 1.0 && f.time > 0.97);
        assert_eq!(f.spectrum.len(), 64);
        p.switch_to(preset_state_json(5).unwrap()).unwrap();
        p.render(1024);
    }

    #[test]
    fn a_malformed_point_is_an_error_not_a_panic() {
        assert!(matches!(SoundPlayer::new(48000, "{}".into()), Err(CoreError::InvalidPoint { .. })));
        assert!(render_stats("nope".into(), 1.0, 8000).is_err());
        let p = SoundPlayer::new(48000, preset_state_json(0).unwrap()).unwrap();
        assert!(p.set_point("[]".into()).is_err());
    }

    #[test]
    fn render_stats_measure_a_preset() {
        let st = render_stats(preset_state_json(0).unwrap(), 1.0, 22050).unwrap();
        assert!(st.rms > 1e-4 && st.peak >= st.rms);
    }

    #[test]
    fn the_schema_is_the_dumped_one() {
        assert!(schema_json().contains("\"formulaIds\""));
    }
}
