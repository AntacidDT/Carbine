//! Audio assets and the mixer, without a device.
//!
//! Carbine reads two audio files directly: `.wav`, whose container and samples are
//! parsed here, and `.mp3`, which is handed to a pure-Rust decoder. A decoded file
//! becomes a [`Clip`]: interleaved `f32` samples at the rate and channel count the
//! file names. The [`Mixer`] keeps a voice per node and fills the buffer the output
//! device asks for; opening the device is the platform layer's job, which is why
//! nothing in this crate knows what a speaker is.
//!
//! The mixer is deliberately small: volume, looping, and linear resampling to the
//! device rate. It exists so a scene can play more than one thing at once without
//! the scripting layer doing arithmetic.

mod mixer;
mod mp3;
mod mp4;
mod wav;

use std::fmt;
use std::path::Path;
use std::sync::Arc;

pub use mixer::Mixer;

/// Why a file did not become a [`Clip`]. The path is kept so the message a script
/// sees can say which file it was.
#[derive(Debug, Clone)]
pub enum AudioError {
    /// The file could not be read at all.
    Source { path: String, why: String },
    /// The file was read but its audio would not decode.
    Decode { path: String, why: String },
    /// The extension is not one Carbine's audio loaders know.
    Unsupported { path: String, why: String },
}

impl fmt::Display for AudioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source { path, why } => {
                write!(
                    formatter,
                    "the audio file \"{path}\" could not be read: {why}"
                )
            }
            Self::Decode { path, why } => {
                write!(
                    formatter,
                    "the audio file \"{path}\" would not decode: {why}"
                )
            }
            Self::Unsupported { path, why } => write!(
                formatter,
                "\"{path}\" is not an audio file Carbine reads: {why}. Audio comes to \
                 Carbine as .wav or .mp3."
            ),
        }
    }
}

impl std::error::Error for AudioError {}

/// One decoded sound: interleaved `f32` samples, and the rate and channel count they
/// were decoded at. Clips are reference counted, so a voice costs a pointer rather
/// than a copy of the song.
#[derive(Clone, Debug)]
pub struct Clip {
    data: Arc<ClipData>,
}

#[derive(Debug)]
struct ClipData {
    samples: Vec<f32>,
    rate: u32,
    channels: u16,
}

impl Clip {
    /// Wraps samples that are already interleaved. The count must divide evenly by
    /// the channel count, and neither the rate nor the channels may be zero.
    pub fn new(samples: Vec<f32>, rate: u32, channels: u16) -> Result<Self, String> {
        if rate == 0 {
            return Err("its sample rate is zero".to_owned());
        }
        if channels == 0 {
            return Err("it names no channels".to_owned());
        }
        if !samples.len().is_multiple_of(channels as usize) {
            return Err("its samples do not divide into whole frames".to_owned());
        }
        Ok(Self {
            data: Arc::new(ClipData {
                samples,
                rate,
                channels,
            }),
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.data.rate
    }

    pub fn channels(&self) -> u16 {
        self.data.channels
    }

    pub fn frames(&self) -> usize {
        self.data.samples.len() / self.data.channels as usize
    }

    /// How long the clip runs, in seconds.
    pub fn duration(&self) -> f32 {
        self.frames() as f32 / self.data.rate as f32
    }

    /// One channel of one frame, already interleaved. The mixer asks for frames it
    /// has already checked are inside the clip.
    pub(crate) fn at(&self, frame: usize, channel: usize) -> f32 {
        self.data.samples[frame * self.data.channels as usize + channel]
    }
}

/// The audio track of one `.mp4`, or `None` when the film has none. The video
/// frames are the platform layer's business; this is what the mixer plays beside
/// them.
pub fn load_mp4_audio(path: impl AsRef<Path>) -> Result<Option<Clip>, AudioError> {
    mp4::audio(path.as_ref())
}

/// Reads one `.wav` or `.mp3` file into a [`Clip`].
pub fn load(path: impl AsRef<Path>) -> Result<Clip, AudioError> {
    let path = path.as_ref();
    let shown = path.display().to_string();
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    match extension.as_str() {
        "wav" => wav::read(path),
        "mp3" => mp3::read(path),
        _ => Err(AudioError::Unsupported {
            path: shown,
            why: "only .wav and .mp3 are audio files".to_owned(),
        }),
    }
}
