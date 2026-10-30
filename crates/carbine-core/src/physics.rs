//! The solver: gravity, the floor, bodies that hold each other apart, and the
//! single step that runs them.
//!
//! A step is the only moment the engine moves anything on its own. Bodies pull
//! against `gravityPull`, the floor stops them, and two bodies whose colliders
//! are switched on refuse to share the same meters — and owe each other whatever
//! `damage` says that is worth.

use glam::Vec3;

use crate::geometry::{self, Solid};
use crate::motion::Motion;
use crate::node::{NodeId, Scene, VecField};
use crate::player::{GROUND_HEIGHT, PlayerController};

/// The longest one step may be, in seconds. A window that was dragged, or a
/// debugger left open, cannot fling the world through the floor.
pub const MAX_STEP: f32 = 0.1;

/// How wide the player is to the solver, in meters. A first person player has no
/// body of its own, and still has to be stopped by a pillar.
pub const PLAYER_WIDTH: f32 = 0.6;

/// How many times one step tries to pull the pairs apart. Stepping out of one
/// body leans into the next, so a stack needs a few passes to settle.
const PUSH_PASSES: usize = 3;

/// Up, the height axis of a world that stands on its z.
const UP: Vec3 = Vec3::Z;

/// Per second, how much of a sliding body's sideways speed the floor eats. The
/// solver has no friction of its own, so without this a shove from a quake, a gust
/// or a blast would keep a crate moving long after the weather had stopped.
const FLOOR_DRAG: f32 = 3.0;

/// Anything the solver may put somewhere, or hold exactly where it is.
struct Body {
    /// The node the collider belongs to. For the player this is the object drawn
    /// as its body, which may be none at all.
    node: Option<NodeId>,
    solid: Solid,
    /// Whether a push may move it. Scenery holds its ground.
    movable: bool,
    /// The player, which gives way to a wall but never to a crate it walks into.
    player: bool,
}

impl Scene {
    /// Advances the world by `dt` seconds: the player first, because the camera
    /// follows it, then everything gravity owns, then the bodies that refuse to
    /// overlap, then what those touches cost. The camera and the drawn body come
    /// next, so neither shows the position a collision has already corrected, and
    /// the crowds fly last of all, over a world that is where it will be drawn.
    pub fn step(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, MAX_STEP);
        self.contacts.clear();
        self.step_player(dt);
        self.count_down_hits(dt);
        self.run_disasters(dt);

        for id in self.node_tree() {
            self.integrate(id, dt);
        }

        self.push_apart();
        self.hurt_by_contacts();
        self.follow_camera();
        self.place_body();
        self.run_particles(dt);
    }

    /// Pulls one node down unless it is anchored or has no gravity. The floor is
    /// the one thing every falling body meets before its own collider is asked.
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
            // A body brought to the floor this step is sliding on it, and a floor
            // with no grip at all never lets go of the speed a disaster gave it.
            let grip = (-FLOOR_DRAG * dt).exp();
            velocity.x *= grip;
            velocity.y *= grip;
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

    /// Pulls every overlapping pair of colliders apart, scenery holding its
    /// ground. A pair whose answer cannot be checked — the maths went wrong, or
    /// the two bodies are too flat to say which way out is shortest — is left
    /// alone for this step rather than pushed somewhere worse.
    fn push_apart(&mut self) {
        for _ in 0..PUSH_PASSES {
            let bodies = self.bodies();
            let mut moves = vec![Vec3::ZERO; bodies.len()];
            let mut pushed = false;

            for index in 0..bodies.len() {
                for other in index + 1..bodies.len() {
                    let (a, b) = (&bodies[index], &bodies[other]);
                    if !Solid::in_boxes(&a.solid, &b.solid) {
                        continue;
                    }
                    let Some(hit) = geometry::contact(&a.solid, &b.solid) else {
                        continue;
                    };
                    if !geometry::separates(&a.solid, &b.solid, &hit) {
                        continue;
                    }
                    let Some((a_share, b_share)) = gives_way(a, b) else {
                        continue;
                    };

                    // The normal runs out of the first body and into the second,
                    // so the first goes back along it and the second on with it.
                    if a_share > 0.0 {
                        moves[index] -= hit.normal * (hit.depth * a_share);
                        pushed = true;
                    }
                    if b_share > 0.0 {
                        moves[other] += hit.normal * (hit.depth * b_share);
                        pushed = true;
                    }
                    self.remember(a.node, b.node);
                }
            }

            if !pushed {
                break;
            }
            for (body, push) in bodies.iter().zip(moves) {
                if push.length_squared() > 0.0 {
                    self.settle(body, push);
                }
            }
        }
    }

    /// Everything that takes part in collisions right now, in world meters. Each
    /// pass asks again, because a body that has just been moved is somewhere new.
    fn bodies(&self) -> Vec<Body> {
        let mut bodies = Vec::new();
        let player = self.player;

        if player.enabled
            && let Some(solid) = player_hull(&player)
        {
            bodies.push(Body {
                node: player.body,
                solid,
                movable: true,
                player: true,
            });
        }

        for (id, solid) in self.colliders() {
            // The player's own body is already in the walk above, as a person and
            // not as whatever object it happens to wear.
            if player.enabled && player.body == Some(id) {
                continue;
            }
            bodies.push(Body {
                node: Some(id),
                solid,
                movable: self.is_dynamic(id).unwrap_or(false),
                player: false,
            });
        }
        bodies
    }

    /// Moves one body out of what it was riding through, and takes away the speed
    /// that drove it in. What is left slides along the surface.
    fn settle(&mut self, body: &Body, world_move: Vec3) {
        let direction = world_move.normalize_or_zero();
        if body.player {
            let mut player = self.player;
            player.position += world_move;
            // Being lifted is standing on something, and a jump from there is a
            // jump the script can ask for.
            player.grounded |= direction.z > 0.5;
            let into = player.velocity.dot(direction);
            if into < 0.0 {
                player.velocity -= direction * into;
            }
            self.player = player;
            return;
        }

        let Some(node) = body.node else { return };
        let Ok(transform) = self.transform(node) else {
            return;
        };
        if self
            .set_vec_field(
                node,
                VecField::Position,
                transform.position + self.local_delta(node, world_move),
            )
            .is_err()
        {
            return;
        }
        let Ok(mut motion) = self.component_mut::<Motion>(node) else {
            return;
        };
        let into = motion.velocity.dot(direction);
        if into < 0.0 {
            motion.velocity -= direction * into;
        }
    }

    /// A push written the way a node's own `position` holds it: back through its
    /// parent's transform, so a body under a turned or shrunk parent moves the way
    /// it was pushed rather than the way its local axes happen to point.
    fn local_delta(&self, id: NodeId, world: Vec3) -> Vec3 {
        let Some(parent) = self.parent(id).ok().flatten() else {
            return world;
        };
        let Some(inverse) = self
            .global_matrix(parent)
            .ok()
            .and_then(|model| model.try_inverse())
        else {
            return world;
        };
        inverse.transform_vector3(world)
    }

    /// Which two bodies rode into each other, once per pair however many passes it
    /// took to settle them.
    fn remember(&mut self, a: Option<NodeId>, b: Option<NodeId>) {
        let (Some(a), Some(b)) = (a, b) else {
            return;
        };
        if !self.contacts.contains(&(a, b)) && !self.contacts.contains(&(b, a)) {
            self.contacts.push((a, b));
        }
    }

    /// The nodes this one was touching at the end of the last step. The player is
    /// the object attached as its body, so a player that wears nothing reports
    /// nothing.
    pub fn touches(&self, id: NodeId) -> Vec<NodeId> {
        self.contacts
            .iter()
            .filter_map(|(a, b)| {
                if *a == id {
                    Some(*b)
                } else if *b == id {
                    Some(*a)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Every node below the root, parents before children.
    pub(crate) fn node_tree(&self) -> Vec<NodeId> {
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

/// The player as the solver sees it: a capsule from its feet to its eyes. It is
/// not the object drawn as its body, which can be anything or nothing, so a first
/// person player is blocked by a pillar exactly as a person would be.
fn player_hull(player: &PlayerController) -> Option<Solid> {
    let radius = PLAYER_WIDTH * 0.5;
    let feet = player.position + UP * radius;
    let head = player.position + UP * radius.max(player.eye_height);
    Solid::capsule([feet, head], radius)
}

/// How a pair divides the push. Scenery keeps its ground and whatever ran into it
/// gives way entirely; two falling bodies split it; the player never yields to a
/// crate it walks into, only to a wall. `None` is a pair with nobody to move.
fn gives_way(a: &Body, b: &Body) -> Option<(f32, f32)> {
    match (a.movable, b.movable) {
        (false, false) => None,
        (true, false) => Some((1.0, 0.0)),
        (false, true) => Some((0.0, 1.0)),
        (true, true) if a.player => Some((0.0, 1.0)),
        (true, true) if b.player => Some((1.0, 0.0)),
        (true, true) => Some((0.5, 0.5)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{FrameInput, Key};
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

    /// A body the world holds still: it takes part in collisions and nothing
    /// pushes it anywhere.
    fn scenery(scene: &mut Scene, size: Vec3, at: Vec3) -> NodeId {
        let id = scene.create("cube", Some("Scenery".into()), None).unwrap();
        scene.set_vec_field(id, VecField::Size, size).unwrap();
        scene.set_vec_field(id, VecField::Position, at).unwrap();
        scene.set_collision(id, true).unwrap();
        id
    }

    /// A one metre cube gravity owns and that collides, dropped from `height`.
    fn falling(scene: &mut Scene, at: Vec3) -> NodeId {
        let id = scene.create("cube", Some("Crate".into()), None).unwrap();
        scene
            .set_vec_field(id, VecField::Size, Vec3::splat(1.0))
            .unwrap();
        scene.set_vec_field(id, VecField::Position, at).unwrap();
        scene.set_uses_gravity(id, true).unwrap();
        scene.set_collision(id, true).unwrap();
        id
    }

    fn walk_into(scene: &mut Scene, steps: usize) {
        for _ in 0..steps {
            scene.update_input(&FrameInput {
                keys: vec![(Key::W, true)],
                ..Default::default()
            });
            scene.step(1.0 / 60.0);
        }
    }

    #[test]
    fn a_falling_cube_lands_on_a_table_rather_than_the_floor() {
        let mut scene = Scene::new();
        let table = scenery(
            &mut scene,
            Vec3::new(2.0, 2.0, 0.2),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let cube = falling(&mut scene, Vec3::new(0.0, 0.0, 3.0));

        for _ in 0..300 {
            scene.step(1.0 / 60.0);
        }

        // The top of the table is 1.1 up, so a one metre cube rests with its
        // centre half a metre above that.
        let height = scene.vec_field(cube, VecField::Position).unwrap().z;
        assert!((height - 1.6).abs() < 1e-3, "came to rest at {height}");
        assert!(
            scene.touches(cube).contains(&table),
            "and knows what it is standing on"
        );
    }

    #[test]
    fn a_body_that_never_asked_falls_straight_through() {
        let mut scene = Scene::new();
        scenery(
            &mut scene,
            Vec3::new(2.0, 2.0, 0.2),
            Vec3::new(0.0, 0.0, 1.0),
        );
        let id = scene.create("cube", None, None).unwrap();
        scene
            .set_vec_field(id, VecField::Size, Vec3::splat(1.0))
            .unwrap();
        scene
            .set_vec_field(id, VecField::Position, Vec3::new(0.0, 0.0, 3.0))
            .unwrap();
        scene.set_uses_gravity(id, true).unwrap();

        for _ in 0..300 {
            scene.step(1.0 / 60.0);
        }
        let height = scene.vec_field(id, VecField::Position).unwrap().z;
        assert!(
            (height - 0.5).abs() < 1e-3,
            "ghosts reach the floor: {height}"
        );
    }

    #[test]
    fn two_crates_settle_one_on_top_of_the_other() {
        let mut scene = Scene::new();
        let lower = falling(&mut scene, Vec3::new(0.0, 0.0, 1.0));
        let upper = falling(&mut scene, Vec3::new(0.0, 0.0, 2.2));

        for _ in 0..400 {
            scene.step(1.0 / 60.0);
        }

        let rest = scene.vec_field(lower, VecField::Position).unwrap();
        let top = scene.vec_field(upper, VecField::Position).unwrap();
        assert!(
            (rest.z - 0.5).abs() < 1e-2,
            "the bottom crate on the floor: {rest}"
        );
        assert!((top.z - 1.5).abs() < 1e-2, "and the other on top: {top}");
        assert!(scene.touches(upper).contains(&lower));
    }

    #[test]
    fn a_player_walks_into_a_pillar_and_stays_outside_it() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        scenery(
            &mut scene,
            Vec3::new(0.5, 0.5, 3.0),
            Vec3::new(2.0, 0.0, 1.5),
        );

        walk_into(&mut scene, 120);
        let position = scene.player_position();

        // The pillar's near face is 1.75 away and the player is three tenths
        // through anything it touches.
        assert!(
            (1.0..1.5).contains(&position.x),
            "stopped with its nose against the pillar: {position}"
        );
        assert!(position.y.abs() < 0.05, "and never slid around it");
    }

    #[test]
    fn a_player_stands_on_a_crate_it_climbs_onto() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        scenery(&mut scene, Vec3::splat(1.0), Vec3::new(0.0, 0.0, 1.0));
        scene.set_player_position(Vec3::new(0.0, 0.0, 1.8));

        for _ in 0..200 {
            scene.step(1.0 / 60.0);
        }

        let position = scene.player_position();
        assert!(
            (position.z - 1.5).abs() < 1e-2,
            "feet on the top face of the crate: {position}"
        );
        assert!(scene.player_is_grounded(), "standing on it counts");
    }

    #[test]
    fn walking_into_a_crate_shoves_the_crate_not_the_player() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        let crate_ = falling(&mut scene, Vec3::new(1.5, 0.0, 0.5));

        walk_into(&mut scene, 60);
        let crate_at = scene.vec_field(crate_, VecField::Position).unwrap();
        let position = scene.player_position();

        assert!(crate_at.x > 1.6, "the crate gave way: {crate_at}");
        assert!(position.x > 0.6, "and the player kept walking: {position}");
    }

    #[test]
    fn a_walk_cannot_slip_through_a_thin_wall() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        scene.set_player_speed(8.0);
        scenery(
            &mut scene,
            Vec3::new(0.2, 4.0, 3.0),
            Vec3::new(2.0, 0.0, 1.5),
        );

        walk_into(&mut scene, 120);
        let position = scene.player_position();

        assert!(
            position.x < 1.7,
            "eight metres a second is thirteen centimetres a step, not a teleport: {position}"
        );
    }

    /// An actor the size of a metre cube with its collider switched on. A hazard
    /// holds its ground and bites; the other falls and takes what it is given.
    fn actor(scene: &mut Scene, at: Vec3, health: f32, damage: f32, hazard: bool) -> NodeId {
        let id = scene
            .spawn_entity(Some("cube"), None, None, Some(at), health, damage)
            .unwrap();
        scene
            .set_vec_field(id, VecField::Size, Vec3::splat(1.0))
            .unwrap();
        scene.set_collision(id, true).unwrap();
        if !hazard {
            scene.set_uses_gravity(id, true).unwrap();
        }
        id
    }

    #[test]
    fn a_hazard_bites_once_per_pause_rather_than_once_per_frame() {
        let mut scene = Scene::new();
        let lava = actor(&mut scene, Vec3::new(0.0, 0.0, 1.0), 40.0, 1.0, true);
        let crate_ = actor(&mut scene, Vec3::new(0.0, 0.0, 3.0), 100.0, 0.0, false);

        for _ in 0..180 {
            scene.step(1.0 / 60.0);
        }

        let health = scene.health(crate_).unwrap();
        assert!(health < 100.0, "lying in lava costs a hit point: {health}");
        assert!(
            health > 90.0,
            "one hit point every half second, not every frame: {health}"
        );
        assert_eq!(
            scene.health(lava).unwrap(),
            40.0,
            "and the crate bites nobody back"
        );
    }

    #[test]
    fn a_fight_played_out_in_contacts_ends_with_a_corpse_on_the_floor() {
        let mut scene = Scene::new();
        let lava = actor(&mut scene, Vec3::new(0.0, 0.0, 1.0), 40.0, 10.0, true);
        let fighter = actor(&mut scene, Vec3::new(0.0, 0.0, 3.0), 25.0, 8.0, false);

        for _ in 0..240 {
            scene.step(1.0 / 60.0);
        }

        assert!(!scene.alive(fighter).unwrap(), "the fighter is down");
        assert!(scene.alive(lava).unwrap(), "and only the lava is left up");
        assert!(
            !scene.has_collision(fighter).unwrap(),
            "a corpse is no longer in anyone's way"
        );

        let settled = scene.health(lava).unwrap();
        for _ in 0..300 {
            scene.step(1.0 / 60.0);
        }
        assert_eq!(scene.health(lava).unwrap(), settled, "a corpse cannot hurt");

        let height = scene.vec_field(fighter, VecField::Position).unwrap().z;
        assert!(
            (height - 0.5).abs() < 1e-2,
            "the corpse drops through the lava it died on and comes to rest on the floor: {height}"
        );
        assert_eq!(scene.node_count(), 2, "and it stays in the world");
    }

    #[test]
    fn a_player_walks_into_lava_and_pays_for_it() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        let body = scene
            .spawn_entity(Some("cube"), None, None, None, 30.0, 0.0)
            .unwrap();
        scene.set_player_body(Some(body)).unwrap();
        actor(&mut scene, Vec3::new(2.0, 0.0, 1.0), 40.0, 4.0, true);

        walk_into(&mut scene, 120);

        let health = scene.health(body).unwrap();
        assert!(
            health < 30.0,
            "the body wears the damage it walks into: {health}"
        );
        assert!(health > 10.0, "half a second between bites: {health}");
    }
}
