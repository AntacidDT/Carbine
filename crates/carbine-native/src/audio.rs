//! The platform audio layer: a mixer, an output stream, and the bridge that keeps
//! both in step with the scene.
//!
//! The scene knows which sound nodes are playing and how loud; this module keeps one
//! mixer voice per node. The device callback only touches the mixer. The frame path
//! calls [`sync`] after the world has stepped, which starts and stops voices, writes
//! the playhead back into the scene, and turns a finished clip off.
//!
//! A machine with no output device is not an error. The mixer still advances when the
//! world steps, so a headless test sees a sound start, run and end exactly as a
//! windowed game does.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use carbine_audio::{AudioError, Clip, Mixer};
use carbine_core::{NodeId, Scene, SoundNode};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

thread_local! {
    /// `cpal::Stream` is not required to be `Send` on every backend, so the output
    /// lives on the thread that opened it, beside the renderer.
    static AUDIO: RefCell<AudioEngine> = RefCell::new(AudioEngine::new());
}

struct AudioEngine {
    mixer: Arc<Mutex<Mixer>>,
    output: Option<Output>,
    tried_output: bool,
    clips: HashMap<String, Clip>,
    /// The path each live voice was started from, so a changed source replaces it.
    sources: HashMap<u64, String>,
}

impl AudioEngine {
    fn new() -> Self {
        Self {
            // The rate is replaced by the device's own before a voice starts. The
            // headless numbers are the ones a mixer would pick for a desktop.
            mixer: Arc::new(Mutex::new(Mixer::new(48_000, 2))),
            output: None,
            tried_output: false,
            clips: HashMap::new(),
            sources: HashMap::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Mixer> {
        self.mixer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Opens the output once. A machine without a device, or one whose device is
    /// busy, keeps the mixer and simply plays to nobody.
    fn ensure_output(&mut self) {
        if self.tried_output || self.output.is_some() {
            return;
        }
        self.tried_output = true;
        if let Ok(output) = Output::open(self.mixer.clone()) {
            self.output = Some(output);
        }
    }

    fn clip(&mut self, path: &str) -> Result<Clip, AudioError> {
        if let Some(clip) = self.clips.get(path) {
            return Ok(clip.clone());
        }
        let clip = carbine_audio::load(path)?;
        self.clips.insert(path.to_owned(), clip.clone());
        Ok(clip)
    }

    fn play(
        &mut self,
        node: NodeId,
        path: &str,
        volume: f32,
        looping: bool,
    ) -> Result<f32, AudioError> {
        let clip = self.clip(path)?;
        let duration = clip.duration();
        self.lock().start(node.0, clip, volume, looping);
        self.sources.insert(node.0, path.to_owned());
        Ok(duration)
    }

    fn stop(&mut self, node: NodeId) {
        self.lock().stop(node.0);
        self.sources.remove(&node.0);
    }

    /// Reconciles the scene's sound nodes with the mixer, advances a silent mixer,
    /// and writes the playheads back.
    fn sync(&mut self, scene: &mut Scene, nodes: &[SoundNode], dt: f32, realtime: bool) {
        if realtime {
            self.ensure_output();
        }

        let mut live: Vec<u64> = nodes.iter().map(|node| node.id.0).collect();
        // A video's sound track is a voice too, and it is not a Sound node. Its
        // own layer starts and stops it; here it is only protected from the sweep.
        live.extend(scene.video_nodes().iter().map(|video| video.id.0));
        let stale: Vec<u64> = self
            .sources
            .keys()
            .copied()
            .filter(|id| !live.contains(id))
            .collect();
        for id in stale {
            self.lock().stop(id);
            self.sources.remove(&id);
        }

        for node in nodes {
            let id = node.id.0;
            if !node.playing {
                self.lock().stop(id);
                self.sources.remove(&id);
                continue;
            }
            let Some(path) = node.source.as_deref() else {
                self.lock().stop(id);
                self.sources.remove(&id);
                let _ = scene.set_sound_playing(node.id, false);
                continue;
            };
            let known = self.sources.get(&id);
            if known.map(String::as_str) != Some(path) {
                match self.clip(path) {
                    Ok(clip) => {
                        self.lock().start(id, clip, node.volume, node.looping);
                        self.sources.insert(id, path.to_owned());
                    }
                    Err(_) => {
                        self.lock().stop(id);
                        self.sources.remove(&id);
                        let _ = scene.set_sound_playing(node.id, false);
                        continue;
                    }
                }
            } else {
                self.lock().set_volume(id, node.volume);
                self.lock().set_looping(id, node.looping);
            }
        }

        // A device pulls the mixer itself. Without one, the world's clock is the only
        // clock there is, so a sound still ends after its length of game time.
        if self.output.is_none() {
            self.lock().advance(dt);
        }

        for node in nodes {
            if !node.playing {
                continue;
            }
            let id = node.id.0;
            if self.lock().is_finished(id) {
                let duration = self.lock().position(id).unwrap_or(node.time);
                self.lock().stop(id);
                self.sources.remove(&id);
                let _ = scene.set_sound_playing(node.id, false);
                let _ = scene.set_sound_time(node.id, duration);
            } else if let Some(position) = self.lock().position(id) {
                let _ = scene.set_sound_time(node.id, position);
            }
        }
    }
}

/// Runs the audio bridge once a frame. `realtime` says whether a window is drawing,
/// which is the only time it is worth opening a device; a headless `step()` stays
/// silent and advances on the step's own clock.
pub(crate) fn sync(scene: &mut Scene, dt: f32, realtime: bool) {
    let nodes = scene.sound_nodes();
    AUDIO.with(|cell| {
        cell.borrow_mut().sync(scene, &nodes, dt, realtime);
    });
}

pub(crate) fn play(
    node: NodeId,
    path: &str,
    volume: f32,
    looping: bool,
) -> Result<f32, AudioError> {
    AUDIO.with(|cell| cell.borrow_mut().play(node, path, volume, looping))
}

/// Starts a decoded clip on a node. Used by the video layer, whose sound track is
/// already in memory rather than at a path the mixer could load.
pub(crate) fn play_clip(node: NodeId, clip: Clip, volume: f32, looping: bool) {
    AUDIO.with(|cell| {
        cell.borrow_mut()
            .lock()
            .start(node.0, clip, volume, looping);
    });
}

pub(crate) fn stop(node: NodeId) {
    AUDIO.with(|cell| cell.borrow_mut().stop(node));
}

pub(crate) fn set_volume(node: NodeId, volume: f32) {
    AUDIO.with(|cell| {
        cell.borrow().lock().set_volume(node.0, volume);
    });
}

pub(crate) fn set_looping(node: NodeId, looping: bool) {
    AUDIO.with(|cell| {
        cell.borrow().lock().set_looping(node.0, looping);
    });
}

/// The playhead the mixer is actually at, or `None` when this node has no voice.
pub(crate) fn position(node: NodeId) -> Option<f32> {
    AUDIO.with(|cell| cell.borrow().lock().position(node.0))
}

/// How long a file runs, loading and caching it if this is the first ask.
pub(crate) fn duration(path: &str) -> Result<f32, AudioError> {
    AUDIO.with(|cell| cell.borrow_mut().clip(path).map(|clip| clip.duration()))
}

/// Silences and forgets every voice. Called when the window closes, so no sound
/// outlives the game that started it.
pub(crate) fn stop_all() {
    AUDIO.with(|cell| {
        let mut engine = cell.borrow_mut();
        engine.lock().stop_all();
        engine.sources.clear();
        engine.output = None;
        engine.tried_output = false;
    });
}

pub(crate) fn audio_error(error: AudioError) -> PyErr {
    PyValueError::new_err(error.to_string())
}

struct Output {
    _stream: cpal::Stream,
}

impl Output {
    fn open(mixer: Arc<Mutex<Mixer>>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "there is no audio output device".to_owned())?;
        let default = device
            .default_output_config()
            .map_err(|error| error.to_string())?;
        let config = default.config();
        mixer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .configure(config.sample_rate, config.channels);

        let stream = match default.sample_format() {
            SampleFormat::F32 => build::<f32>(&device, config, mixer, |value| value),
            SampleFormat::I16 => build::<i16>(&device, config, mixer, |value| {
                (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
            }),
            SampleFormat::U16 => build::<u16>(&device, config, mixer, |value| {
                ((value.clamp(-1.0, 1.0) * 0.5 + 0.5) * u16::MAX as f32) as u16
            }),
            SampleFormat::I32 => build::<i32>(&device, config, mixer, |value| {
                (value.clamp(-1.0, 1.0) * i32::MAX as f32) as i32
            }),
            SampleFormat::U8 => build::<u8>(&device, config, mixer, |value| {
                ((value.clamp(-1.0, 1.0) * 0.5 + 0.5) * u8::MAX as f32) as u8
            }),
            other => return Err(format!("the output device asks for {other:?} samples")),
        }
        .map_err(|error| error.to_string())?;

        stream.play().map_err(|error| error.to_string())?;
        Ok(Self { _stream: stream })
    }
}

fn build<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mixer: Arc<Mutex<Mixer>>,
    convert: fn(f32) -> T,
) -> Result<cpal::Stream, cpal::Error>
where
    T: cpal::SizedSample + 'static,
{
    let mut scratch = Vec::new();
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            scratch.resize(data.len(), 0.0f32);
            if let Ok(mut mixer) = mixer.try_lock() {
                mixer.mix(&mut scratch);
            } else {
                scratch.fill(0.0);
            }
            for (out, sample) in data.iter_mut().zip(&scratch) {
                *out = convert(*sample);
            }
        },
        |_error| {},
        None,
    )
}
