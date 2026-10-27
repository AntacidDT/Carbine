use bevy_ecs::prelude::*;
use glam::Vec3;

use crate::node::{NodeId, Scene, SceneError};

/// Earth gravity expressed the Carbine way: `gravityPull` is metres per second
/// squared, so 10 is "normal" and 1.6 would be the Moon.
pub const EARTH_GRAVITY_PULL: f32 = 10.0;

/// How a node relates to the physics step.
#[derive(Component)]
pub(crate) struct Motion {
    pub(crate) anchored: bool,
    pub(crate) gravity: bool,
    pub(crate) gravity_pull: f32,
    /// Meters per second, kept between steps. Only the solver writes it, so a
    /// node sitting still has no opinion about it.
    pub(crate) velocity: Vec3,
}

impl Default for Motion {
    fn default() -> Self {
        Self {
            anchored: false,
            gravity: false,
            gravity_pull: EARTH_GRAVITY_PULL,
            velocity: Vec3::ZERO,
        }
    }
}

impl Scene {
    pub fn is_anchored(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.component::<Motion>(id)?.anchored)
    }

    /// An anchored node is pinned: it cannot be moved by gravity or by a force.
    pub fn set_anchored(&mut self, id: NodeId, anchored: bool) -> Result<(), SceneError> {
        self.component_mut::<Motion>(id)?.anchored = anchored;
        Ok(())
    }

    pub fn uses_gravity(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.component::<Motion>(id)?.gravity)
    }

    pub fn set_uses_gravity(&mut self, id: NodeId, gravity: bool) -> Result<(), SceneError> {
        self.component_mut::<Motion>(id)?.gravity = gravity;
        Ok(())
    }

    pub fn gravity_pull(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.component::<Motion>(id)?.gravity_pull)
    }

    /// How hard gravity pulls this node. 10 is Earth, 0 lets it hang, and a
    /// negative value pulls the other way — up instead of down.
    pub fn set_gravity_pull(&mut self, id: NodeId, pull: f32) -> Result<(), SceneError> {
        self.component_mut::<Motion>(id)?.gravity_pull = pull;
        Ok(())
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

        assert!(scene.is_anchored(id).unwrap());
        assert_eq!(scene.gravity_pull(id).unwrap(), 3.0);
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
}
