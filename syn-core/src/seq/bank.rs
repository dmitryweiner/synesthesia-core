//! The sample bank (PLAN-SAMPLES-SCRIPTS.md, *The bank*): the bytes of
//! `bank.bin` in, the samples of a sound out. The core does no I/O — the
//! host reads the file (or fetches it, or opens an asset) and hands over
//! the bytes.
//!
//! ```text
//! bank.bin = "SYNB" | u32 version = 2 | u32 manifest_len | manifest JSON
//!          | the FLAC streams (mono, 16-bit), back to back — little-endian
//! ```
//!
//! Decoding the whole bank costs ~150 ms and 31.5 MB (S0), so a sound is
//! decoded the first time something asks for it ([`SampleBank::sound`]) and
//! kept, as `i16`, for as long as the bank lives. That is always on the
//! control side — building a [`super::SeqProgram`] — never in `render`.
//! Because the bank keeps every decoded sound, a voice that lets go of a
//! sample never drops the last reference to it on the audio thread.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::Mode;

pub const MAGIC: &[u8; 4] = b"SYNB";
pub const VERSION: u32 = 2;

/// Why a blob is not a bank. A bank that parses but has a broken stream is
/// not an error: that variant decodes to silence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BankError {
    NotABank,
    Version(u32),
    Truncated,
    Manifest(String),
}

impl fmt::Display for BankError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BankError::NotABank => write!(f, "not a sample bank (no SYNB header)"),
            BankError::Version(v) => write!(f, "sample bank version {v}, this core reads {VERSION}"),
            BankError::Truncated => write!(f, "sample bank is truncated"),
            BankError::Manifest(e) => write!(f, "sample bank manifest: {e}"),
        }
    }
}

impl std::error::Error for BankError {}

/// One sample as the manifest describes it: where its FLAC stream is, and
/// where the recording came from.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Entry {
    #[serde(default)]
    pub file: String,
    pub sr: u32,
    pub frames: u32,
    /// From the first byte after the manifest.
    #[serde(default)]
    pub offset: u64,
    #[serde(default)]
    pub bytes: u64,
    /// The first 16 hex digits of the stream's SHA-256.
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub src: String,
    #[serde(default)]
    pub page: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub title: String,
}

#[derive(Deserialize)]
struct Manifest {
    version: u32,
    sounds: BTreeMap<String, Vec<Entry>>,
}

/// A decoded sample: mono, at its own rate (24, 32 or 48 kHz in the first
/// bank), what it was cut for.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub sr: u32,
    pub data: Vec<i16>,
    pub mode: Mode,
}

/// A sound's variants in the bank's order: `owl:1` is `variants[1]`.
pub type Variants = Arc<[Sample]>;

pub struct SampleBank {
    /// The streams, from the first byte after the manifest.
    streams: Vec<u8>,
    sounds: BTreeMap<String, Vec<Entry>>,
    decoded: Mutex<BTreeMap<String, Variants>>,
}

impl fmt::Debug for SampleBank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SampleBank")
            .field("sounds", &self.sounds.len())
            .field("bytes", &self.streams.len())
            .finish()
    }
}

impl SampleBank {
    /// Parses `bank.bin`. Nothing is decoded yet.
    pub fn from_bytes(mut blob: Vec<u8>) -> Result<SampleBank, BankError> {
        if blob.len() < 12 || &blob[..4] != MAGIC {
            return Err(BankError::NotABank);
        }
        let word = |i: usize| u32::from_le_bytes([blob[i], blob[i + 1], blob[i + 2], blob[i + 3]]);
        let version = word(4);
        if version != VERSION {
            return Err(BankError::Version(version));
        }
        let end = 12usize.checked_add(word(8) as usize).ok_or(BankError::Truncated)?;
        if end > blob.len() {
            return Err(BankError::Truncated);
        }
        let m: Manifest =
            serde_json::from_slice(&blob[12..end]).map_err(|e| BankError::Manifest(e.to_string()))?;
        if m.version != VERSION {
            return Err(BankError::Version(m.version));
        }
        let streams = blob.split_off(end);
        for (name, entries) in &m.sounds {
            for e in entries {
                if e.offset.checked_add(e.bytes).is_none_or(|x| x > streams.len() as u64) {
                    return Err(BankError::Manifest(format!("{name}: {} lies past the end", e.file)));
                }
            }
        }
        Ok(SampleBank { streams, sounds: m.sounds, decoded: Mutex::new(BTreeMap::new()) })
    }

    /// A bank of samples already decoded: the tests' sine blips, or a host
    /// that decodes by other means.
    pub fn from_pcm(sounds: Vec<(String, Vec<Sample>)>) -> SampleBank {
        let mut entries = BTreeMap::new();
        let mut decoded = BTreeMap::new();
        for (name, samples) in sounds {
            let e = samples
                .iter()
                .map(|s| Entry { sr: s.sr, frames: s.data.len() as u32, mode: s.mode, ..Entry::default() })
                .collect();
            entries.insert(name.clone(), e);
            decoded.insert(name, Variants::from(samples));
        }
        SampleBank { streams: Vec::new(), sounds: entries, decoded: Mutex::new(decoded) }
    }

    /// A bank of sounds already decoded and shared: what a host that decodes
    /// on another thread hands the player, a sound at a time (the web app's
    /// worklet). The samples are not copied.
    pub fn from_variants(sounds: Vec<(String, Variants)>) -> SampleBank {
        let mut entries = BTreeMap::new();
        let mut decoded = BTreeMap::new();
        for (name, v) in sounds {
            let e = v
                .iter()
                .map(|s| Entry { sr: s.sr, frames: s.data.len() as u32, mode: s.mode, ..Entry::default() })
                .collect();
            entries.insert(name.clone(), e);
            decoded.insert(name, v);
        }
        SampleBank { streams: Vec::new(), sounds: entries, decoded: Mutex::new(decoded) }
    }

    /// The sounds decoded so far, shared (not copied).
    pub fn decoded_sounds(&self) -> Vec<(String, Variants)> {
        let cache = self.decoded.lock().unwrap_or_else(|e| e.into_inner());
        cache.iter().map(|(n, v)| (n.clone(), v.clone())).collect()
    }

    /// The sound names, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.sounds.keys().map(String::as_str)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.sounds.contains_key(name)
    }

    /// A sound's manifest entries (empty for a name the bank lacks).
    pub fn entries(&self, name: &str) -> &[Entry] {
        self.sounds.get(name).map_or(&[], Vec::as_slice)
    }

    /// A sound's samples, decoded on first ask and kept. `None` for a name
    /// the bank lacks; a variant whose stream is broken decodes to an empty
    /// sample (silence), so the numbering of the others stays.
    pub fn sound(&self, name: &str) -> Option<Variants> {
        let entries = self.sounds.get(name)?;
        let mut cache = self.decoded.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(v) = cache.get(name) {
            return Some(v.clone());
        }
        let samples: Vec<Sample> = entries
            .iter()
            .map(|e| Sample { sr: e.sr, data: self.decode(e).unwrap_or_default(), mode: e.mode })
            .collect();
        let v = Variants::from(samples);
        cache.insert(name.to_string(), v.clone());
        Some(v)
    }

    /// Bytes held by the decoded sounds so far.
    pub fn decoded_bytes(&self) -> usize {
        let cache = self.decoded.lock().unwrap_or_else(|e| e.into_inner());
        cache.values().flat_map(|v| v.iter()).map(|s| s.data.len() * 2).sum()
    }

    fn decode(&self, e: &Entry) -> Option<Vec<i16>> {
        let bytes = &self.streams[e.offset as usize..(e.offset + e.bytes) as usize];
        if !e.sha256.is_empty() {
            let digest = Sha256::digest(bytes);
            let hex: String =
                digest.iter().take(e.sha256.len().div_ceil(2)).map(|b| format!("{b:02x}")).collect();
            if !hex.starts_with(&e.sha256) {
                return None;
            }
        }
        decode_flac(bytes, e.sr, e.frames)
    }
}

/// A decoded sound as bytes, to cross from one thread to another where only
/// bytes may (a Web Worker or the main thread → the AudioWorklet):
/// `u32 count`, then per variant `u32 sr | u32 mode (0 hits, 1 bed) | u32
/// frames | frames × i16`, little-endian.
pub fn pack(v: &[Sample]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + v.iter().map(|s| 12 + 2 * s.data.len()).sum::<usize>());
    out.extend_from_slice(&(v.len() as u32).to_le_bytes());
    for s in v {
        out.extend_from_slice(&s.sr.to_le_bytes());
        out.extend_from_slice(&u32::from(s.mode == Mode::Bed).to_le_bytes());
        out.extend_from_slice(&(s.data.len() as u32).to_le_bytes());
        for x in &s.data {
            out.extend_from_slice(&x.to_le_bytes());
        }
    }
    out
}

/// [`pack`] undone; `None` for bytes that are not one.
pub fn unpack(b: &[u8]) -> Option<Vec<Sample>> {
    fn word(b: &[u8], at: &mut usize) -> Option<u32> {
        let w = u32::from_le_bytes(b.get(*at..*at + 4)?.try_into().ok()?);
        *at += 4;
        Some(w)
    }
    let mut at = 0usize;
    let n = word(b, &mut at)?;
    let mut out = Vec::new();
    for _ in 0..n {
        let (sr, mode, frames) = (word(b, &mut at)?, word(b, &mut at)?, word(b, &mut at)? as usize);
        let bytes = b.get(at..at.checked_add(frames.checked_mul(2)?)?)?;
        at += frames * 2;
        let data = bytes.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect();
        out.push(Sample { sr, data, mode: if mode == 1 { Mode::Bed } else { Mode::Hits } });
    }
    (at == b.len()).then_some(out)
}

/// A mono 16-bit FLAC stream at `sr` into exactly `frames` samples.
fn decode_flac(bytes: &[u8], sr: u32, frames: u32) -> Option<Vec<i16>> {
    let mut reader = claxon::FlacReader::new(bytes).ok()?;
    let info = reader.streaminfo();
    if info.channels != 1 || info.bits_per_sample != 16 || info.sample_rate != sr {
        return None;
    }
    let mut out = Vec::with_capacity(frames as usize);
    let mut blocks = reader.blocks();
    let mut buf = Vec::new();
    while let Some(block) = blocks.read_next_or_eof(buf).ok()? {
        out.extend(block.channel(0).iter().map(|&v| v as i16));
        buf = block.into_buffer();
    }
    if out.len() != frames as usize {
        return None;
    }
    Some(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The two fixtures in `tests/data`, made with `flac --best`: an 880 Hz
    /// blip at 24 kHz (1200 frames) and a seamless 250 Hz loop at 32 kHz
    /// (8000 frames).
    const BLIP: &[u8] = include_bytes!("../../tests/data/blip.flac");
    const LOOP: &[u8] = include_bytes!("../../tests/data/loop.flac");

    fn sha16(b: &[u8]) -> String {
        Sha256::digest(b).iter().take(8).map(|x| format!("{x:02x}")).collect()
    }

    /// A `bank.bin` as `build_bank.py` writes it, from the fixtures.
    pub(crate) fn fixture_bank() -> Vec<u8> {
        let manifest = serde_json::json!({
            "version": 2,
            "sounds": {
                "blip": [
                    { "file": "samples/blip/00.flac", "sr": 24000, "frames": 1200, "offset": 0,
                      "bytes": BLIP.len(), "sha256": sha16(BLIP), "mode": "hits", "license": "CC0" },
                    { "file": "samples/blip/01.flac", "sr": 32000, "frames": 8000, "offset": BLIP.len(),
                      "bytes": LOOP.len(), "sha256": sha16(LOOP), "mode": "bed" }
                ],
                "hum": [
                    { "file": "samples/hum/00.flac", "sr": 32000, "frames": 8000, "offset": BLIP.len(),
                      "bytes": LOOP.len(), "sha256": sha16(LOOP), "mode": "bed" }
                ]
            }
        });
        let mj = serde_json::to_vec(&manifest).unwrap();
        let mut b = Vec::new();
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&VERSION.to_le_bytes());
        b.extend_from_slice(&(mj.len() as u32).to_le_bytes());
        b.extend_from_slice(&mj);
        b.extend_from_slice(BLIP);
        b.extend_from_slice(LOOP);
        b
    }

    #[test]
    fn a_bank_parses_and_decodes_on_demand() {
        let bank = SampleBank::from_bytes(fixture_bank()).unwrap();
        assert_eq!(bank.names().collect::<Vec<_>>(), ["blip", "hum"]);
        assert_eq!(bank.entries("blip")[0].license, "CC0");
        assert_eq!(bank.decoded_bytes(), 0);
        let blip = bank.sound("blip").unwrap();
        assert_eq!(bank.decoded_bytes(), (1200 + 8000) * 2);
        assert_eq!((blip.len(), blip[0].sr, blip[0].data.len(), blip[0].mode), (2, 24000, 1200, Mode::Hits));
        assert_eq!((blip[1].sr, blip[1].data.len(), blip[1].mode), (32000, 8000, Mode::Bed));
        // The blip: ~29000 peak, faded in.
        let peak = blip[0].data.iter().map(|v| v.unsigned_abs()).max().unwrap();
        assert!((28000..=29001).contains(&peak), "{peak}");
        assert!(blip[0].data[0].unsigned_abs() < 100);
        // Kept: the second ask is the same allocation.
        assert!(Arc::ptr_eq(&blip, &bank.sound("blip").unwrap()));
        assert!(bank.sound("owl").is_none());
        assert!(!bank.contains("owl"));
    }

    #[test]
    fn a_broken_stream_is_a_silent_variant_not_an_error() {
        let mut b = fixture_bank();
        let last = b.len() - 100;
        b[last] ^= 0xff; // inside the loop's stream: its hash no longer matches
        let bank = SampleBank::from_bytes(b).unwrap();
        let v = bank.sound("blip").unwrap();
        assert_eq!(v[0].data.len(), 1200);
        assert!(v[1].data.is_empty());
    }

    #[test]
    fn what_is_not_a_bank_says_why() {
        assert_eq!(SampleBank::from_bytes(b"RIFF....".to_vec()).unwrap_err(), BankError::NotABank);
        let mut b = fixture_bank();
        b[4] = 3;
        assert_eq!(SampleBank::from_bytes(b).unwrap_err(), BankError::Version(3));
        let b = fixture_bank();
        assert_eq!(SampleBank::from_bytes(b[..40].to_vec()).unwrap_err(), BankError::Truncated);
        let b = fixture_bank();
        assert!(matches!(SampleBank::from_bytes(b[..b.len() - 1].to_vec()), Err(BankError::Manifest(_))));
    }

    #[test]
    fn a_sound_packs_and_unpacks_and_a_bank_is_built_from_shared_sounds() {
        let bank = SampleBank::from_bytes(fixture_bank()).unwrap();
        let v = bank.sound("blip").unwrap();
        let bytes = pack(&v);
        assert_eq!(bytes.len(), 4 + 2 * 12 + 2 * (1200 + 8000));
        assert_eq!(unpack(&bytes).unwrap(), v.to_vec());
        assert!(unpack(&bytes[..bytes.len() - 1]).is_none());
        assert!(unpack(&[bytes.clone(), vec![0]].concat()).is_none());
        let shared = SampleBank::from_variants(bank.decoded_sounds());
        assert!(Arc::ptr_eq(&shared.sound("blip").unwrap(), &v), "shared, not copied");
        assert!(!shared.contains("hum"), "only what was decoded");
    }

    #[test]
    fn an_in_memory_bank_needs_no_flac() {
        let bank = SampleBank::from_pcm(vec![(
            "tick".into(),
            vec![Sample { sr: 48000, data: vec![1000; 10], mode: Mode::Hits }],
        )]);
        assert!(bank.contains("tick"));
        assert_eq!(bank.entries("tick")[0].frames, 10);
        assert_eq!(bank.sound("tick").unwrap()[0].data, vec![1000; 10]);
    }
}
