//! The solver: gravity, the floor, bodies that hold each other apart, and the
//! single step that runs them.
//!
//! A step is the only moment the engine moves anything on its own. Bodies pull
//! against `gravityPull`, the floor stops them, and two bodies whose colliders
//! are switched on refuse to share the same meters — and owe each other whatever
//! `damage` says that is worth.

use glam::{Quat, Vec3};

use crate::geometry::{self, Contact, Solid};
use crate::motion::Motion;
use crate::node::{NodeId, Scene, VecField};
use crate::player::PlayerController;

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
/// or a blast would keep a crate moving long after the weather had stopped. A
/// vehicle with a driver on the controls is the one body this never touches.
const FLOOR_DRAG: f32 = 3.0;

/// Anything the solver may put somewhere, or hold exactly where it is.
struct Body {
    /// The node the collider belongs to. For the player this is the object drawn
    /// as its body, which may be none at all.
    node: Option<NodeId>,
    /// The convex pieces of the body, in world meters. A box or a ball is one
    /// piece; a mesh is one per surface it is drawn from, because a hull around
    /// the whole of a vehicle is a brick with the room inside it filled in.
    pieces: Vec<Solid>,
    /// Meters per second, as of this pass. A pair that rides into each other trades
    /// it, and the answer is only worth anything if the body is left reporting it.
    velocity: Vec3,
    /// Whether a push may move it. Scenery holds its ground.
    movable: bool,
    /// Kilograms. Two bodies that may both move divide the push by it.
    mass: f32,
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
        // A driver's wishes are read before anything moves, so the speed they ask for is
        // the speed the solver integrates this step rather than the one after it.
        self.step_vehicles(dt);

        for id in self.node_tree() {
            self.integrate(id, dt);
        }

        // Every body is cut to where it now stands before anything is asked to shove
        // it, so a mesh of thousands of points is carried into world metres once a step
        // rather than at each of the passes below - and not at all while it stands still.
        self.recut();

        self.push_apart();
        self.hurt_by_contacts();
        // Riders come last of the things that move: they are held where their seats
        // ended up, so nothing has to wait for a car to settle before its driver is
        // drawn in it.
        self.place_seats();
        // The player's eyes ride the same seat its body was just welded to, so the
        // camera below it looks out of a car that has already been driven.
        self.ride_seat();
        self.follow_camera();
        self.place_body();
        self.run_particles(dt);
    }

    /// Pulls one node down unless it is anchored or has no gravity, and turns one
    /// the drive model has spun. The floor is the one thing every falling body meets
    /// before its own collider is asked.
    fn integrate(&mut self, id: NodeId, dt: f32) {
        let Ok(motion) = self.component::<Motion>(id) else {
            return;
        };
        if motion.anchored {
            return;
        }
        // A body riding in a seat is not its own to move: the step puts it where the
        // seat is, and gravity or a wheel has no claim on it until it gets out.
        if self.rides_a_seat(id) {
            return;
        }
        let spin = motion.spin;
        let gravity = motion.gravity;
        let pull = motion.gravity_pull;
        let mut velocity = motion.velocity;
        let Ok(transform) = self.transform(id) else {
            return;
        };

        // A spin is a turn the drive model asked for, and it is taken here rather than
        // there so that a body has exactly one place the world moves it: an anchored
        // car is not allowed to roll, and so is not allowed to pivot either.
        if spin != 0.0 {
            let turned = Quat::from_rotation_z(spin.to_radians() * dt) * transform.rotation;
            if self.set_rotation(id, turned).is_err() {
                return;
            }
        }
        if !gravity {
            return;
        }
        velocity.z -= pull * dt;
        let mut position = transform.position + velocity * dt;

        let scale = self.vec_field(id, VecField::Scale).unwrap_or(Vec3::ONE);
        // The lowest point of the body as it is drawn, in the node's own metres and then
        // stretched by the scale it is wearing. A body with no surface of its own but a
        // model under it has to be measured from the model: an import hangs its geometry
        // wherever the file stood it, which for a real vehicle is most of a metre below
        // the pivot, so parking the pivot on the floor buries the wheels under it.
        let bottom = self
            .local_bounds(id)
            .map(|(min, _)| min.z * scale.z)
            .unwrap_or(0.0);
        let rest = position.z + bottom;

        // The ground under the body is the flat floor or the map it stands on, so a
        // crate dropped on a hill comes to rest on the hill and not through it.
        let floor = self.floor_at(position.x, position.y);
        if rest <= floor {
            position.z += floor - rest;
            if velocity.z < 0.0 {
                velocity.z = 0.0;
            }
            // A body brought to the floor this step is sliding on it, and a floor
            // with no grip at all never lets go of the speed a disaster gave it. A
            // vehicle a driver is asking something of is not sliding: its tyres have
            // already said how fast it goes this step, and the floor does not
            // second-guess them.
            if !self.is_driven(id) {
                let grip = (-FLOOR_DRAG * dt).exp();
                velocity.x *= grip;
                velocity.y *= grip;
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

    /// Pulls every overlapping pair of colliders apart, scenery holding its
    /// ground, and trades the pace the pair arrived with between them. A pair whose
    /// answer cannot be checked — the maths went wrong, or the two bodies are too
    /// flat to say which way out is shortest — is left alone for this step rather
    /// than pushed somewhere worse.
    fn push_apart(&mut self) {
        for _ in 0..PUSH_PASSES {
            let mut bodies = self.bodies();
            let mut moves = vec![Vec3::ZERO; bodies.len()];
            let mut pushed = false;

            for index in 0..bodies.len() {
                for other in index + 1..bodies.len() {
                    let (a, b) = (&bodies[index], &bodies[other]);
                    // Two bodies hung one under the other are one shape to the
                    // person who wrote them: a turret that shoves its own hull
                    // spends the step digging itself out of the thing it rides on.
                    if self.in_the_same_branch(a.node, b.node) {
                        continue;
                    }
                    let Some(hit) = contact_between(&a.pieces, &b.pieces) else {
                        continue;
                    };
                    let Some((a_share, b_share)) = gives_way(a, b) else {
                        continue;
                    };
                    let (a_node, b_node) = (a.node, b.node);

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
                    // The two bodies are the pair's only two entries, and the first
                    // is always the earlier one, so the split gives each its own.
                    let (head, tail) = bodies.split_at_mut(other);
                    trade(&mut head[index], &mut tail[0], hit.normal, a_share, b_share);
                    self.remember(a_node, b_node);
                }
            }

            if !pushed {
                break;
            }
            for (body, push) in bodies.iter().zip(moves) {
                self.keep_pace(body);
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

        // A player sitting in a seat takes up the room its car already takes up: the
        // person's own hull would shove the vehicle it rides in, so the solver is told
        // the car is the body and nothing else.
        if player.enabled
            && !self.rides_somewhere()
            && let Some(solid) = player_hull(&player)
        {
            bodies.push(Body {
                node: player.body,
                pieces: vec![solid],
                velocity: player.velocity,
                movable: true,
                // A person is the one body whose weight is never asked: the rule
                // below refuses to let a crate shove them off their feet, which the
                // pace trade reads as standing on something heavier than anything.
                mass: 0.0,
                player: true,
            });
        }

        for (id, pieces) in self.colliders() {
            // The player's own body is already in the walk above, as a person and
            // not as whatever object it happens to wear.
            if player.enabled && player.body == Some(id) {
                continue;
            }
            // A rider is inside the car it sits in, and a body that is inside another
            // body would spend the step fighting its way out of it.
            if self.rides_a_seat(id) {
                continue;
            }
            let movable = self.is_dynamic(id).unwrap_or(false);
            // A body that holds its ground is read as standing still, whatever its own
            // motion says, so a pair is traded against the ground it stands on.
            let velocity = match movable {
                true => self
                    .component::<Motion>(id)
                    .map(|motion| motion.velocity)
                    .unwrap_or(Vec3::ZERO),
                false => Vec3::ZERO,
            };
            bodies.push(Body {
                node: Some(id),
                pieces,
                velocity,
                movable,
                mass: self.mass_of(id),
                player: false,
            });
        }
        bodies
    }

    /// Whether one of these two nodes is the other hung under it. Such a pair is one
    /// body to whoever wrote it, so the solver is asked to leave it alone: a part
    /// that shoves the thing it rides on can only dig itself out of it, and a model
    /// already counts its own parts once.
    fn in_the_same_branch(&self, a: Option<NodeId>, b: Option<NodeId>) -> bool {
        let (Some(a), Some(b)) = (a, b) else {
            return false;
        };
        self.is_ancestor(a, b).unwrap_or(false) || self.is_ancestor(b, a).unwrap_or(false)
    }

    /// Moves one body out of what it was riding through. That is only the part a
    /// step cannot do with speed alone: a body arrives wherever it arrived, and the
    /// meters it already shares with another have to be given back before the next
    /// one is taken. The pace of the pair is traded where the touch is found, so a
    /// shoved body goes off along its own way rather than standing still and being
    /// dragged a little further every step.
    fn settle(&mut self, body: &Body, world_move: Vec3) {
        let direction = world_move.normalize_or_zero();
        if body.player {
            let mut player = self.player;
            player.position += world_move;
            // Being lifted is standing on something, and a jump from there is a
            // jump the script can ask for.
            player.grounded |= direction.z > 0.5;
            self.player = player;
            return;
        }

        let Some(node) = body.node else { return };
        let Ok(transform) = self.transform(node) else {
            return;
        };
        let _ = self.set_vec_field(
            node,
            VecField::Position,
            transform.position + self.local_delta(node, world_move),
        );
    }

    /// Leaves a body reporting the pace its touches gave it, so that the next pass —
    /// and every step after it — travels at that pace instead of being shoved again
    /// from a standstill. A body that holds its ground was offered none of it, and the
    /// node it belongs to keeps whatever its own motion says.
    fn keep_pace(&mut self, body: &Body) {
        if !body.movable {
            return;
        }
        if body.player {
            let mut player = self.player;
            player.velocity = body.velocity;
            self.player = player;
            return;
        }
        let Some(node) = body.node else { return };
        if let Ok(mut motion) = self.component_mut::<Motion>(node) {
            motion.velocity = body.velocity;
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

/// The shallowest way the two bodies are through each other, asked of every pair of
/// their pieces. A box or a ball has one piece, so this is the one test it always
/// was; a mesh has one per surface, and only the surfaces that actually ride into
/// something answer for it. The space between them is left empty rather than
/// filled in by the brick around the whole body.
fn contact_between(a: &[Solid], b: &[Solid]) -> Option<Contact> {
    let mut shallowest: Option<Contact> = None;

    for first in a {
        for second in b {
            let Some(hit) = geometry::contact(first, second) else {
                continue;
            };
            if !geometry::separates(first, second, &hit) {
                continue;
            }
            if shallowest.is_none_or(|past| hit.depth < past.depth) {
                shallowest = Some(hit);
            }
        }
    }
    shallowest
}

/// What a touch does to the pace of the pair. The gap between them is divided by
/// `gives_way`, and the speed they arrived with is divided exactly the same way,
/// because the body made to close most of the meters is the one that owes most of
/// its own way for them. So a pair that rides into each other leaves at one speed
/// along the line they met on: two crates of a weight both go off at the middle of
/// what they came with, a heavy one hardly changes and a light one goes with it,
/// and a body that runs into something holding its ground loses its way entirely.
/// Only a pair still arriving trades; one already pulling apart has nothing left to
/// give, and the way a body slides along a surface is nobody's business here.
fn trade(a: &mut Body, b: &mut Body, normal: Vec3, a_share: f32, b_share: f32) {
    let closing = a.velocity.dot(normal) - b.velocity.dot(normal);
    if !closing.is_finite() || closing <= 0.0 {
        return;
    }
    a.velocity -= normal * (closing * a_share);
    b.velocity += normal * (closing * b_share);
}

/// How a pair divides the push. Scenery keeps its ground and whatever ran into it
/// gives way entirely; two falling bodies split it by weight, each giving way in
/// proportion to how heavy the other is; the player never yields to a crate it
/// walks into, only to a wall. `None` is a pair with nobody to move.
fn gives_way(a: &Body, b: &Body) -> Option<(f32, f32)> {
    match (a.movable, b.movable) {
        (false, false) => None,
        (true, false) => Some((1.0, 0.0)),
        (false, true) => Some((0.0, 1.0)),
        (true, true) if a.player => Some((0.0, 1.0)),
        (true, true) if b.player => Some((1.0, 0.0)),
        (true, true) => {
            let total = a.mass + b.mass;
            if total <= 0.0 || !total.is_finite() {
                return Some((0.5, 0.5));
            }
            Some((b.mass / total, a.mass / total))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{FrameInput, Key};
    use crate::motion::EARTH_GRAVITY_PULL;
    use crate::player::GROUND_HEIGHT;

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

    /// What a body is pointing at, in degrees about the upright axis.
    fn yaw(scene: &Scene, id: NodeId) -> f32 {
        scene.vec_field(id, VecField::RotationEuler).unwrap().z
    }

    fn spin(scene: &mut Scene, id: NodeId, rate: f32) {
        scene.component_mut::<Motion>(id).unwrap().spin = rate;
    }

    #[test]
    fn a_spun_body_turns_on_the_spot() {
        let (mut scene, id) = dropped(0.0, 10.0);
        scene.set_uses_gravity(id, false).unwrap();
        spin(&mut scene, id, 90.0);

        // Ten tenths of a second, because a step is never longer than a tenth: a
        // quarter turn written as ninety degrees a second takes a second of steps.
        for _ in 0..10 {
            scene.step(0.1);
        }

        assert!(
            yaw(&scene, id) > 89.9 && yaw(&scene, id) < 90.1,
            "{}",
            yaw(&scene, id)
        );
        let position = scene.vec_field(id, VecField::Position).unwrap();
        assert_eq!(position, Vec3::new(0.0, 0.0, 10.0), "and turns only");
    }

    #[test]
    fn an_anchored_body_holds_its_direction() {
        let (mut scene, id) = dropped(0.0, 10.0);
        spin(&mut scene, id, 90.0);
        scene.set_anchored(id, true).unwrap();

        for _ in 0..10 {
            scene.step(0.1);
        }

        assert_eq!(yaw(&scene, id), 0.0, "a pinned car cannot be steered");
    }

    #[test]
    fn a_body_turns_while_it_falls_and_still_spins_after_it_lands() {
        let (mut scene, id) = dropped(EARTH_GRAVITY_PULL, 10.0);
        spin(&mut scene, id, 60.0);

        scene.step(0.1);
        assert!(yaw(&scene, id) > 5.9 && yaw(&scene, id) < 6.1);
        assert!(
            height(&scene, id) < 10.0,
            "the turn did not cancel the fall"
        );

        let nose = scene.rotation(id).unwrap() * Vec3::X;
        for _ in 0..300 {
            scene.step(1.0 / 60.0);
        }
        assert!(
            (height(&scene, id) - 0.5).abs() < 1e-3,
            "and it came to the floor normally"
        );
        let turned = scene.rotation(id).unwrap() * Vec3::X;
        assert!(
            nose.distance(turned) > 0.1,
            "spinning is not something a floor stops"
        );
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

    /// A two metre hull with a one metre part overlapping its side by sixty-five
    /// centimetres, hung under the hull or set down beside it as `under` says. Both
    /// bodies collide and both fall.
    fn pair(scene: &mut Scene, under: bool) -> (NodeId, NodeId) {
        let hull = scene.create("cube", Some("Hull".into()), None).unwrap();
        scene
            .set_vec_field(hull, VecField::Size, Vec3::splat(2.0))
            .unwrap();
        scene
            .set_vec_field(hull, VecField::Position, Vec3::new(0.0, 0.0, 10.0))
            .unwrap();
        scene.set_uses_gravity(hull, true).unwrap();
        scene.set_collision(hull, true).unwrap();

        // The parent's size carries the child's own metres into the world, so the part
        // written under it is half as big and half as far out to end up in the same
        // place as the one stood beside it.
        let part = scene
            .create("cube", Some("Part".into()), under.then_some(hull))
            .unwrap();
        scene
            .set_vec_field(
                part,
                VecField::Size,
                Vec3::splat(if under { 0.5 } else { 1.0 }),
            )
            .unwrap();
        scene
            .set_vec_field(
                part,
                VecField::Position,
                Vec3::new(
                    if under { 0.425 } else { 0.85 },
                    0.0,
                    if under { 0.0 } else { 10.0 },
                ),
            )
            .unwrap();
        scene.set_uses_gravity(part, true).unwrap();
        scene.set_collision(part, true).unwrap();
        (hull, part)
    }

    #[test]
    fn a_part_hung_on_a_body_does_not_dig_itself_out_of_it() {
        let mut scene = Scene::new();
        let (hull, part) = pair(&mut scene, true);

        scene.step(1.0 / 60.0);

        // A part shoved out of the thing it rides on spends the step clawing its way
        // free of its own body, so the solver is told a pair hung one under the other
        // is one body and left where the script put it.
        let at = scene.vec_field(part, VecField::Position).unwrap();
        assert_eq!(at.x, 0.425, "the part kept its place on the hull: {at}");
        assert!(
            !scene.touches(part).contains(&hull),
            "and owes it no contact either"
        );
    }

    #[test]
    fn the_same_two_bodies_beside_each_other_are_pulled_apart() {
        let mut scene = Scene::new();
        let (_, part) = pair(&mut scene, false);

        scene.step(1.0 / 60.0);

        // The proof that it is the branch, not the overlap, that holds the test above
        // still: the same two boxes with nothing hanging them together separate.
        let at = scene.vec_field(part, VecField::Position).unwrap();
        assert!(at.x > 0.85, "the part is pushed clear of the hull: {at}");
    }

    #[test]
    fn an_anvil_barely_gives_way_to_a_bolt() {
        let mut scene = Scene::new();
        let anvil = falling(&mut scene, Vec3::new(0.0, 0.0, 5.0));
        let bolt = falling(&mut scene, Vec3::new(0.8, 0.0, 5.0));
        scene.set_mass(anvil, 2_000.0).unwrap();
        scene.set_mass(bolt, 0.05).unwrap();

        // Twenty centimetres of the two boxes is shared out between them.
        scene.step(1.0 / 60.0);

        let anvil = scene.vec_field(anvil, VecField::Position).unwrap().x;
        let bolt = scene.vec_field(bolt, VecField::Position).unwrap().x - 0.8;
        assert!(bolt > 0.19, "the bolt takes nearly all of it: {bolt}");
        assert!(anvil.abs() < 1e-3, "and the anvil the rest: {anvil}");
    }

    /// A box already on the floor and already moving at `pace` meters a second,
    /// with `kg` of weight behind it.
    fn sliding(scene: &mut Scene, size: Vec3, at: Vec3, kg: f32, pace: f32) -> NodeId {
        let id = scene.create("cube", Some("Runner".into()), None).unwrap();
        scene.set_vec_field(id, VecField::Size, size).unwrap();
        scene.set_vec_field(id, VecField::Position, at).unwrap();
        scene.set_uses_gravity(id, true).unwrap();
        scene.set_collision(id, true).unwrap();
        scene.set_mass(id, kg).unwrap();
        scene.component_mut::<Motion>(id).unwrap().velocity = Vec3::X * pace;
        id
    }

    fn run(scene: &Scene, id: NodeId) -> f32 {
        scene.component::<Motion>(id).unwrap().velocity.x
    }

    #[test]
    fn a_heavy_body_that_barely_yields_keeps_most_of_its_way() {
        let mut scene = Scene::new();
        let anvil = sliding(
            &mut scene,
            Vec3::splat(1.0),
            Vec3::new(0.0, 0.0, 0.5),
            2_000.0,
            5.0,
        );
        let bolt = sliding(
            &mut scene,
            Vec3::splat(0.2),
            Vec3::new(0.55, 0.0, 0.1),
            0.05,
            0.0,
        );

        scene.step(1.0 / 60.0);

        // The floor eats a little of the anvil's pace, as it does of anything it
        // holds, but the bolt it rode into takes none of it: two thousand kilograms
        // gives way over a twenty thousandth of the gap, and owes only that much of
        // its own way.
        let kept = run(&scene, anvil);
        assert!(kept > 4.6, "an anvil does not stop at a bolt: {kept}");
        let bolt_at = scene.vec_field(bolt, VecField::Position).unwrap().x;
        assert!(bolt_at > 0.62, "and the bolt took the gap: {bolt_at}");
    }

    #[test]
    fn a_body_that_yields_fully_loses_the_speed_it_came_with() {
        let mut scene = Scene::new();
        let anvil = sliding(
            &mut scene,
            Vec3::splat(1.0),
            Vec3::new(0.0, 0.0, 0.5),
            2_000.0,
            5.0,
        );
        scenery(
            &mut scene,
            Vec3::new(0.2, 1.0, 1.0),
            Vec3::new(0.55, 0.0, 0.5),
        );

        scene.step(1.0 / 60.0);
        let stopped = run(&scene, anvil);
        assert!(
            stopped.abs() < 0.1,
            "scenery keeps its ground, so the anvil gives way all of it: {stopped}"
        );
    }

    #[test]
    fn a_shoved_body_leaves_at_the_pace_it_was_given() {
        let mut scene = Scene::new();
        let pusher = sliding(
            &mut scene,
            Vec3::splat(1.0),
            Vec3::new(0.0, 0.0, 0.5),
            200.0,
            5.0,
        );
        let shoved = sliding(
            &mut scene,
            Vec3::splat(1.0),
            Vec3::new(0.5, 0.0, 0.5),
            200.0,
            0.0,
        );

        scene.step(1.0 / 60.0);

        // Two bodies of a weight leave a ride-in at the middle of what they came
        // with, so the one that stood still now has a way of its own to travel by.
        // A shove is a push, not the solver dragging a body through the meters they
        // happened to share and leaving it exactly as still as it was.
        let from = run(&scene, pusher);
        let into = run(&scene, shoved);
        assert!(
            (2.2..2.8).contains(&into),
            "half the pace went into the shoved body: {into}"
        );
        assert!(
            (2.2..2.8).contains(&from),
            "and the pusher kept the other half: {from}"
        );
    }

    #[test]
    fn an_anchored_body_refuses_a_shove_from_a_moving_one() {
        let mut scene = Scene::new();
        let anvil = sliding(
            &mut scene,
            Vec3::splat(1.0),
            Vec3::new(0.0, 0.0, 0.5),
            4_000.0,
            5.0,
        );
        let bollard = scene.create("cube", Some("Bollard".into()), None).unwrap();
        scene
            .set_vec_field(bollard, VecField::Size, Vec3::new(0.2, 1.0, 1.0))
            .unwrap();
        scene
            .set_vec_field(bollard, VecField::Position, Vec3::new(0.55, 0.0, 0.5))
            .unwrap();
        scene.set_collision(bollard, true).unwrap();
        scene.set_anchored(bollard, true).unwrap();
        scene.component_mut::<Motion>(bollard).unwrap().velocity = Vec3::X * 3.0;

        for _ in 0..4 {
            scene.step(1.0 / 60.0);
        }

        let where_it_was = scene.vec_field(bollard, VecField::Position).unwrap();
        let held = (where_it_was - Vec3::new(0.55, 0.0, 0.5)).length();
        assert!(
            held < 1e-3,
            "nothing an anchored body touches may move it: {held}"
        );
        let pace = scene.component::<Motion>(bollard).unwrap().velocity;
        assert!(
            (pace - Vec3::X * 3.0).length() < 1e-3,
            "and nobody's speed is written onto it either: {pace}"
        );
        let stopped = run(&scene, anvil);
        assert!(
            stopped.abs() < 0.1,
            "the anvil is left to give way all of it: {stopped}"
        );
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
