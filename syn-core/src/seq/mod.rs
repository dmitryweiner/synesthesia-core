//! The sequencer layers of a point (PLAN-SAMPLES-SCRIPTS.md): rhythms
//! written in Strudel's mini-notation, played on recorded samples and
//! synthesized drums.
//!
//! - `mini` parses a pattern string into a `pattern::Pattern`, which answers
//!   "which events start in this span" without allocating (S1).
//! - `bank` turns the host's `bank.bin` bytes into samples, decoded per
//!   sound on demand (S2).
//! - `voice` plays them: the sampler and the five synth drums (S2).
//! - `sequencer` joins them: a [`SeqProgram`] is built on the control side
//!   (patterns parsed, sounds decoded), handed to the engine, and played
//!   there without allocating (S2).

pub mod bank;
pub mod mini;
pub mod pattern;
pub mod sequencer;
pub mod voice;

use serde::{Deserialize, Serialize};

pub use bank::{BankError, Sample, SampleBank, Variants};
pub use mini::{parse, ParseError};
pub use pattern::{Event, Events, Frac, Pattern, Sound};
pub use sequencer::{SeqProgram, Sequencer};
pub use voice::Drum;

/// Layer slots in a point — a fixed count, like the four LFOs, so the genome
/// is fixed-size.
pub const LAYERS: usize = 4;

/// How a layer plays its sound, and what a sample was cut for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// The pattern's events, one shot each.
    #[default]
    Hits,
    /// The sample looped under everything; the pattern is ignored.
    Bed,
}

/// The `seq` section of a point. Phase S3 puts it into `AppState`; until
/// then a host hands it to `syn_player::Player::set_seq`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SeqState {
    /// Cycles per second: 0.5 is one 4-beat bar at 120 bpm.
    pub cps: f64,
    pub layers: Vec<Layer>,
}

impl Default for SeqState {
    fn default() -> Self {
        SeqState { cps: 0.5, layers: Vec::new() }
    }
}

impl SeqState {
    /// No layer is on: the point sounds as it would without the section.
    pub fn is_silent(&self) -> bool {
        !self.layers.iter().any(|l| l.on)
    }

    /// The same layers playing the same things — what a [`SeqProgram`] is
    /// built from. Gain, speed, density, dry and the tempo may differ: the
    /// engine reads those from the point every block.
    pub fn same_structure(&self, other: &SeqState) -> bool {
        fn key(l: &Layer) -> (bool, &str, &str, Mode) {
            (l.on, &l.sound, &l.pattern, l.mode)
        }
        self.layers.len() == other.layers.len()
            && self.layers.iter().zip(&other.layers).all(|(a, b)| key(a) == key(b))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub on: bool,
    /// A name from the bank or a synth drum (`bd sd hh oh cp`), optionally
    /// with a variant (`crickets:3`): what a bare number in the pattern
    /// plays, and what a bed loops.
    pub sound: String,
    /// Mini-notation; ignored for a bed.
    pub pattern: String,
    /// 0–1.
    pub gain: f64,
    /// Playback rate: pitch and length, 0.5–2.
    pub speed: f64,
    /// The fraction of events kept (Strudel's `degradeBy(1 − density)`).
    pub density: f64,
    /// Straight to the master, past the FX chain.
    pub dry: bool,
    pub mode: Mode,
    /// 0–1: how far each hit is from the machine — up to 30 % quieter and
    /// 15 ms later, by hash, the same every render. Not a gene (yet): kept
    /// with the point like a hand-written pattern.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub humanize: f64,
}

fn is_zero(x: &f64) -> bool {
    *x == 0.0
}

impl Default for Layer {
    fn default() -> Self {
        Layer {
            on: false,
            sound: String::new(),
            pattern: String::new(),
            gain: 0.5,
            speed: 1.0,
            density: 1.0,
            dry: false,
            mode: Mode::Hits,
            humanize: 0.0,
        }
    }
}

/// `"owl:2"` → `("owl", 2)`; `"owl"` → `("owl", 0)`.
pub fn split_sound(s: &str) -> (&str, u16) {
    match s.rsplit_once(':') {
        Some((name, n)) => match n.parse() {
            Ok(n) => (name, n),
            Err(_) => (s, 0),
        },
        None => (s, 0),
    }
}
