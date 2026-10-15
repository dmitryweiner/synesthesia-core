//! The sequencer layers of a point (PLAN-SAMPLES-SCRIPTS.md): rhythms
//! written in Strudel's mini-notation, played on recorded samples and
//! synthesized drums. Phase S1 is the pattern engine: `mini` parses a
//! string into a `pattern::Pattern`, which answers "which events start in
//! this span" without allocating.

pub mod mini;
pub mod pattern;

pub use mini::{parse, ParseError};
pub use pattern::{Event, Events, Frac, Pattern, Sound};
