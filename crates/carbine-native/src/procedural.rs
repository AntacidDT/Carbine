//! `carbine.procedural` — the generators that make content instead of asking for it.
//!
//! The first of them is the particle generator, and it is not a weather switch: it
//! takes a shape, a colour or an image and turns it into a crowd of things flying
//! through the world. What each one is doing there - where it stands, how fast it
//! falls, what it splashes into - is the core's business, so an editor and a script
//! get the same weather.
//!
//! A system is a node, which is why `particles()` hands back a `Particle` and not a
//! register: it can hang on a chimney and fly from its top, or stand on its own in
//! the middle of the sky. Because it is a node, it is parented, moved, coloured and
//! destroyed the way any other object is, and because it has no body, the knobs it
//! adds are about its crowd rather than about a shape.

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::PyAny;

use carbine_core::{NodeId, SceneError, Seed, VecField};

use crate::math::{PyQuat, PyVec3, extract_vec3};
use crate::node_id_of;
use crate::object::{
    add_child_on, children_of, handle_id, name_of, parent_of, remove_child_on, set_children_on,
    set_color_on, set_name_on, set_parent_on, set_texture_on, set_vec3, set_visible_on, texture_of,
    vec3_of, visible_of,
};
use crate::with_scene;

/// The generator register, `carbine.procedural`.
#[pyclass(frozen, module = "carbine", name = "Procedural")]
pub struct PyProcedural;

impl PyProcedural {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[pymethods]
impl PyProcedural {
    /// Writes a particle system into the world and hands back its handle. Every
    /// knob is optional; one that is left out keeps the value the engine gives a
    /// fresh system, so `particles()` alone is a rain veil and
    /// `particles(shape="circle", color="#fff8f0", speed=1.2)` is snow.
    ///
    /// A streak is drawn long along the way it travels, a square keeps its edges and
    /// a circle is soft; `image` skins the quad each drop is cut from.
    ///
    /// The crowd is laid out once every knob has been written, so an `area`, a
    /// `lifetime` or a `count` named here names the veil the system starts with.
    #[pyo3(name = "particles")]
    #[pyo3(signature = (*, name=None, parent=None, shape=None, count=None, size=None,
                        speed=None, direction=None, area=None, lifetime=None, seed=None,
                        color=None, image=None, active=None, collide=None, splash=None,
                        recycle=None))]
    #[allow(clippy::too_many_arguments)]
    fn create_particles(
        &self,
        name: Option<String>,
        parent: Option<Bound<'_, PyAny>>,
        shape: Option<&str>,
        count: Option<usize>,
        size: Option<f64>,
        speed: Option<f64>,
        direction: Option<Bound<'_, PyAny>>,
        area: Option<Bound<'_, PyAny>>,
        lifetime: Option<f64>,
        seed: Option<Bound<'_, PyAny>>,
        color: Option<Bound<'_, PyAny>>,
        image: Option<String>,
        active: Option<bool>,
        collide: Option<bool>,
        splash: Option<usize>,
        recycle: Option<bool>,
    ) -> PyResult<Particle> {
        let parent = parent.as_ref().map(node_id_of).transpose()?;
        let direction = direction.as_ref().map(extract_vec3).transpose()?;
        let area = area.as_ref().map(extract_vec3).transpose()?;
        let seed = seed.as_ref().map(wanted_seed).transpose()?;

        let id = with_scene(|scene| scene.create_particles(name, parent))?;
        with_scene(|scene| {
            if let Some(shape) = shape {
                scene.set_particle_shape(id, shape)?;
            }
            if let Some(size) = size {
                scene.set_particle_size(id, size as f32)?;
            }
            if let Some(speed) = speed {
                scene.set_particle_speed(id, speed as f32)?;
            }
            if let Some(direction) = direction {
                scene.set_particle_direction(id, direction)?;
            }
            if let Some(area) = area {
                scene.set_particle_area(id, area)?;
            }
            if let Some(lifetime) = lifetime {
                scene.set_particle_lifetime(id, lifetime as f32)?;
            }
            if let Some(count) = count {
                scene.set_particle_count(id, count)?;
            }
            if let Some(splash) = splash {
                scene.set_particle_splash(id, splash)?;
            }
            if let Some(collide) = collide {
                scene.set_particles_collide(id, collide)?;
            }
            if let Some(recycle) = recycle {
                scene.set_particles_recycle(id, recycle)?;
            }
            if let Some(image) = image {
                scene.set_texture(id, Some(image))?;
            }
            // The dice comes after the settings that decide where a drop is born,
            // so the veil a script gets is the one its own seed names.
            if let Some(seed) = seed {
                scene.set_particle_seed(id, seed)?;
            }
            // The system was written flying, so its first crowd was laid with the
            // defaults it is about to be given. Laying it again is the only way the
            // crowd on screen matches the knobs the script named.
            scene.restart_particles(id)?;
            if let Some(active) = active {
                scene.set_particles_active(id, active)?;
            }
            Ok(())
        })?;

        // A colour is written the way any node's is, which may name one axis at a
        // time, so it goes through the object's own setter rather than the scene's.
        if let Some(color) = color.as_ref() {
            set_color_on(id, color)?;
        }

        Ok(Particle::from_id(id))
    }

    /// The shapes a `Particle.shape` accepts.
    #[pyo3(name = "shapes")]
    fn particle_shapes(&self) -> Vec<&'static str> {
        carbine_core::ParticleShape::NAMES.to_vec()
    }

    fn __repr__(&self) -> &'static str {
        "<Procedural generators>"
    }
}

/// A seed written the way a script writes it: a word, or a number. The number is
/// allowed to be as large as the dice the engine keeps, so a seed read off one
/// system can be written straight back into another.
fn wanted_seed(value: &Bound<'_, PyAny>) -> PyResult<Seed> {
    if let Ok(text) = value.extract::<String>() {
        return Ok(Seed::of(&text));
    }
    if let Ok(number) = value.extract::<i64>() {
        return Ok(Seed::of_number(number));
    }
    if let Ok(number) = value.extract::<u64>() {
        return Ok(Seed(number));
    }
    Err(PyTypeError::new_err(
        "a seed is a word or a whole number, up to 18446744073709551615",
    ))
}

/// One particle system: a node with a crowd in the air around it.
#[pyclass(module = "carbine", name = "Particle")]
#[derive(Clone, Copy)]
pub struct Particle {
    id: NodeId,
}

impl Particle {
    pub(crate) fn from_id(id: NodeId) -> Self {
        Self { id }
    }

    pub(crate) fn node_id(&self) -> NodeId {
        self.id
    }

    /// Reads the system as the scene holds it. One that has been destroyed answers
    /// `DestroyedError`, the same way a destroyed object does.
    fn read<T>(
        &self,
        get: impl FnOnce(&carbine_core::Scene) -> Result<T, SceneError>,
    ) -> PyResult<T> {
        with_scene(|scene| get(scene))
    }
}

#[pymethods]
impl Particle {
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

    /// Always `"particles"`, which is what tells this node from a body.
    #[getter]
    fn kind(&self) -> PyResult<String> {
        with_scene(|scene| Ok(scene.kind(self.id)?.to_owned()))
    }

    /// The node this one hangs under. A system on a chimney flies from the top of
    /// the chimney, and `None` hangs it on the scene root.
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

    /// Meters from the scene origin, and the middle of the box the crowd flies in.
    /// Moving a system leaves what is already in the air where it was, so call
    /// `restart()` to lay the whole veil at the new place.
    #[getter]
    fn position(&self) -> PyResult<PyVec3> {
        vec3_of(self.id, VecField::Position)
    }

    #[setter]
    fn set_position(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_vec3(self.id, VecField::Position, &value)
    }

    /// Turning the node turns the fall with it: a system laid on its side sprays
    /// sideways, and one pointed up a slope throws along it.
    #[getter]
    fn rotation(&self) -> PyResult<PyQuat> {
        let rotation = with_scene(|scene| scene.rotation(self.id))?;
        Ok(PyQuat::from_value(rotation))
    }

    #[setter]
    fn set_rotation(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        crate::object::set_rotation(self.id, &value)
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

    /// How the drops are cut: `"streak"`, `"square"` or `"circle"`.
    #[getter]
    fn shape(&self) -> PyResult<String> {
        Ok(self
            .read(|scene| scene.particle_shape(self.id))?
            .name()
            .to_owned())
    }

    #[setter]
    fn set_shape(&self, name: &str) -> PyResult<()> {
        with_scene(|scene| scene.set_particle_shape(self.id, name))
    }

    /// How many things the system keeps in the air. Growing it lays the new drops
    /// and leaves the ones already flying alone; shrinking it takes the extras out.
    #[getter]
    fn count(&self) -> PyResult<usize> {
        self.read(|scene| scene.particle_count(self.id))
    }

    #[setter]
    fn set_count(&self, count: usize) -> PyResult<()> {
        with_scene(|scene| scene.set_particle_count(self.id, count))
    }

    /// Meters across a drop. A streak is six of these long along its own way.
    #[getter]
    fn size(&self) -> PyResult<f32> {
        self.read(|scene| scene.particle_size(self.id))
    }

    #[setter]
    fn set_size(&self, size: f64) -> PyResult<()> {
        with_scene(|scene| scene.set_particle_size(self.id, size as f32))
    }

    /// Meters a second along the way the system points.
    #[getter]
    fn speed(&self) -> PyResult<f32> {
        self.read(|scene| scene.particle_speed(self.id))
    }

    #[setter]
    fn set_speed(&self, speed: f64) -> PyResult<()> {
        with_scene(|scene| scene.set_particle_speed(self.id, speed as f32))
    }

    /// The way the crowd travels, in world meters. Asking for no direction at all
    /// means straight down, which is what rain wants.
    #[getter]
    fn direction(&self) -> PyResult<PyVec3> {
        self.read(|scene| Ok(PyVec3::plain(scene.particle_direction(self.id)?)))
    }

    #[setter]
    fn set_direction(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        let direction = extract_vec3(&value)?;
        with_scene(|scene| scene.set_particle_direction(self.id, direction))
    }

    /// The box new drops are placed in, meters, centred on the node.
    #[getter]
    fn area(&self) -> PyResult<PyVec3> {
        self.read(|scene| Ok(PyVec3::plain(scene.particle_area(self.id)?)))
    }

    #[setter]
    fn set_area(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        let area = extract_vec3(&value)?;
        with_scene(|scene| scene.set_particle_area(self.id, area))
    }

    /// Seconds a drop lives for.
    #[getter]
    fn lifetime(&self) -> PyResult<f32> {
        self.read(|scene| scene.particle_lifetime(self.id))
    }

    #[setter]
    fn set_lifetime(&self, seconds: f64) -> PyResult<()> {
        with_scene(|scene| scene.set_particle_lifetime(self.id, seconds as f32))
    }

    /// The dice the crowd was laid out with. Writing a new one throws the veil away
    /// and lays the one that seed names, which is how the same weather comes back.
    /// A word is as good as a number.
    #[getter]
    fn seed(&self) -> PyResult<u64> {
        self.read(|scene| scene.particle_seed(self.id))
    }

    #[setter]
    fn set_seed(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        let seed = wanted_seed(&value)?;
        with_scene(|scene| scene.set_particle_seed(self.id, seed))
    }

    /// Whether the system is doing anything. Off leaves the node, its settings and
    /// its place in the tree and takes every drop out of the air.
    #[getter]
    fn active(&self) -> PyResult<bool> {
        self.read(|scene| scene.particles_are_active(self.id))
    }

    #[setter]
    fn set_active(&self, value: bool) -> PyResult<()> {
        with_scene(|scene| scene.set_particles_active(self.id, value))
    }

    /// Whether the drops are stopped by bodies. Off, they fall straight through the
    /// world; on, they land on the floor and on anything else with a collider.
    #[getter]
    fn collide(&self) -> PyResult<bool> {
        self.read(|scene| scene.particles_collide(self.id))
    }

    #[setter]
    fn set_collide(&self, value: bool) -> PyResult<()> {
        with_scene(|scene| scene.set_particles_collide(self.id, value))
    }

    /// How many droplets an impact throws back, 0 for none. Only means something
    /// while the system collides.
    #[getter]
    fn splash(&self) -> PyResult<usize> {
        self.read(|scene| scene.particle_splash(self.id))
    }

    #[setter]
    fn set_splash(&self, count: usize) -> PyResult<()> {
        with_scene(|scene| scene.set_particle_splash(self.id, count))
    }

    /// Whether a spent drop is born again at the top of the box. Off, the system
    /// runs out and the sky clears on its own.
    #[getter]
    fn recycle(&self) -> PyResult<bool> {
        self.read(|scene| scene.particles_recycle(self.id))
    }

    #[setter]
    fn set_recycle(&self, value: bool) -> PyResult<()> {
        with_scene(|scene| scene.set_particles_recycle(self.id, value))
    }

    /// Whether the crowd is hung on the sky. On, its box rides with the eyes
    /// instead of with the node, which is what makes a weather veil.
    #[getter(onSky)]
    fn on_sky(&self) -> PyResult<bool> {
        self.read(|scene| scene.particles_on_sky(self.id))
    }

    #[setter(onSky)]
    fn set_on_sky(&self, value: bool) -> PyResult<()> {
        with_scene(|scene| scene.set_particles_on_sky(self.id, value))
    }

    /// The colour the drops are painted, from a `Vec3`, a three tuple, or a hex
    /// string such as `"#eb7a34"`. Each drop keeps a random part of it, which is
    /// what stops a thousand drops reading as one flat sheet.
    #[getter]
    fn color(&self) -> PyResult<PyVec3> {
        vec3_of(self.id, VecField::Color)
    }

    #[setter]
    fn set_color(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        set_color_on(self.id, &value)
    }

    /// The colour as `#rrggbb`, the way a design tool writes it.
    #[getter(colorHex)]
    fn color_hex(&self) -> PyResult<String> {
        with_scene(|scene| scene.color_hex(self.id))
    }

    /// The image a drop is cut from, as a path. `None` paints them flat.
    #[getter]
    fn texture(&self) -> PyResult<Option<String>> {
        texture_of(self.id)
    }

    #[setter]
    fn set_texture(&self, value: Option<String>) -> PyResult<()> {
        set_texture_on(self.id, value)
    }

    /// Opacity from 0 (invisible) to 255 (opaque), shared by every drop.
    #[getter]
    fn transparent(&self) -> PyResult<f32> {
        with_scene(|scene| scene.alpha(self.id))
    }

    #[setter]
    fn set_transparent(&self, alpha: f32) -> PyResult<()> {
        with_scene(|scene| scene.set_alpha(self.id, alpha))
    }

    /// Whether the node is drawn at all. Hidden, its crowd goes with it.
    #[getter]
    fn visible(&self) -> PyResult<bool> {
        visible_of(self.id)
    }

    #[setter]
    fn set_visible(&self, value: bool) -> PyResult<()> {
        set_visible_on(self.id, value)
    }

    /// How many of them are in the air right now.
    #[getter(inAir)]
    fn in_air(&self) -> PyResult<usize> {
        self.read(|scene| scene.particles_in_air(self.id))
    }

    /// How many drops this system brought up short during the last step.
    #[getter]
    fn impacts(&self) -> PyResult<usize> {
        self.read(|scene| scene.particle_impacts(self.id))
    }

    /// Throws the crowd away and lays a new one at where the system hangs now, which
    /// is how a script moves a veil after moving, turning or re-hanging it.
    fn restart(&self) -> PyResult<()> {
        with_scene(|scene| scene.restart_particles(self.id))
    }

    /// Takes the system and its whole crowd out of the world.
    fn destroy(&self) -> PyResult<()> {
        with_scene(|scene| scene.destroy(self.id))
    }

    fn __repr__(&self) -> String {
        match with_scene(|scene| {
            Ok((
                scene.name(self.id)?.to_owned(),
                scene.particle_count(self.id)?,
                scene.particles_are_active(self.id)?,
            ))
        }) {
            Ok((name, count, active)) => format!(
                "<Particle \"{name}\" count={count} {} id={}>",
                if active { "active" } else { "idle" },
                self.id.0
            ),
            Err(_) => format!("<Particle destroyed id={}>", self.id.0),
        }
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        handle_id(other) == Some(self.id)
    }

    fn __hash__(&self) -> u64 {
        self.id.0
    }
}
