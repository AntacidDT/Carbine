use carbine_core::{NodeId, VecField};
use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::math::{PyQuat, PyVec3};
use crate::object::{
    Object, anchored_of, gravity_of, gravity_pull_of, handle_id, kind_of, name_of, set_anchored_on,
    set_gravity_on, set_gravity_pull_on, set_name_on, set_rotation, set_texture_on, set_vec3,
    set_visible_on, texture_of, vec3_of, visible_of,
};
use crate::with_scene;

/// An actor in the world: it has hit points, it can be killed, and in
/// multiplayer it is what gets replicated.
#[pyclass(module = "carbine", name = "Entity")]
#[derive(Clone)]
pub struct Entity {
    id: NodeId,
}

impl Entity {
    pub(crate) fn from_id(id: NodeId) -> Self {
        Self { id }
    }

    pub(crate) fn node_id(&self) -> NodeId {
        self.id
    }
}

#[pymethods]
impl Entity {
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
        kind_of(self.id)
    }

    /// The entity's body, or None for a logic-only entity.
    #[getter]
    fn object(&self) -> PyResult<Option<Object>> {
        let has_body = with_scene(|scene| scene.has_body(self.id))?;
        Ok(if has_body {
            Some(Object::from_id(self.id))
        } else {
            None
        })
    }

    /// Hit points remaining. Zero or below means the game should kill it.
    #[getter]
    fn health(&self) -> PyResult<f32> {
        with_scene(|scene| scene.health(self.id))
    }

    #[setter]
    fn set_health(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| scene.set_health(self.id, value as f32))
    }

    /// Hit points dealt on contact.
    #[getter]
    fn damage(&self) -> PyResult<f32> {
        with_scene(|scene| scene.damage(self.id))
    }

    #[setter]
    fn set_damage(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| scene.set_damage(self.id, value as f32))
    }

    #[getter]
    fn position(&self) -> PyResult<PyVec3> {
        vec3_of(self.id, VecField::Position)
    }

    #[setter]
    fn set_position(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_vec3(self.id, VecField::Position, &value)
    }

    #[getter]
    fn size(&self) -> PyResult<PyVec3> {
        vec3_of(self.id, VecField::Size)
    }

    #[setter]
    fn set_size(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_vec3(self.id, VecField::Size, &value)
    }

    #[getter]
    fn color(&self) -> PyResult<PyVec3> {
        vec3_of(self.id, VecField::Color)
    }

    #[setter]
    fn set_color(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_vec3(self.id, VecField::Color, &value)
    }

    /// The image painted over the entity's body, as a path.
    #[getter]
    fn texture(&self) -> PyResult<Option<String>> {
        texture_of(self.id)
    }

    #[setter]
    fn set_texture(&self, value: Option<String>) -> PyResult<()> {
        set_texture_on(self.id, value)
    }

    /// Whether the entity's body is drawn. The actor keeps existing either way.
    #[getter]
    fn visible(&self) -> PyResult<bool> {
        visible_of(self.id)
    }

    #[setter]
    fn set_visible(&self, value: bool) -> PyResult<()> {
        set_visible_on(self.id, value)
    }

    #[getter]
    fn rotation(&self) -> PyResult<PyQuat> {
        let rotation = with_scene(|scene| scene.rotation(self.id))?;
        Ok(PyQuat::from_value(rotation))
    }

    #[setter]
    fn set_rotation(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_rotation(self.id, &value)
    }

    /// Euler angles in degrees.
    #[getter(rotationEuler)]
    fn rotation_euler(&self) -> PyResult<PyVec3> {
        vec3_of(self.id, VecField::RotationEuler)
    }

    #[setter(rotationEuler)]
    fn set_rotation_euler(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_vec3(self.id, VecField::RotationEuler, &value)
    }

    #[getter]
    fn anchored(&self) -> PyResult<bool> {
        anchored_of(self.id)
    }

    #[setter]
    fn set_anchored(&self, value: bool) -> PyResult<()> {
        set_anchored_on(self.id, value)
    }

    #[getter]
    fn gravity(&self) -> PyResult<bool> {
        gravity_of(self.id)
    }

    #[setter]
    fn set_gravity(&self, value: bool) -> PyResult<()> {
        set_gravity_on(self.id, value)
    }

    #[getter(gravityPull)]
    fn gravity_pull(&self) -> PyResult<f32> {
        gravity_pull_of(self.id)
    }

    #[setter(gravityPull)]
    fn set_gravity_pull(&self, value: f64) -> PyResult<()> {
        set_gravity_pull_on(self.id, value as f32)
    }

    /// Removes the actor and leaves its body in the world.
    fn kill(&self) -> PyResult<()> {
        with_scene(|scene| scene.kill_entity(self.id))
    }

    /// Removes the actor and its body.
    fn destroy(&self) -> PyResult<()> {
        with_scene(|scene| scene.destroy(self.id))
    }

    fn __repr__(&self) -> String {
        match name_of(self.id) {
            Ok(name) => format!("<Entity \"{name}\" id={}>", self.id.0),
            Err(_) => format!("<Entity destroyed id={}>", self.id.0),
        }
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        handle_id(other) == Some(self.id)
    }

    fn __hash__(&self) -> u64 {
        self.id.0
    }
}
