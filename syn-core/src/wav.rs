//! Minimal WAV encoding — enough to hand a render to another tool or to an
//! ear. 16-bit PCM, mono or stereo.

/// Encodes mono samples as a 16-bit PCM WAV file.
pub fn encode_mono(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    encode(samples, 1, sample_rate)
}

/// Encodes two ears as a 16-bit PCM stereo WAV file (the shorter decides
/// the length).
pub fn encode_stereo(left: &[f32], right: &[f32], sample_rate: u32) -> Vec<u8> {
    let frames: Vec<f32> = left.iter().zip(right).flat_map(|(l, r)| [*l, *r]).collect();
    encode(&frames, 2, sample_rate)
}

fn encode(samples: &[f32], channels: u16, sample_rate: u32) -> Vec<u8> {
    let bytes = samples.len() * 2;
    let mut out = Vec::with_capacity(44 + bytes);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + bytes) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2 * u32::from(channels)).to_le_bytes()); // byte rate
    out.extend_from_slice(&(2 * channels).to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(bytes as u32).to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_describes_the_data() {
        let wav = encode_mono(&[0.0, 1.0, -1.0, 0.5], 48000);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 8);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 8);
        assert_eq!(i16::from_le_bytes(wav[46..48].try_into().unwrap()), 32767);
        assert_eq!(i16::from_le_bytes(wav[48..50].try_into().unwrap()), -32767);
    }

    #[test]
    fn a_stereo_file_interleaves_the_ears() {
        let wav = encode_stereo(&[1.0, 0.0], &[-1.0, 0.5], 44100);
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(wav[28..32].try_into().unwrap()), 44100 * 4);
        assert_eq!(u16::from_le_bytes(wav[32..34].try_into().unwrap()), 4);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 8);
        assert_eq!(i16::from_le_bytes(wav[44..46].try_into().unwrap()), 32767);
        assert_eq!(i16::from_le_bytes(wav[46..48].try_into().unwrap()), -32767);
    }
}
