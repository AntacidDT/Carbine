use carbine_core::{NodeId, VecField};
use pyo3::IntoPyObjectExt;
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::math::{PyQuat, PyVec3, axis_at, extract_axes, vec3_field};
use crate::mesh::PyMesh;
use crate::node_id_of;
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

/// The colour painted on a node, from a `Vec3`, a three tuple, or a hex string
/// such as `"#eb7a34"`. Both handle classes wear colours the same way.
pub(crate) fn set_color_on(id: NodeId, value: &Bound<'_, PyAny>) -> PyResult<()> {
    if let Ok(hex) = value.extract::<String>() {
        return with_scene(|scene| scene.set_color_hex(id, &hex));
    }
    set_vec3(id, VecField::Color, value)
}

/// Whether the solver holds this body apart from the others. Nothing collides
/// until a script switches it on, and switching it off keeps the shape.
pub(crate) fn collision_of(id: NodeId) -> PyResult<bool> {
    with_scene(|scene| scene.has_collision(id))
}

pub(crate) fn set_collision_on(id: NodeId, value: bool) -> PyResult<()> {
    with_scene(|scene| scene.set_collision(id, value))
}

/// The collider cut from the body, as a name: box, sphere, capsule or mesh.
pub(crate) fn collider_shape_of(id: NodeId) -> PyResult<String> {
    with_scene(|scene| Ok(scene.collision_shape(id)?.name().to_owned()))
}

pub(crate) fn set_collider_shape_on(id: NodeId, name: &str) -> PyResult<()> {
    with_scene(|scene| scene.set_collision_shape_name(id, name))
}

/// Whoever this node rode into during the last step, handed back as the class it
/// actually is: an Entity for an actor, an Object for scenery.
pub(crate) fn touching_handles<'py>(
    py: Python<'py>,
    id: NodeId,
) -> PyResult<Vec<Bound<'py, PyAny>>> {
    let touched = with_scene(|scene| Ok(scene.touches(id)))?;
    handles_for(py, touched)
}

/// A node handed back as the class it actually is: a Particle when it flies a
/// crowd, an Entity when it carries hit points, an Object otherwise. All three
/// compare equal to each other, so a script can keep comparing handles against
/// handles.
pub(crate) fn handle_for<'py>(py: Python<'py>, node: NodeId) -> PyResult<Bound<'py, PyAny>> {
    if with_scene(|scene| scene.is_particle_system(node))? {
        crate::procedural::Particle::from_id(node).into_bound_py_any(py)
    } else if with_scene(|scene| scene.is_entity(node))? {
        crate::entity::Entity::from_id(node).into_bound_py_any(py)
    } else {
        Object::from_id(node).into_bound_py_any(py)
    }
}

pub(crate) fn handles_for<'py>(
    py: Python<'py>,
    nodes: Vec<NodeId>,
) -> PyResult<Vec<Bound<'py, PyAny>>> {
    nodes.into_iter().map(|node| handle_for(py, node)).collect()
}

/// The node behind any handle class: an object, the entity wearing it and a
/// particle system are all nodes, so they compare equal.
pub(crate) fn handle_id(value: &Bound<'_, PyAny>) -> Option<NodeId> {
    if let Ok(object) = value.extract::<PyRef<'_, Object>>() {
        return Some(object.node_id());
    }
    if let Ok(entity) = value.extract::<PyRef<'_, crate::entity::Entity>>() {
        return Some(entity.node_id());
    }
    value
        .extract::<PyRef<'_, crate::procedural::Particle>>()
        .ok()
        .map(|crowd| crowd.node_id())
}

/// The node this one hangs under, as the class it is. Every node in a scene has a
/// parent except the root itself, which answers `None`.
pub(crate) fn parent_of<'py>(py: Python<'py>, id: NodeId) -> PyResult<Option<Bound<'py, PyAny>>> {
    let parent = with_scene(|scene| scene.parent(id))?;
    match parent {
        Some(parent) => handle_for(py, parent).map(Some),
        None => Ok(None),
    }
}

/// `None` sends the node to the scene root, which is where a detached node stops
/// moving with the branch it came off.
pub(crate) fn set_parent_on(id: NodeId, value: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
    let parent = value.map(node_id_of).transpose()?;
    with_scene(|scene| scene.reparent(id, parent))
}

/// The nodes under this one, in the order they are walked - which is also the
/// order UI elements are painted in.
pub(crate) fn children_of<'py>(py: Python<'py>, id: NodeId) -> PyResult<Vec<Bound<'py, PyAny>>> {
    let kids = with_scene(|scene| scene.children(id))?;
    handles_for(py, kids)
}

/// Reads any one handle or a run of them, since a child list is written both
/// ways: `panel.children = [a, b]` and `panel.children = sphere`.
fn child_ids(value: &Bound<'_, PyAny>) -> PyResult<Vec<NodeId>> {
    if let Some(id) = handle_id(value) {
        return Ok(vec![id]);
    }
    let mut kids = Vec::new();
    let items = value
        .try_iter()
        .map_err(|_| PyTypeError::new_err(NODE_LIST))?;
    for item in items {
        let item = item.map_err(|_| PyTypeError::new_err(NODE_LIST))?;
        kids.push(node_id_of(&item)?);
    }
    Ok(kids)
}

const NODE_LIST: &str = "expected an Object, an Entity or a Particle, or a list of them";

/// Replaces the child list. Left-out children move to the scene root rather than
/// being destroyed, and nodes newly listed are adopted.
pub(crate) fn set_children_on(id: NodeId, value: &Bound<'_, PyAny>) -> PyResult<()> {
    let kids = child_ids(value)?;
    with_scene(|scene| scene.set_children(id, &kids))
}

pub(crate) fn add_child_on(id: NodeId, value: &Bound<'_, PyAny>) -> PyResult<()> {
    let child = node_id_of(value)?;
    with_scene(|scene| scene.add_child(id, child))
}

/// Takes a child off this node and hangs it on the scene root. It is removed from
/// the branch, not from the world: `destroy` is the call that means that.
pub(crate) fn remove_child_on(id: NodeId, value: &Bound<'_, PyAny>) -> PyResult<()> {
    let child = node_id_of(value)?;
    with_scene(|scene| scene.remove_child(id, child))
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

    /// The node this one hangs under, which moves it along with its own transform.
    /// Only the scene root answers None.
    #[getter]
    fn parent<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        parent_of(py, self.id)
    }

    #[setter]
    fn set_parent(&self, value: Option<Bound<'_, PyAny>>) -> PyResult<()> {
        set_parent_on(self.id, value.as_ref())
    }

    /// What hangs under this node, in the order it is walked and painted. Writing
    /// either a list or one node replaces the branch: the order given becomes the
    /// order walked, unlisted children move to the scene root, and a node named
    /// here but hanging elsewhere is adopted. Nothing is destroyed.
    #[getter]
    fn children<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyAny>>> {
        children_of(py, self.id)
    }

    #[setter]
    fn set_children(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_children_on(self.id, &value)
    }

    /// Hangs another node under this one, at the end of the children, so it starts
    /// riding along with this node's transform.
    #[pyo3(name = "addChild")]
    fn add_child(&self, child: Bound<'_, PyAny>) -> PyResult<()> {
        add_child_on(self.id, &child)
    }

    /// Takes a child off this node and hangs it on the scene root. It leaves the
    /// branch, not the world - `destroy` is the call that means that.
    #[pyo3(name = "removeChild")]
    fn remove_child(&self, child: Bound<'_, PyAny>) -> PyResult<()> {
        remove_child_on(self.id, &child)
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
        set_color_on(self.id, &value)
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

    /// In the solver, or out of it. False until a script says otherwise, and a
    /// body that never asked falls straight through everything.
    #[getter]
    fn collision(&self) -> PyResult<bool> {
        collision_of(self.id)
    }

    #[setter]
    fn set_collision(&self, value: bool) -> PyResult<()> {
        set_collision_on(self.id, value)
    }

    /// The collider cut from the body: "box", "sphere", "capsule" or "mesh".
    /// Writing a name switches collisions on; the shape it answers with before
    /// that is the one the body would get.
    #[getter(collisionShape)]
    fn collision_shape(&self) -> PyResult<String> {
        collider_shape_of(self.id)
    }

    #[setter(collisionShape)]
    fn set_collision_shape(&self, name: &str) -> PyResult<()> {
        set_collider_shape_on(self.id, name)
    }

    /// Whatever this body rode into during the last step, as the handle class it
    /// is. Empty until something has actually been touching it.
    fn touching<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyAny>>> {
        touching_handles(py, self.id)
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
