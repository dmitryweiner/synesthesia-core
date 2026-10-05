//! Numbers for the picture, ported from the web app's
//! `src/analysis/picture.ts` (its PLAN.md #22, B8): read from the
//! simulation's V channel (the "ink"), not from pixels, so palette and light
//! don't enter. Any renderer that can hand over its field — the GPU's read
//! back, the CPU `Picture`'s own — can be measured the same way.
//!
//! - `coverage`: share of cells holding pattern (the canvas filling, or dying)
//! - `edges`: share of cells on a steep boundary (how intricate it is)
//! - `change`: mean |ΔV| since the previous sample (still moving, or frozen)

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PictureMetrics {
    pub coverage: f64,
    pub edges: f64,
    /// NaN without a previous sample.
    pub change: f64,
    pub alive: bool,
}

/// V above this is pattern (background V ≈ 0, pattern 0.2–0.4).
const INK: f32 = 0.1;
/// |∇V| per cell above this is a boundary.
const EDGE: f64 = 0.04;
/// Less than 1 % of the canvas is not a picture.
const ALIVE: f64 = 0.01;

pub fn picture_metrics(v: &[f32], width: usize, height: usize, prev: Option<&[f32]>) -> PictureMetrics {
    let (mut ink, mut edges) = (0usize, 0usize);
    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            if v[i] > INK {
                ink += 1;
            }
            let gx = if x + 1 < width { f64::from(v[i + 1]) - f64::from(v[i]) } else { 0.0 };
            let gy = if y + 1 < height { f64::from(v[i + width]) - f64::from(v[i]) } else { 0.0 };
            if (gx * gx + gy * gy).sqrt() > EDGE {
                edges += 1;
            }
        }
    }
    let change = match prev {
        Some(p) if p.len() == v.len() => {
            v.iter().zip(p).map(|(a, b)| (f64::from(*a) - f64::from(*b)).abs()).sum::<f64>() / v.len() as f64
        }
        _ => f64::NAN,
    };
    let n = (width * height) as f64;
    PictureMetrics {
        coverage: ink as f64 / n,
        edges: edges as f64 / n,
        change,
        alive: ink as f64 / n >= ALIVE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(phase: f64) -> Vec<f32> {
        let (w, h) = (96, 64);
        (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f64, (i / w) as f64);
                let s = (x * 0.31 + phase).sin() * (y * 0.23 - phase).cos() + 0.3 * ((x + y) * 0.07).sin();
                (if s > 0.35 { 0.15 + 0.2 * s } else { 0.02 * s.abs() }) as f32
            })
            .collect()
    }

    #[test]
    fn a_synthetic_field_measures_as_it_did_in_the_web_app() {
        // pictureMetrics() on the same fields, 2026-10-05
        let (a, b) = (field(0.0), field(0.4));
        let m = picture_metrics(&a, 96, 64, None);
        assert_eq!((m.coverage, m.edges, m.alive), (0.2649739583333333, 0.17740885416666666, true));
        assert!(m.change.is_nan());
        let m = picture_metrics(&b, 96, 64, Some(&a));
        assert_eq!((m.coverage, m.edges), (0.2771809895833333, 0.17724609375));
        assert!((m.change - 0.0482479551807199).abs() < 1e-12, "{}", m.change);
    }

    #[test]
    fn an_empty_field_is_not_alive() {
        let m = picture_metrics(&[0.0; 64], 8, 8, None);
        assert!(!m.alive && m.coverage == 0.0 && m.edges == 0.0);
    }
}
