//! The audio track of an `.mp4`.
//!
//! The film itself is decoded by the platform layer. Its sound is usually AAC in
//! the same container, which is another codec and another track; the demuxer hands
//! back only the audio packets, and the mixer gets a clip. A silent film answers
//! `None` rather than a refusal.

use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::default::{get_codecs, get_probe};

use crate::{AudioError, Clip};

pub(crate) fn audio(path: &Path) -> Result<Option<Clip>, AudioError> {
    let bytes = std::fs::read(path).map_err(|error| AudioError::Source {
        path: path.display().to_string(),
        why: error.to_string(),
    })?;
    decode(&bytes).map_err(|why| AudioError::Decode {
        path: path.display().to_string(),
        why,
    })
}

fn decode(bytes: &[u8]) -> Result<Option<Clip>, String> {
    let stream = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(bytes.to_vec())),
        Default::default(),
    );
    let mut hint = Hint::new();
    hint.with_extension("mp4");

    let mut format = get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| error.to_string())?;

    let Some(track) = format.default_track(TrackType::Audio) else {
        return Ok(None);
    };
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or_else(|| "the audio track names no codec".to_owned())?;
    let mut decoder = get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(|error| error.to_string())?;

    let mut samples = Vec::new();
    let mut rate = 0;
    let mut channels = 0;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => return Err(error.to_string()),
        };
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(error.to_string()),
        };
        let spec = decoded.spec();
        if rate == 0 {
            rate = spec.rate();
            channels = spec.channels().count() as u16;
        }
        let frames = decoded.frames();
        let width = spec.channels().count();
        if rate == 0 || width == 0 {
            continue;
        }
        let mut interleaved = vec![0.0f32; frames * width];
        decoded.copy_to_slice_interleaved(&mut interleaved);
        samples.extend_from_slice(&interleaved);
    }

    if samples.is_empty() {
        return Ok(None);
    }
    Clip::new(samples, rate, channels)
        .map(Some)
        .map_err(|why| why.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn film() -> Option<Vec<u8>> {
        let directory = std::env::temp_dir().join("carbine-audio-mp4-tests");
        std::fs::create_dir_all(&directory).ok()?;
        let mp4 = directory.join("tone.mp4");
        let status = std::process::Command::new("ffmpeg")
            .args(["-y", "-loglevel", "error", "-f", "lavfi", "-i"])
            .arg("sine=frequency=440:duration=0.2")
            .args(["-c:a", "aac", "-vn"])
            .arg(&mp4)
            .status()
            .ok()?;
        if !status.success() {
            return None;
        }
        let bytes = std::fs::read(&mp4).ok();
        std::fs::remove_dir_all(&directory).ok();
        bytes
    }

    #[test]
    fn the_sound_track_of_an_mp4_decodes() {
        let Some(bytes) = film() else {
            return;
        };
        let clip = decode(&bytes).unwrap().expect("an audio track");
        assert!(clip.duration() > 0.1, "{}", clip.duration());
        assert!(clip.channels() > 0);
    }
}
