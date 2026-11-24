//! The film layer: probing an `.mp4`, decoding its frames off the frame thread, and
//! starting its sound track in the mixer.
//!
//! H.264 and AAC are not codecs Carbine writes; they are delegated to the system's
//! `ffmpeg` for frames and to a pure-Rust decoder for the audio track. The frames
//! arrive on a worker thread and are uploaded to the GPU at the playhead the scene
//! keeps. A machine without `ffmpeg` can still load and inspect the file; calling
//! `play()` says what is missing.

use std::collections::HashMap;
use std::io::{BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};
use std::sync::{Arc, Mutex};

use carbine_audio::Clip;
use carbine_core::{NodeId, Scene};
use carbine_render::Renderer;

use crate::audio;

thread_local! {
    static FILMS: std::cell::RefCell<FilmEngine> =
        std::cell::RefCell::new(FilmEngine::new());
}

/// What the probe found.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Meta {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub duration: f32,
}

struct Frame {
    pts: f32,
    rgba: Vec<u8>,
}

struct Playback {
    source: String,
    meta: Meta,
    receiver: Receiver<Frame>,
    child: Arc<Mutex<Child>>,
    audio: Option<Clip>,
    volume: f32,
    looping: bool,
    last: Option<Frame>,
    pending: Option<Frame>,
    uploaded: Option<f32>,
    ended: bool,
}

impl Drop for Playback {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

struct FilmEngine {
    films: HashMap<u64, Playback>,
}

impl FilmEngine {
    fn new() -> Self {
        Self {
            films: HashMap::new(),
        }
    }

    fn play(
        &mut self,
        node: NodeId,
        path: &str,
        volume: f32,
        looping: bool,
    ) -> Result<Meta, String> {
        let mut meta = probe(path)?;
        // A phone video can be 4K. Cap the decoded frame so a screen in a scene
        // does not spend sixty megabytes per frame; the film keeps its aspect.
        if meta.width > 1280 {
            let height = (meta.height as f32 * 1280.0 / meta.width as f32).round() as u32;
            meta.height = height.max(2).next_multiple_of(2);
            meta.width = 1280;
        }
        // A film with a codec we cannot decode still plays its picture; only the
        // probe and the frame decoder are required.
        let audio = carbine_audio::load_mp4_audio(path).ok().flatten();
        if let Some(clip) = &audio {
            audio::play_clip(node, clip.clone(), volume, looping);
        }
        let (receiver, child) = spawn_frames(path, meta)?;
        self.films.insert(
            node.0,
            Playback {
                source: path.to_owned(),
                meta,
                receiver,
                child,
                audio,
                volume,
                looping,
                last: None,
                pending: None,
                uploaded: None,
                ended: false,
            },
        );
        Ok(meta)
    }

    fn stop(&mut self, node: NodeId) {
        self.films.remove(&node.0);
        audio::stop(node);
    }

    fn stop_all(&mut self) {
        self.films.clear();
    }

    fn set_looping(&mut self, node: NodeId, looping: bool) {
        if let Some(film) = self.films.get_mut(&node.0) {
            film.looping = looping;
        }
    }

    fn restart(&mut self, node: NodeId) {
        let Some(film) = self.films.get(&node.0) else {
            return;
        };
        let source = film.source.clone();
        let meta = film.meta;
        let looping = film.looping;
        let volume = film.volume;
        let audio_clip = film.audio.clone();

        let (receiver, child) = match spawn_frames(&source, meta) {
            Ok(spawned) => spawned,
            Err(_) => return,
        };
        if let Some(clip) = &audio_clip {
            audio::play_clip(node, clip.clone(), volume, looping);
        }
        self.films.insert(
            node.0,
            Playback {
                source,
                meta,
                receiver,
                child,
                audio: audio_clip,
                volume,
                looping,
                last: None,
                pending: None,
                uploaded: None,
                ended: false,
            },
        );
    }

    /// Pushes the playhead and whatever frame belongs to it. Without a renderer the
    /// film still runs on the game clock, which is what a headless `step()` sees.
    fn sync(&mut self, scene: &mut Scene, mut renderer: Option<&mut Renderer>, dt: f32) {
        let nodes = scene.video_nodes();
        let live: Vec<u64> = nodes.iter().map(|node| node.id.0).collect();
        let stale: Vec<u64> = self
            .films
            .keys()
            .copied()
            .filter(|id| !live.contains(id))
            .collect();
        for id in stale {
            self.films.remove(&id);
            audio::stop(NodeId(id));
        }

        for node in &nodes {
            if !node.playing {
                continue;
            }
            let mut restart = false;
            let mut finish = false;
            let mut duration = 0.0;

            {
                let Some(film) = self.films.get_mut(&node.id.0) else {
                    let _ = scene.set_video_playing(node.id, false);
                    continue;
                };

                let time = node.time + dt.max(0.0);
                let _ = scene.set_video_time(node.id, time);

                loop {
                    if let Some(frame) = film.pending.take() {
                        if frame.pts <= time + 0.001 {
                            film.last = Some(frame);
                            continue;
                        }
                        film.pending = Some(frame);
                        break;
                    }
                    match film.receiver.try_recv() {
                        Ok(frame) => film.pending = Some(frame),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            film.ended = true;
                            break;
                        }
                    }
                }

                if let Some(frame) = &film.last
                    && film.uploaded != Some(frame.pts)
                {
                    if let Some(renderer) = renderer.as_deref_mut() {
                        renderer.upload_video(
                            node.id.0,
                            film.meta.width,
                            film.meta.height,
                            &frame.rgba,
                        );
                    }
                    film.uploaded = Some(frame.pts);
                }

                let finished = time >= film.meta.duration && film.pending.is_none() && film.ended;
                let past_end = time >= film.meta.duration + 1.0 / film.meta.fps.max(1.0);
                if finished || past_end {
                    if film.looping {
                        restart = true;
                    } else {
                        finish = true;
                        duration = film.meta.duration;
                    }
                }
            }

            if restart {
                let _ = scene.set_video_time(node.id, 0.0);
                self.restart(node.id);
            } else if finish {
                let _ = scene.set_video_playing(node.id, false);
                self.films.remove(&node.id.0);
                audio::stop(node.id);
                let _ = scene.set_video_time(node.id, duration);
            }
        }
    }
}

pub(crate) fn play(node: NodeId, path: &str, volume: f32, looping: bool) -> Result<Meta, String> {
    FILMS.with(|cell| cell.borrow_mut().play(node, path, volume, looping))
}

pub(crate) fn stop(node: NodeId) {
    FILMS.with(|cell| cell.borrow_mut().stop(node));
}

pub(crate) fn set_looping(node: NodeId, looping: bool) {
    FILMS.with(|cell| cell.borrow_mut().set_looping(node, looping));
}

pub(crate) fn sync(scene: &mut Scene, renderer: Option<&mut Renderer>, dt: f32) {
    FILMS.with(|cell| cell.borrow_mut().sync(scene, renderer, dt));
}

pub(crate) fn stop_all() {
    FILMS.with(|cell| cell.borrow_mut().stop_all());
}

pub(crate) fn video_error(why: String) -> pyo3::PyErr {
    pyo3::exceptions::PyValueError::new_err(why)
}

fn probe(path: &str) -> Result<Meta, String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,r_frame_rate",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1",
            path,
        ])
        .output()
        .map_err(|error| {
            format!("ffprobe is needed to read a video, and it did not run: {error}")
        })?;
    if !output.status.success() {
        return Err(format!(
            "ffprobe refused \"{path}\": {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut width = 0;
    let mut height = 0;
    let mut fps = 0.0;
    let mut duration = 0.0;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "width" => width = value.trim().parse().unwrap_or(0),
            "height" => height = value.trim().parse().unwrap_or(0),
            "r_frame_rate" => fps = fraction(value.trim()).unwrap_or(0.0),
            "duration" => duration = value.trim().parse().unwrap_or(0.0),
            _ => {}
        }
    }
    if width == 0 || height == 0 {
        return Err(format!("\"{path}\" has no video track"));
    }
    if fps <= 0.0 {
        fps = 30.0;
    }
    Ok(Meta {
        width,
        height,
        fps,
        duration,
    })
}

fn fraction(text: &str) -> Option<f32> {
    if let Some((top, bottom)) = text.split_once('/') {
        let top: f32 = top.parse().ok()?;
        let bottom: f32 = bottom.parse().ok()?;
        (bottom != 0.0).then_some(top / bottom)
    } else {
        text.parse().ok()
    }
}

fn spawn_frames(path: &str, meta: Meta) -> Result<(Receiver<Frame>, Arc<Mutex<Child>>), String> {
    let scale = format!("scale={}:{}", meta.width, meta.height);
    let mut child = Command::new("ffmpeg")
        .args(["-v", "error", "-i", path, "-an", "-sn", "-vf", &scale])
        .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            format!("ffmpeg is needed to decode a video, and it did not run: {error}")
        })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "the video decoder has no output".to_owned())?;
    let child = Arc::new(Mutex::new(child));
    let (sender, receiver): (SyncSender<Frame>, Receiver<Frame>) = std::sync::mpsc::sync_channel(8);

    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let frame_len = meta.width as usize * meta.height as usize * 4;
        let mut buffer = vec![0u8; frame_len];
        let mut index = 0u32;
        loop {
            if reader.read_exact(&mut buffer).is_err() {
                break;
            }
            let frame = Frame {
                pts: index as f32 / meta.fps.max(1.0),
                rgba: buffer.clone(),
            };
            if sender.send(frame).is_err() {
                break;
            }
            index += 1;
        }
    });

    Ok((receiver, child))
}
