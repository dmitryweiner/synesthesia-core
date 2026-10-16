//! The evolutionary operators, ported from
//! `../synesthesia/src/genome/evolve.ts`. Pure: every function takes the RNG,
//! so a test can replay a search exactly.

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::dsp::rng::{gaussian, Rng};
use crate::schema::{schema, GeneDef, GeneKind};

use super::genes::{any_layer_on, gene_index, genes, is_gene_active, route_slots, Genome};

fn formula_enabled_idx() -> &'static [usize] {
    static IDX: OnceLock<Vec<usize>> = OnceLock::new();
    IDX.get_or_init(|| schema().formulas.iter().map(|f| gene_index(&format!("a.{}.enabled", f.id))).collect())
}

fn cont_idx() -> &'static [usize] {
    static IDX: OnceLock<Vec<usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        genes().iter().enumerate().filter(|(_, g)| g.kind == GeneKind::Cont).map(|(i, _)| i).collect()
    })
}

fn explicit_coupling_idx() -> &'static [usize] {
    static IDX: OnceLock<Vec<usize>> = OnceLock::new();
    // The explicit sound→image couplings are the ones with a floor; the card
    // couplings are free. Both live under `c.` — the explicit ones are those
    // the web app lists after the card ones.
    IDX.get_or_init(|| {
        const EXPLICIT: [&str; 4] = ["loudToPulse", "onsetToFlash", "onsetToSeed", "spectrumToTint"];
        EXPLICIT.iter().map(|k| gene_index(&format!("c.{k}"))).collect()
    })
}

fn fx_toggles() -> &'static [usize] {
    static IDX: OnceLock<Vec<usize>> = OnceLock::new();
    IDX.get_or_init(|| schema().fx_on_keys.iter().map(|k| gene_index(&format!("fx.{k}"))).collect())
}

fn fx_choices() -> &'static [usize] {
    static IDX: OnceLock<Vec<usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        ["fx.filterType", "fx.chorusMode", "fx.phaserStages"].iter().map(|id| gene_index(id)).collect()
    })
}

fn card_toggles() -> &'static [usize] {
    static IDX: OnceLock<Vec<usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        genes()
            .iter()
            .enumerate()
            .filter(|(_, g)| g.kind == GeneKind::Bool && g.group.starts_with("v."))
            .map(|(i, _)| i)
            .collect()
    })
}

fn lfo_shape_idx() -> &'static [usize] {
    static IDX: OnceLock<Vec<usize>> = OnceLock::new();
    IDX.get_or_init(|| (0..schema().lfo_count).map(|i| gene_index(&format!("lfo.{i}.shape"))).collect())
}

/// The sample/drum layers' genes (`seq.*`), appended after the web app's.
fn seq_idx() -> &'static [usize] {
    static IDX: OnceLock<Vec<usize>> = OnceLock::new();
    IDX.get_or_init(|| {
        genes().iter().enumerate().filter(|(_, g)| g.group.starts_with("seq")).map(|(i, _)| i).collect()
    })
}

/// What the layers' genes hold for a point without layers.
fn no_layers() -> &'static Genome {
    static G: OnceLock<Genome> = OnceLock::new();
    G.get_or_init(|| super::codec::encode_genome(&crate::state::AppState::new()))
}

fn gate_idx() -> &'static [Option<usize>] {
    static IDX: OnceLock<Vec<Option<usize>>> = OnceLock::new();
    IDX.get_or_init(|| genes().iter().map(|g| g.active_if.as_deref().map(gene_index)).collect())
}

fn is_gate() -> &'static [bool] {
    static IS: OnceLock<Vec<bool>> = OnceLock::new();
    IS.get_or_init(|| {
        let gates: HashSet<usize> = gate_idx().iter().flatten().copied().collect();
        (0..genes().len()).map(|i| gates.contains(&i)).collect()
    })
}

#[derive(Clone, Default)]
pub struct MutateOptions<'a> {
    /// Std-dev of the gaussian step, in normalized units.
    pub sigma: f64,
    /// How many active continuous genes get a random step.
    pub k: usize,
    /// Probability of one structural (discrete) move.
    pub structural_prob: f64,
    /// Previous step to continue along; only continuous genes are read.
    pub momentum: Option<&'a [f64]>,
    pub momentum_weight: f64,
    /// Genes that must not change — the dimensions of a rejected step.
    pub avoid: Option<&'a HashSet<usize>>,
}

fn pick(rng: &mut dyn Rng, list: &[usize]) -> usize {
    list[((rng.next() * list.len() as f64) as usize).min(list.len() - 1)]
}

fn random_choice(rng: &mut dyn Rng, def: &GeneDef, not_equal: f64) -> f64 {
    let n = (def.max - def.min + 1.0) as usize;
    if n <= 1 {
        return def.min;
    }
    let mut v = def.min + (rng.next() * n as f64).floor();
    if v == not_equal {
        v = def.min + ((v - def.min + 1.0 + (rng.next() * (n - 1) as f64).floor()) % n as f64);
    }
    v
}

pub fn enabled_formula_count(g: &[f64]) -> usize {
    formula_enabled_idx().iter().filter(|i| g[**i] == 1.0).count()
}

/// Indices where two genomes differ.
pub fn diff_dims(a: &[f64], b: &[f64]) -> Vec<usize> {
    (0..a.len().min(b.len())).filter(|i| a[*i] != b[*i]).collect()
}

/// How close two genes may be and still count as the same value: far tighter
/// than any slider step, far looser than the last bits.
pub const SAME_GENE: f64 = 1e-9;

/// Whether two genomes are the same point, to the precision a point is kept
/// in — the web app's `samePoint` (`src/ui/settingsModel.ts`), which compares
/// the parameters at 9 significant digits.
///
/// Encoding a point and decoding it again moves the last bits of the
/// log-scaled genes (a rate, a filter frequency), so an app that asks "did
/// the user change anything in Settings?" must not ask [`diff_dims`], which
/// is exact: it would call every close an edit.
pub fn same_genome(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= SAME_GENE)
}

/// Scales the explicit sound→image couplings up until they sum to the floor.
fn lift_coupling_floor(g: &mut Genome) {
    let floor = schema().coupling_floor;
    let idx = explicit_coupling_idx();
    for _ in 0..4 {
        let sum: f64 = idx.iter().map(|i| g[*i]).sum();
        if sum >= floor - 1e-12 {
            return;
        }
        if sum <= 1e-9 {
            for i in idx {
                g[*i] = floor / idx.len() as f64;
            }
            return;
        }
        let free: Vec<usize> = idx.iter().copied().filter(|i| g[*i] < 1.0).collect();
        if free.is_empty() {
            return;
        }
        let free_sum: f64 = free.iter().map(|i| g[*i]).sum();
        let need = floor - sum;
        for i in free.iter() {
            g[*i] = if free_sum > 0.0 {
                (g[*i] * (1.0 + need / free_sum)).clamp(0.0, 1.0)
            } else {
                (g[*i] + need / free.len() as f64).clamp(0.0, 1.0)
            };
        }
    }
}

/// Enforces 1..=MAX_ENABLED_FORMULAS enabled formulas and the coupling floor.
pub fn repair(g: &Genome, rng: &mut dyn Rng) -> Genome {
    let mut out = g.clone();
    lift_coupling_floor(&mut out);
    let on: Vec<usize> = formula_enabled_idx().iter().copied().filter(|i| out[*i] == 1.0).collect();
    if on.is_empty() {
        let i = pick(rng, formula_enabled_idx());
        out[i] = 1.0;
    } else {
        let mut extra = on.len() as i64 - schema().max_enabled_formulas as i64;
        let mut pool = on;
        while extra > 0 && !pool.is_empty() {
            let j = (rng.next() * pool.len() as f64) as usize;
            let j = j.min(pool.len() - 1);
            out[pool[j]] = 0.0;
            pool.remove(j);
            extra -= 1;
        }
    }
    out
}

// --- structural moves ----------------------------------------------------

fn toggle_one_of(g: &mut Genome, rng: &mut dyn Rng, list: &[usize]) -> bool {
    let i = pick(rng, list);
    g[i] = if g[i] == 1.0 { 0.0 } else { 1.0 };
    true
}

fn rechoose_one_of(g: &mut Genome, rng: &mut dyn Rng, list: &[usize], only_active: bool) -> bool {
    let candidates: Vec<usize> =
        list.iter().copied().filter(|i| !only_active || is_gene_active(g, *i)).collect();
    if candidates.is_empty() {
        return false;
    }
    let i = pick(rng, &candidates);
    g[i] = random_choice(rng, &genes()[i], g[i]);
    true
}

fn route_move(g: &mut Genome, rng: &mut dyn Rng, avoid: &HashSet<usize>) -> bool {
    let slots: Vec<usize> =
        (0..route_slots()).filter(|i| !avoid.contains(&gene_index(&format!("route.{i}.depth")))).collect();
    if slots.is_empty() {
        return false;
    }
    let active: Vec<usize> =
        slots.iter().copied().filter(|i| g[gene_index(&format!("route.{i}.on"))] == 1.0).collect();
    let inactive: Vec<usize> =
        slots.iter().copied().filter(|i| g[gene_index(&format!("route.{i}.on"))] == 0.0).collect();
    let r = rng.next();
    if !inactive.is_empty() && (active.is_empty() || r < 0.45) {
        let s = pick(rng, &inactive);
        g[gene_index(&format!("route.{s}.on"))] = 1.0;
        g[gene_index(&format!("route.{s}.src"))] = (rng.next() * schema().lfo_count as f64).floor();
        let t_max = genes()[gene_index(&format!("route.{s}.target"))].max;
        g[gene_index(&format!("route.{s}.target"))] = (rng.next() * (t_max + 1.0)).floor();
        // Depth in the outer halves of [-1,1] so a new route is audible.
        let mag = 0.15 + rng.next() * 0.45;
        let sign = if rng.next() < 0.5 { -1.0 } else { 1.0 };
        g[gene_index(&format!("route.{s}.depth"))] = (0.5 + sign * mag).clamp(0.0, 1.0);
        g[gene_index(&format!("route.{s}.exp"))] = if rng.next() < 0.5 { 1.0 } else { 0.0 };
        return true;
    }
    if active.is_empty() {
        return false;
    }
    let s = pick(rng, &active);
    if r < 0.7 {
        g[gene_index(&format!("route.{s}.on"))] = 0.0;
    } else {
        let ti = gene_index(&format!("route.{s}.target"));
        g[ti] = random_choice(rng, &genes()[ti], g[ti]);
    }
    true
}

/// One of the layers (PLAN-SAMPLES-SCRIPTS.md): switch one on — with a
/// sound, a built-in rhythm and a modest gain — or, when two or more are
/// on, one off.
fn layer_toggle(g: &mut Genome, rng: &mut dyn Rng) -> bool {
    let layers = schema().seq_layers;
    let on = |g: &Genome, i: usize| g[gene_index(&format!("seq.{i}.on"))] == 1.0;
    let (active, inactive): (Vec<usize>, Vec<usize>) = (0..layers).partition(|&i| on(g, i));
    if !inactive.is_empty() && (active.len() < 2 || rng.next() < 0.5) {
        let i = pick(rng, &inactive);
        g[gene_index(&format!("seq.{i}.on"))] = 1.0;
        let si = gene_index(&format!("seq.{i}.sound"));
        g[si] = random_choice(rng, &genes()[si], -1.0);
        set_bed_by_sound(g, i);
        rechoose_rhythm(g, rng, i);
        g[gene_index(&format!("seq.{i}.gain"))] = 0.3 + rng.next() * 0.3;
        return true;
    }
    if active.len() < 2 {
        return false;
    }
    g[gene_index(&format!("seq.{}.on", pick(rng, &active)))] = 0.0;
    true
}

/// A layer that changes its sound plays it as what it was cut for: a bed
/// for crickets, hits for an owl.
fn set_bed_by_sound(g: &mut Genome, i: usize) {
    let sound = g[gene_index(&format!("seq.{i}.sound"))] as usize;
    let bed = schema().seq_sounds.get(sound).is_some_and(|s| s.mode == crate::seq::Mode::Bed);
    g[gene_index(&format!("seq.{i}.bed"))] = f64::from(bed);
}

/// A built-in rhythm, never "custom" (0): the search cannot write one.
fn rechoose_rhythm(g: &mut Genome, rng: &mut dyn Rng, i: usize) {
    let ri = gene_index(&format!("seq.{i}.rhythm"));
    let n = genes()[ri].max as usize;
    let mut v = 1 + ((rng.next() * n as f64) as usize).min(n - 1);
    if v as f64 == g[ri] && n > 1 {
        v = 1 + v % n;
    }
    g[ri] = v as f64;
}

fn layer_rechoose(g: &mut Genome, rng: &mut dyn Rng, sound: bool) -> bool {
    let active: Vec<usize> =
        (0..schema().seq_layers).filter(|i| g[gene_index(&format!("seq.{i}.on"))] == 1.0).collect();
    if active.is_empty() {
        return false;
    }
    let i = pick(rng, &active);
    if sound {
        let si = gene_index(&format!("seq.{i}.sound"));
        g[si] = random_choice(rng, &genes()[si], g[si]);
        set_bed_by_sound(g, i);
    } else {
        rechoose_rhythm(g, rng, i);
    }
    true
}

const MOVE_WEIGHTS: [f64; 7] = [3.0, 2.0, 1.0, 1.0, 1.5, 1.0, 2.5];
/// The binaural beat on or off (PLAN-BINAURAL.md): a rare move — a point
/// beats or it does not, the way it is bright or dark, and the search
/// should not flip it often.
const BIN_MOVE_WEIGHT: f64 = 0.5;
/// Where a beat the search switches on starts: theta to alpha, Hz.
const BIN_BEAT_START: (f64, f64) = (4.0, 12.0);

/// The binaural beat: switched off, or on at a beat between theta and
/// alpha (from there the continuous moves take it anywhere in its range).
fn bin_toggle(g: &mut Genome, rng: &mut dyn Rng) -> bool {
    let on = gene_index("bin.on");
    if g[on] == 1.0 {
        g[on] = 0.0;
        return true;
    }
    g[on] = 1.0;
    let beat = gene_index("bin.beat");
    let (lo, hi) = BIN_BEAT_START;
    g[beat] = super::genes::gene_from_value(&genes()[beat], lo + rng.next() * (hi - lo));
    true
}
/// The layers' three moves — toggle one, rechoose its sound, its rhythm —
/// together weighted like the formula toggles. They join only when a layer
/// is on: the search varies the layers the user put in, it does not add
/// layers to a point without any (the scout does not hear them — decided
/// with the user 2026-10-15). A point without layers mutates exactly as it
/// did before there were layers.
const LAYER_MOVE_WEIGHTS: [f64; 3] = [1.0, 1.0, 1.0];

fn structural_move(g: &mut Genome, rng: &mut dyn Rng, avoid: &HashSet<usize>) {
    let layers = any_layer_on(g);
    let mut weights: Vec<f64> = MOVE_WEIGHTS.iter().chain(&[BIN_MOVE_WEIGHT]).copied().collect();
    if layers {
        weights.extend(LAYER_MOVE_WEIGHTS);
    }
    let sum: f64 = weights.iter().sum();
    for _ in 0..4 {
        let mut r = rng.next() * sum;
        for (which, weight) in weights.iter().enumerate() {
            r -= weight;
            if r > 0.0 {
                continue;
            }
            let done = match which {
                0 => toggle_one_of(g, rng, formula_enabled_idx()),
                1 => toggle_one_of(g, rng, fx_toggles()),
                2 => rechoose_one_of(g, rng, fx_choices(), true),
                3 => toggle_one_of(g, rng, card_toggles()),
                4 => rechoose_one_of(g, rng, &[gene_index("v.palette.paletteId")], false),
                5 => rechoose_one_of(g, rng, lfo_shape_idx(), false),
                6 => route_move(g, rng, avoid),
                7 => bin_toggle(g, rng),
                8 => layer_toggle(g, rng),
                9 => layer_rechoose(g, rng, true),
                _ => layer_rechoose(g, rng, false),
            };
            if done {
                return;
            }
            break;
        }
    }
}

/// One proposal: momentum along the previous step, a sparse gaussian kick on
/// `k` active continuous genes, and — with `structural_prob` — one discrete
/// move. Inactive genes are never touched.
pub fn mutate(g: &Genome, rng: &mut dyn Rng, opts: &MutateOptions) -> Genome {
    let mut out = g.clone();
    let empty = HashSet::new();
    let avoid = opts.avoid.unwrap_or(&empty);

    if opts.structural_prob > 0.0 && rng.next() < opts.structural_prob {
        structural_move(&mut out, rng, avoid);
    }

    if let Some(momentum) = opts.momentum {
        let w = opts.momentum_weight;
        if w != 0.0 {
            for i in cont_idx() {
                let m = momentum.get(*i).copied().unwrap_or(0.0);
                if m == 0.0 || avoid.contains(i) || !is_gene_active(&out, *i) {
                    continue;
                }
                out[*i] = (out[*i] + w * m).clamp(0.0, 1.0);
            }
        }
    }

    if opts.k > 0 && opts.sigma > 0.0 {
        let mut candidates: Vec<usize> =
            cont_idx().iter().copied().filter(|i| !avoid.contains(i) && is_gene_active(&out, *i)).collect();
        let n = opts.k.min(candidates.len());
        for picked in 0..n {
            let j = picked + (rng.next() * (candidates.len() - picked) as f64) as usize;
            let j = j.min(candidates.len() - 1);
            candidates.swap(picked, j);
            let i = candidates[picked];
            out[i] = (out[i] + gaussian(rng) * opts.sigma).clamp(0.0, 1.0);
        }
    }
    out
}

/// A fully random but sane genome: 1–3 formulas, sparse FX and routes.
pub fn random_genome(rng: &mut dyn Rng) -> Genome {
    let mut g: Genome = vec![0.0; genes().len()];
    // The layers' genes take no draws: a random point has no layers, and
    // the web app's genes get the same numbers they always did.
    for (i, d) in genes().iter().enumerate().take(seq_idx()[0]) {
        g[i] = match d.kind {
            GeneKind::Cont => rng.next(),
            GeneKind::Choice => d.min + (rng.next() * (d.max - d.min + 1.0)).floor(),
            GeneKind::Bool => 0.0,
        };
    }
    for i in fx_toggles() {
        g[*i] = if rng.next() < 0.4 { 1.0 } else { 0.0 };
    }
    g[gene_index("fx.limiterOn")] = 1.0;
    for i in card_toggles() {
        g[*i] = if rng.next() < 0.6 { 1.0 } else { 0.0 };
    }
    for s in 0..route_slots() {
        g[gene_index(&format!("route.{s}.on"))] = if rng.next() < 0.25 { 1.0 } else { 0.0 };
    }
    let count = 1 + (rng.next() * 3.0) as usize;
    let mut pool: Vec<usize> = formula_enabled_idx().to_vec();
    for _ in 0..count {
        if pool.is_empty() {
            break;
        }
        let j = ((rng.next() * pool.len() as f64) as usize).min(pool.len() - 1);
        g[pool[j]] = 1.0;
        pool.remove(j);
    }
    for &i in seq_idx() {
        g[i] = no_layers()[i];
    }
    // Keep random gains tame: 0.05..0.5 of the slider.
    for f in &schema().formulas {
        g[gene_index(&format!("a.{}.gain", f.id))] = 0.05 + rng.next() * 0.45;
    }
    let mut g = repair(&g, rng);
    // Space (PLAN-BINAURAL.md), drawn after everything else so the earlier
    // genes get the numbers they always did: every voice somewhere around
    // the head, the point 0.3–0.8 wide — a 🎲 point is not flat by
    // accident. No beat: the search switches it on (`bin_toggle`).
    for f in &schema().formulas {
        let i = gene_index(&format!("a.{}.az", f.id));
        g[i] = if g[gene_index(&format!("a.{}.enabled", f.id))] == 1.0 {
            RANDOM_AZ.0 + rng.next() * (RANDOM_AZ.1 - RANDOM_AZ.0)
        } else {
            no_layers()[i]
        };
    }
    g[gene_index("bin.width")] = 0.3 + rng.next() * 0.5;
    g
}

/// A random voice's azimuth, normalized: ±72° — wide, but not glued to an ear.
const RANDOM_AZ: (f64, f64) = (0.1, 0.9);

/// Morph position between two genomes. Continuous genes interpolate; discrete
/// ones switch as soon as `t > 0` — except a gate closing, which stays open
/// until `t = 1` so the genes it gates can fade to their neutral value first.
pub fn lerp_genome(a: &Genome, b: &Genome, t: f64) -> Genome {
    if t >= 1.0 {
        return b.clone();
    }
    let mut out = vec![0.0; a.len()];
    for (i, d) in genes().iter().enumerate() {
        if d.kind != GeneKind::Cont {
            let closing = is_gate()[i] && a[i] == 1.0 && b[i] == 0.0;
            out[i] = if t > 0.0 && !closing { b[i] } else { a[i] };
            continue;
        }
        let (mut from, mut to) = (a[i], b[i]);
        if let (Some(gi), Some(neutral)) = (gate_idx()[i], d.neutral) {
            if a[gi] == 0.0 && b[gi] == 1.0 {
                from = neutral;
            } else if a[gi] == 1.0 && b[gi] == 0.0 {
                to = neutral;
            }
        }
        out[i] = from + (to - from) * t;
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChangeDir {
    Up,
    Down,
    On,
    Off,
    Switch,
}

#[derive(Clone, Debug)]
pub struct GeneChange {
    pub id: String,
    pub label: String,
    pub from: f64,
    pub to: f64,
    pub dir: ChangeDir,
}

/// What changed between two genomes, in words.
pub fn diff_summary(a: &Genome, b: &Genome) -> Vec<GeneChange> {
    diff_dims(a, b)
        .into_iter()
        .map(|i| {
            let d = &genes()[i];
            let dir = match d.kind {
                GeneKind::Bool => {
                    if b[i] == 1.0 {
                        ChangeDir::On
                    } else {
                        ChangeDir::Off
                    }
                }
                GeneKind::Choice => ChangeDir::Switch,
                GeneKind::Cont => {
                    if b[i] > a[i] {
                        ChangeDir::Up
                    } else {
                        ChangeDir::Down
                    }
                }
            };
            GeneChange { id: d.id.clone(), label: d.label.clone(), from: a[i], to: b[i], dir }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::rng::Mulberry32;
    use crate::genome::codec::{decode_genome, encode_genome, is_valid_genome};
    use crate::state::presets;

    fn opts() -> MutateOptions<'static> {
        MutateOptions { sigma: 0.15, k: 6, structural_prob: 1.0, ..MutateOptions::default() }
    }

    #[test]
    fn a_point_without_layers_never_grows_one() {
        let mut rng = Mulberry32::new(5);
        for p in presets().iter().filter(|p| p.state.seq.is_silent()) {
            let mut g = encode_genome(&p.state);
            for _ in 0..200 {
                g = mutate(&g, &mut rng, &opts());
                for &i in seq_idx() {
                    assert_eq!(g[i], no_layers()[i], "{}: {} moved", p.name, genes()[i].id);
                }
            }
        }
        let r = random_genome(&mut rng);
        assert!(!any_layer_on(&r) && is_valid_genome(&r));
    }

    #[test]
    fn the_layers_a_point_has_are_varied() {
        use crate::seq::{Layer, SeqState};
        let mut state = presets()[0].state.clone();
        state.seq = SeqState {
            cps: 0.5,
            layers: vec![Layer {
                on: true,
                sound: "owl".into(),
                pattern: "0(3,8)".into(),
                ..Layer::default()
            }],
        };
        let start = encode_genome(&state);
        let mut rng = Mulberry32::new(9);
        let (mut sounds, mut rhythms, mut more) = (0, 0, 0);
        for _ in 0..400 {
            let g = mutate(&start, &mut rng, &opts());
            assert!(is_valid_genome(&g));
            let d = decode_genome(&g);
            let on: Vec<&Layer> = d.seq.layers.iter().filter(|l| l.on).collect();
            assert!(!on.is_empty(), "the last layer is never switched off");
            for l in &on {
                assert!(!l.pattern.is_empty(), "the search picks built-in rhythms only");
                let cut = &schema().seq_sounds.iter().find(|s| s.name == l.sound).unwrap().mode;
                if l.sound != "owl" {
                    assert_eq!(&l.mode, cut, "{}: played as it was cut", l.sound);
                    sounds += 1;
                }
                if l.pattern != "0(3,8)" {
                    rhythms += 1;
                }
            }
            if on.len() > 1 {
                more += 1;
            }
        }
        assert!(sounds > 10 && rhythms > 10 && more > 10, "{sounds} {rhythms} {more}");
    }

    #[test]
    fn a_point_that_went_through_the_codec_is_still_the_same_point() {
        for p in presets() {
            let g = encode_genome(&p.state);
            let round_trip = encode_genome(&decode_genome(&g));
            assert!(same_genome(&g, &round_trip), "{} came back as another point", p.name);
            let mut nudged = g.clone();
            let i = genes().iter().position(|d| d.kind == GeneKind::Cont).expect("a continuous gene");
            nudged[i] += 1e-3; // the smallest move a slider makes
            assert!(!same_genome(&g, &nudged), "a slider's step is a change");
        }
    }

    #[test]
    fn a_random_genome_is_valid_and_playable() {
        let mut rng = Mulberry32::new(7);
        for _ in 0..30 {
            let g = random_genome(&mut rng);
            assert!(is_valid_genome(&g));
            let n = enabled_formula_count(&g);
            assert!((1..=schema().max_enabled_formulas).contains(&n), "{n} formulas");
        }
    }

    #[test]
    fn repair_keeps_the_formula_count_and_the_coupling_floor() {
        let mut rng = Mulberry32::new(11);
        let mut g = encode_genome(&presets()[0].state);
        for i in formula_enabled_idx() {
            g[*i] = 1.0;
        }
        for i in explicit_coupling_idx() {
            g[*i] = 0.0;
        }
        let fixed = repair(&g, &mut rng);
        assert_eq!(enabled_formula_count(&fixed), schema().max_enabled_formulas);
        let sum: f64 = explicit_coupling_idx().iter().map(|i| fixed[*i]).sum();
        assert!(sum >= schema().coupling_floor - 1e-9, "coupling sum {sum}");
    }

    #[test]
    fn mutation_never_touches_a_gene_behind_a_closed_gate() {
        let mut rng = Mulberry32::new(3);
        let g = encode_genome(&presets()[2].state);
        let opts = MutateOptions { sigma: 0.3, k: 40, structural_prob: 0.0, ..MutateOptions::default() };
        let out = mutate(&g, &mut rng, &opts);
        for i in 0..g.len() {
            if !is_gene_active(&g, i) {
                assert_eq!(g[i], out[i], "gene {} moved behind a closed gate", genes()[i].id);
            }
        }
    }

    #[test]
    fn a_morph_ends_where_it_was_going() {
        let a = encode_genome(&presets()[0].state);
        let b = encode_genome(&presets()[5].state);
        assert_eq!(lerp_genome(&a, &b, 1.0), b);
        // At t = 0 everything still sits where it was, except the genes whose
        // gate is about to open: those start from their neutral value, so a
        // formula enters at gain 0 rather than at whatever it held before.
        let start = lerp_genome(&a, &b, 0.0);
        for (i, d) in genes().iter().enumerate() {
            if start[i] == a[i] {
                continue;
            }
            let gate = gate_idx()[i].expect("only a gated gene may start elsewhere");
            assert_eq!(a[gate], 0.0);
            assert_eq!(b[gate], 1.0);
            assert_eq!(Some(start[i]), d.neutral, "{} should start at its neutral", d.id);
        }
        let mid = lerp_genome(&a, &b, 0.5);
        for (i, d) in genes().iter().enumerate() {
            if d.kind == GeneKind::Cont && a[i] != b[i] && gate_idx()[i].is_none() {
                assert!((mid[i] - 0.5 * (a[i] + b[i])).abs() < 1e-9, "{} not halfway", d.id);
            }
        }
    }

    #[test]
    fn a_closing_gate_stays_open_until_the_end_of_the_morph() {
        let mut a = encode_genome(&presets()[0].state);
        let mut b = a.clone();
        let gate = gene_index("a.additive.enabled");
        let gain = gene_index("a.additive.gain");
        a[gate] = 1.0;
        a[gain] = 0.8;
        b[gate] = 0.0;
        let mid = lerp_genome(&a, &b, 0.5);
        assert_eq!(mid[gate], 1.0, "the formula must keep sounding while it fades");
        assert!(mid[gain] < 0.8, "and its gain must be on the way to neutral");
        assert_eq!(lerp_genome(&a, &b, 1.0)[gate], 0.0);
    }

    #[test]
    fn the_diff_says_what_changed() {
        let a = encode_genome(&presets()[0].state);
        let b = encode_genome(&presets()[1].state);
        let changes = diff_summary(&a, &b);
        assert!(!changes.is_empty());
        assert!(changes.iter().any(|c| matches!(c.dir, ChangeDir::On | ChangeDir::Off)));
    }
}
