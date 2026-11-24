//! The `.wav` loader: RIFF chunks, PCM and IEEE float samples, nothing else.
//!
//! WAV is the one audio format Carbine parses itself. The container is a list of
//! length-prefixed chunks, the two that matter are `fmt ` and `data`, and the samples
//! are either unsigned bytes, signed little-endian integers, or floats. A file that
//! asks for a codec beyond those is refused with a sentence rather than played as
//! noise.

use std::path::Path;

use crate::{AudioError, Clip};

const PCM: u16 = 1;
const FLOAT: u16 = 3;
const EXTENSIBLE: u16 = 0xfffe;

struct Format {
    kind: u16,
    channels: u16,
    rate: u32,
    block_align: u16,
    bits: u16,
}

pub(crate) fn read(path: &Path) -> Result<Clip, AudioError> {
    let bytes = std::fs::read(path).map_err(|error| AudioError::Source {
        path: path.display().to_string(),
        why: error.to_string(),
    })?;
    parse(&bytes).map_err(|why| AudioError::Decode {
        path: path.display().to_string(),
        why,
    })
}

fn parse(bytes: &[u8]) -> Result<Clip, String> {
    if bytes.len() < 12 {
        return Err("it is shorter than a RIFF header".to_owned());
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("it is not a RIFF/WAVE file".to_owned());
    }

    let mut format: Option<Format> = None;
    let mut data: Option<(usize, usize)> = None;
    let mut at = 12;

    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        at += 8;

        // A streaming WAV may name a data size that runs past the bytes on disk; the
        // samples that are there are still playable, so the chunk is clipped rather
        // than the whole file refused.
        let end = at.saturating_add(size).min(bytes.len());
        match id {
            b"fmt " => format = Some(read_format(&bytes[at..end])?),
            b"data" => data = Some((at, end - at)),
            _ => {}
        }

        // Chunks are padded to an even length. The final chunk may omit the padding,
        // which makes the next read start past the end and simply stop the walk.
        at = end + (size & 1);
    }

    let format = format.ok_or_else(|| "it has no fmt chunk".to_owned())?;
    let (start, length) = data.ok_or_else(|| "it has no data chunk".to_owned())?;
    if length == 0 {
        return Err("its data chunk is empty".to_owned());
    }

    let bytes_per_sample = match format.bits {
        8 | 16 | 24 | 32 => format.bits as usize / 8,
        64 if format.kind == FLOAT => 8,
        _ => {
            return Err(format!(
                "it asks for {} bit samples, which Carbine does not read",
                format.bits
            ));
        }
    };
    if format.channels == 0 {
        return Err("it names no channels".to_owned());
    }
    if format.rate == 0 {
        return Err("its sample rate is zero".to_owned());
    }

    let frame_bytes = format.channels as usize * bytes_per_sample;
    let stride = if format.block_align as usize >= frame_bytes {
        format.block_align as usize
    } else {
        frame_bytes
    };
    let frames = length / stride;
    if frames == 0 {
        return Err("it holds less than one whole frame".to_owned());
    }

    let mut samples = Vec::with_capacity(frames * format.channels as usize);
    for frame in 0..frames {
        let base = start + frame * stride;
        for channel in 0..format.channels as usize {
            let offset = base + channel * bytes_per_sample;
            samples.push(read_sample(
                &bytes[offset..offset + bytes_per_sample],
                format.kind,
                format.bits,
            ));
        }
    }

    Clip::new(samples, format.rate, format.channels)
}

fn read_format(bytes: &[u8]) -> Result<Format, String> {
    if bytes.len() < 16 {
        return Err("its fmt chunk is shorter than sixteen bytes".to_owned());
    }
    let mut kind = u16::from_le_bytes(bytes[0..2].try_into().unwrap());
    let channels = u16::from_le_bytes(bytes[2..4].try_into().unwrap());
    let rate = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let block_align = u16::from_le_bytes(bytes[12..14].try_into().unwrap());
    let bits = u16::from_le_bytes(bytes[14..16].try_into().unwrap());

    if kind == EXTENSIBLE {
        if bytes.len() < 40 {
            return Err("its extensible fmt chunk is too short".to_owned());
        }
        // The first two bytes of the SubFormat GUID are the format code the
        // non-extensible header would have carried.
        kind = u16::from_le_bytes(bytes[24..26].try_into().unwrap());
        // wValidBitsPerSample is not how many bytes are in each container. In a
        // 24-in-32 file the sample is left-justified in four bytes; reading three
        // would take the wrong bits, so the container width stays as it was.
        let _valid = u16::from_le_bytes(bytes[18..20].try_into().unwrap());
    }

    match kind {
        PCM => {}
        FLOAT => {}
        other => {
            return Err(format!(
                "it holds format {other}, and Carbine reads only PCM and IEEE float"
            ));
        }
    }

    Ok(Format {
        kind,
        channels,
        rate,
        block_align,
        bits,
    })
}

fn read_sample(bytes: &[u8], kind: u16, bits: u16) -> f32 {
    if kind == FLOAT {
        return match bits {
            32 => f32::from_le_bytes(bytes.try_into().unwrap()),
            64 => f64::from_le_bytes(bytes.try_into().unwrap()) as f32,
            _ => 0.0,
        };
    }

    match bits {
        // Eight bit PCM is unsigned, with 128 as silence.
        8 => (bytes[0] as f32 - 128.0) / 128.0,
        16 => i16::from_le_bytes(bytes.try_into().unwrap()) as f32 / 32_768.0,
        24 => {
            let value = (bytes[0] as i32) | ((bytes[1] as i32) << 8) | ((bytes[2] as i32) << 16);
            let signed = (value << 8) >> 8;
            signed as f32 / 8_388_608.0
        }
        32 => {
            let value = i32::from_le_bytes(bytes.try_into().unwrap());
            value as f32 / 2_147_483_648.0
        }
        _ => 0.0,
    }
}

/// A one-second silent mono WAV, used by the MP3 test as an encoder input.
#[cfg(test)]
pub(crate) fn silent_wav() -> Vec<u8> {
    let rate = 8_000u32;
    let frames = rate as usize;
    let mut data = Vec::with_capacity(frames * 2);
    for _ in 0..frames {
        data.extend_from_slice(&0i16.to_le_bytes());
    }
    tests::wav(PCM, 1, rate, 16, &data)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn wav(kind: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let block_align = channels * bits / 8;
        let byte_rate = rate * block_align as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36u32 + data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&channels.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&byte_rate.to_le_bytes());
        bytes.extend_from_slice(&block_align.to_le_bytes());
        bytes.extend_from_slice(&bits.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(data);
        bytes
    }

    #[test]
    fn sixteen_bit_pcm_becomes_signed_floats() {
        let mut data = Vec::new();
        for sample in [0i16, 16_384, -16_384, 32_767, -32_768] {
            data.extend_from_slice(&sample.to_le_bytes());
        }
        let clip = parse(&wav(PCM, 1, 44_100, 16, &data)).unwrap();

        assert_eq!((clip.sample_rate(), clip.channels()), (44_100, 1));
        assert_eq!(clip.frames(), 5);
        assert!((clip.at(1, 0) - 0.5).abs() < 1e-6);
        assert!((clip.at(2, 0) + 0.5).abs() < 1e-6);
        assert!((clip.at(4, 0) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn eight_bit_pcm_is_unsigned_with_128_as_silence() {
        let clip = parse(&wav(PCM, 1, 8_000, 8, &[128, 255, 0])).unwrap();

        assert!((clip.at(0, 0)).abs() < 1e-6);
        assert!((clip.at(1, 0) - (127.0 / 128.0)).abs() < 1e-6);
        assert!((clip.at(2, 0) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn stereo_frames_keep_their_channels_together() {
        let mut data = Vec::new();
        for value in [1i16, 2, 3, 4] {
            data.extend_from_slice(&value.to_le_bytes());
        }
        let clip = parse(&wav(PCM, 2, 48_000, 16, &data)).unwrap();

        assert_eq!(clip.channels(), 2);
        assert_eq!(clip.frames(), 2);
        assert!((clip.at(0, 1) - 2.0 / 32_768.0).abs() < 1e-6);
        assert!((clip.at(1, 0) - 3.0 / 32_768.0).abs() < 1e-6);
    }

    #[test]
    fn float_samples_are_read_as_floats() {
        let mut data = Vec::new();
        for value in [0.0f32, 0.25, -0.5] {
            data.extend_from_slice(&value.to_le_bytes());
        }
        let clip = parse(&wav(FLOAT, 1, 48_000, 32, &data)).unwrap();

        assert!((clip.at(1, 0) - 0.25).abs() < 1e-6);
        assert!((clip.at(2, 0) + 0.5).abs() < 1e-6);
    }

    #[test]
    fn extensible_pcm_keeps_the_container_width_of_its_samples() {
        // 24-in-32: four bytes per frame, the sample left-justified in the word.
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&EXTENSIBLE.to_le_bytes());
        fmt.extend_from_slice(&1u16.to_le_bytes()); // one channel
        fmt.extend_from_slice(&8_000u32.to_le_bytes()); // rate
        fmt.extend_from_slice(&32_000u32.to_le_bytes()); // byte rate
        fmt.extend_from_slice(&4u16.to_le_bytes()); // block align
        fmt.extend_from_slice(&32u16.to_le_bytes()); // container bits
        fmt.extend_from_slice(&22u16.to_le_bytes()); // cbSize
        fmt.extend_from_slice(&24u16.to_le_bytes()); // valid bits
        fmt.extend_from_slice(&1u32.to_le_bytes()); // speaker mask
        fmt.extend_from_slice(&1u16.to_le_bytes()); // SubFormat: PCM
        fmt.extend_from_slice(&[0u8; 14]); // the rest of the GUID
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&((4 + 8 + fmt.len() + 8 + 8) as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&fmt);
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&8u32.to_le_bytes());
        bytes.extend_from_slice(&0x4000_0000i32.to_le_bytes()); // 0.5 once shifted
        bytes.extend_from_slice(&(0xC000_0000i64 as i32).to_le_bytes()); // -0.5

        let clip = parse(&bytes).unwrap();

        assert_eq!(clip.frames(), 2);
        assert!((clip.at(0, 0) - 0.5).abs() < 1e-6, "{}", clip.at(0, 0));
        assert!((clip.at(1, 0) + 0.5).abs() < 1e-6, "{}", clip.at(1, 0));
    }

    #[test]
    fn a_file_that_is_not_wav_is_refused_with_a_sentence() {
        let error = parse(b"not a wave file").unwrap_err();
        assert!(error.contains("RIFF"), "{error}");

        let error = parse(&wav(2, 1, 44_100, 16, &[0, 0])).unwrap_err();
        assert!(error.contains("only PCM"), "{error}");
    }

    #[test]
    fn an_odd_chunk_is_skipped_by_its_padding() {
        let mut bytes = wav(PCM, 1, 8_000, 8, &[128]);
        // Put a one-byte unknown chunk between the header and the data chunk; the
        // walk must step over its pad byte to find `data`.
        let split = bytes.len() - 1 - 8;
        let mut with_chunk = bytes[..split].to_vec();
        with_chunk.extend_from_slice(b"junk");
        with_chunk.extend_from_slice(&1u32.to_le_bytes());
        with_chunk.push(0);
        with_chunk.push(0);
        with_chunk.extend_from_slice(&bytes[split..]);
        bytes = with_chunk;

        let clip = parse(&bytes).unwrap();
        assert_eq!(clip.frames(), 1);
    }
}
