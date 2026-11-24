//! The mixer: one voice per node, filled into whatever buffer the device asks for.
//!
//! A voice keeps a [`Clip`] and a cursor measured in the clip's own frames. The
//! cursor advances by the ratio between the clip's rate and the device's, so a
//! 44.1 kHz file plays at the right speed on a 48 kHz device without the file being
//! resampled on load. Channels are mapped the simple way: mono fills every output
//! channel, stereo fills the first two, and anything wider is folded to mono when the
//! device has one channel.

use std::collections::HashMap;

use crate::Clip;

struct Voice {
    clip: Clip,
    /// Position in the clip's frames, fractional so resampling stays smooth.
    cursor: f64,
    volume: f32,
    looping: bool,
    playing: bool,
    finished: bool,
}

/// A set of voices mixed into interleaved `f32` output.
pub struct Mixer {
    rate: u32,
    channels: u16,
    voices: HashMap<u64, Voice>,
}

impl Mixer {
    /// A silent mixer at the given output rate and channel count. The device may
    /// change either with [`Mixer::configure`] before the first voice starts.
    pub fn new(rate: u32, channels: u16) -> Self {
        Self {
            rate: rate.max(1),
            channels: channels.max(1),
            voices: HashMap::new(),
        }
    }

    /// Tells the mixer what the output device is actually running at. Voices already
    /// playing keep their cursors; only the rate they advance at changes.
    pub fn configure(&mut self, rate: u32, channels: u16) {
        self.rate = rate.max(1);
        self.channels = channels.max(1);
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Starts a voice, replacing any voice already on that node. Playback begins at
    /// the start of the clip.
    pub fn start(&mut self, node: u64, clip: Clip, volume: f32, looping: bool) {
        self.voices.insert(
            node,
            Voice {
                clip,
                cursor: 0.0,
                volume: sane_volume(volume),
                looping,
                playing: true,
                finished: false,
            },
        );
    }

    /// Stops and forgets one voice. Returns whether there was one.
    pub fn stop(&mut self, node: u64) -> bool {
        self.voices.remove(&node).is_some()
    }

    pub fn stop_all(&mut self) {
        self.voices.clear();
    }

    pub fn set_volume(&mut self, node: u64, volume: f32) -> bool {
        match self.voices.get_mut(&node) {
            Some(voice) => {
                voice.volume = sane_volume(volume);
                true
            }
            None => false,
        }
    }

    pub fn set_looping(&mut self, node: u64, looping: bool) -> bool {
        match self.voices.get_mut(&node) {
            Some(voice) => {
                voice.looping = looping;
                // A voice that reached the end of a non-looping clip and was then
                // told to loop should come back rather than stay finished.
                if looping && voice.finished {
                    voice.finished = false;
                    voice.playing = true;
                    voice.cursor = 0.0;
                }
                true
            }
            None => false,
        }
    }

    /// The voice's playhead in seconds, or `None` when the node has no voice.
    pub fn position(&self, node: u64) -> Option<f32> {
        let voice = self.voices.get(&node)?;
        Some((voice.cursor / voice.clip.sample_rate() as f64) as f32)
    }

    /// Whether a non-looping voice has run out. The scene hears this and turns the
    /// node's `playing` off.
    pub fn is_finished(&self, node: u64) -> bool {
        self.voices.get(&node).is_some_and(|voice| voice.finished)
    }

    pub fn is_empty(&self) -> bool {
        self.voices.is_empty()
    }

    /// Fills an interleaved output buffer. The buffer is zeroed first, every playing
    /// voice is added in, and the sum is clamped so two loud clips cannot leave the
    /// device's range.
    pub fn mix(&mut self, output: &mut [f32]) {
        output.fill(0.0);
        if self.voices.is_empty() {
            return;
        }

        let output_channels = self.channels as usize;
        let output_frames = output.len() / output_channels.max(1);
        if output_frames == 0 {
            return;
        }

        for voice in self.voices.values_mut() {
            if !voice.playing || voice.finished {
                continue;
            }
            let clip = voice.clip.clone();
            let clip_frames = clip.frames();
            if clip_frames == 0 {
                voice.playing = false;
                voice.finished = true;
                continue;
            }

            let step = clip.sample_rate() as f64 / self.rate as f64;
            let clip_channels = clip.channels() as usize;
            let mut frame = 0;

            while frame < output_frames {
                if voice.cursor >= clip_frames as f64 {
                    if voice.looping {
                        voice.cursor %= clip_frames as f64;
                    } else {
                        voice.cursor = clip_frames as f64;
                        voice.playing = false;
                        voice.finished = true;
                        break;
                    }
                }

                let at = voice.cursor;
                let first = at.floor() as usize;
                let fraction = (at - first as f64) as f32;
                let second = if first + 1 < clip_frames {
                    first + 1
                } else if voice.looping {
                    0
                } else {
                    first
                };

                for channel in 0..output_channels {
                    let sample = channel_sample(
                        &clip,
                        first,
                        second,
                        fraction,
                        channel,
                        output_channels,
                        clip_channels,
                    );
                    output[frame * output_channels + channel] += sample * voice.volume;
                }

                voice.cursor += step;
                frame += 1;
            }
        }

        for sample in output.iter_mut() {
            *sample = sample.clamp(-1.0, 1.0);
        }
    }

    /// Moves every voice forward without producing samples. A headless game, or a
    /// machine with no output device, still sees its sounds end and its playheads
    /// move.
    pub fn advance(&mut self, seconds: f32) {
        let seconds = seconds.max(0.0) as f64;
        for voice in self.voices.values_mut() {
            if !voice.playing || voice.finished {
                continue;
            }
            let clip_frames = voice.clip.frames();
            if clip_frames == 0 {
                voice.playing = false;
                voice.finished = true;
                continue;
            }
            voice.cursor += seconds * voice.clip.sample_rate() as f64;
            if voice.cursor >= clip_frames as f64 {
                if voice.looping {
                    voice.cursor %= clip_frames as f64;
                } else {
                    voice.cursor = clip_frames as f64;
                    voice.playing = false;
                    voice.finished = true;
                }
            }
        }
    }
}

fn sane_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn channel_sample(
    clip: &Clip,
    first: usize,
    second: usize,
    fraction: f32,
    channel: usize,
    output_channels: usize,
    clip_channels: usize,
) -> f32 {
    let read = |frame: usize, channel: usize| clip.at(frame, channel);

    if output_channels == 1 {
        if clip_channels == 1 {
            return read(first, 0) * (1.0 - fraction) + read(second, 0) * fraction;
        }
        let mut sum = 0.0;
        for channel in 0..clip_channels {
            sum += read(first, channel) * (1.0 - fraction) + read(second, channel) * fraction;
        }
        return sum / clip_channels as f32;
    }

    if clip_channels == 1 {
        return read(first, 0) * (1.0 - fraction) + read(second, 0) * fraction;
    }

    if channel < clip_channels {
        read(first, channel) * (1.0 - fraction) + read(second, channel) * fraction
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, channels: u16, rate: u32, value: f32) -> Clip {
        Clip::new(vec![value; frames * channels as usize], rate, channels).unwrap()
    }

    #[test]
    fn one_voice_fills_the_buffer_at_its_volume() {
        let mut mixer = Mixer::new(48_000, 2);
        mixer.start(7, tone(4, 2, 48_000, 0.5), 0.5, false);
        let mut output = [0.0f32; 8];

        mixer.mix(&mut output);

        for sample in output {
            assert!((sample - 0.25).abs() < 1e-6);
        }
    }

    #[test]
    fn a_mono_clip_fills_both_output_channels() {
        let mut mixer = Mixer::new(48_000, 2);
        mixer.start(1, tone(2, 1, 48_000, 0.25), 1.0, false);
        let mut output = [0.0f32; 4];

        mixer.mix(&mut output);

        assert!((output[0] - 0.25).abs() < 1e-6);
        assert!((output[1] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_stereo_clip_folds_to_one_output_channel() {
        let clip = Clip::new(vec![1.0, -1.0, 0.5, 0.5], 48_000, 2).unwrap();
        let mut mixer = Mixer::new(48_000, 1);
        mixer.start(1, clip, 1.0, false);
        let mut output = [0.0f32; 2];

        mixer.mix(&mut output);

        assert!(output[0].abs() < 1e-6);
        assert!((output[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_finished_voice_stops_and_reports_its_end() {
        let mut mixer = Mixer::new(48_000, 1);
        mixer.start(3, tone(2, 1, 48_000, 0.5), 1.0, false);
        let mut output = [0.0f32; 4];

        mixer.mix(&mut output);

        assert!(mixer.is_finished(3));
        assert!(output[2..].iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn a_looping_voice_wraps_instead_of_finishing() {
        let mut mixer = Mixer::new(48_000, 1);
        mixer.start(3, tone(2, 1, 48_000, 0.5), 1.0, true);
        let mut output = [0.0f32; 6];

        mixer.mix(&mut output);

        assert!(!mixer.is_finished(3));
        assert!(output.iter().all(|sample| (*sample - 0.5).abs() < 1e-6));
    }

    #[test]
    fn a_playhead_is_measured_in_seconds() {
        let mut mixer = Mixer::new(48_000, 1);
        mixer.start(9, tone(48_000, 1, 48_000, 0.0), 1.0, false);

        mixer.advance(0.25);

        assert!((mixer.position(9).unwrap() - 0.25).abs() < 1e-5);
    }

    #[test]
    fn silence_and_nonsense_volumes_are_bounded() {
        let mut mixer = Mixer::new(48_000, 1);
        mixer.start(1, tone(1, 1, 48_000, 1.0), 4.0, false);
        let mut output = [0.0f32; 1];
        mixer.mix(&mut output);
        assert!((output[0] - 1.0).abs() < 1e-6);

        mixer.set_volume(1, f32::NAN);
        assert!(mixer.set_volume(1, f32::NAN));
    }
}
