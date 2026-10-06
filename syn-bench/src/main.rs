//! The sound bench (synesthesia PLAN-CORE.md C6): render points with the
//! core and measure them, without a browser. It is the web app's
//! `scripts/analyze.mjs` sound modes, moved here with the sound — the same
//! flags, the same columns:
//!
//! ```text
//! syn-bench                                   every built-in preset, 30 s @ 22050 Hz
//! syn-bench --preset 0,3 --secs 40 --sr 22050
//! syn-bench --token <#s= link or token>       a shared point
//! syn-bench --mutants 6 --random 12 --seed 1  + 👍/👎 proposals per preset, + random points
//! syn-bench --repeat 4                        render k uses seed k for every point: mean ± sd
//! syn-bench --character --ref 0,3,5,6,8       + character columns, distance to a reference group
//! syn-bench --wav DIR --png DIR --json FILE   listen, look (a log waterfall), keep the numbers
//! syn-bench --onsets [--secs 16]              onset hits and the swell's swing per point
//! syn-bench --switch 0,3,10,7 [--at 20]       clicks at preset switches (the apps' fade-out → switch → fade-in)
//! syn-bench --configs 30@22050,24@11025       does a cheap render rank like an expensive one? (Spearman ρ)
//! ```
//!
//! Points render in parallel on every core; each render is deterministic,
//! so a run repeats exactly. Unlike the browser's convolver, the core's FDN
//! reverb has no seeded room: `--repeat` varies only the noise generators'
//! seeds, and a point without noise reads ±0.00.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Instant;

use rayon::prelude::*;
use syn_core::analysis::character::{analyze_character, Character};
use syn_core::analysis::clicks::detect_clicks;
use syn_core::analysis::fractal::{analyze_sound, SoundAnalysis};
use syn_core::analysis::spectrogram::{log_spectrogram, SpectrogramOptions};
use syn_core::dsp::rng::Mulberry32;
use syn_core::genome::codec::{decode_genome, encode_genome};
use syn_core::genome::evolve::random_genome;
use syn_core::genome::explorer::{Explorer, ExplorerOptions};
use syn_core::share::decode_token;
use syn_core::state::{presets, AppState};
use syn_core::{render_offline, Engine, BLOCK};
use syn_player::Player;

struct Args {
    flags: BTreeMap<String, String>,
}

impl Args {
    fn parse() -> Self {
        let mut flags = BTreeMap::new();
        let mut it = std::env::args().skip(1).peekable();
        while let Some(a) = it.next() {
            let Some(name) = a.strip_prefix("--") else { continue };
            let value = match it.peek() {
                Some(v) if !v.starts_with("--") => it.next().unwrap_or_default(),
                _ => String::new(),
            };
            flags.insert(name.to_string(), value);
        }
        Args { flags }
    }
    fn has(&self, k: &str) -> bool {
        self.flags.contains_key(k)
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.flags.get(k).map(String::as_str).filter(|v| !v.is_empty())
    }
    fn num(&self, k: &str, default: f64) -> f64 {
        self.get(k).and_then(|v| v.parse().ok()).unwrap_or(default)
    }
    fn list(&self, k: &str) -> Option<Vec<usize>> {
        self.get(k).map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
    }
}

struct Point {
    group: &'static str,
    label: String,
    state: AppState,
}

/// The points to measure, in the web bench's order: the shared one, each
/// preset with its proposals, then the random ones.
fn candidates(a: &Args) -> Vec<Point> {
    let mut out = Vec::new();
    let mut rng = Mulberry32::new(a.num("seed", 1.0) as u32);
    if let Some(t) = a.get("token") {
        match decode_token(t) {
            Some(s) => out.push(Point {
                group: "token",
                label: s.preset_name.clone().unwrap_or_else(|| "shared point".into()),
                state: s,
            }),
            None => eprintln!("--token: not a point"),
        }
    }
    let all: Vec<usize> = (0..presets().len()).collect();
    let idx = a.list("preset").unwrap_or(if a.has("token") { Vec::new() } else { all });
    let mutants = a.num("mutants", 0.0) as usize;
    for i in idx {
        let Some(p) = presets().get(i) else { continue };
        out.push(Point { group: "preset", label: format!("{i}: {}", p.name), state: p.state.clone() });
        for m in 0..mutants {
            let mut ex = Explorer::new(encode_genome(&p.state), ExplorerOptions::default());
            let g = if m % 2 == 0 {
                ex.like(None, &mut rng).clone()
            } else {
                ex.like(None, &mut rng);
                ex.dislike(None, &mut rng).clone()
            };
            let thumb = if m % 2 == 0 { "👍" } else { "👎" };
            out.push(Point { group: "mutant", label: format!("  {thumb} of {i}"), state: decode_genome(&g) });
        }
    }
    for r in 0..a.num("random", 0.0) as usize {
        out.push(Point {
            group: "random",
            label: format!("random {r}"),
            state: decode_genome(&random_genome(&mut rng)),
        });
    }
    out
}

fn file_name(label: &str) -> String {
    let s: String = label.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    s.split('_').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("_")
}

fn write_wav(dir: &str, label: &str, samples: &[f32], sr: f64) {
    let _ = fs::create_dir_all(dir);
    let path = Path::new(dir).join(format!("{}.wav", file_name(label)));
    if let Err(e) = fs::write(&path, syn_core::wav::encode_mono(samples, sr as u32)) {
        eprintln!("{}: {e}", path.display());
    }
}

/// A log-frequency waterfall (30 Hz – 12 kHz, 72 dB, brighter = louder) over
/// the loudness curve — an agent can't hear, but it can read a picture.
fn write_png(dir: &str, label: &str, samples: &[f32], sr: f64) {
    let (cols, rows, strip) = (1200usize, 320usize, 80usize);
    let s = log_spectrogram(samples, sr, SpectrogramOptions { columns: cols, rows, ..Default::default() });
    let top = s.db.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let (w, h) = (cols, rows + strip);
    let mut px = vec![16u8; w * h * 3];
    for r in 0..rows {
        for c in 0..cols {
            let t = ((s.db[r * cols + c] - (top - 72.0)) / 72.0).clamp(0.0, 1.0);
            // one hue, dark to light
            let rgb =
                [(20.0 + 200.0 * t * t) as u8, (30.0 + 190.0 * t) as u8, (60.0 + 195.0 * t.sqrt()) as u8];
            px[(r * w + c) * 3..(r * w + c) * 3 + 3].copy_from_slice(&rgb);
        }
    }
    let (lo, hi) =
        s.loudness.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| (a.min(*v), b.max(*v)));
    let span = (hi - lo).max(1.0);
    for c in 0..cols {
        let y = rows + strip - 2 - (((s.loudness[c] - lo) / span) * (strip - 4) as f32) as usize;
        px[(y * w + c) * 3..(y * w + c) * 3 + 3].copy_from_slice(&[230, 230, 230]);
    }
    let _ = fs::create_dir_all(dir);
    let path = Path::new(dir).join(format!("{}.png", file_name(label)));
    let write = || -> Result<(), Box<dyn std::error::Error>> {
        let file = fs::File::create(&path)?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(&px)?;
        Ok(())
    };
    if let Err(e) = write() {
        eprintln!("{}: {e}", path.display());
    }
}

fn fmt(v: f64, d: usize) -> String {
    if v.is_finite() {
        format!("{v:.d$}")
    } else {
        "  — ".into()
    }
}

fn mean(xs: &[f64]) -> f64 {
    let f: Vec<f64> = xs.iter().copied().filter(|x| x.is_finite()).collect();
    if f.is_empty() {
        f64::NAN
    } else {
        f.iter().sum::<f64>() / f.len() as f64
    }
}

fn sd(xs: &[f64]) -> f64 {
    let m = mean(xs);
    let f: Vec<f64> = xs.iter().copied().filter(|x| x.is_finite()).collect();
    if f.is_empty() {
        f64::NAN
    } else {
        (f.iter().map(|x| (x - m).powi(2)).sum::<f64>() / f.len() as f64).sqrt()
    }
}

const CHARACTER_KEYS: [&str; 7] =
    ["dropout", "swing", "lowShare", "harmonicity", "roughness", "motion1s", "motion10s"];

fn metrics(a: &SoundAnalysis, c: &Character) -> BTreeMap<&'static str, f64> {
    BTreeMap::from([
        ("score", a.score),
        ("envBeta", a.env_beta),
        ("centroidBeta", a.centroid_beta),
        ("envHiguchi", a.env_higuchi),
        ("boxDim", a.box_dim),
        ("loudness", a.loudness),
        ("dropout", c.dropout),
        ("swing", c.swing),
        ("lowShare", c.low_share),
        ("harmonicity", c.harmonicity),
        ("roughness", c.roughness),
        ("motion1s", c.motion_1s),
        ("motion10s", c.motion_10s),
    ])
}

fn pad(s: &str, n: usize) -> String {
    let mut out: String = s.chars().take(n).collect();
    let len = out.chars().count();
    out.extend(std::iter::repeat_n(' ', n - len));
    out
}

fn main() {
    let a = Args::parse();
    let points = candidates(&a);
    if points.is_empty() {
        eprintln!("no points to measure");
        std::process::exit(1);
    }
    if a.has("onsets") {
        return onsets(&a, &points);
    }
    if a.has("switch") {
        return switch(&a);
    }
    if a.has("configs") {
        return configs(&a, &points);
    }
    fractality(&a, &points);
}

fn fractality(a: &Args, points: &[Point]) {
    let secs = a.num("secs", 30.0);
    let sr = a.num("sr", 22050.0);
    let repeat = (a.num("repeat", 1.0) as usize).max(1);
    let character = a.has("character") || a.has("ref");
    let char_head = if character { "drop  swing low   harm  rough mot1  mot10 " } else { "" };
    let score_head = if repeat > 1 { "score±sd    " } else { "score  " };
    let of = if repeat > 1 { format!(", mean of {repeat} renders") } else { String::new() };
    println!(
        "{} {score_head}envβ   cenβ   HFD    box    dB    {char_head}({secs}s @ {sr} Hz{of})",
        pad("point", 34)
    );

    let results: Vec<(BTreeMap<&str, f64>, f64, bool, f64)> = points
        .par_iter()
        .enumerate()
        .map(|(pi, p)| {
            let t0 = Instant::now();
            let mut runs = Vec::new();
            let mut silent = false;
            for rep in 0..repeat {
                let seed = rep as u32 + 1;
                let x = render_offline(&p.state, secs, sr, seed);
                let an = analyze_sound(&x, sr);
                silent |= an.silent;
                runs.push(metrics(&an, &analyze_character(&x, sr)));
                let sub = |dir: &str| if repeat > 1 { format!("{dir}/seed{seed}") } else { dir.to_string() };
                let name = if p.group == "mutant" { format!("{}_{pi}", p.label) } else { p.label.clone() };
                if let Some(dir) = a.get("png") {
                    write_png(&sub(dir), &name, &x, sr);
                }
                if let Some(dir) = a.get("wav") {
                    if p.group != "mutant" {
                        write_wav(&sub(dir), &p.label, &x, sr);
                    }
                }
            }
            let mut m = BTreeMap::new();
            for k in runs[0].keys() {
                m.insert(*k, mean(&runs.iter().map(|r| r[k]).collect::<Vec<_>>()));
            }
            let score_sd = sd(&runs.iter().map(|r| r["score"]).collect::<Vec<_>>());
            (m, score_sd, silent, t0.elapsed().as_secs_f64())
        })
        .collect();

    for (p, (m, score_sd, silent, took)) in points.iter().zip(&results) {
        let score = if repeat > 1 {
            format!("{}±{}  ", fmt(m["score"], 2), fmt(*score_sd, 2))
        } else {
            fmt(m["score"], 2)
        };
        let chars = if character {
            format!(
                "  {} {} {} {} {} {} {}",
                pad(&fmt(m["dropout"], 1), 5),
                pad(&fmt(m["swing"], 1), 5),
                pad(&fmt(m["lowShare"], 2), 5),
                pad(&fmt(m["harmonicity"], 2), 5),
                pad(&fmt(m["roughness"], 2), 5),
                pad(&fmt(m["motion1s"], 1), 5),
                pad(&fmt(m["motion10s"], 1), 5),
            )
        } else {
            String::new()
        };
        println!(
            "{} {score}   {}   {}   {}   {}   {}{chars}{}   {took:.1}s",
            pad(&p.label, 34),
            fmt(m["envBeta"], 2),
            fmt(m["centroidBeta"], 2),
            fmt(m["envHiguchi"], 2),
            fmt(m["boxDim"], 2),
            fmt(m["loudness"], 0),
            if *silent { "  SILENT" } else { "" },
        );
    }

    if let Some(refs) = a.list("ref") {
        reference_distance(points, &results.iter().map(|r| r.0.clone()).collect::<Vec<_>>(), &refs);
    }

    let mut groups: Vec<&str> = Vec::new();
    for p in points {
        if !groups.contains(&p.group) {
            groups.push(p.group);
        }
    }
    if groups.len() > 1 {
        println!("\nmean score by group:");
        for g in groups {
            let s: Vec<f64> = points
                .iter()
                .zip(&results)
                .filter(|(p, _)| p.group == g)
                .map(|(_, r)| r.0["score"])
                .collect();
            println!("  {} n={}  {:.3} ± {:.3}", pad(g, 8), s.len(), mean(&s), sd(&s));
        }
    }

    if let Some(path) = a.get("json") {
        let rows: Vec<serde_json::Value> = points
            .iter()
            .zip(&results)
            .map(|(p, (m, score_sd, silent, _))| {
                let mut o = serde_json::json!({ "group": p.group, "label": p.label, "silent": silent });
                for (k, v) in m {
                    o[*k] = serde_json::json!(v);
                }
                if repeat > 1 {
                    o["scoreSd"] = serde_json::json!(score_sd);
                }
                o
            })
            .collect();
        if let Err(e) = fs::write(path, serde_json::to_string_pretty(&rows).unwrap_or_default()) {
            eprintln!("{path}: {e}");
        }
    }
}

/// How far each point sits from a reference group of presets — per metric
/// z-scores against the group's mean/sd (with a floor on sd, so a family that
/// happens to agree does not turn a small difference into a huge z), RMS over
/// the metrics; the metrics beyond 2 sd say WHERE a point differs.
fn reference_distance(points: &[Point], results: &[BTreeMap<&str, f64>], refs: &[usize]) {
    let keys =
        ["envBeta", "centroidBeta", "boxDim"].iter().copied().chain(CHARACTER_KEYS).collect::<Vec<_>>();
    let floor = |k: &str| match k {
        "envBeta" | "centroidBeta" => 0.15,
        "boxDim" | "lowShare" | "harmonicity" => 0.05,
        "dropout" => 1.0,
        "swing" => 2.0,
        "roughness" => 0.02,
        "motion1s" => 0.3,
        _ => 0.5,
    };
    let members: Vec<usize> = points
        .iter()
        .enumerate()
        .filter(|(_, p)| p.group == "preset" && refs.iter().any(|i| p.label.starts_with(&format!("{i}: "))))
        .map(|(i, _)| i)
        .collect();
    let stat: BTreeMap<&str, (f64, f64)> = keys
        .iter()
        .map(|k| {
            let xs: Vec<f64> = members.iter().map(|i| results[*i][k]).collect();
            (*k, (mean(&xs), sd(&xs).max(floor(k))))
        })
        .collect();
    let names: Vec<&str> = members.iter().map(|i| points[*i].label.as_str()).collect();
    println!("\ndistance to the reference group ({}):", names.join(", "));
    println!(
        "  {}",
        keys.iter()
            .map(|k| format!("{k} {}±{}", fmt(stat[k].0, 2), fmt(stat[k].1, 2)))
            .collect::<Vec<_>>()
            .join("  ")
    );
    for (p, r) in points.iter().zip(results) {
        let zs: Vec<(&str, f64)> = keys
            .iter()
            .map(|k| (*k, (r[k] - stat[k].0) / stat[k].1))
            .filter(|(_, z)| z.is_finite())
            .collect();
        let d = (zs.iter().map(|(_, z)| z * z).sum::<f64>() / zs.len().max(1) as f64).sqrt();
        let far: Vec<String> = zs
            .iter()
            .filter(|(_, z)| z.abs() > 2.0)
            .map(|(k, z)| format!("{k}{}{z:.1}", if *z > 0.0 { "+" } else { "" }))
            .collect();
        println!("  {} {}  {}", pad(&p.label, 32), fmt(d, 2), far.join(" "));
    }
}

/// Onset hits per point through the engine as the apps run it (the picture
/// seeds growth on every hit), and how far the loudness swell swings.
fn onsets(a: &Args, points: &[Point]) {
    let secs = a.num("secs", 16.0);
    let sr = a.num("sr", 48000.0);
    println!("{} hits   swell        ({secs}s @ {sr} Hz, the engine with its FX)", pad("point", 28));
    let rows: Vec<(u64, f64, f64)> = points
        .par_iter()
        .map(|p| {
            let mut e = Engine::new(sr, &p.state, 1);
            let mut buf = vec![0.0f32; BLOCK];
            let (mut lo, mut hi) = (1.0f64, -1.0f64);
            for _ in 0..(secs * sr / BLOCK as f64) as usize {
                e.render(&mut buf);
                if e.time() > 2.0 {
                    let f = e.features();
                    lo = lo.min(f.swell);
                    hi = hi.max(f.swell);
                }
            }
            (e.hits(), lo, hi)
        })
        .collect();
    for (p, (hits, lo, hi)) in points.iter().zip(rows) {
        println!("{} {:<6} {}…{}", pad(&p.label, 28), hits, fmt(lo, 2), fmt(hi, 2));
    }
}

/// HF energy (RMS of the second difference) of the loudest 5 ms frame in [from, to).
fn hf_peak(x: &[f32], sr: f64, from: f64, to: f64) -> f64 {
    let frame = (sr * 0.005) as usize;
    let (lo, hi) = ((from * sr) as usize / frame, ((to * sr) as usize / frame).min(x.len() / frame));
    (lo.max(1)..hi)
        .map(|f| {
            let e: f64 = (0..frame)
                .map(|i| {
                    let j = f * frame + i;
                    let d = f64::from(x[j]) - 2.0 * f64::from(x[j - 1]) + f64::from(x[j - 2]);
                    d * d
                })
                .sum();
            (e / frame as f64).sqrt()
        })
        .fold(0.0, f64::max)
}

/// Plays each preset of `--switch` for `--at` seconds and switches to the
/// next as the apps do (fade out, 100 ms, switch, fade in): clicks by the
/// detector near each switch and elsewhere, and the switch instant's HF
/// energy against the loudest frame of the whole render.
fn switch(a: &Args) {
    let order = a.list("switch").unwrap_or_else(|| vec![0, 3, 10, 7]);
    let at = a.num("at", 20.0);
    let sr = a.num("sr", 22050.0);
    let Some(first) = order.first().and_then(|i| presets().get(*i)) else { return };
    let p = Player::new(sr, &first.state);
    p.fade_in();
    let (mut x, mut buf, mut times) = (Vec::new(), Vec::new(), Vec::new());
    let mut take = |p: &Player, secs: f64, x: &mut Vec<f32>| {
        p.render_into((secs * sr).round() as usize, &mut buf);
        x.extend_from_slice(&buf);
    };
    take(&p, at, &mut x);
    for i in &order[1..] {
        let Some(next) = presets().get(*i) else { continue };
        p.fade_out();
        take(&p, 0.1, &mut x);
        times.push(x.len() as f64 / sr);
        p.switch_to(next.state.clone());
        p.fade_in();
        take(&p, at, &mut x);
    }
    let clicks = detect_clicks(&x, sr);
    let near = |t: f64| clicks.iter().filter(|c| (**c - t).abs() < 0.3).count();
    let whole = hf_peak(&x, sr, 0.0, x.len() as f64 / sr);
    println!(
        "switches {:?}, {at}s each @ {sr} Hz — the switch instant's HF vs the loudest frame ({whole:.4}):",
        order
    );
    for (t, i) in times.iter().zip(&order[1..]) {
        println!(
            "  → {i} at {t:.2}s: {} clicks near, instant HF {:.5}",
            near(*t),
            hf_peak(&x, sr, t - 0.005, t + 0.01)
        );
    }
    let elsewhere = clicks.len() - times.iter().map(|t| near(*t)).sum::<usize>();
    println!("  elsewhere: {elsewhere} (the detector also fires on plucks and strikes — see --png)");
    if let Some(dir) = a.get("wav") {
        write_wav(dir, "switches", &x, sr);
    }
    if let Some(dir) = a.get("png") {
        write_png(dir, "switches", &x, sr);
    }
}

fn spearman(a: &[f64], b: &[f64]) -> f64 {
    let ranks = |x: &[f64]| {
        let mut idx: Vec<usize> = (0..x.len()).collect();
        idx.sort_by(|i, j| x[*i].total_cmp(&x[*j]));
        let mut r = vec![0.0; x.len()];
        for (k, i) in idx.iter().enumerate() {
            r[*i] = k as f64;
        }
        r
    };
    let (ra, rb) = (ranks(a), ranks(b));
    let m = (a.len() as f64 - 1.0) / 2.0;
    let (mut c, mut va, mut vb) = (0.0, 0.0, 0.0);
    for i in 0..a.len() {
        c += (ra[i] - m) * (rb[i] - m);
        va += (ra[i] - m).powi(2);
        vb += (rb[i] - m).powi(2);
    }
    c / (va * vb).sqrt()
}

/// Scores every point under each `secs@rate` and prints each config's
/// Spearman ρ against the first, with its cost per point — how the scout's
/// render was chosen (PLAN-CORE.md phase 4).
fn configs(a: &Args, points: &[Point]) {
    let cfgs: Vec<(f64, f64)> = a
        .get("configs")
        .unwrap_or("30@22050")
        .split(',')
        .filter_map(|c| c.split_once('@').and_then(|(s, r)| Some((s.parse().ok()?, r.parse().ok()?))))
        .collect();
    let mut table: Vec<Vec<f64>> = Vec::new();
    println!("{} points; ρ against {}s@{}:", points.len(), cfgs[0].0, cfgs[0].1);
    for (secs, sr) in &cfgs {
        let t0 = Instant::now();
        let scores: Vec<f64> = points
            .par_iter()
            .map(|p| {
                let an = analyze_sound(&render_offline(&p.state, *secs, *sr, 1), *sr);
                if an.silent {
                    -1.0
                } else {
                    an.score
                }
            })
            .collect();
        let took = t0.elapsed().as_secs_f64();
        let rho = table.first().map_or(1.0, |first| spearman(first, &scores));
        println!(
            "  {} ρ={rho:.2}  {:.2}s a point (wall, all cores)",
            pad(&format!("{secs}s@{sr}"), 11),
            took / points.len() as f64
        );
        table.push(scores);
    }
}
