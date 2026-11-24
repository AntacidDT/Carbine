//! The `.mp3` loader, handed to a pure-Rust decoder.
//!
//! Carbine does not carry its own MP3 decoder: the format is a stack of Huffman
//! tables, an IMDCT and a polyphase synthesis bank, and a second implementation of
//! it would be a second thing to be wrong. The decoder is a library dependency, the
//! same way the BC7 *encoder* is; what Carbine owns is the decision to read MP3 at
//! all and the `Clip` it hands the mixer.

use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::default::{get_codecs, get_probe};

use crate::{AudioError, Clip};

pub(crate) fn read(path: &Path) -> Result<Clip, AudioError> {
    let bytes = std::fs::read(path).map_err(|error| AudioError::Source {
        path: path.display().to_string(),
        why: error.to_string(),
    })?;
    decode(&bytes).map_err(|why| AudioError::Decode {
        path: path.display().to_string(),
        why,
    })
}

fn decode(bytes: &[u8]) -> Result<Clip, String> {
    let stream = MediaSourceStream::new(
        Box::new(std::io::Cursor::new(bytes.to_vec())),
        Default::default(),
    );
    let mut hint = Hint::new();
    hint.with_extension("mp3");

    let mut format = get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| error.to_string())?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| "the file has no audio track".to_owned())?;
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
            // A damaged frame is skipped; the rest of the file is still playable.
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
        return Err("it decoded to no samples".to_owned());
    }

    Clip::new(samples, rate, channels)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-second silent MP3, written by the test suite rather than kept as a
    /// binary fixture. The encoder is the system's; a machine without one skips the
    /// test rather than failing, because the decoder is what is under test.
    fn silent_mp3() -> Option<Vec<u8>> {
        let directory = std::env::temp_dir().join("carbine-audio-mp3-tests");
        std::fs::create_dir_all(&directory).ok()?;
        let wav = directory.join("silence.wav");
        let mp3 = directory.join("silence.mp3");
        std::fs::write(&wav, crate::wav::silent_wav()).ok()?;
        let status = std::process::Command::new("ffmpeg")
            .args(["-y", "-loglevel", "error", "-i"])
            .arg(&wav)
            .arg(&mp3)
            .status()
            .ok()?;
        if !status.success() {
            return None;
        }
        let bytes = std::fs::read(&mp3).ok();
        std::fs::remove_dir_all(&directory).ok();
        bytes
    }

    #[test]
    fn an_mp3_decodes_into_whole_frames() {
        let Some(bytes) = silent_mp3() else {
            return;
        };
        let clip = decode(&bytes).unwrap();

        assert!(clip.duration() > 0.5, "{}", clip.duration());
        assert!(clip.channels() > 0);
        assert!(clip.frames() > 0);
    }

    #[test]
    fn bytes_that_are_not_mp3_are_refused() {
        assert!(decode(b"not an mp3").is_err());
    }
}
