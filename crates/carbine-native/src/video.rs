//! `carbine.new("Video")` — one `.mp4` drawn on a rectangle with its own sound.

use pyo3::prelude::*;
use pyo3::types::PyAny;

use carbine_core::SceneError;

use crate::audio;
use crate::film;
use crate::math::{PyQuat, PyVec3};
use crate::object::{
    add_child_on, children_of, handle_id, name_of, parent_of, remove_child_on, set_children_on,
    set_name_on, set_parent_on, set_vec3, vec3_of,
};
use crate::with_scene;

/// One film: a rectangle that shows the frame the decoder last handed over, with the
/// film's own sound played by the mixer. It is a node, so it is moved, turned,
/// parented and destroyed like any other piece of the world.
#[pyclass(module = "carbine", name = "Video")]
#[derive(Clone, Copy)]
pub struct Video {
    id: carbine_core::NodeId,
}

impl Video {
    pub(crate) fn from_id(id: carbine_core::NodeId) -> Self {
        Self { id }
    }

    pub(crate) fn node_id(&self) -> carbine_core::NodeId {
        self.id
    }
}

#[pymethods]
impl Video {
    #[getter]
    fn id(&self) -> u64 {
        self.id.0
    }

    #[getter]
    fn name(&self) -> PyResult<String> {
        name_of(self.id)
    }

    #[setter]
    fn set_name(&self, value: String) -> PyResult<()> {
        set_name_on(self.id, value)
    }

    #[getter]
    fn kind(&self) -> PyResult<String> {
        with_scene(|scene| Ok(scene.kind(self.id)?.to_owned()))
    }

    #[getter]
    fn parent<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        parent_of(py, self.id)
    }

    #[setter]
    fn set_parent(&self, value: Option<Bound<'_, PyAny>>) -> PyResult<()> {
        set_parent_on(self.id, value.as_ref())
    }

    #[getter]
    fn children<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyAny>>> {
        children_of(py, self.id)
    }

    #[setter]
    fn set_children(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_children_on(self.id, &value)
    }

    #[pyo3(name = "addChild")]
    fn add_child(&self, child: Bound<'_, PyAny>) -> PyResult<()> {
        add_child_on(self.id, &child)
    }

    #[pyo3(name = "removeChild")]
    fn remove_child(&self, child: Bound<'_, PyAny>) -> PyResult<()> {
        remove_child_on(self.id, &child)
    }

    #[getter]
    fn position(&self) -> PyResult<PyVec3> {
        vec3_of(self.id, carbine_core::VecField::Position)
    }

    #[setter]
    fn set_position(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_vec3(self.id, carbine_core::VecField::Position, &value)
    }

    #[getter]
    fn rotation(&self) -> PyResult<PyQuat> {
        let rotation = with_scene(|scene| scene.rotation(self.id))?;
        Ok(PyQuat::from_value(rotation))
    }

    #[setter]
    fn set_rotation(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        crate::object::set_rotation(self.id, &value)
    }

    #[getter(rotationEuler)]
    fn rotation_euler(&self) -> PyResult<PyVec3> {
        vec3_of(self.id, carbine_core::VecField::RotationEuler)
    }

    #[setter(rotationEuler)]
    fn set_rotation_euler(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_vec3(self.id, carbine_core::VecField::RotationEuler, &value)
    }

    /// Local scale: metres on a world screen, pixels on the overlay.
    #[getter]
    fn scale(&self) -> PyResult<PyVec3> {
        vec3_of(self.id, carbine_core::VecField::Scale)
    }

    #[setter]
    fn set_scale(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_vec3(self.id, carbine_core::VecField::Scale, &value)
    }

    /// The `.mp4` this node plays. Writing a new source stops the film.
    #[getter]
    fn source(&self) -> PyResult<Option<String>> {
        with_scene(|scene| scene.video_source(self.id))
    }

    #[setter]
    fn set_source(&self, value: Option<String>) -> PyResult<()> {
        with_scene(|scene| scene.set_video_source(self.id, value))?;
        film::stop(self.id);
        Ok(())
    }

    #[getter]
    fn volume(&self) -> PyResult<f32> {
        with_scene(|scene| scene.video_volume(self.id))
    }

    #[setter]
    fn set_volume(&self, volume: f32) -> PyResult<()> {
        with_scene(|scene| scene.set_video_volume(self.id, volume))?;
        let volume = with_scene(|scene| scene.video_volume(self.id))?;
        audio::set_volume(self.id, volume);
        Ok(())
    }

    #[getter]
    fn looping(&self) -> PyResult<bool> {
        with_scene(|scene| scene.video_looping(self.id))
    }

    #[setter]
    fn set_looping(&self, looping: bool) -> PyResult<()> {
        with_scene(|scene| scene.set_video_looping(self.id, looping))?;
        film::set_looping(self.id, looping);
        audio::set_looping(self.id, looping);
        Ok(())
    }

    #[getter]
    fn playing(&self) -> PyResult<bool> {
        with_scene(|scene| scene.video_playing(self.id))
    }

    #[setter]
    fn set_playing(&self, playing: bool) -> PyResult<()> {
        if playing { self.play() } else { self.stop() }
    }

    /// The playhead in seconds, as of the last step or frame.
    #[getter]
    fn time(&self) -> PyResult<f32> {
        with_scene(|scene| scene.video_time(self.id))
    }

    /// How long the film runs, in seconds. Zero until it has been started once.
    #[getter]
    fn duration(&self) -> PyResult<f32> {
        with_scene(|scene| scene.video_duration(self.id))
    }

    /// The film's own pixel width, once it has been started.
    #[getter]
    fn width(&self) -> PyResult<u32> {
        with_scene(|scene| scene.video_width(self.id))
    }

    #[getter]
    fn height(&self) -> PyResult<u32> {
        with_scene(|scene| scene.video_height(self.id))
    }

    #[getter]
    fn fps(&self) -> PyResult<f32> {
        with_scene(|scene| scene.video_fps(self.id))
    }

    /// Probes the film, starts its picture and its sound. Raises `ValueError` when
    /// no source is set, the file is not there, or `ffmpeg`/`ffprobe` cannot read it.
    fn play(&self) -> PyResult<()> {
        let (path, volume, looping) = with_scene(|scene| {
            let path = scene
                .video_source(self.id)?
                .ok_or_else(|| SceneError::BadVideo {
                    path: "<none>".to_owned(),
                    why: "it names no source".to_owned(),
                })?;
            Ok((
                path,
                scene.video_volume(self.id)?,
                scene.video_looping(self.id)?,
            ))
        })?;

        let meta = film::play(self.id, &path, volume, looping).map_err(film::video_error)?;
        with_scene(|scene| {
            scene.set_video_metadata(self.id, meta.width, meta.height, meta.fps, meta.duration)?;
            scene.set_video_playing(self.id, true)?;
            scene.set_video_time(self.id, 0.0)?;
            Ok(())
        })
    }

    /// Stops the picture and the sound, and puts the playhead back at the start.
    fn stop(&self) -> PyResult<()> {
        film::stop(self.id);
        with_scene(|scene| {
            scene.set_video_playing(self.id, false)?;
            scene.set_video_time(self.id, 0.0)?;
            Ok(())
        })
    }

    fn destroy(&self) -> PyResult<()> {
        film::stop(self.id);
        with_scene(|scene| scene.destroy(self.id))
    }

    fn __repr__(&self) -> String {
        match with_scene(|scene| {
            Ok((
                scene.name(self.id)?.to_owned(),
                scene.video_source(self.id)?,
                scene.video_playing(self.id)?,
            ))
        }) {
            Ok((name, source, playing)) => format!(
                "<Video \"{name}\" source={} {} id={}>",
                source.unwrap_or_else(|| "None".to_owned()),
                if playing { "playing" } else { "stopped" },
                self.id.0
            ),
            Err(_) => format!("<Video destroyed id={}>", self.id.0),
        }
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        handle_id(other) == Some(self.id)
    }

    fn __hash__(&self) -> u64 {
        self.id.0
    }
}
