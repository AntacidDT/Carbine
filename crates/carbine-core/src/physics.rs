//! The solver: gravity, the floor, and the single step that runs them.
//!
//! A step is the only moment the engine moves anything on its own. Bodies pull
//! against `gravityPull` until their lowest point reaches the floor, and the
//! player controller walks through the same call.

use glam::Vec3;

use crate::motion::Motion;
use crate::node::{NodeId, Scene, VecField};
use crate::player::GROUND_HEIGHT;

/// The longest one step may be, in seconds. A window that was dragged, or a
/// debugger left open, cannot fling the world through the floor.
pub const MAX_STEP: f32 = 0.1;

impl Scene {
    /// Advances the world by `dt` seconds: the player first, because the camera
    /// follows it, then everything gravity owns.
    pub fn step(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, MAX_STEP);
        self.step_player(dt);
        self.place_body();

        for id in self.node_tree() {
            self.integrate(id, dt);
        }
    }

    /// Pulls one node down unless it is anchored or has no gravity. Nothing
    /// collides yet, so the only thing a falling body meets is the floor.
    fn integrate(&mut self, id: NodeId, dt: f32) {
        let Ok(motion) = self.component::<Motion>(id) else {
            return;
        };
        if motion.anchored || !motion.gravity {
            return;
        }
        let mut velocity = motion.velocity;
        let Ok(transform) = self.transform(id) else {
            return;
        };

        velocity.z -= motion.gravity_pull * dt;
        let mut position = transform.position + velocity * dt;

        let size = self.vec_field(id, VecField::Size).unwrap_or(Vec3::ONE);
        let bottom = self
            .mesh(id)
            .ok()
            .and_then(|mesh| mesh.bounds())
            .map(|(min, _)| min.z)
            .unwrap_or(0.0);
        let rest = position.z + bottom * size.z;

        if rest <= GROUND_HEIGHT {
            position.z += GROUND_HEIGHT - rest;
            if velocity.z < 0.0 {
                velocity.z = 0.0;
            }
        }

        if self
            .set_vec_field(id, VecField::Position, position)
            .is_err()
        {
            return;
        }
        let Ok(mut motion) = self.component_mut::<Motion>(id) else {
            return;
        };
        motion.velocity = velocity;
    }

    /// Every node below the root, parents before children.
    fn node_tree(&self) -> Vec<NodeId> {
        let mut ids = Vec::new();
        let mut stack = vec![self.root()];

        while let Some(id) = stack.pop() {
            if id != self.root() {
                ids.push(id);
            }
            if let Ok(children) = self.children(id) {
                stack.extend(children.iter().rev().copied());
            }
        }
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::EARTH_GRAVITY_PULL;

    /// A one metre cube, dropped from `height` with gravity switched on.
    fn dropped(pull: f32, height: f32) -> (Scene, NodeId) {
        let mut scene = Scene::new();
        let id = scene.create("cube", Some("Cube".into()), None).unwrap();
        scene
            .set_vec_field(id, VecField::Size, Vec3::new(1.0, 1.0, 1.0))
            .unwrap();
        scene
            .set_vec_field(id, VecField::Position, Vec3::new(0.0, 0.0, height))
            .unwrap();
        scene.set_uses_gravity(id, true).unwrap();
        scene.set_gravity_pull(id, pull).unwrap();
        (scene, id)
    }

    fn height(scene: &Scene, id: NodeId) -> f32 {
        scene.vec_field(id, VecField::Position).unwrap().z
    }

    #[test]
    fn gravity_takes_hold_only_once_the_step_runs() {
        let (mut scene, id) = dropped(EARTH_GRAVITY_PULL, 10.0);

        assert_eq!(height(&scene, id), 10.0);
        scene.step(0.1);
        assert!(height(&scene, id) < 10.0);
    }

    #[test]
    fn a_falling_cube_comes_to_rest_on_its_bottom_face() {
        let (mut scene, id) = dropped(EARTH_GRAVITY_PULL, 10.0);

        for _ in 0..300 {
            scene.step(1.0 / 60.0);
        }

        // A one metre cube on the floor has its centre a metre-half up.
        assert!((height(&scene, id) - 0.5).abs() < 1e-3);
    }

    #[test]
    fn the_moon_is_slower_than_the_earth() {
        let (mut earth, earth_id) = dropped(EARTH_GRAVITY_PULL, 10.0);
        let (mut moon, moon_id) = dropped(1.6, 10.0);

        for _ in 0..20 {
            earth.step(1.0 / 60.0);
            moon.step(1.0 / 60.0);
        }

        assert!(height(&earth, earth_id) < height(&moon, moon_id));
    }

    #[test]
    fn an_anchored_or_unpulled_body_stays_put() {
        let (mut scene, id) = dropped(EARTH_GRAVITY_PULL, 10.0);

        scene.set_anchored(id, true).unwrap();
        scene.step(1.0);
        assert_eq!(height(&scene, id), 10.0);

        scene.set_anchored(id, false).unwrap();
        scene.set_uses_gravity(id, false).unwrap();
        scene.step(1.0);
        assert_eq!(height(&scene, id), 10.0);

        scene.set_uses_gravity(id, true).unwrap();
        scene.set_gravity_pull(id, 0.0).unwrap();
        scene.step(1.0);
        assert_eq!(height(&scene, id), 10.0);
    }

    #[test]
    fn a_flat_plane_rests_exactly_where_it_lands() {
        let mut scene = Scene::new();
        let ground = scene.create("plane", Some("Ground".into()), None).unwrap();
        scene
            .set_vec_field(ground, VecField::Position, Vec3::new(0.0, 0.0, 4.0))
            .unwrap();
        scene.set_uses_gravity(ground, true).unwrap();

        for _ in 0..300 {
            scene.step(1.0 / 60.0);
        }
        assert!((height(&scene, ground) - GROUND_HEIGHT).abs() < 1e-3);
    }

    #[test]
    fn a_stalled_frame_cannot_throw_the_world_through_the_floor() {
        let (mut scene, id) = dropped(EARTH_GRAVITY_PULL, 10.0);

        scene.step(1000.0);
        let position = scene.vec_field(id, VecField::Position).unwrap();
        assert!(position.z.is_finite());
        assert!(position.z > 9.0, "one clamped step, not a teleport");
    }

    #[test]
    fn one_step_moves_the_player_and_the_world() {
        let (mut scene, id) = dropped(EARTH_GRAVITY_PULL, 10.0);
        scene.set_player_enabled(true);
        scene.set_player_position(Vec3::new(0.0, 0.0, 5.0));

        scene.step(1.0 / 60.0);
        assert!(scene.player_position().z < 5.0);
        assert!(height(&scene, id) < 10.0);
    }

    #[test]
    fn elements_of_the_overlay_are_left_alone() {
        let mut scene = Scene::new();
        let button = scene.create_ui("square", None, None).unwrap();

        scene
            .set_vec_field(button, VecField::Position, Vec3::new(40.0, 40.0, 0.0))
            .unwrap();
        scene.step(1.0);
        assert_eq!(
            scene.vec_field(button, VecField::Position).unwrap().z,
            0.0,
            "UI is measured in pixels and never falls"
        );
    }
}
