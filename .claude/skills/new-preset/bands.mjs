#!/usr/bin/env node
// Band-level check of rendered WAVs (from `syn-bench --wav`): what a
// change did to the bass, and proof that the band you meant to keep did not
// move. Written while fixing Overtone steppe's "too rough" bass.
//
//   node .claude/skills/new-preset/bands.mjs <dir> [--f0 55] [--keep 600-2400]
//
// Columns, per WAV (the first file — sorted by preset number — is the
// reference for the last two):
//   lowRough  Plomp–Levelt roughness of the spectral peaks below 400 Hz,
//             divided by their total amplitude (level-independent)
//   fund      share of the 30–400 Hz energy within ±5 Hz of --f0: a strong
//             smooth fundamental reads "round and fat"
//   body      share of all energy below 150 Hz
//   grit dB   150–600 Hz energy relative to 30–150 Hz
//   wobble dB how much the <150 Hz level moves between 1 s windows
//   keep Δ dB / corr   mean level difference and correlation of the --keep
//             band's contour against the reference — 0.0 / 1.00 means that
//             band did not change at all
//
// Compare WAVs of the same render length and rate (renders are deterministic):
// with random rooms the same point read lowRough 0.046 and 0.034.
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const args = process.argv.slice(2);
const dir = args.find((a) => !a.startsWith('--'));
const opt = (name, dflt) => { const i = args.indexOf(`--${name}`); return i >= 0 ? args[i + 1] : dflt; };
if (!dir) {
  console.error('usage: bands.mjs <dir-with-wavs> [--f0 55] [--keep 600-2400]');
  process.exit(1);
}
const F0 = Number(opt('f0', 55));
const [KEEP_LO, KEEP_HI] = opt('keep', '600-2400').split('-').map(Number);

function readWav(p) {
  const b = readFileSync(p);
  const sr = b.readUInt32LE(24);
  const n = (b.length - 44) / 2;
  const x = new Float64Array(n);
  for (let i = 0; i < n; i++) x[i] = b.readInt16LE(44 + i * 2) / 32768;
  return { x, sr };
}

function fft(re, im) {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) { let t = re[i]; re[i] = re[j]; re[j] = t; t = im[i]; im[i] = im[j]; im[j] = t; }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = (-2 * Math.PI) / len;
    const wr = Math.cos(ang), wi = Math.sin(ang);
    for (let i = 0; i < n; i += len) {
      let cr = 1, ci = 0;
      for (let k = 0; k < len / 2; k++) {
        const a = i + k, b = a + len / 2;
        const tr = re[b] * cr - im[b] * ci, ti = re[b] * ci + im[b] * cr;
        re[b] = re[a] - tr; im[b] = im[a] - ti; re[a] += tr; im[a] += ti;
        const nr = cr * wr - ci * wi; ci = cr * wi + ci * wr; cr = nr;
      }
    }
  }
}

function analyze(path) {
  const { x, sr } = readWav(path);
  const N = 8192;
  const hop = Math.floor(0.25 * sr);
  const hz = sr / N;
  const hann = Float64Array.from({ length: N }, (_, i) => 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (N - 1)));
  const band = (mag, lo, hi) => { let e = 0; for (let k = Math.ceil(lo / hz); k <= Math.floor(hi / hz); k++) e += mag[k] * mag[k]; return e; };
  let rough = 0, frames = 0, fund = 0, low = 0, body = 0, grit = 0, total = 0;
  const bodyDb = [];
  const keep = [];
  for (let o = 2 * sr; o + N <= x.length; o += hop) { // skip the fade-in
    const re = new Float64Array(N), im = new Float64Array(N);
    for (let i = 0; i < N; i++) re[i] = x[o + i] * hann[i];
    fft(re, im);
    const mag = new Float64Array(N / 2);
    for (let k = 0; k < N / 2; k++) mag[k] = Math.hypot(re[k], im[k]);
    const b = band(mag, 30, 150);
    fund += band(mag, F0 - 5, F0 + 5);
    low += band(mag, 30, 400);
    body += b;
    grit += band(mag, 150, 600);
    total += band(mag, 30, sr / 2 - 10);
    bodyDb.push(10 * Math.log10(b + 1e-12));
    keep.push(10 * Math.log10(band(mag, KEEP_LO, KEEP_HI) + 1e-12));
    // Sethares' roughness of the peaks below 400 Hz
    let max = 0;
    for (let k = Math.ceil(30 / hz); k <= 400 / hz; k++) max = Math.max(max, mag[k]);
    const peaks = [];
    for (let k = Math.ceil(30 / hz) + 1; k < 400 / hz; k++) {
      if (mag[k] > mag[k - 1] && mag[k] >= mag[k + 1] && mag[k] > max * 0.01) peaks.push({ f: k * hz, a: mag[k] });
    }
    let r = 0, amp = 0;
    for (let i = 0; i < peaks.length; i++) {
      amp += peaks[i].a;
      const s = 0.24 / (0.0207 * peaks[i].f + 18.96);
      for (let j = i + 1; j < peaks.length; j++) {
        const d = s * (peaks[j].f - peaks[i].f);
        if (d > 3) break;
        r += Math.min(peaks[i].a, peaks[j].a) * (Math.exp(-3.51 * d) - Math.exp(-5.75 * d));
      }
    }
    if (amp > 0) { rough += r / amp; frames++; }
  }
  const secs = [];
  for (let i = 0; i + 4 <= bodyDb.length; i += 4) secs.push(bodyDb.slice(i, i + 4).reduce((a, v) => a + v, 0) / 4);
  const m = secs.reduce((a, v) => a + v, 0) / secs.length;
  const wobble = Math.sqrt(secs.reduce((a, v) => a + (v - m) ** 2, 0) / secs.length);
  return { rough: rough / frames, fund: fund / low, body: body / total, grit: 10 * Math.log10(grit / body), wobble, keep };
}

const files = readdirSync(dir).filter((n) => n.endsWith('.wav')).sort((a, b) => parseInt(a, 10) - parseInt(b, 10));
if (!files.length) { console.error(`no .wav in ${dir}`); process.exit(1); }
const rows = files.map((f) => ({ f, ...analyze(join(dir, f)) }));
const ref = rows[0].keep;
const col = (v, d = 2) => v.toFixed(d).padStart(7);
console.log(`${'file'.padEnd(36)} lowRough    fund    body grit dB  wobble | ${KEEP_LO}-${KEEP_HI} Hz vs ${rows[0].f}: Δ dB, corr`);
for (const r of rows) {
  const n = Math.min(ref.length, r.keep.length);
  let d = 0, ma = 0, mb = 0;
  for (let i = 0; i < n; i++) { d += r.keep[i] - ref[i]; ma += ref[i]; mb += r.keep[i]; }
  ma /= n; mb /= n;
  let num = 0, da = 0, db = 0;
  for (let i = 0; i < n; i++) { num += (ref[i] - ma) * (r.keep[i] - mb); da += (ref[i] - ma) ** 2; db += (r.keep[i] - mb) ** 2; }
  console.log(`${r.f.padEnd(36)} ${col(r.rough, 3)} ${col(r.fund)} ${col(r.body)} ${col(r.grit, 1)} ${col(r.wobble, 1)} | ${col(d / n, 1)} ${col(num / Math.sqrt(da * db))}`);
}
