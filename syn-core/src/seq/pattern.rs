//! The pattern engine (PLAN-SAMPLES-SCRIPTS.md, *The pattern engine*): a
//! pattern is a tree evaluated as *span → events* — Strudel's and Tidal's
//! model, not their code. Time is in cycles, as exact fractions, so a step
//! boundary is never missed or hit twice by rounding: two queries over
//! adjoining spans return exactly the events of one query over both.
//!
//! The tree is built once, when the point changes (`mini::parse`), and only
//! read afterwards: `Pattern::query` recurses on the stack and writes into a
//! fixed array (`Events`), so it allocates nothing and can run inside
//! `Engine::render`. Randomness (`?`, `|`) is a hash of the event's time,
//! a per-node salt and the caller's seed — a pure function of time, like the
//! LFOs, so every render of a point hears the same bar.

use std::cmp::Ordering;
use std::ops::{Add, Div, Mul, Sub};

/// An exact number of cycles: always reduced, `den > 0`, so `==` is equality
/// of value and the hash of a time does not depend on how it was reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Frac {
    num: i64,
    den: i64,
}

/// What `Frac::approx` rounds to: 2⁻²⁰ of a cycle (under 1 µs at the
/// fastest `cps`). The parser bounds how far a pattern rescales time
/// (`MAX_SCALE`), so grid × scale × cycles stays inside `i64`.
pub const GRID: i64 = 1 << 20;

const fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

const fn gcd128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

impl Frac {
    pub const ZERO: Frac = Frac { num: 0, den: 1 };
    pub const ONE: Frac = Frac { num: 1, den: 1 };

    /// `num / den`. A zero denominator gives zero rather than a panic: a
    /// pattern must never stop the audio thread.
    pub fn new(num: i64, den: i64) -> Frac {
        Frac::reduce(i128::from(num), i128::from(den))
    }

    pub const fn int(n: i64) -> Frac {
        Frac { num: n, den: 1 }
    }

    /// `x` cycles on the 1/`GRID` lattice. The engine turns its clock into
    /// spans with this, so adjoining blocks share their boundary exactly.
    pub fn approx(x: f64) -> Frac {
        Frac::new((x * GRID as f64).round() as i64, GRID)
    }

    pub const fn num(self) -> i64 {
        self.num
    }

    pub const fn den(self) -> i64 {
        self.den
    }

    pub fn to_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// The cycle this time falls in.
    pub const fn floor(self) -> i64 {
        self.num.div_euclid(self.den)
    }

    pub const fn ceil(self) -> i64 {
        -(-self.num).div_euclid(self.den)
    }

    fn reduce(n: i128, d: i128) -> Frac {
        if d == 0 {
            return Frac::ZERO;
        }
        let (n, d) = if d < 0 { (-n, -d) } else { (n, d) };
        if let (Ok(n64), Ok(d64)) = (i64::try_from(n), i64::try_from(d)) {
            // The common case, and the only one in wasm worth being fast:
            // i64 division is native there, i128 division is a libcall.
            let g = gcd(n64.unsigned_abs(), d64 as u64) as i64;
            return Frac { num: n64 / g, den: d64 / g };
        }
        let g = gcd128(n.unsigned_abs(), d as u128) as i128;
        let (n, d) = (n / g, d / g);
        match (i64::try_from(n), i64::try_from(d)) {
            (Ok(num), Ok(den)) => Frac { num, den },
            // Out of range even reduced: round onto the grid instead of
            // panicking. The parser's `MAX_SCALE` keeps this unreachable.
            _ => Frac::approx(n as f64 / d as f64),
        }
    }
}

impl Add for Frac {
    type Output = Frac;
    fn add(self, o: Frac) -> Frac {
        if self.den == o.den {
            return Frac::reduce(i128::from(self.num) + i128::from(o.num), i128::from(self.den));
        }
        let (a, b) = (i128::from(self.den), i128::from(o.den));
        Frac::reduce(i128::from(self.num) * b + i128::from(o.num) * a, a * b)
    }
}

impl Sub for Frac {
    type Output = Frac;
    fn sub(self, o: Frac) -> Frac {
        self + Frac { num: -o.num, den: o.den }
    }
}

impl Mul for Frac {
    type Output = Frac;
    fn mul(self, o: Frac) -> Frac {
        Frac::reduce(i128::from(self.num) * i128::from(o.num), i128::from(self.den) * i128::from(o.den))
    }
}

impl Div for Frac {
    type Output = Frac;
    fn div(self, o: Frac) -> Frac {
        Frac::reduce(i128::from(self.num) * i128::from(o.den), i128::from(self.den) * i128::from(o.num))
    }
}

impl Ord for Frac {
    fn cmp(&self, o: &Frac) -> Ordering {
        (i128::from(self.num) * i128::from(o.den)).cmp(&(i128::from(o.num) * i128::from(self.den)))
    }
}

impl PartialOrd for Frac {
    fn partial_cmp(&self, o: &Frac) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// What a step plays: `name` indexes the pattern's `names` (`sd`,
/// `woodblock:2`); `None` is the layer's own sound, variant `n` (`2`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sound {
    pub name: Option<u16>,
    pub n: u16,
}

/// One event ("hap"): it starts at `onset` and its step lasts `dur`, both
/// in cycles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub onset: Frac,
    pub dur: Frac,
    pub sound: Sound,
}

/// How many events one query may return (decision 8): enough for a block,
/// a fixed array so nothing grows on the audio thread.
pub const MAX_EVENTS: usize = 64;

/// A query's result: at most `MAX_EVENTS`, and a count of what did not fit.
#[derive(Clone, Debug)]
pub struct Events {
    buf: [Event; MAX_EVENTS],
    len: usize,
    /// Events dropped because the array was full — counted, never grown.
    pub overflow: u32,
}

impl Default for Events {
    fn default() -> Self {
        let none = Event { onset: Frac::ZERO, dur: Frac::ZERO, sound: Sound { name: None, n: 0 } };
        Events { buf: [none; MAX_EVENTS], len: 0, overflow: 0 }
    }
}

impl Events {
    pub fn clear(&mut self) {
        self.len = 0;
        self.overflow = 0;
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[Event] {
        &self.buf[..self.len]
    }

    pub fn push(&mut self, e: Event) {
        if self.len < MAX_EVENTS {
            self.buf[self.len] = e;
            self.len += 1;
        } else {
            self.overflow += 1;
        }
    }

    /// In onset order, stably (an insertion sort: the array is short and
    /// mostly sorted already, and it allocates nothing).
    pub fn sort(&mut self) {
        for i in 1..self.len {
            let mut j = i;
            while j > 0 && self.buf[j - 1].onset > self.buf[j].onset {
                self.buf.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    fn map_from(&mut self, from: usize, f: impl Fn(&mut Event)) {
        for e in &mut self.buf[from..self.len] {
            f(e);
        }
    }

    fn retain_from(&mut self, from: usize, keep: impl Fn(&Event) -> bool) {
        let mut w = from;
        for r in from..self.len {
            if keep(&self.buf[r]) {
                self.buf[w] = self.buf[r];
                w += 1;
            }
        }
        self.len = w;
    }
}

pub(crate) type NodeId = u32;

/// One step of a sequence: `start` and `width` are fractions of the cycle.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Step {
    pub node: NodeId,
    pub start: Frac,
    pub width: Frac,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Node {
    Silence,
    /// One event per cycle, at its start.
    Atom(Sound),
    /// `"a b [c d]"`: each step squeezed into its share of every cycle
    /// (`steps[from..to]`), keeping the cycle number — so `"<a b> c"`
    /// alternates.
    Seq {
        from: u32,
        to: u32,
    },
    /// `"a , b"`: `kids[from..to]` over the same span.
    Stack {
        from: u32,
        to: u32,
    },
    /// `"a | b"`: one of `kids[from..to]` per cycle, by hash.
    Choice {
        from: u32,
        to: u32,
        salt: u32,
    },
    /// `"a*2"`, `"a/2"`: time sped up by `factor`.
    Fast {
        child: NodeId,
        factor: Frac,
    },
    /// `"a?0.3"`: each event dropped with probability `prob`, by hash.
    Degrade {
        child: NodeId,
        prob: f64,
        salt: u32,
    },
}

/// A parsed pattern: an arena of nodes (a subtree may be shared, as a
/// Euclidean rhythm shares its step) and the sound names its steps use.
#[derive(Clone, Debug)]
pub struct Pattern {
    pub(crate) nodes: Vec<Node>,
    pub(crate) steps: Vec<Step>,
    pub(crate) kids: Vec<NodeId>,
    pub(crate) names: Vec<String>,
    pub(crate) root: NodeId,
    /// Salts handed out so far; a combinator applied later takes the next.
    pub(crate) salts: u32,
}

/// splitmix64's finalizer.
const fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A number in [0, 1) that depends only on a time, a salt and a seed —
/// decision 8's hash. The sequencer uses it for `density` too.
pub fn unit_hash(t: Frac, salt: u64, seed: u64) -> f64 {
    let mut h = mix(seed ^ 0x9e37_79b9_7f4a_7c15);
    h = mix(h ^ salt.wrapping_mul(0xd6e8_feb8_6659_fd93));
    h = mix(h ^ t.num as u64);
    h = mix(h ^ (t.den as u64).rotate_left(32));
    (h >> 11) as f64 / (1u64 << 53) as f64
}

impl Default for Pattern {
    fn default() -> Self {
        Pattern::silence()
    }
}

impl Pattern {
    pub fn silence() -> Pattern {
        Pattern {
            nodes: vec![Node::Silence],
            steps: Vec::new(),
            kids: Vec::new(),
            names: Vec::new(),
            root: 0,
            salts: 0,
        }
    }

    pub fn is_silent(&self) -> bool {
        matches!(self.nodes[self.root as usize], Node::Silence)
    }

    /// The names the steps use; `Sound::name` indexes this.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// Strudel's `fast`: `p.fast(2)` is `"[p]*2"`.
    pub fn fast(mut self, factor: Frac) -> Pattern {
        if factor <= Frac::ZERO {
            return Pattern::silence();
        }
        self.root = self.push(Node::Fast { child: self.root, factor });
        self
    }

    pub fn slow(self, factor: Frac) -> Pattern {
        if factor <= Frac::ZERO {
            return Pattern::silence();
        }
        self.fast(Frac::ONE / factor)
    }

    /// Strudel's `degradeBy`: each event dropped with probability `prob`.
    pub fn degrade_by(mut self, prob: f64) -> Pattern {
        let salt = self.salts;
        self.salts += 1;
        self.root = self.push(Node::Degrade { child: self.root, prob, salt });
        self
    }

    pub(crate) fn push(&mut self, n: Node) -> NodeId {
        self.nodes.push(n);
        (self.nodes.len() - 1) as NodeId
    }

    /// Appends the events whose onset lies in `[begin, end)` to `out`.
    /// `seed` picks the random choices (`?`, `|`): the sequencer passes a
    /// hash of the point's seed and the layer.
    pub fn query(&self, begin: Frac, end: Frac, seed: u64, out: &mut Events) {
        self.query_node(self.root, begin, end, seed, out);
    }

    fn query_node(&self, id: NodeId, a: Frac, b: Frac, seed: u64, out: &mut Events) {
        if a >= b {
            return;
        }
        match self.nodes[id as usize] {
            Node::Silence => {}
            Node::Atom(sound) => {
                let mut c = a.ceil();
                while Frac::int(c) < b {
                    out.push(Event { onset: Frac::int(c), dur: Frac::ONE, sound });
                    c += 1;
                }
            }
            Node::Seq { from, to } => {
                for c in a.floor()..b.ceil() {
                    let cycle = Frac::int(c);
                    for st in &self.steps[from as usize..to as usize] {
                        let s0 = cycle + st.start;
                        let lo = a.max(s0);
                        let hi = b.min(s0 + st.width);
                        if lo >= hi {
                            continue;
                        }
                        // The step's slot of cycle c is cycle c of the child.
                        let mark = out.len;
                        self.query_node(
                            st.node,
                            cycle + (lo - s0) / st.width,
                            cycle + (hi - s0) / st.width,
                            seed,
                            out,
                        );
                        out.map_from(mark, |e| {
                            e.onset = s0 + (e.onset - cycle) * st.width;
                            e.dur = e.dur * st.width;
                        });
                    }
                }
            }
            Node::Stack { from, to } => {
                for &k in &self.kids[from as usize..to as usize] {
                    self.query_node(k, a, b, seed, out);
                }
            }
            Node::Choice { from, to, salt } => {
                let n = (to - from) as usize;
                for c in a.floor()..b.ceil() {
                    let pick = (unit_hash(Frac::int(c), u64::from(salt), seed) * n as f64) as usize;
                    let k = self.kids[from as usize + pick.min(n - 1)];
                    self.query_node(k, a.max(Frac::int(c)), b.min(Frac::int(c + 1)), seed, out);
                }
            }
            Node::Fast { child, factor } => {
                let mark = out.len;
                self.query_node(child, a * factor, b * factor, seed, out);
                out.map_from(mark, |e| {
                    e.onset = e.onset / factor;
                    e.dur = e.dur / factor;
                });
            }
            Node::Degrade { child, prob, salt } => {
                let mark = out.len;
                self.query_node(child, a, b, seed, out);
                out.retain_from(mark, |e| unit_hash(e.onset, u64::from(salt), seed) >= prob);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractions_reduce_and_compare_by_value() {
        assert_eq!(Frac::new(2, 4), Frac::new(-1, -2));
        assert_eq!(Frac::new(1, 3) + Frac::new(1, 6), Frac::new(1, 2));
        assert_eq!(Frac::new(3, 8) * Frac::new(4, 3), Frac::new(1, 2));
        assert_eq!(Frac::new(1, 2) / Frac::new(1, 4), Frac::int(2));
        assert!(Frac::new(1, 3) < Frac::new(34, 100));
        assert_eq!(Frac::new(-1, 2).floor(), -1);
        assert_eq!(Frac::new(-1, 2).ceil(), 0);
        assert_eq!(Frac::new(7, 2).ceil(), 4);
        assert_eq!(Frac::new(1, 0), Frac::ZERO);
        assert_eq!(Frac::approx(0.375), Frac::new(3, 8));
    }

    #[test]
    fn events_overflow_is_counted_not_grown() {
        let mut e = Events::default();
        let ev = Event { onset: Frac::ZERO, dur: Frac::ONE, sound: Sound { name: None, n: 0 } };
        for _ in 0..MAX_EVENTS + 5 {
            e.push(ev);
        }
        assert_eq!((e.len(), e.overflow), (MAX_EVENTS, 5));
        e.clear();
        assert_eq!((e.len(), e.overflow), (0, 0));
    }

    #[test]
    fn the_hash_is_uniform_enough() {
        let n = 10_000;
        let mean = (0..n).map(|i| unit_hash(Frac::new(i, 8), 1, 42)).sum::<f64>() / n as f64;
        assert!((mean - 0.5).abs() < 0.02, "{mean}");
        assert_ne!(unit_hash(Frac::ONE, 1, 42), unit_hash(Frac::ONE, 2, 42));
        assert_ne!(unit_hash(Frac::ONE, 1, 42), unit_hash(Frac::ONE, 1, 43));
    }
}
