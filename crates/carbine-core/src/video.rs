//! Video as a node: a `.mp4` drawn on a rectangle, with its own sound track.
//!
//! The scene keeps the settings a script writes and the metadata the decoder
//! reports; the platform layer runs the film, uploads its frames and starts its
//! audio. A video is a node like a map or a sound, so it can be moved, turned,
//! parented and destroyed the way any other piece of the world can.

use bevy_ecs::prelude::*;

use crate::node::{NodeId, Scene, SceneError, VecField};

/// The kind word a video node answers to.
pub const KIND: &str = "video";

/// What a video node holds. The frames live in the platform layer; this is what a
/// script wrote and what the decoder reported.
#[derive(Component, Clone)]
pub(crate) struct Video {
    pub(crate) source: Option<String>,
    pub(crate) volume: f32,
    pub(crate) looping: bool,
    pub(crate) playing: bool,
    pub(crate) time: f32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) fps: f32,
    pub(crate) duration: f32,
}

impl Default for Video {
    fn default() -> Self {
        Self {
            source: None,
            volume: 1.0,
            looping: false,
            playing: false,
            time: 0.0,
            width: 0,
            height: 0,
            fps: 0.0,
            duration: 0.0,
        }
    }
}

/// One video node as the platform layer reads it.
#[derive(Clone, Debug)]
pub struct VideoNode {
    pub id: NodeId,
    pub source: Option<String>,
    pub volume: f32,
    pub looping: bool,
    pub playing: bool,
    pub time: f32,
}

impl Scene {
    /// Writes a video node into the world: a rectangle sixteen by nine metres
    /// until the film reports its own shape. It draws the frame the decoder last
    /// handed over, so it is part of the world rather than an overlay.
    pub fn create_video(
        &mut self,
        name: Option<String>,
        parent: Option<NodeId>,
    ) -> Result<NodeId, SceneError> {
        let data = crate::primitives::build("rectangle").ok_or(SceneError::MissingComponent)?;
        let id = self.spawn(KIND, name, parent, data);
        let entity = self.entity(id)?;
        self.world.entity_mut(entity).insert(Video::default());
        self.set_vec_field(id, VecField::Scale, crate::Vec3::new(16.0, 9.0, 1.0))?;
        // A film is a picture, not a painted shape: white keeps its colours the
        // colours the decoder wrote until a script tints it.
        self.set_vec_field(id, VecField::Color, crate::Vec3::ONE)?;
        Ok(id)
    }

    pub fn is_video(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.component::<Video>(id).is_ok())
    }

    fn video(&self, id: NodeId) -> Result<&Video, SceneError> {
        self.component::<Video>(id)
    }

    fn video_mut(&mut self, id: NodeId) -> Result<Mut<'_, Video>, SceneError> {
        self.component_mut::<Video>(id)
    }

    pub fn video_source(&self, id: NodeId) -> Result<Option<String>, SceneError> {
        Ok(self.video(id)?.source.clone())
    }

    /// Points the node at an `.mp4`. A new source stops the film, so `play()` is
    /// always the call that starts it.
    pub fn set_video_source(
        &mut self,
        id: NodeId,
        source: Option<String>,
    ) -> Result<(), SceneError> {
        if let Some(path) = &source {
            video_file(path).map_err(|why| SceneError::BadVideo {
                path: path.clone(),
                why,
            })?;
        }
        self.spread(id, move |scene, member| {
            let mut video = scene.video_mut(member)?;
            video.source = source.clone();
            video.playing = false;
            video.time = 0.0;
            video.width = 0;
            video.height = 0;
            video.fps = 0.0;
            video.duration = 0.0;
            Ok(())
        })
    }

    pub fn video_volume(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.video(id)?.volume)
    }

    pub fn set_video_volume(&mut self, id: NodeId, volume: f32) -> Result<(), SceneError> {
        let volume = sane_volume(volume);
        self.spread(id, move |scene, member| {
            scene.video_mut(member)?.volume = volume;
            Ok(())
        })
    }

    pub fn video_looping(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.video(id)?.looping)
    }

    pub fn set_video_looping(&mut self, id: NodeId, looping: bool) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.video_mut(member)?.looping = looping;
            Ok(())
        })
    }

    pub fn video_playing(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.video(id)?.playing)
    }

    pub fn set_video_playing(&mut self, id: NodeId, playing: bool) -> Result<(), SceneError> {
        self.video_mut(id)?.playing = playing;
        Ok(())
    }

    pub fn video_time(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.video(id)?.time)
    }

    pub fn set_video_time(&mut self, id: NodeId, seconds: f32) -> Result<(), SceneError> {
        let seconds = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        self.video_mut(id)?.time = seconds;
        Ok(())
    }

    pub fn video_width(&self, id: NodeId) -> Result<u32, SceneError> {
        Ok(self.video(id)?.width)
    }

    pub fn video_height(&self, id: NodeId) -> Result<u32, SceneError> {
        Ok(self.video(id)?.height)
    }

    pub fn video_fps(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.video(id)?.fps)
    }

    pub fn video_duration(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.video(id)?.duration)
    }

    /// Writes what the decoder found and shapes the rectangle to the film's own
    /// aspect ratio. One metre high, as wide as the film is.
    pub fn set_video_metadata(
        &mut self,
        id: NodeId,
        width: u32,
        height: u32,
        fps: f32,
        duration: f32,
    ) -> Result<(), SceneError> {
        {
            let mut video = self.video_mut(id)?;
            video.width = width;
            video.height = height;
            video.fps = if fps.is_finite() { fps.max(0.0) } else { 0.0 };
            video.duration = if duration.is_finite() {
                duration.max(0.0)
            } else {
                0.0
            };
        }
        // A world rectangle is shaped by the film; a UI panel keeps the size its
        // layout gave it, because there scale is measured in pixels.
        let entity = self.entity(id)?;
        if height > 0 && self.world.get::<crate::ui::Ui>(entity).is_none() {
            let aspect = width as f32 / height as f32;
            self.set_vec_field(id, VecField::Scale, crate::Vec3::new(aspect, 1.0, 1.0))?;
        }
        Ok(())
    }

    /// Every video node, ordered by id, for the platform layer.
    pub fn video_nodes(&self) -> Vec<VideoNode> {
        let mut nodes: Vec<VideoNode> = self
            .ids
            .iter()
            .filter_map(|(id, entity)| {
                self.world.get::<Video>(*entity).map(|video| VideoNode {
                    id: *id,
                    source: video.source.clone(),
                    volume: video.volume,
                    looping: video.looping,
                    playing: video.playing,
                    time: video.time,
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

fn video_file(path: &str) -> Result<(), String> {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension != "mp4" {
        return Err("only .mp4 is a video file".to_owned());
    }
    if !std::path::Path::new(path).is_file() {
        return Err("there is no file at that path".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn film(name: &str) -> String {
        let path =
            std::env::temp_dir().join(format!("carbine-video-{}-{name}.mp4", std::process::id()));
        std::fs::write(&path, b"not really a film").unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_video_is_a_rectangle_with_sensible_defaults() {
        let mut scene = Scene::new();
        let id = scene.create_video(Some("screen".to_owned()), None).unwrap();

        assert!(scene.is_video(id).unwrap());
        assert_eq!(scene.kind(id).unwrap(), KIND);
        assert_eq!(scene.name(id).unwrap(), "screen");
        assert_eq!(scene.video_source(id).unwrap(), None);
        assert_eq!(scene.video_volume(id).unwrap(), 1.0);
        assert!(!scene.video_looping(id).unwrap());
        assert!(!scene.video_playing(id).unwrap());
        assert_eq!(scene.video_time(id).unwrap(), 0.0);
        assert_eq!(scene.video_width(id).unwrap(), 0);
    }

    #[test]
    fn a_source_must_be_an_mp4_that_exists() {
        let mut scene = Scene::new();
        let id = scene.create_video(None, None).unwrap();
        let source = film("good");

        scene.set_video_source(id, Some(source.clone())).unwrap();
        assert_eq!(scene.video_source(id).unwrap(), Some(source));

        let refused = scene.set_video_source(id, Some("clip.mkv".to_owned()));
        assert!(matches!(refused, Err(SceneError::BadVideo { .. })));
        let missing = scene.set_video_source(id, Some("/no/such/clip.mp4".to_owned()));
        assert!(matches!(missing, Err(SceneError::BadVideo { .. })));

        scene.set_video_source(id, None).unwrap();
        assert_eq!(scene.video_source(id).unwrap(), None);
    }

    #[test]
    fn metadata_shapes_the_rectangle_to_the_film() {
        let mut scene = Scene::new();
        let id = scene.create_video(None, None).unwrap();
        scene
            .set_video_metadata(id, 1920, 1080, 30.0, 12.0)
            .unwrap();

        assert_eq!(scene.video_width(id).unwrap(), 1920);
        assert_eq!(scene.video_height(id).unwrap(), 1080);
        assert!((scene.video_fps(id).unwrap() - 30.0).abs() < 1e-6);
        assert!((scene.video_duration(id).unwrap() - 12.0).abs() < 1e-6);
        let scale = scene.vec_field(id, VecField::Scale).unwrap();
        assert!((scale.x - 16.0 / 9.0).abs() < 1e-6);
        assert!((scale.y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_ui_video_is_an_overlay_element_not_a_world_object() {
        let mut scene = Scene::new();
        let id = scene.create_ui("video", None, None).unwrap();

        assert!(scene.is_ui(id).unwrap());
        assert!(scene.is_video(id).unwrap());
        assert_eq!(scene.kind(id).unwrap(), KIND);
        assert!(scene.draw_items().is_empty());
        assert!(
            scene
                .ui_items()
                .iter()
                .any(|item| item.node == id && item.video == Some(id.0))
        );

        // A film does not resize a UI panel; its layout owns its pixel size.
        let before = scene.vec_field(id, VecField::Scale).unwrap();
        scene.set_video_metadata(id, 1920, 1080, 30.0, 2.0).unwrap();
        assert_eq!(scene.vec_field(id, VecField::Scale).unwrap(), before);
    }

    #[test]
    fn a_ui_image_keeps_its_packed_skin() {
        let mut scene = Scene::new();
        let id = scene.create_ui("image", None, None).unwrap();
        let path = crate::test_skin("ui-image");
        scene.set_texture(id, Some(path.clone())).unwrap();

        assert!(scene.is_ui(id).unwrap());
        assert_eq!(scene.kind(id).unwrap(), "image");
        let items = scene.ui_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].texture, Some(path.as_str()));
    }

    #[test]
    fn a_video_reaches_the_frame_as_its_own_texture() {
        let mut scene = Scene::new();
        let id = scene.create_video(None, None).unwrap();
        scene.set_video_metadata(id, 640, 360, 24.0, 4.0).unwrap();

        let items = scene.draw_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].video, Some(id.0));
        assert_eq!(items[0].texture, None);
    }
}
