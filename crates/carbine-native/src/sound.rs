//! `carbine.new("Sound")` — one sound as a node.
//!
//! The handle only forwards settings to the scene and asks the audio layer to load,
//! start and stop a voice. The mixer does the arithmetic; the scene remembers what a
//! script wrote, so a sound keeps its volume and source across a step and a frame.

use pyo3::prelude::*;
use pyo3::types::PyAny;

use carbine_core::{NodeId, SceneError};

use crate::audio;
use crate::math::{PyQuat, PyVec3};
use crate::object::{
    add_child_on, children_of, handle_id, name_of, parent_of, remove_child_on, set_children_on,
    set_name_on, set_parent_on, set_vec3, vec3_of,
};
use crate::with_scene;

/// One sound: a node with a file, a volume, a loop flag and a playhead. It draws
/// nothing and the solver does not push it, but it hangs in the tree like any other
/// node, so a sound can ride on a moving object.
#[pyclass(module = "carbine", name = "Sound")]
#[derive(Clone, Copy)]
pub struct Sound {
    id: NodeId,
}

impl Sound {
    pub(crate) fn from_id(id: NodeId) -> Self {
        Self { id }
    }

    pub(crate) fn node_id(&self) -> NodeId {
        self.id
    }

    fn read<T>(
        &self,
        get: impl FnOnce(&carbine_core::Scene) -> Result<T, SceneError>,
    ) -> PyResult<T> {
        with_scene(|scene| get(scene))
    }
}

#[pymethods]
impl Sound {
    /// The number the scene and the other handles use for this node.
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

    /// Always `"sound"`, which is what tells this node from a body.
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

    /// Meters from the scene origin. The mixer does not pan yet, so this is the place
    /// a sound will be heard from once it does.
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

    /// The `.wav` or `.mp3` the node plays. Writing a new source stops the node, so
    /// `play()` is always the call that starts it.
    #[getter]
    fn source(&self) -> PyResult<Option<String>> {
        self.read(|scene| scene.sound_source(self.id))
    }

    #[setter]
    fn set_source(&self, value: Option<String>) -> PyResult<()> {
        with_scene(|scene| scene.set_sound_source(self.id, value))?;
        audio::stop(self.id);
        Ok(())
    }

    /// How loud the node is, from 0 to 1. A number outside that is held at the edge.
    #[getter]
    fn volume(&self) -> PyResult<f32> {
        self.read(|scene| scene.sound_volume(self.id))
    }

    #[setter]
    fn set_volume(&self, volume: f32) -> PyResult<()> {
        with_scene(|scene| scene.set_sound_volume(self.id, volume))?;
        let volume = with_scene(|scene| scene.sound_volume(self.id))?;
        audio::set_volume(self.id, volume);
        Ok(())
    }

    /// Whether the clip starts again when it reaches the end.
    #[getter]
    fn looping(&self) -> PyResult<bool> {
        self.read(|scene| scene.sound_looping(self.id))
    }

    #[setter]
    fn set_looping(&self, looping: bool) -> PyResult<()> {
        with_scene(|scene| scene.set_sound_looping(self.id, looping))?;
        audio::set_looping(self.id, looping);
        Ok(())
    }

    /// The same switch under its short name: `sound.loop = True` keeps it going,
    /// False makes one `play()` play the clip once.
    #[getter(r#loop)]
    fn loop_flag(&self) -> PyResult<bool> {
        self.read(|scene| scene.sound_looping(self.id))
    }

    #[setter(r#loop)]
    fn set_loop_flag(&self, looping: bool) -> PyResult<()> {
        self.set_looping(looping)
    }

    /// Whether the node has been asked to play. A non-looping clip that has finished
    /// reads `False`; a looping one keeps reading `True` until `stop()`.
    #[getter]
    fn playing(&self) -> PyResult<bool> {
        self.read(|scene| scene.sound_playing(self.id))
    }

    #[setter]
    fn set_playing(&self, playing: bool) -> PyResult<()> {
        if playing { self.play() } else { self.stop() }
    }

    /// Seconds into the clip as of the last step or frame. Writing it seeks a
    /// playing voice; a stopped sound starts from there on the next `play()`.
    #[getter]
    fn time(&self) -> PyResult<f32> {
        if let Some(position) = audio::position(self.id) {
            return Ok(position);
        }
        self.read(|scene| scene.sound_time(self.id))
    }

    #[setter]
    fn set_time(&self, seconds: f64) -> PyResult<()> {
        let seconds = if seconds.is_finite() {
            seconds.max(0.0) as f32
        } else {
            0.0
        };
        with_scene(|scene| scene.set_sound_time(self.id, seconds))?;
        audio::seek(self.id, seconds);
        Ok(())
    }

    /// Moves the playhead without stopping the voice.
    fn seek(&self, seconds: f64) -> PyResult<()> {
        self.set_time(seconds)
    }

    /// Whether this voice is placed in the world: panned around the listener and
    /// faded with distance.
    #[getter]
    fn spatial(&self) -> PyResult<bool> {
        self.read(|scene| scene.sound_spatial(self.id))
    }

    #[setter]
    fn set_spatial(&self, spatial: bool) -> PyResult<()> {
        with_scene(|scene| scene.set_sound_spatial(self.id, spatial))
    }

    /// How long the file runs, in seconds. The first ask loads and caches it.
    #[getter]
    fn duration(&self) -> PyResult<f32> {
        let source = self.read(|scene| scene.sound_source(self.id))?;
        match source {
            Some(path) => audio::duration(&path).map_err(audio::audio_error),
            None => Ok(0.0),
        }
    }

    /// Loads the source and starts it at the current playhead (the start, unless a
    /// script sought somewhere). Raises `ValueError` when no source is set, the file
    /// is not there, or it is not a readable `.wav`/`.mp3`.
    fn play(&self) -> PyResult<()> {
        let (path, volume, looping, time) = with_scene(|scene| {
            let path = scene
                .sound_source(self.id)?
                .ok_or_else(|| SceneError::BadSound {
                    path: "<none>".to_owned(),
                    why: "it names no source".to_owned(),
                })?;
            Ok((
                path,
                scene.sound_volume(self.id)?,
                scene.sound_looping(self.id)?,
                scene.sound_time(self.id)?,
            ))
        })?;

        audio::play(self.id, &path, volume, looping, time).map_err(audio::audio_error)?;
        with_scene(|scene| {
            scene.set_sound_playing(self.id, true)?;
            scene.set_sound_time(self.id, time)?;
            Ok(())
        })
    }

    /// Silences the node and puts its playhead back at the start.
    fn stop(&self) -> PyResult<()> {
        audio::stop(self.id);
        with_scene(|scene| {
            scene.set_sound_playing(self.id, false)?;
            scene.set_sound_time(self.id, 0.0)?;
            Ok(())
        })
    }

    /// Takes the sound and its voice out of the world.
    fn destroy(&self) -> PyResult<()> {
        audio::stop(self.id);
        with_scene(|scene| scene.destroy(self.id))
    }

    fn __repr__(&self) -> String {
        match with_scene(|scene| {
            Ok((
                scene.name(self.id)?.to_owned(),
                scene.sound_source(self.id)?,
                scene.sound_playing(self.id)?,
            ))
        }) {
            Ok((name, source, playing)) => format!(
                "<Sound \"{name}\" source={} {} id={}>",
                source.unwrap_or_else(|| "None".to_owned()),
                if playing { "playing" } else { "stopped" },
                self.id.0
            ),
            Err(_) => format!("<Sound destroyed id={}>", self.id.0),
        }
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        handle_id(other) == Some(self.id)
    }

    fn __hash__(&self) -> u64 {
        self.id.0
    }
}
