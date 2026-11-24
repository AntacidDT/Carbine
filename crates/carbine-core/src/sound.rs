//! Sound as a node: a file, a volume, a loop flag and a playhead.
//!
//! The scene does not know what a speaker is. It keeps the *settings* of a sound the
//! way it keeps the skin of an object: a path, and the numbers a script wrote. The
//! platform audio layer reads those settings each frame, keeps one mixer voice per
//! node, and writes the playhead back, so a headless `step()` and a windowed
//! `render()` agree about when a sound has ended.

use bevy_ecs::prelude::*;

use crate::motion::Motion;
use crate::node::{
    Children, DEFAULT_COLOR, NodeId, NodeKind, NodeName, NodeTexture, ObjectColor, Opacity, Parent,
    Scene, SceneError, TextureTiling, Transform, Visible,
};

/// The kind word a sound node answers to.
pub const KIND: &str = "sound";

/// What a sound node holds. The mixer owns the decoded samples; this is the part a
/// script writes.
#[derive(Component, Clone)]
pub(crate) struct Sound {
    pub(crate) source: Option<String>,
    pub(crate) volume: f32,
    pub(crate) looping: bool,
    pub(crate) playing: bool,
    /// Seconds into the clip as of the last step. The audio layer keeps it current.
    pub(crate) time: f32,
}

impl Default for Sound {
    fn default() -> Self {
        Self {
            source: None,
            volume: 1.0,
            looping: false,
            playing: false,
            time: 0.0,
        }
    }
}

/// One sound node as the audio layer reads it. Collected before a step so the mixer
/// can be updated without holding a borrow on the scene.
#[derive(Clone, Debug)]
pub struct SoundNode {
    pub id: NodeId,
    pub source: Option<String>,
    pub volume: f32,
    pub looping: bool,
    pub playing: bool,
    pub time: f32,
}

impl Scene {
    /// Writes a sound node into the world. It is a node like a light: it can hang on a
    /// moving object, but it draws nothing and the solver does not push it.
    pub fn create_sound(
        &mut self,
        name: Option<String>,
        parent: Option<NodeId>,
    ) -> Result<NodeId, SceneError> {
        let parent_id = parent.unwrap_or(self.root());
        self.entity(parent_id)?;
        self.branch_allows(parent_id, false)?;

        let id = self.reserve();
        let entity = self
            .world
            .spawn((
                NodeName(name.unwrap_or_else(|| "Sound".to_owned())),
                NodeKind(KIND.to_owned()),
                Transform::default(),
                ObjectColor(DEFAULT_COLOR),
                Opacity::default(),
                NodeTexture::default(),
                TextureTiling::default(),
                Visible::default(),
                Motion::default(),
                Parent(parent_id),
                Children::default(),
                Sound::default(),
            ))
            .id();
        self.insert(id, entity);
        self.attach(id, parent_id);

        Ok(id)
    }

    pub fn is_sound(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.component::<Sound>(id).is_ok())
    }

    fn sound(&self, id: NodeId) -> Result<&Sound, SceneError> {
        self.component::<Sound>(id)
    }

    fn sound_mut(&mut self, id: NodeId) -> Result<Mut<'_, Sound>, SceneError> {
        self.component_mut::<Sound>(id)
    }

    /// The audio file this node plays, as a path, or `None` when it names none.
    pub fn sound_source(&self, id: NodeId) -> Result<Option<String>, SceneError> {
        Ok(self.sound(id)?.source.clone())
    }

    /// Points the node at a `.wav` or `.mp3`. Writing a source stops whatever the
    /// node was playing, so `play()` is always the call that starts it.
    pub fn set_sound_source(
        &mut self,
        id: NodeId,
        source: Option<String>,
    ) -> Result<(), SceneError> {
        if let Some(path) = &source {
            audio_file(path).map_err(|why| SceneError::BadSound {
                path: path.clone(),
                why,
            })?;
        }
        self.spread(id, move |scene, member| {
            let mut sound = scene.sound_mut(member)?;
            sound.source = source.clone();
            sound.playing = false;
            sound.time = 0.0;
            Ok(())
        })
    }

    /// How loud the node is, from 0 to 1. A number outside that is held at the edge;
    /// a number that is not a number is silence.
    pub fn sound_volume(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.sound(id)?.volume)
    }

    pub fn set_sound_volume(&mut self, id: NodeId, volume: f32) -> Result<(), SceneError> {
        let volume = sane_volume(volume);
        self.spread(id, move |scene, member| {
            scene.sound_mut(member)?.volume = volume;
            Ok(())
        })
    }

    /// Whether the clip starts again when it reaches the end.
    pub fn sound_looping(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.sound(id)?.looping)
    }

    pub fn set_sound_looping(&mut self, id: NodeId, looping: bool) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.sound_mut(member)?.looping = looping;
            Ok(())
        })
    }

    /// Whether a script has asked this node to play. The mixer may finish a
    /// non-looping clip and turn this back off; that write comes from the audio
    /// layer, not from a script.
    pub fn sound_playing(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.sound(id)?.playing)
    }

    /// Starts or stops the node as a setting. `play()` on the Python handle is what
    /// also loads the file and raises if it cannot.
    pub fn set_sound_playing(&mut self, id: NodeId, playing: bool) -> Result<(), SceneError> {
        self.sound_mut(id)?.playing = playing;
        Ok(())
    }

    /// The playhead in seconds, as of the last step.
    pub fn sound_time(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.sound(id)?.time)
    }

    /// Writes the playhead back after the mixer moved it. A script may also seek.
    pub fn set_sound_time(&mut self, id: NodeId, seconds: f32) -> Result<(), SceneError> {
        let seconds = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        self.sound_mut(id)?.time = seconds;
        Ok(())
    }

    /// Every sound node, ordered by id, for the audio layer to reconcile with its
    /// mixer once a step.
    pub fn sound_nodes(&self) -> Vec<SoundNode> {
        let mut nodes: Vec<SoundNode> = self
            .ids
            .iter()
            .filter_map(|(id, entity)| {
                self.world.get::<Sound>(*entity).map(|sound| SoundNode {
                    id: *id,
                    source: sound.source.clone(),
                    volume: sound.volume,
                    looping: sound.looping,
                    playing: sound.playing,
                    time: sound.time,
                })
            })
            .collect();
        nodes.sort_by_key(|node| node.id.0);
        nodes
    }
}

fn sane_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Whether a path is one of the two audio files the runtime reads. The file has to
/// be there, but it is not decoded here: decoding is the audio layer's job, and a
/// script that names a broken `.wav` hears about it when it calls `play()`.
fn audio_file(path: &str) -> Result<(), String> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension != "wav" && extension != "mp3" {
        return Err("only .wav and .mp3 are audio files".to_owned());
    }
    if !std::path::Path::new(path).is_file() {
        return Err("there is no file at that path".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn audio(name: &str) -> String {
        let path =
            std::env::temp_dir().join(format!("carbine-sound-{}-{name}.wav", std::process::id()));
        std::fs::write(&path, b"RIFF").unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_sound_is_a_node_with_sensible_defaults() {
        let mut scene = Scene::new();
        let id = scene.create_sound(Some("music".to_owned()), None).unwrap();

        assert!(scene.is_sound(id).unwrap());
        assert_eq!(scene.kind(id).unwrap(), KIND);
        assert_eq!(scene.name(id).unwrap(), "music");
        assert_eq!(scene.sound_source(id).unwrap(), None);
        assert_eq!(scene.sound_volume(id).unwrap(), 1.0);
        assert!(!scene.sound_looping(id).unwrap());
        assert!(!scene.sound_playing(id).unwrap());
        assert_eq!(scene.sound_time(id).unwrap(), 0.0);
    }

    #[test]
    fn a_source_must_be_a_wav_or_mp3_that_exists() {
        let mut scene = Scene::new();
        let id = scene.create_sound(None, None).unwrap();
        let source = audio("good");

        scene.set_sound_source(id, Some(source.clone())).unwrap();
        assert_eq!(scene.sound_source(id).unwrap(), Some(source));

        let refused = scene.set_sound_source(id, Some("music.ogg".to_owned()));
        assert!(matches!(refused, Err(SceneError::BadSound { .. })));

        let missing = scene.set_sound_source(id, Some("/no/such/file.wav".to_owned()));
        assert!(matches!(missing, Err(SceneError::BadSound { .. })));

        scene.set_sound_source(id, None).unwrap();
        assert_eq!(scene.sound_source(id).unwrap(), None);
    }

    #[test]
    fn volume_is_held_between_silence_and_full() {
        let mut scene = Scene::new();
        let id = scene.create_sound(None, None).unwrap();

        scene.set_sound_volume(id, 4.0).unwrap();
        assert_eq!(scene.sound_volume(id).unwrap(), 1.0);
        scene.set_sound_volume(id, -3.0).unwrap();
        assert_eq!(scene.sound_volume(id).unwrap(), 0.0);
        scene.set_sound_volume(id, f32::NAN).unwrap();
        assert_eq!(scene.sound_volume(id).unwrap(), 0.0);
    }

    #[test]
    fn changing_the_source_stops_the_playhead() {
        let mut scene = Scene::new();
        let id = scene.create_sound(None, None).unwrap();
        let source = audio("restart");

        scene.set_sound_source(id, Some(source.clone())).unwrap();
        scene.set_sound_playing(id, true).unwrap();
        scene.set_sound_time(id, 12.0).unwrap();
        scene.set_sound_source(id, Some(source)).unwrap();

        assert!(!scene.sound_playing(id).unwrap());
        assert_eq!(scene.sound_time(id).unwrap(), 0.0);
    }

    #[test]
    fn a_copy_carries_the_sound_and_a_linked_one_shares_it() {
        let mut scene = Scene::new();
        let id = scene.create_sound(None, None).unwrap();
        let source = audio("copy");
        scene.set_sound_source(id, Some(source.clone())).unwrap();
        scene.set_sound_volume(id, 0.25).unwrap();
        scene.set_sound_looping(id, true).unwrap();

        let copy = scene.duplicate(id, 1).unwrap()[0];
        assert_eq!(scene.sound_source(copy).unwrap(), Some(source.clone()));
        assert_eq!(scene.sound_volume(copy).unwrap(), 0.25);
        assert!(scene.sound_looping(copy).unwrap());

        let twin = scene.link_duplicate(id, 1).unwrap()[0];
        scene.set_sound_volume(id, 0.75).unwrap();
        assert_eq!(scene.sound_volume(twin).unwrap(), 0.75);
        scene.set_sound_playing(id, true).unwrap();
        assert!(!scene.sound_playing(twin).unwrap());
    }

    #[test]
    fn the_audio_layer_sees_every_sound_in_order() {
        let mut scene = Scene::new();
        let first = scene.create_sound(Some("first".to_owned()), None).unwrap();
        let second = scene.create_sound(Some("second".to_owned()), None).unwrap();
        scene.set_sound_playing(second, true).unwrap();

        let nodes = scene.sound_nodes();

        assert_eq!(nodes.len(), 2);
        assert_eq!((nodes[0].id, nodes[1].id), (first, second));
        assert!(nodes[1].playing);
    }
}
