use carbine_core::{NodeId, VecField};
use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::math::{PyQuat, PyVec3, axis_at, extract_axes, vec3_field};
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
    let axes = extract_axes(value)?;
    for (index, given) in axes.into_iter().enumerate() {
        let Some(value) = given else { continue };
        with_scene(|scene| scene.set_vec_field_axis(id, field, axis_at(index), value))?;
    }
    Ok(())
}

/// Reading the axes one at a time keeps an axis the object already has, so an
/// assignment can name only what it wants to change.
pub(crate) fn set_rotation(id: NodeId, value: &Bound<'_, PyAny>) -> PyResult<()> {
    if let Ok(rotation) = value.extract::<PyRef<'_, PyQuat>>() {
        return with_scene(|scene| scene.set_rotation(id, rotation.value()));
    }
    set_vec3(id, VecField::RotationEuler, value)
}

/// The image a node is skinned with, as a path. `None` paints it in its own
/// colour again.
pub(crate) fn texture_of(id: NodeId) -> PyResult<Option<String>> {
    with_scene(|scene| scene.texture(id))
}

pub(crate) fn set_texture_on(id: NodeId, value: Option<String>) -> PyResult<()> {
    with_scene(|scene| scene.set_texture(id, value))
}

/// Whether a node is drawn. Hiding a node hides everything under it.
pub(crate) fn visible_of(id: NodeId) -> PyResult<bool> {
    with_scene(|scene| scene.is_visible(id))
}

pub(crate) fn set_visible_on(id: NodeId, value: bool) -> PyResult<()> {
    with_scene(|scene| scene.set_visible(id, value))
}

/// How a node takes part in the physics step. An object and an entity wearing it
/// are the same node, so both handle classes show the same three knobs.
pub(crate) fn anchored_of(id: NodeId) -> PyResult<bool> {
    with_scene(|scene| scene.is_anchored(id))
}

pub(crate) fn set_anchored_on(id: NodeId, value: bool) -> PyResult<()> {
    with_scene(|scene| scene.set_anchored(id, value))
}

pub(crate) fn gravity_of(id: NodeId) -> PyResult<bool> {
    with_scene(|scene| scene.uses_gravity(id))
}

pub(crate) fn set_gravity_on(id: NodeId, value: bool) -> PyResult<()> {
    with_scene(|scene| scene.set_uses_gravity(id, value))
}

pub(crate) fn gravity_pull_of(id: NodeId) -> PyResult<f32> {
    with_scene(|scene| scene.gravity_pull(id))
}

pub(crate) fn set_gravity_pull_on(id: NodeId, value: f32) -> PyResult<()> {
    with_scene(|scene| scene.set_gravity_pull(id, value))
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

    /// The object's dimensions in meters at rotation identity, as
    /// `(length, width, height)`; `z` is the vertical size. Any axis may be
    /// `...` to leave it as it is.
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

    /// Accepts a `Quat`, or euler degrees as `(x, y, z)` where any axis may be
    /// `...` to keep the angle it already has.
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

    /// sRGB channels in 0..1.
    #[getter]
    fn color(&self) -> PyResult<PyVec3> {
        vec3_of(self.id, VecField::Color)
    }

    /// Accepts a `Vec3`, a three tuple, or a hex string such as `"#eb7a34"`. A
    /// trailing alpha in the hex, like `"#eb7a3480"`, only means anything on a UI
    /// element.
    #[setter]
    fn set_color(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        if let Ok(hex) = value.extract::<String>() {
            return with_scene(|scene| scene.set_color_hex(self.id, &hex));
        }
        set_vec3(self.id, VecField::Color, &value)
    }

    /// The colour as `#rrggbb`, the way a design tool writes it.
    #[getter(colorHex)]
    fn color_hex(&self) -> PyResult<String> {
        with_scene(|scene| scene.color_hex(self.id))
    }

    /// How round the corners are, in pixels. UI elements only.
    #[getter]
    fn corner(&self) -> PyResult<f32> {
        with_scene(|scene| scene.corner(self.id))
    }

    #[setter]
    fn set_corner(&self, corner: f32) -> PyResult<()> {
        with_scene(|scene| scene.set_corner(self.id, corner))
    }

    /// Polygon sides: 4 fills the box, 3 is a triangle, 64 reads as a circle.
    #[getter]
    fn polygon(&self) -> PyResult<u32> {
        with_scene(|scene| scene.polygon_sides(self.id))
    }

    #[setter]
    fn set_polygon(&self, sides: u32) -> PyResult<()> {
        with_scene(|scene| scene.set_polygon_sides(self.id, sides))
    }

    /// Outline thickness in pixels; 0 leaves the element filled solid.
    #[getter]
    fn outline(&self) -> PyResult<f32> {
        with_scene(|scene| scene.outline(self.id))
    }

    #[setter]
    fn set_outline(&self, thickness: f32) -> PyResult<()> {
        with_scene(|scene| scene.set_outline(self.id, thickness))
    }

    /// Opacity from 0 (invisible) to 255 (opaque).
    #[getter]
    fn transparent(&self) -> PyResult<f32> {
        with_scene(|scene| scene.alpha(self.id))
    }

    #[setter]
    fn set_transparent(&self, alpha: f32) -> PyResult<()> {
        with_scene(|scene| scene.set_alpha(self.id, alpha))
    }

    /// The image painted over the object's own colour, as a path. Assign a
    /// string to skin it, `None` to paint it flat again.
    #[getter]
    fn texture(&self) -> PyResult<Option<String>> {
        texture_of(self.id)
    }

    #[setter]
    fn set_texture(&self, value: Option<String>) -> PyResult<()> {
        set_texture_on(self.id, value)
    }

    /// Whether the object is drawn at all. False hides it and everything pinned
    /// under it, which is how a first person player wears a body it cannot see.
    #[getter]
    fn visible(&self) -> PyResult<bool> {
        visible_of(self.id)
    }

    #[setter]
    fn set_visible(&self, value: bool) -> PyResult<()> {
        set_visible_on(self.id, value)
    }

    /// Pinned: gravity and the solver leave the object exactly where it is.
    #[getter]
    fn anchored(&self) -> PyResult<bool> {
        anchored_of(self.id)
    }

    #[setter]
    fn set_anchored(&self, value: bool) -> PyResult<()> {
        set_anchored_on(self.id, value)
    }

    /// Whether `Scene::step` pulls this object down. False by default.
    #[getter]
    fn gravity(&self) -> PyResult<bool> {
        gravity_of(self.id)
    }

    #[setter]
    fn set_gravity(&self, value: bool) -> PyResult<()> {
        set_gravity_on(self.id, value)
    }

    /// Metres per second squared. 10 is Earth, 1.6 the Moon, 0 lets it hang.
    #[getter(gravityPull)]
    fn gravity_pull(&self) -> PyResult<f32> {
        gravity_pull_of(self.id)
    }

    #[setter(gravityPull)]
    fn set_gravity_pull(&self, value: f64) -> PyResult<()> {
        set_gravity_pull_on(self.id, value as f32)
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
