use bevy_ecs::prelude::*;
use glam::Vec3;

use crate::node::{NodeId, Scene, SceneError, VecField};

/// Earth gravity expressed the Carbine way: `gravityPull` is metres per second
/// squared, so 10 is "normal" and 1.6 would be the Moon.
pub const EARTH_GRAVITY_PULL: f32 = 10.0;

/// Kilograms per cubic metre a body is taken to weigh when nobody has said. A
/// metre cube is therefore 200 kg, and the weight a script has to beat to shove
/// one aside.
pub const MASS_PER_CUBIC_METRE: f32 = 200.0;

/// How a node relates to the physics step.
#[derive(Component)]
pub(crate) struct Motion {
    pub(crate) anchored: bool,
    pub(crate) gravity: bool,
    pub(crate) gravity_pull: f32,
    /// Kilograms, or `None` for whatever its box weighs. Only a script writes it;
    /// the solver reads it and never changes it.
    pub(crate) mass: Option<f32>,
    /// Meters per second, kept between steps. Only the solver writes it, so a
    /// node sitting still has no opinion about it.
    pub(crate) velocity: Vec3,
    /// Degrees per second about the upright axis, which the drive model writes on a
    /// vehicle each step and the step turns into a rotation. Nothing else spins a
    /// body on its own: a crate shoved by a quake slides, it does not pirouette.
    pub(crate) spin: f32,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            anchored: false,
            gravity: false,
            gravity_pull: EARTH_GRAVITY_PULL,
            mass: None,
            velocity: Vec3::ZERO,
            spin: 0.0,
        }
    }
}

impl Scene {
    pub fn is_anchored(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.component::<Motion>(id)?.anchored)
    }

    /// An anchored node is pinned: it cannot be moved by gravity or by a force.
    /// Whether a body is pinned is part of what it is, so it is written down the
    /// whole link group.
    pub fn set_anchored(&mut self, id: NodeId, anchored: bool) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.component_mut::<Motion>(member)?.anchored = anchored;
            Ok(())
        })
    }

    pub fn uses_gravity(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.component::<Motion>(id)?.gravity)
    }

    pub fn set_uses_gravity(&mut self, id: NodeId, gravity: bool) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.set_gravity_one(member, gravity)
        })
    }

    /// Gravity on one body alone. What the engine decides about a corpse is not
    /// something the rest of its group hears about.
    pub(crate) fn set_gravity_one(&mut self, id: NodeId, gravity: bool) -> Result<(), SceneError> {
        self.component_mut::<Motion>(id)?.gravity = gravity;
        Ok(())
    }

    pub fn gravity_pull(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.component::<Motion>(id)?.gravity_pull)
    }

    /// How hard gravity pulls this node. 10 is Earth, 0 lets it hang, and a
    /// negative value pulls the other way — up instead of down.
    pub fn set_gravity_pull(&mut self, id: NodeId, pull: f32) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.component_mut::<Motion>(member)?.gravity_pull = pull;
            Ok(())
        })
    }

    /// What this node weighs in kilograms: the mass written on it, or what its
    /// box weighs at `MASS_PER_CUBIC_METRE` while nobody has said. A node whose
    /// size cannot be read is taken for a metre cube.
    pub fn mass(&self, id: NodeId) -> Result<f32, SceneError> {
        self.component::<Motion>(id)?;
        Ok(self.mass_of(id))
    }

    /// How heavy this node is to the solver. A body that weighs nothing stays
    /// nothing rather than an infinity, so a figure the solver divides by is
    /// always a number.
    pub(crate) fn mass_of(&self, id: NodeId) -> f32 {
        let written = self.component::<Motion>(id).ok().and_then(|m| m.mass);
        written.unwrap_or_else(|| match self.vec_field(id, VecField::Size) {
            Ok(size) => (size.x * size.y * size.z).max(0.0) * MASS_PER_CUBIC_METRE,
            Err(_) => MASS_PER_CUBIC_METRE,
        })
    }

    /// How hard this node is to push somewhere. Weight does not change how fast
    /// gravity pulls it, only how much it gives way to what it bumps into.
    pub fn set_mass(&mut self, id: NodeId, kg: f32) -> Result<(), SceneError> {
        let kg = kg.max(0.0);
        self.spread(id, move |scene, member| {
            scene.component_mut::<Motion>(member)?.mass = Some(kg);
            Ok(())
        })
    }

    /// Whether the solver should move this node: an anchored node stays put no
    /// matter what else is set.
    pub fn is_dynamic(&self, id: NodeId) -> Result<bool, SceneError> {
        let motion = self.component::<Motion>(id)?;
        Ok(motion.gravity && !motion.anchored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::VecField;
    use glam::Vec3;

    #[test]
    fn a_new_object_is_free_but_unpulled() {
        let mut scene = Scene::new();
        let id = scene.create("rectangle", None, None).unwrap();

        assert!(!scene.is_anchored(id).unwrap());
        assert!(!scene.uses_gravity(id).unwrap());
        assert_eq!(scene.gravity_pull(id).unwrap(), EARTH_GRAVITY_PULL);
        assert!(!scene.is_dynamic(id).unwrap());
    }

    #[test]
    fn anchoring_beats_gravity() {
        let mut scene = Scene::new();
        let id = scene.create("rectangle", None, None).unwrap();

        scene.set_uses_gravity(id, true).unwrap();
        assert!(scene.is_dynamic(id).unwrap());

        scene.set_anchored(id, true).unwrap();
        assert!(!scene.is_dynamic(id).unwrap());
    }

    #[test]
    fn gravity_pull_is_stripped_of_clamping_in_both_directions() {
        let mut scene = Scene::new();
        let id = scene.create("rectangle", None, None).unwrap();

        scene.set_gravity_pull(id, 1.6).unwrap();
        assert_eq!(scene.gravity_pull(id).unwrap(), 1.6);

        scene.set_gravity_pull(id, 0.0).unwrap();
        assert_eq!(scene.gravity_pull(id).unwrap(), 0.0);

        scene.set_gravity_pull(id, -5.0).unwrap();
        assert_eq!(scene.gravity_pull(id).unwrap(), -5.0);

        scene.set_gravity_pull(id, 250.0).unwrap();
        assert_eq!(scene.gravity_pull(id).unwrap(), 250.0);
    }

    #[test]
    fn entities_share_the_same_motion_state_as_objects() {
        let mut scene = Scene::new();
        let id = scene
            .spawn_entity(Some("rectangle"), None, None, None, 40.0, 7.0)
            .unwrap();

        scene.set_anchored(id, true).unwrap();
        scene.set_gravity_pull(id, 3.0).unwrap();
        scene.set_mass(id, 5.0).unwrap();

        assert!(scene.is_anchored(id).unwrap());
        assert_eq!(scene.gravity_pull(id).unwrap(), 3.0);
        assert_eq!(scene.mass(id).unwrap(), 5.0);
        assert!(matches!(
            scene.is_anchored(scene.root()),
            Err(SceneError::MissingComponent)
        ));
    }

    #[test]
    fn a_destroyed_node_reports_its_state_as_gone() {
        let mut scene = Scene::new();
        let id = scene.create("rectangle", None, None).unwrap();
        scene
            .set_vec_field(id, VecField::Position, Vec3::X)
            .unwrap();
        scene.destroy(id).unwrap();

        assert!(matches!(
            scene.is_dynamic(id),
            Err(SceneError::NodeDestroyed)
        ));
    }

    #[test]
    fn a_body_weighs_the_box_it_was_made_from() {
        let mut scene = Scene::new();
        let crate_ = scene.create("cube", None, None).unwrap();

        // The cube primitive at its own size is a metre cube.
        assert_eq!(scene.mass(crate_).unwrap(), MASS_PER_CUBIC_METRE);

        scene
            .set_vec_field(crate_, VecField::Size, Vec3::new(2.0, 3.0, 1.0))
            .unwrap();
        assert_eq!(
            scene.mass(crate_).unwrap(),
            6.0 * MASS_PER_CUBIC_METRE,
            "six of them"
        );

        // The root is not a body and has no weight to report.
        assert!(matches!(
            scene.mass(scene.root()),
            Err(SceneError::MissingComponent)
        ));
    }

    #[test]
    fn a_written_mass_beats_the_box() {
        let mut scene = Scene::new();
        let id = scene.create("cube", None, None).unwrap();

        scene.set_mass(id, 42.0).unwrap();
        assert_eq!(scene.mass(id).unwrap(), 42.0);

        // Growing the box afterwards changes the drawing, not the weight.
        scene
            .set_vec_field(id, VecField::Size, Vec3::splat(9.0))
            .unwrap();
        assert_eq!(scene.mass(id).unwrap(), 42.0);

        scene.set_mass(id, -8.0).unwrap();
        assert_eq!(
            scene.mass(id).unwrap(),
            0.0,
            "nothing weighs less than nothing"
        );
    }

    #[test]
    fn mass_travels_with_the_copy_and_the_link_group() {
        let mut scene = Scene::new();
        let id = scene.create("cube", None, None).unwrap();
        scene.set_mass(id, 90.0).unwrap();

        let linked = scene.link_duplicate(id, 1).unwrap();
        assert_eq!(scene.mass(linked[0]).unwrap(), 90.0);

        scene.set_mass(linked[0], 12.0).unwrap();
        assert_eq!(scene.mass(id).unwrap(), 12.0, "a group hears about it");

        let copied = scene.duplicate(linked[0], 1).unwrap();
        assert_eq!(
            scene.mass(copied[0]).unwrap(),
            12.0,
            "and so does a copy on its own"
        );
    }
}
