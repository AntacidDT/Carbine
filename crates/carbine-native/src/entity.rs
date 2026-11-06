use carbine_core::{NodeId, VecField};
use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::math::{PyQuat, PyVec3};
use crate::object::{
    Object, add_child_on, anchored_of, children_of, collider_type_of, collision_of, find_under,
    gravity_of, gravity_pull_of, handle_id, kind_of, mass_of, model_of, name_of, parent_of,
    remove_child_on, seat_under, set_anchored_on, set_children_on, set_collider_type_on,
    set_collision_on, set_color_on, set_gravity_on, set_gravity_pull_on, set_mass_on, set_model_on,
    set_name_on, set_parent_on, set_rotation, set_texture_on, set_tiling_on, set_vec3,
    set_visible_on, sit_on, stand_on, texture_of, tiling_of, touching_handles, vec3_of, visible_of,
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

    /// The node this actor hangs under. An actor is a node in the same tree as any
    /// object, so it rides along with its parent's transform whether or not it was
    /// given a body.
    #[getter]
    fn parent<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        parent_of(py, self.id)
    }

    #[setter]
    fn set_parent(&self, value: Option<Bound<'_, PyAny>>) -> PyResult<()> {
        set_parent_on(self.id, value.as_ref())
    }

    /// The nodes hanging under this one, in the order they are walked. Actors come
    /// back as `Entity` handles and plain nodes as `Object` handles.
    #[getter]
    fn children<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyAny>>> {
        children_of(py, self.id)
    }

    #[setter]
    fn set_children(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_children_on(self.id, &value)
    }

    /// Hangs another node under this actor, at the end of its children.
    #[pyo3(name = "addChild")]
    fn add_child(&self, child: Bound<'_, PyAny>) -> PyResult<()> {
        add_child_on(self.id, &child)
    }

    /// Takes a child off this actor and hangs it on the scene root, leaving it in
    /// the world.
    #[pyo3(name = "removeChild")]
    fn remove_child(&self, child: Bound<'_, PyAny>) -> PyResult<()> {
        remove_child_on(self.id, &child)
    }

    /// Hit points remaining. At zero the engine has already dealt with the death,
    /// so a script only has to read it.
    #[getter]
    fn health(&self) -> PyResult<f32> {
        with_scene(|scene| scene.health(self.id))
    }

    #[setter]
    fn set_health(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| scene.set_health(self.id, value as f32))
    }

    /// Hit points this actor deals, on contact and through `hit`.
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

    /// Accepts a `Vec3`, a three tuple, or a hex string such as `"#eb7a34"`, the
    /// same way an object's colour does.
    #[setter]
    fn set_color(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_color_on(self.id, &value)
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

    /// How many metres of this actor's body one tile of its skin covers. `0` writes
    /// the whole picture across each face instead.
    #[getter(textureTiling)]
    fn texture_tiling(&self) -> PyResult<f32> {
        tiling_of(self.id)
    }

    #[setter(textureTiling)]
    fn set_texture_tiling(&self, value: f64) -> PyResult<()> {
        set_tiling_on(self.id, value as f32)
    }

    /// The model this actor's body wears, as the path it was asked for. Assigning a
    /// `.glb` hangs the file's nodes under the actor, so an imported rig is still a
    /// set of parts a script can reach.
    #[getter]
    fn model(&self) -> PyResult<Option<String>> {
        model_of(self.id)
    }

    #[setter]
    fn set_model(&self, value: Option<String>) -> PyResult<()> {
        set_model_on(self.id, value)
    }

    /// The first node under this actor carrying the name, searched down the branch.
    fn find<'py>(&self, py: Python<'py>, name: String) -> PyResult<Option<Bound<'py, PyAny>>> {
        find_under(py, self.id, &name)
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

    /// Kilograms, or what the actor's box weighs at two hundred to the cubic
    /// metre while nobody has said.
    #[getter]
    fn mass(&self) -> PyResult<f32> {
        mass_of(self.id)
    }

    #[setter]
    fn set_mass(&self, value: f64) -> PyResult<()> {
        set_mass_on(self.id, value as f32)
    }

    /// In the solver, or out of it. An actor that does not collide cannot be
    /// bitten by anything, since contact is how damage travels.
    #[getter]
    fn collision(&self) -> PyResult<bool> {
        collision_of(self.id)
    }

    #[setter]
    fn set_collision(&self, value: bool) -> PyResult<()> {
        set_collision_on(self.id, value)
    }

    /// How the solver reads this body: "box", "sphere" or "mesh". A mesh is the
    /// body's own surfaces, each one a solid of its own.
    /// Writing a name switches collisions on.
    #[getter(collisionType)]
    fn collision_type(&self) -> PyResult<String> {
        collider_type_of(self.id)
    }

    #[setter(collisionType)]
    fn set_collision_type(&self, name: &str) -> PyResult<()> {
        set_collider_type_on(self.id, name)
    }

    /// Whatever this actor rode into during the last step: an Entity for an
    /// actor, an Object for scenery.
    fn touching<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyAny>>> {
        touching_handles(py, self.id)
    }

    /// Whether there is any health left. A corpse answers false, takes no more
    /// hits, and cannot hurt anything.
    #[getter]
    fn alive(&self) -> PyResult<bool> {
        with_scene(|scene| scene.alive(self.id))
    }

    /// Seconds one hit holds the next one off, whether it came from contact or
    /// from `hit`. Half a second by default.
    #[getter(hitPause)]
    fn hit_pause(&self) -> PyResult<f32> {
        with_scene(|scene| scene.hit_pause(self.id))
    }

    #[setter(hitPause)]
    fn set_hit_pause(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| scene.set_hit_pause(self.id, value as f32))
    }

    /// Whether the body is left lying in the world when it dies, inert but still
    /// falling. False removes the node, and everything pinned under it.
    #[getter(stayAfterDeath)]
    fn stays_after_death(&self) -> PyResult<bool> {
        with_scene(|scene| scene.stays_after_death(self.id))
    }

    #[setter(stayAfterDeath)]
    fn set_stays_after_death(&self, value: bool) -> PyResult<()> {
        with_scene(|scene| scene.set_stays_after_death(self.id, value))
    }

    /// Which seat this actor is riding in, or None for one standing on its own feet.
    #[getter(seatedIn)]
    fn seated_in<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        seat_under(py, self.id)
    }

    /// Sits this actor in a seat, and from then the step holds it at the seat's place:
    /// an actor in a car neither falls nor shoves. Whoever was in that seat before gets
    /// out.
    fn sit(&self, seat: &Bound<'_, PyAny>) -> PyResult<()> {
        sit_on(self.id, seat)
    }

    /// Gets this actor out of whatever it was sitting in, where the seat had it and
    /// still carrying the seat's speed. A dying actor is stood out by the engine, with a
    /// shove, whether or not anybody asks.
    fn stand(&self) -> PyResult<()> {
        stand_on(self.id)
    }

    /// Bites another actor for this one's `damage` and answers with the hit
    /// points that actually landed. Zero means the target was already dead or
    /// still hurting from the last hit, or that this actor is a corpse with
    /// nothing left to give.
    fn hit(&self, other: &Bound<'_, PyAny>) -> PyResult<f32> {
        let target = crate::node_id_of(other)?;
        with_scene(|scene| scene.hit_entity(self.id, target))
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
