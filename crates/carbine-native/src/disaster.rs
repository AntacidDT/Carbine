//! The weather: `carbine.physics.new("earthquake")`.
//!
//! A disaster is not an object. It has no place in the tree and nothing to draw,
//! because it is the rule the solver asks about every step rather than a body the
//! solver moves. So a handle is a set of knobs on that rule - how hard, how far,
//! where, and whether it is happening right now - and every read and write goes to
//! the scene, the same way the player controller does.

use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::math::{PyVec3, extract_vec3};
use crate::object::handle_for;
use crate::{node_id_of, with_scene};

/// The weather register, `carbine.physics`.
#[pyclass(frozen, module = "carbine", name = "Physics")]
pub struct PyPhysics;

impl PyPhysics {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[pymethods]
impl PyPhysics {
    /// Writes a disaster into the world, switched off, and hands back its handle.
    /// Nothing shakes until `start()`.
    #[pyo3(name = "new")]
    #[pyo3(signature = (kind, *, name = None))]
    fn create_disaster(&self, kind: &str, name: Option<String>) -> PyResult<Disaster> {
        let id = with_scene(|scene| scene.create_disaster(kind, name))?;
        Ok(Disaster::from_id(id))
    }

    /// The kinds `new()` accepts.
    #[pyo3(name = "kinds")]
    fn disaster_kinds(&self) -> Vec<&'static str> {
        carbine_core::DisasterKind::NAMES.to_vec()
    }

    /// The handles currently shaking the world, in the order they were written.
    fn running(&self) -> PyResult<Vec<Disaster>> {
        let ids = with_scene(|scene| Ok(scene.running_disasters()))?;
        Ok(ids.into_iter().map(Disaster::from_id).collect())
    }

    /// Switches everything off at once. What the weather already did - a crate
    /// shoved out of place - stays done.
    #[pyo3(name = "stopAll")]
    fn stop_all(&self) -> PyResult<()> {
        with_scene(|scene| {
            scene.stop_all_disasters();
            Ok(())
        })
    }

    fn __repr__(&self) -> &'static str {
        "<Physics weather>"
    }
}

/// One weather event: an acceleration the whole world is asked about, once a step.
#[pyclass(module = "carbine", name = "Disaster")]
#[derive(Clone, Copy)]
pub struct Disaster {
    id: u64,
}

impl Disaster {
    pub(crate) fn from_id(id: u64) -> Self {
        Self { id }
    }

    /// Reads the event as the scene holds it. A disaster that has been destroyed
    /// answers `ValueError`, the same way a destroyed node does.
    fn read<T>(&self, get: impl FnOnce(&carbine_core::Disaster) -> T) -> PyResult<T> {
        with_scene(|scene| scene.disaster(self.id).map(|event| get(&event)))
    }
}

#[pymethods]
impl Disaster {
    /// The number the scene and `carbine.physics.running()` use for this one.
    #[getter]
    fn id(&self) -> u64 {
        self.id
    }

    /// `"earthquake"`, `"wind"`, `"tornado"` or `"blast"` - which rule the world is
    /// asked by. It never changes after `new()`; write another one instead.
    #[getter]
    fn kind(&self) -> PyResult<String> {
        self.read(|event| event.kind().name().to_owned())
    }

    /// A label to tell two storms apart. Defaults to the kind.
    #[getter]
    fn name(&self) -> PyResult<String> {
        self.read(|event| event.name().to_owned())
    }

    #[setter]
    fn set_name(&self, value: String) -> PyResult<()> {
        with_scene(|scene| scene.set_disaster_name(self.id, value))
    }

    /// How hard it shoves, on the scale where every step up doubles it. 5 is the
    /// weather the world has every day, 7 is what the news reports, and past 10 the
    /// engine stops listening.
    #[getter]
    fn magnitude(&self) -> PyResult<f32> {
        self.read(|event| event.magnitude())
    }

    #[setter]
    fn set_magnitude(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| scene.set_disaster_magnitude(self.id, value as f32))
    }

    /// Seconds of it. Zero - which a wind and a funnel start with - keeps going
    /// until `stop()`; a blast comes ready-made with less than half a second.
    #[getter]
    fn duration(&self) -> PyResult<f32> {
        self.read(|event| event.duration())
    }

    #[setter]
    fn set_duration(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| scene.set_disaster_duration(self.id, value as f32))
    }

    /// Seconds of it the solver has already run, which is also the clock its
    /// shaking is read from. Reading the time of a weather event is fair; writing
    /// it would be strange, so there is nothing to write.
    #[getter]
    fn elapsed(&self) -> PyResult<f32> {
        self.read(|event| event.elapsed())
    }

    /// Where it is worst, in meters. Ignored while `node` is set, because the
    /// object it rides is the centre then.
    #[getter]
    fn epicentre(&self) -> PyResult<PyVec3> {
        self.read(|event| PyVec3::plain(event.epicentre()))
    }

    #[setter]
    fn set_epicentre(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        let value = extract_vec3(&value)?;
        with_scene(|scene| scene.set_disaster_epicentre(self.id, value))
    }

    /// Meters before it stops being felt. Zero, which is how every event starts,
    /// means the whole world.
    #[getter]
    fn reach(&self) -> PyResult<f32> {
        self.read(|event| event.reach())
    }

    #[setter]
    fn set_reach(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| scene.set_disaster_reach(self.id, value as f32))
    }

    /// Which way a wind blows, read back as given. Only `wind` asks; the others
    /// find their own direction from where they stand.
    #[getter]
    fn direction(&self) -> PyResult<PyVec3> {
        self.read(|event| PyVec3::plain(event.direction()))
    }

    #[setter]
    fn set_direction(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        let value = extract_vec3(&value)?;
        with_scene(|scene| scene.set_disaster_direction(self.id, value))
    }

    /// The object this one rides: its place in the world becomes the centre, so a
    /// funnel bolted to a fan follows the fan about. `None` puts the epicentre back.
    #[getter]
    fn node<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        let node = self.read(|event| event.node())?;
        match node {
            Some(node) => handle_for(py, node).map(Some),
            None => Ok(None),
        }
    }

    #[setter]
    fn set_node(&self, value: Option<Bound<'_, PyAny>>) -> PyResult<()> {
        let node = match value.as_ref() {
            Some(handle) => Some(node_id_of(handle)?),
            None => None,
        };
        with_scene(|scene| scene.set_disaster_node(self.id, node))
    }

    /// Whether the solver is asking it how hard to shove right now.
    #[getter]
    fn running(&self) -> PyResult<bool> {
        self.read(|event| event.is_running())
    }

    /// Turns it on. Starting one that is already running does nothing twice.
    fn start(&self) -> PyResult<()> {
        with_scene(|scene| scene.start_disaster(self.id))
    }

    /// Turns it off, leaving it written and startable again.
    fn stop(&self) -> PyResult<()> {
        with_scene(|scene| scene.stop_disaster(self.id))
    }

    /// Takes the rule out of the world. What it already shoved stays shoved: this
    /// removes the weather, not its mark on the scene.
    fn destroy(&self) -> PyResult<()> {
        with_scene(|scene| scene.remove_disaster(self.id))
    }

    /// How hard, and which way, this event shoves at one point in meters - from
    /// wherever it actually sits, and zero while it is switched off. A script may
    /// ask the weather a question; it is not allowed to answer for it.
    #[pyo3(name = "accelerationAt")]
    fn acceleration_at(&self, at: Bound<'_, PyAny>) -> PyResult<PyVec3> {
        let at = extract_vec3(&at)?;
        with_scene(|scene| scene.disaster_acceleration_at(self.id, at)).map(PyVec3::plain)
    }

    fn __repr__(&self) -> String {
        match self.read(|event| {
            (
                event.kind().name().to_owned(),
                event.name().to_owned(),
                event.magnitude(),
                event.is_running(),
            )
        }) {
            Ok((kind, name, magnitude, running)) => format!(
                "<Disaster \"{name}\" kind={kind} magnitude={magnitude} {} id={}>",
                if running { "running" } else { "stopped" },
                self.id
            ),
            Err(_) => format!("<Disaster destroyed id={}>", self.id),
        }
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, Disaster>>()
            .map(|event| event.id == self.id)
            .unwrap_or(false)
    }

    fn __hash__(&self) -> u64 {
        self.id
    }
}
