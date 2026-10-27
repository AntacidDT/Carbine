use carbine_core::{NodeId, VecField};
use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::math::{PyQuat, PyVec3, extract_vec3, vec3_field};
use crate::mesh::PyMesh;
use crate::with_scene;

/// Shared accessor bodies for the classes that wrap a scene node.
pub(crate) fn name_of(id: NodeId) -> PyResult<String> {
    with_scene(|scene| scene.name(id).map(str::to_owned))
}

pub(crate) fn set_name_on(id: NodeId, value: String) -> PyResult<()> {
    with_scene(|scene| scene.set_name(id, value))
}

pub(crate) fn kind_of(id: NodeId) -> PyResult<String> {
    with_scene(|scene| scene.kind(id).map(str::to_owned))
}

pub(crate) fn vec3_of(id: NodeId, field: VecField) -> PyResult<PyVec3> {
    vec3_field(id, field)
}

pub(crate) fn set_vec3(id: NodeId, field: VecField, value: &Bound<'_, PyAny>) -> PyResult<()> {
    let value = extract_vec3(value)?;
    with_scene(|scene| scene.set_vec_field(id, field, value))
}

/// The node behind either handle class: an object and the entity wearing it are
/// the same node, so they compare equal.
pub(crate) fn handle_id(value: &Bound<'_, PyAny>) -> Option<NodeId> {
    if let Ok(object) = value.extract::<PyRef<'_, Object>>() {
        return Some(object.node_id());
    }
    value
        .extract::<PyRef<'_, crate::entity::Entity>>()
        .ok()
        .map(|entity| entity.node_id())
}

/// The authored world: geometry, transform, colour. Objects are not
/// replicated and are not damageable; entities are.
#[pyclass(module = "carbine", name = "Object")]
#[derive(Clone)]
pub struct Object {
    id: NodeId,
}

impl Object {
    pub(crate) fn from_id(id: NodeId) -> Self {
        Self { id }
    }

    pub(crate) fn node_id(&self) -> NodeId {
        self.id
    }
}

#[pymethods]
impl Object {
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

    #[getter]
    fn parent(&self) -> PyResult<Option<Object>> {
        let parent = with_scene(|scene| scene.parent(self.id))?;
        Ok(parent.map(Object::from_id))
    }

    #[setter]
    fn set_parent(&self, value: Option<Bound<'_, PyAny>>) -> PyResult<()> {
        let parent = match value.as_ref() {
            Some(handle) => Some(crate::node_id_of(handle)?),
            None => None,
        };
        with_scene(|scene| scene.reparent(self.id, parent))
    }

    #[getter]
    fn children(&self) -> PyResult<Vec<Object>> {
        let children = with_scene(|scene| scene.children(self.id))?;
        Ok(children.into_iter().map(Object::from_id).collect())
    }

    /// Meters from the scene origin.
    #[getter]
    fn position(&self) -> PyResult<PyVec3> {
        vec3_of(self.id, VecField::Position)
    }

    #[setter]
    fn set_position(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_vec3(self.id, VecField::Position, &value)
    }

    /// The object's dimensions in meters at rotation identity.
    #[getter]
    fn size(&self) -> PyResult<PyVec3> {
        vec3_of(self.id, VecField::Size)
    }

    #[setter]
    fn set_size(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_vec3(self.id, VecField::Size, &value)
    }

    #[getter]
    fn scale(&self) -> PyResult<PyVec3> {
        vec3_of(self.id, VecField::Scale)
    }

    #[setter]
    fn set_scale(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_vec3(self.id, VecField::Scale, &value)
    }

    #[getter]
    fn rotation(&self) -> PyResult<PyQuat> {
        let rotation = with_scene(|scene| scene.rotation(self.id))?;
        Ok(PyQuat::from_value(rotation))
    }

    #[setter]
    fn set_rotation(&self, value: PyRef<'_, PyQuat>) -> PyResult<()> {
        with_scene(|scene| scene.set_rotation(self.id, value.value()))
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

    /// sRGB channels in 0..1.
    #[getter]
    fn color(&self) -> PyResult<PyVec3> {
        vec3_of(self.id, VecField::Color)
    }

    #[setter]
    fn set_color(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_vec3(self.id, VecField::Color, &value)
    }

    #[getter]
    fn mesh(&self) -> PyResult<PyMesh> {
        let data = with_scene(|scene| scene.mesh(self.id))?;
        Ok(PyMesh::from_data(data))
    }

    fn destroy(&self) -> PyResult<()> {
        with_scene(|scene| scene.destroy(self.id))
    }

    fn __repr__(&self) -> String {
        match with_scene(|scene| {
            Ok((
                scene.name(self.id)?.to_owned(),
                scene.kind(self.id)?.to_owned(),
            ))
        }) {
            Ok((name, kind)) => format!("<Object \"{name}\" kind=\"{kind}\" id={}>", self.id.0),
            Err(_) => format!("<Object destroyed id={}>", self.id.0),
        }
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        handle_id(other) == Some(self.id)
    }

    fn __hash__(&self) -> u64 {
        self.id.0
    }
}
