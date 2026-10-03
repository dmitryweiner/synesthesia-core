//! The render side of a live app, shared by every platform.
//!
//! An app owns an audio device and one thread that pulls PCM from here
//! ([`Player::render`]); its control side changes the point
//! ([`Player::set_state`], [`Player::switch_to`]) and its picture asks what is
//! being *heard* now ([`Player::frame_at`]). This crate is the part of that
//! which is the same on Android, iOS and the console:
//!
//! - **Commands, not locks, between control and audio.** A control call only
//!   sends a command; the render call applies pending commands between two
//!   blocks. Points the audio side replaces are handed back and dropped on
//!   the control side, not between two blocks.
//! - **Feature frames on the played clock.** Every ~21 ms of rendered audio a
//!   [`Frame`] is stamped with the engine time and kept in a short ring. A
//!   device buffers audio ahead of what is heard, so a picture asks for the
//!   frame at the *played* time, which the app reads from its device.
//! - **Fades.** Output starts silent and fades in, and fades out before a
//!   stop, so neither clicks; the point's own master gain is not touched.
//!
//! No threads and no clocks of its own: the app supplies both.

use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;

use syn_core::engine::SPECTRUM_BANDS;
use syn_core::state::AppState;
use syn_core::{AudioFeatures, Engine, BLOCK};

/// How often a frame is published, in blocks: 8 × 128 samples ≈ 21 ms at
/// 48 kHz — what the console publishes and more than a screen needs.
pub const FRAME_EVERY_BLOCKS: usize = 8;
/// Frames kept for [`Player::frame_at`]: ~5.5 s at 48 kHz, far more than any
/// device buffers ahead.
pub const FRAME_RING: usize = 256;
/// Default length of a fade in or out, seconds.
pub const FADE_SECONDS: f64 = 0.08;

/// What the picture and the meters read, published from the render side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// Engine time at the end of the audio this frame describes, seconds —
    /// the LFO clock.
    pub time: f64,
    pub peak: f32,
    pub rms: f32,
    pub limiter_db: f32,
    pub features: AudioFeatures,
    /// Log-spaced spectrum, 0..255 per band.
    pub spectrum: [u8; SPECTRUM_BANDS],
    /// Onset hits since the engine started.
    pub hits: u64,
}

enum Command {
    /// Glide to a new point (a morph sends these continuously).
    SetState(Box<AppState>),
    /// Hard switch: drop the tails and start the new point cleanly.
    SwitchTo(Box<AppState>),
    /// Ramp the output level to `to` over `seconds`.
    Fade { to: f32, seconds: f64 },
}

struct RenderSide {
    engine: Engine,
    commands: Receiver<Command>,
    retired: Sender<Box<AppState>>,
    /// The engine always renders whole blocks (its FX chain runs on a
    /// 128-sample quantum, as Web Audio's does); what a caller did not take
    /// of the last block waits here for the next call.
    block: Vec<f32>,
    block_pos: usize,
    /// Samples handed to the caller so far — the clock of [`Player::time`].
    delivered: u64,
    since_frame: usize,
    frame_peak: f32,
    frame_sum: f64,
    frame_n: usize,
    /// Output level and where it is heading, per sample.
    level: f32,
    level_target: f32,
    level_step: f32,
}

/// The engine of one live session, safe to call from a control thread and
/// one audio thread at once.
pub struct Player {
    sample_rate: f64,
    commands: Sender<Command>,
    retired: Mutex<Receiver<Box<AppState>>>,
    render: Mutex<RenderSide>,
    frames: Mutex<VecDeque<Frame>>,
}

impl Player {
    /// A player for `state` at `sample_rate`, silent until [`Player::fade_in`].
    pub fn new(sample_rate: f64, state: &AppState) -> Self {
        let (tx, rx) = channel();
        let (tx_retired, rx_retired) = channel();
        let engine = Engine::new(sample_rate, state, state_seed(state));
        Player {
            sample_rate,
            commands: tx,
            retired: Mutex::new(rx_retired),
            render: Mutex::new(RenderSide {
                engine,
                commands: rx,
                retired: tx_retired,
                block: vec![0.0; BLOCK],
                block_pos: BLOCK,
                delivered: 0,
                since_frame: 0,
                frame_peak: 0.0,
                frame_sum: 0.0,
                frame_n: 0,
                level: 0.0,
                level_target: 0.0,
                level_step: 0.0,
            }),
            frames: Mutex::new(VecDeque::with_capacity(FRAME_RING)),
        }
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Glide to `state`: parameters move, nothing is rebuilt.
    pub fn set_state(&self, state: AppState) {
        self.send(Command::SetState(Box::new(state)));
    }

    /// Switch to `state` hard: the old point's tails are dropped.
    pub fn switch_to(&self, state: AppState) {
        self.send(Command::SwitchTo(Box::new(state)));
    }

    /// Fade the output in from wherever it is, over [`FADE_SECONDS`].
    pub fn fade_in(&self) {
        self.send(Command::Fade { to: 1.0, seconds: FADE_SECONDS });
    }

    /// Fade the output out; [`Player::is_silent`] says when it is done.
    pub fn fade_out(&self) {
        self.send(Command::Fade { to: 0.0, seconds: FADE_SECONDS });
    }

    fn send(&self, cmd: Command) {
        // The receiver lives as long as `self`, so this cannot fail.
        let _ = self.commands.send(cmd);
        self.drop_retired();
    }

    /// Frees the points the render side has replaced — here, on the caller's
    /// thread, rather than on the audio thread.
    fn drop_retired(&self) {
        if let Ok(rx) = self.retired.try_lock() {
            while rx.try_recv().is_ok() {}
        }
    }

    /// Renders `frames` mono samples into `out` (resized to fit), applying
    /// pending commands first. Called by the app's audio thread only. The
    /// samples do not depend on how a stream is cut into calls.
    pub fn render_into(&self, frames: usize, out: &mut Vec<f32>) {
        out.clear();
        out.resize(frames, 0.0);
        let mut r = self.render.lock().unwrap_or_else(|e| e.into_inner());
        r.apply_commands(self.sample_rate);
        let mut done = 0;
        while done < frames {
            if r.block_pos == BLOCK {
                r.render_block();
                r.since_frame += 1;
                if r.since_frame >= FRAME_EVERY_BLOCKS {
                    r.since_frame = 0;
                    let frame = r.take_frame();
                    // A reader holding the ring costs this frame, never the sound.
                    if let Ok(mut ring) = self.frames.try_lock() {
                        if ring.len() == FRAME_RING {
                            ring.pop_front();
                        }
                        ring.push_back(frame);
                    }
                }
            }
            let n = (frames - done).min(BLOCK - r.block_pos);
            let from = r.block_pos;
            out[done..done + n].copy_from_slice(&r.block[from..from + n]);
            r.block_pos += n;
            done += n;
        }
        r.delivered += frames as u64;
    }

    /// Seconds of audio handed out so far. An app that starts a device
    /// reads this first: what the device has played since, added to it, is
    /// the played time [`Player::frame_at`] wants.
    pub fn time(&self) -> f64 {
        let r = self.render.lock().unwrap_or_else(|e| e.into_inner());
        r.delivered as f64 / self.sample_rate
    }

    /// True once a fade out has reached silence and every sample of it has
    /// been handed out (and before any fade in).
    pub fn is_silent(&self) -> bool {
        let r = self.render.lock().unwrap_or_else(|e| e.into_inner());
        r.level == 0.0 && r.level_target == 0.0 && r.block[r.block_pos..].iter().all(|v| *v == 0.0)
    }

    /// The frame describing what is heard at engine time `t`: the latest one
    /// stamped at or before `t`, else the oldest kept. `None` before the first
    /// frame is published.
    pub fn frame_at(&self, t: f64) -> Option<Frame> {
        let ring = self.frames.lock().unwrap_or_else(|e| e.into_inner());
        ring.iter().rev().find(|f| f.time <= t).or_else(|| ring.front()).copied()
    }

    /// The newest frame, whatever has been heard of it.
    pub fn latest_frame(&self) -> Option<Frame> {
        self.frames.lock().unwrap_or_else(|e| e.into_inner()).back().copied()
    }
}

impl RenderSide {
    fn apply_commands(&mut self, sr: f64) {
        while let Ok(cmd) = self.commands.try_recv() {
            match cmd {
                Command::SetState(s) => {
                    self.engine.set_state(&s);
                    let _ = self.retired.send(s);
                }
                Command::SwitchTo(s) => {
                    self.engine.switch_to(&s);
                    let _ = self.retired.send(s);
                }
                Command::Fade { to, seconds } => {
                    self.level_target = to;
                    let samples = (seconds * sr).max(1.0) as f32;
                    self.level_step = (to - self.level).abs() / samples;
                    if self.level_step == 0.0 {
                        self.level = to;
                    }
                }
            }
        }
    }

    fn render_block(&mut self) {
        self.engine.render(&mut self.block);
        self.block_pos = 0;
        for s in self.block.iter_mut() {
            if self.level != self.level_target {
                if self.level < self.level_target {
                    self.level = (self.level + self.level_step).min(self.level_target);
                } else {
                    self.level = (self.level - self.level_step).max(self.level_target);
                }
            }
            let v = *s * self.level;
            *s = v;
            self.frame_peak = self.frame_peak.max(v.abs());
            self.frame_sum += f64::from(v) * f64::from(v);
        }
        self.frame_n += BLOCK;
    }

    fn take_frame(&mut self) -> Frame {
        let frame = Frame {
            time: self.engine.time(),
            peak: self.frame_peak,
            rms: (self.frame_sum / self.frame_n.max(1) as f64).sqrt() as f32,
            limiter_db: self.engine.limiter_reduction_db() as f32,
            features: self.engine.features(),
            spectrum: *self.engine.spectrum(),
            hits: self.engine.hits(),
        };
        self.frame_peak = 0.0;
        self.frame_sum = 0.0;
        self.frame_n = 0;
        frame
    }
}

/// A point always sounds the same on the noisy generators, from run to run
/// and app to app, without a global seed: the seed is the point itself
/// (FNV-1a of its JSON — the console's rule).
pub fn state_seed(state: &AppState) -> u32 {
    let json = serde_json::to_string(state).unwrap_or_default();
    let mut h: u32 = 2_166_136_261;
    for b in json.as_bytes() {
        h ^= u32::from(*b);
        h = h.wrapping_mul(16_777_619);
    }
    h
}

/// Loudness of an offline render, for a bench: how long it takes is measured
/// by the caller, who has a clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderStats {
    pub seconds: f64,
    pub rms: f64,
    pub peak: f64,
}

/// Renders `seconds` of `state` offline at `sample_rate`, as the live player
/// would (same seed), and measures it.
pub fn render_stats(state: &AppState, seconds: f64, sample_rate: f64) -> RenderStats {
    let x = syn_core::render_offline(state, seconds, sample_rate, state_seed(state));
    let mut sum = 0.0f64;
    let mut peak = 0.0f64;
    for v in &x {
        let v = f64::from(*v);
        sum += v * v;
        peak = peak.max(v.abs());
    }
    RenderStats { seconds, rms: (sum / x.len().max(1) as f64).sqrt(), peak }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn_core::state::presets;

    const SR: f64 = 22050.0;

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| f64::from(*v) * f64::from(*v)).sum::<f64>() / x.len().max(1) as f64).sqrt()
    }

    fn render(p: &Player, frames: usize) -> Vec<f32> {
        let mut out = Vec::new();
        p.render_into(frames, &mut out);
        out
    }

    #[test]
    fn it_is_silent_until_faded_in_and_then_sounds() {
        let p = Player::new(SR, &presets()[0].state);
        assert!(render(&p, 4096).iter().all(|v| *v == 0.0));
        assert!(p.is_silent());
        p.fade_in();
        let x = render(&p, SR as usize);
        assert!(!p.is_silent());
        assert!(rms(&x[x.len() / 2..]) > 1e-4);
    }

    #[test]
    fn a_fade_in_ramps_rather_than_jumps() {
        let p = Player::new(SR, &presets()[0].state);
        render(&p, 22050); // let the point build up
        p.fade_in();
        let fade = (FADE_SECONDS * SR) as usize;
        let x = render(&p, fade + 4096);
        let peak = |s: &[f32]| s.iter().map(|v| v.abs()).fold(0.0, f32::max);
        assert!(peak(&x[..32]) < 0.1 * peak(&x[fade..]));
    }

    #[test]
    fn a_fade_out_reaches_silence_and_says_so() {
        let p = Player::new(SR, &presets()[0].state);
        p.fade_in();
        render(&p, SR as usize);
        p.fade_out();
        let x = render(&p, (FADE_SECONDS * SR) as usize + 2 * BLOCK);
        assert!(p.is_silent());
        assert!(x[x.len() - BLOCK..].iter().all(|v| *v == 0.0));
    }

    #[test]
    fn frames_are_published_on_the_engine_clock() {
        let p = Player::new(SR, &presets()[0].state);
        p.fade_in();
        assert!(p.latest_frame().is_none());
        render(&p, BLOCK * FRAME_EVERY_BLOCKS * 10);
        let last = p.latest_frame().expect("frames");
        assert!((last.time - p.time()).abs() < 1e-9);
        assert!((p.time() - (BLOCK * FRAME_EVERY_BLOCKS * 10) as f64 / SR).abs() < 1e-9);
        let dt = BLOCK as f64 * FRAME_EVERY_BLOCKS as f64 / SR;
        // What is heard 3.5 frames back is the frame stamped 4 frames back.
        let f = p.frame_at(p.time() - 3.5 * dt).expect("a frame");
        assert!((f.time - (p.time() - 4.0 * dt)).abs() < 1e-9);
        // Before the oldest frame: the oldest one.
        let oldest = p.frame_at(-1.0).expect("a frame");
        assert!((oldest.time - dt).abs() < 1e-9);
        assert!(last.rms > 0.0 && last.peak >= last.rms);
    }

    #[test]
    fn the_ring_keeps_only_the_latest_frames() {
        let p = Player::new(SR, &presets()[0].state);
        render(&p, BLOCK * FRAME_EVERY_BLOCKS * (FRAME_RING + 10));
        let oldest = p.frame_at(-1.0).expect("a frame");
        let dt = BLOCK as f64 * FRAME_EVERY_BLOCKS as f64 / SR;
        assert!((oldest.time - 11.0 * dt).abs() < 1e-9);
    }

    #[test]
    fn a_switch_reaches_the_engine_between_two_renders() {
        let quiet = AppState::new(); // every formula off
        let p = Player::new(SR, &quiet);
        p.fade_in();
        assert!(rms(&render(&p, 8192)) < 1e-9);
        p.switch_to(presets()[0].state.clone());
        assert!(rms(&render(&p, SR as usize)) > 1e-4);
        p.set_state(quiet);
        let x = render(&p, SR as usize * 2);
        assert!(rms(&x[x.len() - 4096..]) < rms(&x[..4096]));
    }

    #[test]
    fn the_sound_does_not_depend_on_how_it_is_cut_into_calls() {
        let a = Player::new(SR, &presets()[3].state);
        let b = Player::new(SR, &presets()[3].state);
        a.fade_in();
        b.fade_in();
        let whole = render(&a, 5000);
        let mut parts = Vec::new();
        for n in [1, 127, 128, 129, 1000, 3615] {
            parts.extend(render(&b, n));
        }
        assert_eq!(whole, parts);
        assert!((a.time() - b.time()).abs() < 1e-12);
    }

    #[test]
    fn faded_in_from_the_start_it_is_the_offline_render() {
        // The same seed and whole blocks: the live sound is the offline one,
        // apart from the opening fade.
        let state = &presets()[0].state;
        let p = Player::new(SR, state);
        p.fade_in();
        let live = render(&p, 8192);
        let offline = syn_core::render_offline(state, 8192.0 / SR, SR, state_seed(state));
        let fade = (FADE_SECONDS * SR) as usize + BLOCK;
        assert_eq!(&live[fade..], &offline[fade..8192]);
    }

    #[test]
    fn the_seed_is_the_point() {
        let s = &presets()[0].state;
        assert_eq!(state_seed(s), state_seed(&s.clone()));
        assert_ne!(state_seed(s), state_seed(&presets()[1].state));
    }

    #[test]
    fn render_stats_measure_a_preset() {
        let st = render_stats(&presets()[0].state, 1.0, SR);
        assert!(st.rms > 1e-4 && st.peak >= st.rms && st.peak <= 1.5);
    }
}
