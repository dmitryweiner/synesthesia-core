//! Mini-notation (PLAN-SAMPLES-SCRIPTS.md, *Mini-notation — the subset*):
//! the Tidal/Strudel syntax for rhythms, parsed once into a `Pattern` when
//! the point changes — never at render time.
//!
//! ```text
//! "bd sd [hh hh] ~"   steps share the cycle; [ ] is a subsequence in one step
//! "<0 1 2>"           one step per cycle, in turn
//! "bd , hh*4"         layered;  "bd | sd"  one of them per cycle, by hash
//! "0*2" "0/2"         faster, slower;  "0!3" "0 ! !"  repeated steps
//! "0?" "0?0.3"        dropped with probability ½ (or the given one)
//! "0(3,8)" "0(3,8,2)" a Euclidean rhythm; the third number rotates it right
//!                     (Strudel's `euclidRot`; Tidal rotates left)
//! "0@3"               a step three times as wide as the others
//! "owl:1" "sd" "2"    a named sound and variant; a bare number is a variant
//!                     of the layer's own sound
//! ```
//!
//! Polymeter `{ }`, `.` and `_` are phase S6; they are errors here, as is
//! anything else outside the subset. A host keeps an unparsable pattern as
//! `""` (silence), so a point always opens.

use std::fmt;

use super::pattern::{Frac, Node, NodeId, Pattern, Sound, Step};

/// The longest pattern string accepted.
pub const MAX_LEN: usize = 512;
/// Brackets nested deeper than this are an error (and bound the recursion).
const MAX_DEPTH: usize = 16;
/// The largest `*`, `/`, `@`, `!` and Euclidean step count.
const MAX_FACTOR: i64 = 128;
/// How far a pattern may rescale time, root to leaf (a product of the
/// rescalings' numerators and denominators). With `pattern::GRID` and a few
/// days of cycles it keeps every fraction a query makes inside `i64`.
const MAX_SCALE: u64 = 1 << 24;
/// The largest variant number, `owl:9999`.
const MAX_VARIANT: i64 = 9999;

/// What is wrong with a pattern, and the byte where the parser noticed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub pos: usize,
    pub msg: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.msg, self.pos)
    }
}

impl std::error::Error for ParseError {}

/// The node every rest points at (`Pattern::silence()` puts it first).
const SILENCE: NodeId = 0;

/// Parses a mini-notation string. An empty string is silence.
pub fn parse(src: &str) -> Result<Pattern, ParseError> {
    if src.len() > MAX_LEN {
        return Err(ParseError { pos: MAX_LEN, msg: "pattern too long" });
    }
    let mut p = Parser { s: src.as_bytes(), pos: 0, depth: 0, pat: Pattern::silence() };
    let root = p.body(None, false)?;
    p.pat.root = root;
    if p.scale() > MAX_SCALE {
        return Err(ParseError { pos: 0, msg: "pattern rescales time too far" });
    }
    Ok(p.pat)
}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
    depth: usize,
    pat: Pattern,
}

impl Parser<'_> {
    fn err<T>(&self, msg: &'static str) -> Result<T, ParseError> {
        Err(ParseError { pos: self.pos, msg })
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn ws(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn next_salt(&mut self) -> u32 {
        self.pat.salts += 1;
        self.pat.salts - 1
    }

    /// Sequences separated by `,` (a stack) or `|` (a choice), up to `close`
    /// (consumed) or the end. Inside `< >` each sequence is slowed by its
    /// width, so it plays one step per cycle.
    fn body(&mut self, close: Option<u8>, slow_each: bool) -> Result<NodeId, ParseError> {
        let mut seqs = Vec::new();
        let mut sep = None;
        loop {
            let (node, weight) = self.sequence()?;
            seqs.push(if slow_each && weight != Frac::ONE && node != SILENCE {
                self.pat.push(Node::Fast { child: node, factor: Frac::ONE / weight })
            } else {
                node
            });
            match self.peek() {
                Some(c @ (b',' | b'|')) => {
                    if sep.is_some_and(|s| s != c) {
                        return self.err("',' and '|' cannot be mixed in one group");
                    }
                    sep = Some(c);
                    self.pos += 1;
                }
                None if close.is_none() => break,
                None => return self.err("unclosed bracket"),
                Some(c) if Some(c) == close => {
                    self.pos += 1;
                    break;
                }
                Some(_) => return self.err("unexpected character"),
            }
        }
        if seqs.len() == 1 {
            return Ok(seqs[0]);
        }
        let from = self.pat.kids.len() as u32;
        self.pat.kids.extend(&seqs);
        let to = self.pat.kids.len() as u32;
        Ok(if sep == Some(b'|') {
            let salt = self.next_salt();
            self.pat.push(Node::Choice { from, to, salt })
        } else {
            self.pat.push(Node::Stack { from, to })
        })
    }

    /// Steps sharing one cycle by their weights; returns the node and the
    /// total weight (what `< >` slows by).
    fn sequence(&mut self) -> Result<(NodeId, Frac), ParseError> {
        let mut steps: Vec<(NodeId, Frac)> = Vec::new();
        loop {
            self.ws();
            match self.peek() {
                None | Some(b',' | b'|' | b']' | b'>' | b'}') => break,
                Some(b'!') => {
                    // A bare `!` repeats the step before it.
                    let Some(&last) = steps.last() else { return self.err("'!' with no step before it") };
                    self.pos += 1;
                    steps.push(last);
                }
                Some(_) => {
                    let (node, weight, reps) = self.step()?;
                    for _ in 0..reps {
                        steps.push((node, weight));
                    }
                }
            }
            if steps.len() > MAX_FACTOR as usize {
                return self.err("too many steps in one sequence");
            }
        }
        Ok(match steps.as_slice() {
            [] => (SILENCE, Frac::ONE),
            &[(node, weight)] => (node, weight),
            _ => {
                let total = steps.iter().fold(Frac::ZERO, |acc, &(_, w)| acc + w);
                (self.seq(&steps, total), total)
            }
        })
    }

    /// A `Seq` node of `(node, weight)` steps; silent steps take their room
    /// but are not stored.
    fn seq(&mut self, steps: &[(NodeId, Frac)], total: Frac) -> NodeId {
        let from = self.pat.steps.len() as u32;
        let mut at = Frac::ZERO;
        for &(node, w) in steps {
            if node != SILENCE {
                self.pat.steps.push(Step { node, start: at / total, width: w / total });
            }
            at = at + w;
        }
        let to = self.pat.steps.len() as u32;
        self.pat.push(Node::Seq { from, to })
    }

    /// One step and its modifiers: the node, its weight, how many times it
    /// repeats.
    fn step(&mut self) -> Result<(NodeId, Frac, usize), ParseError> {
        let mut node = match self.peek() {
            Some(b'~') => {
                self.pos += 1;
                SILENCE
            }
            Some(open @ (b'[' | b'<')) => {
                if self.depth >= MAX_DEPTH {
                    return self.err("brackets nested too deep");
                }
                self.pos += 1;
                self.depth += 1;
                let node =
                    if open == b'[' { self.body(Some(b']'), false)? } else { self.body(Some(b'>'), true)? };
                self.depth -= 1;
                node
            }
            Some(b'{') => return self.err("polymeter { } is not supported yet"),
            Some(c) if c.is_ascii_digit() => {
                let n = self.int(MAX_VARIANT)?;
                self.pat.push(Node::Atom(Sound { name: None, n: n as u16 }))
            }
            Some(c) if c.is_ascii_alphabetic() => self.atom()?,
            _ => return self.err("expected a step"),
        };
        let mut weight = Frac::ONE;
        let mut reps = 1;
        loop {
            match self.peek() {
                Some(b'*') => {
                    self.pos += 1;
                    let factor = self.factor()?;
                    node = self.fast(node, factor);
                }
                Some(b'/') => {
                    self.pos += 1;
                    let factor = self.factor()?;
                    node = self.fast(node, Frac::ONE / factor);
                }
                Some(b'@') => {
                    self.pos += 1;
                    weight = self.factor()?;
                }
                Some(b'!') => {
                    self.pos += 1;
                    reps = if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                        self.int(MAX_FACTOR)?.max(1) as usize
                    } else {
                        reps + 1
                    };
                }
                Some(b'?') => {
                    self.pos += 1;
                    let prob = if self.peek().is_some_and(|c| c.is_ascii_digit() || c == b'.') {
                        let f = self.number()?;
                        if f > Frac::ONE {
                            return self.err("'?' takes a probability, 0 to 1");
                        }
                        f.to_f64()
                    } else {
                        0.5
                    };
                    let salt = self.next_salt();
                    if node != SILENCE {
                        node = self.pat.push(Node::Degrade { child: node, prob, salt });
                    }
                }
                Some(b'(') => {
                    self.pos += 1;
                    node = self.euclid(node)?;
                }
                _ => break,
            }
        }
        Ok((node, weight, reps))
    }

    /// `name` or `name:n`.
    fn atom(&mut self) -> Result<NodeId, ParseError> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_') {
            self.pos += 1;
        }
        let name = std::str::from_utf8(&self.s[start..self.pos]).unwrap_or_default();
        let n = if self.peek() == Some(b':') {
            self.pos += 1;
            self.int(MAX_VARIANT)? as u16
        } else {
            0
        };
        let idx = match self.pat.names.iter().position(|x| x == name) {
            Some(i) => i,
            None => {
                self.pat.names.push(name.to_string());
                self.pat.names.len() - 1
            }
        };
        if idx > usize::from(u16::MAX) {
            return self.err("too many names");
        }
        Ok(self.pat.push(Node::Atom(Sound { name: Some(idx as u16), n })))
    }

    /// `(k,n)` or `(k,n,r)`, after the `(`: the step played on the pulses of
    /// Bjorklund's k-in-n rhythm.
    fn euclid(&mut self, node: NodeId) -> Result<NodeId, ParseError> {
        let mut args = [0i64; 3];
        let mut count = 0;
        loop {
            self.ws();
            if count == 3 {
                return self.err("a Euclidean rhythm takes 2 or 3 numbers");
            }
            args[count] = self.int(MAX_FACTOR)?;
            count += 1;
            self.ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b')') => {
                    self.pos += 1;
                    break;
                }
                _ => return self.err("expected ',' or ')'"),
            }
        }
        if count < 2 {
            return self.err("a Euclidean rhythm takes 2 or 3 numbers");
        }
        let [k, n, r] = args;
        if n == 0 || k == 0 || node == SILENCE {
            return Ok(SILENCE);
        }
        let (k, n) = (k.min(n) as usize, n as usize);
        let bits = bjorklund(k, n);
        let r = r as usize % n;
        let steps: Vec<(NodeId, Frac)> =
            (0..n).map(|i| (if bits[(i + n - r) % n] { node } else { SILENCE }, Frac::ONE)).collect();
        Ok(self.seq(&steps, Frac::int(n as i64)))
    }

    fn fast(&mut self, node: NodeId, factor: Frac) -> NodeId {
        if node == SILENCE || factor == Frac::ONE {
            return node;
        }
        self.pat.push(Node::Fast { child: node, factor })
    }

    /// A positive number no larger than `MAX_FACTOR`.
    fn factor(&mut self) -> Result<Frac, ParseError> {
        let f = self.number()?;
        if f <= Frac::ZERO || f > Frac::int(MAX_FACTOR) {
            return self.err("a factor must be above 0 and at most 128");
        }
        Ok(f)
    }

    /// `12`, `1.5`, `.25` — exactly, as a fraction (up to 6 decimals).
    fn number(&mut self) -> Result<Frac, ParseError> {
        let int = match self.peek() {
            Some(c) if c.is_ascii_digit() => self.int(1_000_000)?,
            Some(b'.') => 0,
            _ => return self.err("expected a number"),
        };
        if self.peek() != Some(b'.') {
            return Ok(Frac::int(int));
        }
        self.pos += 1;
        let (mut num, mut den, mut digits) = (0i64, 1i64, 0);
        while let Some(c @ b'0'..=b'9') = self.peek() {
            digits += 1;
            if digits > 6 {
                return self.err("too many decimals");
            }
            num = num * 10 + i64::from(c - b'0');
            den *= 10;
            self.pos += 1;
        }
        if digits == 0 {
            return self.err("expected a digit after '.'");
        }
        Ok(Frac::int(int) + Frac::new(num, den))
    }

    fn int(&mut self, max: i64) -> Result<i64, ParseError> {
        let start = self.pos;
        let mut v = 0i64;
        while let Some(c @ b'0'..=b'9') = self.peek() {
            v = v * 10 + i64::from(c - b'0');
            if v > max {
                return self.err("number too large");
            }
            self.pos += 1;
        }
        if self.pos == start {
            return self.err("expected a number");
        }
        Ok(v)
    }

    /// The largest rescaling of time from the root to any leaf. Children are
    /// pushed before their parents, so one pass in order sees every child
    /// first, however much the arena shares.
    fn scale(&self) -> u64 {
        let p = &self.pat;
        let span = |f: Frac| f.num().unsigned_abs().saturating_mul(f.den().unsigned_abs());
        let mut s = vec![1u64; p.nodes.len()];
        for (i, node) in p.nodes.iter().enumerate() {
            s[i] = match *node {
                Node::Silence | Node::Atom(_) => 1,
                Node::Seq { from, to } => p.steps[from as usize..to as usize]
                    .iter()
                    .map(|st| s[st.node as usize].saturating_mul(span(st.width)))
                    .max()
                    .unwrap_or(1),
                Node::Stack { from, to } | Node::Choice { from, to, .. } => {
                    p.kids[from as usize..to as usize].iter().map(|&k| s[k as usize]).max().unwrap_or(1)
                }
                Node::Fast { child, factor } => s[child as usize].saturating_mul(span(factor)),
                Node::Degrade { child, .. } => s[child as usize],
            };
        }
        s[p.root as usize]
    }
}

/// Bjorklund's algorithm as Tidal has it: `k` pulses spread over `n` steps,
/// `(3,8)` → `x..x..x.`, `(5,8)` → `x.xx.xx.`.
fn bjorklund(k: usize, n: usize) -> Vec<bool> {
    let mut a: Vec<Vec<bool>> = vec![vec![true]; k];
    let mut b: Vec<Vec<bool>> = vec![vec![false]; n - k];
    while a.len().min(b.len()) > 1 {
        if a.len() > b.len() {
            // More pulses than rests: pair the first |b| pulses with the
            // rests; the leftover pulses become the new remainder.
            let rest = a.split_off(b.len());
            for (x, y) in a.iter_mut().zip(b) {
                x.extend(y);
            }
            b = rest;
        } else {
            let rest = b.split_off(a.len());
            for (x, y) in a.iter_mut().zip(b) {
                x.extend(y);
            }
            b = rest;
        }
    }
    a.into_iter().chain(b).flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seq::pattern::{Events, MAX_EVENTS};

    fn f(n: i64, d: i64) -> Frac {
        Frac::new(n, d)
    }

    /// The events of `[a, b)` as (onset, dur, label), sorted; a label is the
    /// name with its variant (`sd:2`), or `#n` for the layer's own sound.
    /// Queried a cycle at a time, so a long span fits the array.
    fn events_in(p: &Pattern, a: Frac, b: Frac, seed: u64) -> Vec<(Frac, Frac, String)> {
        let mut all = Vec::new();
        let mut t = a;
        while t < b {
            let next = b.min(Frac::int(t.floor() + 1));
            let mut out = Events::default();
            p.query(t, next, seed, &mut out);
            assert_eq!(out.overflow, 0);
            out.sort();
            all.extend_from_slice(out.as_slice());
            t = next;
        }
        all.iter()
            .map(|e| {
                let label = match e.sound.name {
                    Some(i) if e.sound.n == 0 => p.names()[i as usize].clone(),
                    Some(i) => format!("{}:{}", p.names()[i as usize], e.sound.n),
                    None => format!("#{}", e.sound.n),
                };
                (e.onset, e.dur, label)
            })
            .collect()
    }

    fn events(src: &str, cycles: i64) -> Vec<(Frac, Frac, String)> {
        events_in(&parse(src).unwrap(), Frac::ZERO, Frac::int(cycles), 7)
    }

    fn onsets(src: &str, cycles: i64) -> Vec<Frac> {
        events(src, cycles).into_iter().map(|e| e.0).collect()
    }

    fn labels(src: &str, cycles: i64) -> Vec<String> {
        events(src, cycles).into_iter().map(|e| e.2).collect()
    }

    fn ev(on: Frac, dur: Frac, l: &str) -> (Frac, Frac, String) {
        (on, dur, l.to_string())
    }

    #[test]
    fn steps_share_the_cycle() {
        assert_eq!(events("bd sd", 1), vec![ev(f(0, 1), f(1, 2), "bd"), ev(f(1, 2), f(1, 2), "sd")]);
        assert_eq!(onsets("bd sd hh", 1), vec![f(0, 1), f(1, 3), f(2, 3)]);
        assert_eq!(onsets("bd sd", 2), vec![f(0, 1), f(1, 2), f(1, 1), f(3, 2)]);
        assert_eq!(onsets("~ hh ~ hh", 1), vec![f(1, 4), f(3, 4)]);
    }

    #[test]
    fn brackets_make_a_subsequence() {
        assert_eq!(
            events("bd [sd sd]", 1),
            vec![ev(f(0, 1), f(1, 2), "bd"), ev(f(1, 2), f(1, 4), "sd"), ev(f(3, 4), f(1, 4), "sd")]
        );
        assert_eq!(onsets("bd [~ [hh hh]]", 1), vec![f(0, 1), f(3, 4), f(7, 8)]);
    }

    #[test]
    fn angle_brackets_alternate_per_cycle() {
        assert_eq!(labels("<bd sd>", 4), ["bd", "sd", "bd", "sd"]);
        assert_eq!(events("<bd sd>", 2)[1], ev(f(1, 1), f(1, 1), "sd"));
        assert_eq!(labels("<a <b c>>", 4), ["a", "b", "a", "c"]);
        assert_eq!(labels("<a b> c", 2), ["a", "c", "b", "c"]);
        assert_eq!(onsets("<bd [sd sd]>", 2), vec![f(0, 1), f(1, 1), f(3, 2)]);
        assert_eq!(labels("<0 ~ ~ 1>", 8), ["#0", "#1", "#0", "#1"]);
    }

    #[test]
    fn fast_and_slow() {
        assert_eq!(
            events("[a b]*2", 1),
            vec![
                ev(f(0, 1), f(1, 4), "a"),
                ev(f(1, 4), f(1, 4), "b"),
                ev(f(1, 2), f(1, 4), "a"),
                ev(f(3, 4), f(1, 4), "b")
            ]
        );
        assert_eq!(onsets("bd*2 sd", 1), vec![f(0, 1), f(1, 4), f(1, 2)]);
        assert_eq!(events("a/2", 4), vec![ev(f(0, 1), f(2, 1), "a"), ev(f(2, 1), f(2, 1), "a")]);
        assert_eq!(onsets("a*1.5", 2), vec![f(0, 1), f(2, 3), f(4, 3)]);
        // A slowed step starts only where its own cycle starts.
        assert_eq!(onsets("[a b]/2", 2), vec![f(0, 1), f(1, 1)]);
        assert_eq!(onsets("hh*8", 1).len(), 8);
    }

    #[test]
    fn the_combinators_match_the_notation() {
        let p = parse("bd [sd sd] ~ hh").unwrap();
        for (combined, written) in [
            (p.clone().fast(Frac::int(2)), "[bd [sd sd] ~ hh]*2"),
            (p.clone().slow(Frac::new(3, 2)), "[bd [sd sd] ~ hh]/1.5"),
        ] {
            let q = parse(written).unwrap();
            for c in 0..6 {
                let (a, b) = (Frac::int(c), Frac::int(c + 1));
                assert_eq!(events_in(&combined, a, b, 1), events_in(&q, a, b, 1), "{written}, cycle {c}");
            }
        }
    }

    #[test]
    fn euclidean_rhythms() {
        assert_eq!(bjorklund(3, 8), [true, false, false, true, false, false, true, false]);
        assert_eq!(bjorklund(5, 8), [true, false, true, true, false, true, true, false]);
        assert_eq!(bjorklund(2, 5), [true, false, true, false, false]);
        assert_eq!(bjorklund(4, 4), [true; 4]);
        assert_eq!(bjorklund(7, 16).iter().filter(|&&x| x).count(), 7);
        assert_eq!(onsets("bd(3,8)", 1), vec![f(0, 1), f(3, 8), f(3, 4)]);
        assert_eq!(events("bd(3,8)", 1)[0].1, f(1, 8));
        assert_eq!(onsets("bd(5,8)", 1), vec![f(0, 1), f(2, 8), f(3, 8), f(5, 8), f(6, 8)]);
        // Rotated right by 2, as Strudel's euclidRot: x..x..x. → x.x..x..
        assert_eq!(onsets("bd(3,8,2)", 1), vec![f(0, 1), f(2, 8), f(5, 8)]);
        assert_eq!(onsets("bd(3,8,8)", 1), onsets("bd(3,8)", 1));
        assert_eq!(onsets("bd( 3 , 8 )", 1).len(), 3);
        assert_eq!(onsets("bd(0,8)", 1), vec![]);
        assert_eq!(onsets("bd(9,8)", 1).len(), 8);
        // The step is what plays on the pulses: here, two hits each.
        assert_eq!(onsets("bd*2(3,8)", 1).len(), 6);
    }

    #[test]
    fn stacks_play_together() {
        assert_eq!(onsets("bd , hh*2", 1), vec![f(0, 1), f(0, 1), f(1, 2)]);
        assert_eq!(labels("[bd sd, hh hh hh]", 1), ["bd", "hh", "hh", "sd", "hh"]);
        assert_eq!(labels("<a b, c d e>", 3), ["a", "c", "b", "d", "a", "e"]);
    }

    #[test]
    fn weights_and_repeats() {
        assert_eq!(events("a@3 b", 1), vec![ev(f(0, 1), f(3, 4), "a"), ev(f(3, 4), f(1, 4), "b")]);
        assert_eq!(onsets("a!3 b", 1), vec![f(0, 1), f(1, 4), f(1, 2), f(3, 4)]);
        assert_eq!(onsets("a ! ! b", 1), onsets("a!3 b", 1));
        assert_eq!(onsets("a! b", 1), onsets("a a b", 1));
        assert_eq!(onsets("a@1.5 b@0.5", 1), vec![f(0, 1), f(3, 4)]);
    }

    #[test]
    fn names_numbers_and_variants() {
        assert_eq!(labels("bd sd:2 3 owl:1 great-tit", 1), ["bd", "sd:2", "#3", "owl:1", "great-tit"]);
        let p = parse("bd bd:1 sd bd").unwrap();
        assert_eq!(p.names(), ["bd", "sd"]);
    }

    #[test]
    fn rests_and_empty_patterns_are_silent() {
        for src in ["", "   ", "~", "~ ~", "[~ ~]", "<~>", "~*4", "~(3,8)"] {
            assert!(events(src, 4).is_empty(), "{src:?}");
        }
        assert!(parse("").unwrap().is_silent());
    }

    #[test]
    fn degrade_drops_by_hash_the_same_every_time() {
        let p = parse("hh*16?").unwrap();
        let kept: usize = (0..64).map(|c| events_in(&p, Frac::int(c), Frac::int(c + 1), 9).len()).sum();
        assert!((kept as f64 / (64.0 * 16.0) - 0.5).abs() < 0.05, "{kept}");
        assert_eq!(events_in(&p, Frac::ZERO, Frac::int(8), 9), events_in(&p, Frac::ZERO, Frac::int(8), 9));
        assert_ne!(events_in(&p, Frac::ZERO, Frac::int(8), 9), events_in(&p, Frac::ZERO, Frac::int(8), 10));
        let sparse = parse("hh*16?0.9").unwrap();
        let kept: usize = events_in(&sparse, Frac::ZERO, Frac::int(64), 9).len();
        assert!((kept as f64 / (64.0 * 16.0) - 0.1).abs() < 0.03, "{kept}");
        assert_eq!(events("a?0", 4).len(), 4);
        assert_eq!(events("a?1", 4).len(), 0);
        // Two `?` draw independently.
        let two = parse("[a?, b?]*16").unwrap();
        let e = events_in(&two, Frac::ZERO, Frac::int(4), 3);
        let a: Vec<_> = e.iter().filter(|x| x.2 == "a").map(|x| x.0).collect();
        let b: Vec<_> = e.iter().filter(|x| x.2 == "b").map(|x| x.0).collect();
        assert_ne!(a, b);
        // The combinator draws like the notation, with a salt of its own.
        let p = parse("hh*16").unwrap().degrade_by(0.5);
        let kept = events_in(&p, Frac::ZERO, Frac::int(64), 9).len();
        assert!((kept as f64 / (64.0 * 16.0) - 0.5).abs() < 0.05, "{kept}");
    }

    #[test]
    fn choice_picks_one_per_cycle() {
        let p = parse("a | b | c").unwrap();
        let got: Vec<String> = events_in(&p, Frac::ZERO, Frac::int(60), 5).into_iter().map(|e| e.2).collect();
        assert_eq!(got.len(), 60);
        for l in ["a", "b", "c"] {
            assert!(got.iter().filter(|x| *x == l).count() > 10, "{l}: {got:?}");
        }
        assert_eq!(
            got,
            events_in(&p, Frac::ZERO, Frac::int(60), 5).into_iter().map(|e| e.2).collect::<Vec<_>>()
        );
        assert_eq!(labels("[a a | b b]", 1).len(), 2);
    }

    /// The invariants: a query split anywhere returns what the whole query
    /// does, every event's onset lies inside the span asked for, and an
    /// event never spills out of its own step.
    #[test]
    fn queries_split_anywhere_agree() {
        let patterns = [
            "bd sd [hh hh] ~",
            "0(3,8) , ~ sd ~ sd , hh*8?0.3",
            "<0 ~ ~ 1> ~ ~ ~",
            "[a b c]*1.5 <d [e f]>/3",
            "a@3 b!2 [c | d e] f(5,8,3)",
            "<a b, c d e>*2?",
        ];
        let mut seedr = 12345u64;
        let mut rnd = |m: i64| {
            seedr = seedr.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            ((seedr >> 33) as i64) % m
        };
        for src in patterns {
            let p = parse(src).unwrap();
            let whole = events_in(&p, Frac::ZERO, Frac::int(8), 11);
            assert!(!whole.is_empty(), "{src}");
            // Block-sized pieces on the engine's grid, then random cuts.
            let mut pieces = Vec::new();
            let mut t = Frac::ZERO;
            while t < Frac::int(8) {
                let next = (t + Frac::new(1 + rnd(1 << 17), crate::seq::pattern::GRID)).min(Frac::int(8));
                let part = events_in(&p, t, next, 11);
                for e in &part {
                    assert!(e.0 >= t && e.0 < next, "{src}: {e:?} outside [{t:?}, {next:?})");
                }
                pieces.extend(part);
                t = next;
            }
            pieces.sort_by_key(|x| x.0);
            let mut sorted_whole = whole.clone();
            sorted_whole.sort_by_key(|x| x.0);
            // Ties in onset may come in another order; compare as multisets.
            let key = |v: &[(Frac, Frac, String)]| {
                let mut k: Vec<String> = v.iter().map(|e| format!("{:?}{:?}{}", e.0, e.1, e.2)).collect();
                k.sort();
                k
            };
            assert_eq!(key(&pieces), key(&whole), "{src}");
        }
        // "[a b c]*2 d": the first step is the first half of each cycle, and
        // all of its events stay in it.
        for (on, dur, l) in events("[a b c]*2 d", 4) {
            if l != "d" {
                let into = on - Frac::int(on.floor());
                assert!(into + dur <= f(1, 2), "{l} at {on:?}");
            }
        }
    }

    #[test]
    fn a_full_array_counts_what_it_drops() {
        let p = parse("a*100").unwrap();
        let mut out = Events::default();
        p.query(Frac::ZERO, Frac::ONE, 0, &mut out);
        assert_eq!((out.len(), out.overflow as usize), (MAX_EVENTS, 100 - MAX_EVENTS));
    }

    #[test]
    fn errors_say_where() {
        for (src, pos) in [
            ("[a b", 4),
            ("a*", 2),
            ("a*0", 3),
            ("a*500", 5),
            ("{a b}", 0),
            ("a(3)", 4),
            ("a(3,8,1,2)", 8),
            ("a , b | c", 6),
            ("a ]", 2),
            ("! a", 0),
            ("a?2", 3),
            ("a b.c", 3),
            ("$", 0),
        ] {
            let e = parse(src).expect_err(src);
            assert_eq!(e.pos, pos, "{src}: {e}");
        }
        assert!(parse(&"a ".repeat(300)).is_err());
        assert!(parse(&format!("{}a{}", "[".repeat(20), "]".repeat(20))).is_err());
        assert!(parse(&format!("{}a{}", "[".repeat(10), "]".repeat(10))).is_ok());
        // Time rescaled past MAX_SCALE: refused at parse, never overflowed at render.
        assert!(parse("[[[[a*127]*127]*127]*127]").is_err());
    }
}
