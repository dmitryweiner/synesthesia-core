//! A log-frequency spectrogram of a render, ported from the web app's
//! `src/analysis/spectrogram.ts`: an agent can't hear, but it can read a
//! picture. One look at the waterfall settles questions a numeric metric can
//! take an hour to (a bright line stepping across the harmonics IS the
//! whistle; a click is a full-band line reaching below the lowest
//! fundamental).

use super::fft::fft;

pub struct Spectrogram {
    pub columns: usize,
    pub rows: usize,
    /// dBFS, row-major; row 0 is the top (`f_max`). A full-scale sine reads 0.
    pub db: Vec<f32>,
    /// RMS dBFS around each column (0.4 s window).
    pub loudness: Vec<f32>,
    f_min: f64,
    f_max: f64,
    n: usize,
    span: usize,
    sr: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct SpectrogramOptions {
    pub columns: usize,
    pub rows: usize,
    pub f_min: f64,
    pub f_max: f64,
    /// `None`: 4096 at ≥ 16 kHz (5.4 Hz bins at 22 kHz, enough to split a
    /// 55 Hz drone's harmonics), else 2048.
    pub fft_size: Option<usize>,
}

impl Default for SpectrogramOptions {
    fn default() -> Self {
        SpectrogramOptions { columns: 1200, rows: 320, f_min: 30.0, f_max: 12000.0, fft_size: None }
    }
}

impl Spectrogram {
    /// Centre frequency of a row, Hz (rows may be fractional: the edges).
    pub fn row_hz(&self, row: f64) -> f64 {
        row_hz(self.f_max, self.f_min, self.rows, row)
    }

    /// Seconds at the centre of a column.
    pub fn column_seconds(&self, col: usize) -> f64 {
        (column_start(col, self.span, self.columns) + self.n / 2) as f64 / self.sr
    }
}

fn row_hz(f_max: f64, f_min: f64, rows: usize, row: f64) -> f64 {
    f_max * (f_min / f_max).powf(row / (rows - 1) as f64)
}

fn column_start(col: usize, span: usize, columns: usize) -> usize {
    ((col * span) as f64 / (columns - 1) as f64).round() as usize
}

pub fn log_spectrogram(x: &[f32], sr: f64, opts: SpectrogramOptions) -> Spectrogram {
    let columns = opts.columns.max(2);
    let rows = opts.rows.max(2);
    let f_max = opts.f_max.min(sr / 2.0);
    let f_min = opts.f_min.min(f_max / 2.0);
    let n = opts.fft_size.unwrap_or(if sr >= 16000.0 { 4096 } else { 2048 });
    let span = x.len().saturating_sub(n);
    let half_n = n / 2;

    let win: Vec<f64> =
        (0..n).map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos()).collect();
    let win_sum: f64 = win.iter().sum();
    let norm = 1.0 / ((win_sum / 2.0) * (win_sum / 2.0)); // a full-scale sine's peak bin → 1
    let mut re = vec![0.0; n];
    let mut im = vec![0.0; n];
    let mut power = vec![0.0; half_n + 1];
    let mut db = vec![0.0f32; rows * columns];
    // which bins each row covers: between the geometric midpoints to its neighbours
    let lo: Vec<f64> =
        (0..rows).map(|r| row_hz(f_max, f_min, rows, r as f64 + 0.5) * n as f64 / sr).collect();
    let hi: Vec<f64> =
        (0..rows).map(|r| row_hz(f_max, f_min, rows, r as f64 - 0.5) * n as f64 / sr).collect();

    for c in 0..columns {
        let start = column_start(c, span, columns);
        for i in 0..n {
            re[i] = x.get(start + i).map_or(0.0, |v| f64::from(*v)) * win[i];
            im[i] = 0.0;
        }
        fft(&mut re, &mut im, false);
        for k in 0..=half_n {
            power[k] = (re[k] * re[k] + im[k] * im[k]) * norm;
        }
        for r in 0..rows {
            let k0 = lo[r].ceil();
            let k1 = (half_n as f64).min(hi[r].floor());
            let p = if k1 >= k0 {
                // several bins in this row: the strongest, so harmonics stay crisp
                (k0 as usize..=k1 as usize).map(|k| power[k]).fold(0.0, f64::max)
            } else {
                // a row narrower than a bin (the bass): interpolate at its frequency
                let kf = ((half_n - 1) as f64).min(row_hz(f_max, f_min, rows, r as f64) * n as f64 / sr);
                let k = kf.floor() as usize;
                power[k] + (power[k + 1] - power[k]) * (kf - k as f64)
            };
            db[r * columns + c] = (10.0 * (p + 1e-20).log10()) as f32;
        }
    }

    let half = (0.2 * sr).round() as usize;
    let loudness = (0..columns)
        .map(|c| {
            let mid = column_start(c, span, columns) + half_n;
            let a = mid.saturating_sub(half);
            let b = x.len().min(mid + half);
            let s: f64 = x[a.min(b)..b].iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
            (10.0 * (s / (b.saturating_sub(a)).max(1) as f64 + 1e-20).log10()) as f32
        })
        .collect();
    Spectrogram { columns, rows, db, loudness, f_min, f_max, n, span, sr }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_scale_sine_reads_zero_db_in_its_row() {
        let sr = 22050.0;
        let x: Vec<f32> = (0..sr as usize * 2)
            .map(|i| (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / sr).sin() as f32)
            .collect();
        let s = log_spectrogram(&x, sr, SpectrogramOptions { columns: 8, rows: 64, ..Default::default() });
        let col = 4;
        let (best, db) =
            (0..s.rows).map(|r| (r, s.db[r * s.columns + col])).max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        assert!((s.row_hz(best as f64) / 1000.0 - 1.0).abs() < 0.08, "{} Hz", s.row_hz(best as f64));
        assert!(db.abs() < 1.5, "{db} dB");
    }
}
