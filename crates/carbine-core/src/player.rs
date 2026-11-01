//! The player: a body that walks, looks and falls, plus the camera that rides it.
//!
//! Everything here is engine side. A script says `carbine.player().enabled = True`
//! and the WASD maths, the mouse look and the follow camera happen in Rust during
//! [`Scene::step`], from the input the window layer reported.

use glam::{Vec2, Vec3};

use crate::camera::UP;
use crate::input::Key;
use crate::motion::EARTH_GRAVITY_PULL;
use crate::node::{NodeId, Scene, SceneError, VecField};

/// Metres per second a player walks by default.
pub const WALK_SPEED: f32 = 5.0;

/// Degrees of turn per pixel of mouse movement.
pub const LOOK_SENSITIVITY: f32 = 0.12;

/// How far above the feet the eyes sit, in meters.
pub const EYE_HEIGHT: f32 = 1.6;

/// How far behind the body a third person camera parks, in meters.
pub const THIRD_PERSON_DISTANCE: f32 = 4.0;

/// Meters per second a jump leaves the ground with.
pub const JUMP_STRENGTH: f32 = 5.0;

/// The pitch never goes past vertical, or the camera starts flipping.
pub const PITCH_LIMIT: f32 = 89.0;

/// The height everything comes to rest on. Real floors come later; for now the
/// world has one plane at the origin.
pub const GROUND_HEIGHT: f32 = 0.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlayerMode {
    FirstPerson,
    ThirdPerson,
}

impl PlayerMode {
    pub fn from_name(name: &str) -> Option<PlayerMode> {
        Some(match name.trim().to_lowercase().as_str() {
            "first" | "first_person" | "first-person" | "fps" => PlayerMode::FirstPerson,
            "third" | "third_person" | "third-person" | "tps" => PlayerMode::ThirdPerson,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            PlayerMode::FirstPerson => "first",
            PlayerMode::ThirdPerson => "third",
        }
    }
}

/// Where the player is and how it moves. Copy so a read hands over a snapshot.
#[derive(Clone, Copy, Debug)]
pub struct PlayerController {
    pub(crate) enabled: bool,
    pub(crate) mode: PlayerMode,
    /// The feet, in meters. `z` is height, so a grounded player sits at zero.
    pub(crate) position: Vec3,
    pub(crate) velocity: Vec3,
    /// Yaw and pitch in degrees. Yaw turns about `+z`, pitch tilts up from the
    /// ground plane.
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    pub(crate) speed: f32,
    pub(crate) sensitivity: f32,
    pub(crate) eye_height: f32,
    pub(crate) distance: f32,
    pub(crate) gravity_pull: f32,
    pub(crate) jump_strength: f32,
    pub(crate) grounded: bool,
    /// The object drawn as the body, if the script attached one.
    pub(crate) body: Option<NodeId>,
    /// Whether the engine still wants the mouse for looking.
    pub(crate) grab_mouse: bool,
}

impl Default for PlayerController {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: PlayerMode::FirstPerson,
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            speed: WALK_SPEED,
            sensitivity: LOOK_SENSITIVITY,
            eye_height: EYE_HEIGHT,
            distance: THIRD_PERSON_DISTANCE,
            gravity_pull: EARTH_GRAVITY_PULL,
            jump_strength: JUMP_STRENGTH,
            grounded: true,
            body: None,
            grab_mouse: true,
        }
    }
}

impl PlayerController {
    /// The direction the player is facing, including the pitch.
    pub fn look(&self) -> Vec3 {
        let (yaw, pitch) = (self.yaw.to_radians(), self.pitch.to_radians());
        Vec3::new(
            pitch.cos() * yaw.cos(),
            pitch.cos() * yaw.sin(),
            pitch.sin(),
        )
    }

    /// Turn on the horizontal plane, ignoring the pitch, so walking does not climb.
    fn forward(&self) -> Vec3 {
        let yaw = self.yaw.to_radians();
        Vec3::new(yaw.cos(), yaw.sin(), 0.0)
    }

    fn right(&self) -> Vec3 {
        let yaw = self.yaw.to_radians();
        Vec3::new(yaw.sin(), -yaw.cos(), 0.0)
    }

    /// Mouse right turns right, mouse up looks up, and pitch stops short of vertical.
    pub(crate) fn look_at_mouse(&mut self, delta: Vec2) {
        self.yaw = (self.yaw - delta.x * self.sensitivity).rem_euclid(360.0);
        self.pitch = (self.pitch - delta.y * self.sensitivity).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }
}

impl Scene {
    pub fn player(&self) -> PlayerController {
        self.player
    }

    /// Whether the engine drives the walk and the camera.
    pub fn player_is_enabled(&self) -> bool {
        self.player.enabled
    }

    /// Turns automatic control of the camera on or off.
    pub fn set_player_enabled(&mut self, enabled: bool) {
        self.player.enabled = enabled;
        // Taking the camera means asking for the pointer with it; giving the
        // camera back hands the pointer over. A script that wants them split
        // says so afterwards.
        self.player.grab_mouse = enabled;
        self.show_body_for_mode();
    }

    pub fn player_mode(&self) -> PlayerMode {
        self.player.mode
    }

    pub fn set_player_mode(&mut self, mode: PlayerMode) {
        self.player.mode = mode;
        self.show_body_for_mode();
    }

    pub fn player_position(&self) -> Vec3 {
        self.player.position
    }

    /// Meters per second, kept between steps. A script reads it for a speedometer;
    /// only the solver and a teleport write it.
    pub fn player_velocity(&self) -> Vec3 {
        self.player.velocity
    }

    /// Puts the player's feet somewhere. A teleport arrives at rest, so the walk
    /// does not carry on from the new place and the drop starts from still: only
    /// gravity has hold of a player left above the floor.
    ///
    /// A player seated in a seat is where its seat is, so this asks it to get out
    /// first - the feet are the script's again from the moment it writes them.
    pub fn set_player_position(&mut self, position: Vec3) {
        if self.rides_somewhere()
            && let Some(body) = self.player.body
        {
            let _ = self.stand(body);
        }
        self.player.position = position;
        self.player.velocity = Vec3::ZERO;
        self.player.grounded = position.z <= GROUND_HEIGHT + 1e-4;
    }

    pub fn player_yaw(&self) -> f32 {
        self.player.yaw
    }

    pub fn set_player_yaw(&mut self, degrees: f32) {
        self.player.yaw = degrees.rem_euclid(360.0);
    }

    /// Reading pitch back gives the value that was set, clamped or not.
    pub fn player_pitch(&self) -> f32 {
        self.player.pitch
    }

    pub fn set_player_pitch(&mut self, degrees: f32) {
        self.player.pitch = degrees.clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    pub fn player_speed(&self) -> f32 {
        self.player.speed
    }

    pub fn set_player_speed(&mut self, speed: f32) {
        self.player.speed = speed.max(0.0);
    }

    pub fn player_sensitivity(&self) -> f32 {
        self.player.sensitivity
    }

    pub fn set_player_sensitivity(&mut self, sensitivity: f32) {
        self.player.sensitivity = sensitivity.max(0.0);
    }

    pub fn player_eye_height(&self) -> f32 {
        self.player.eye_height
    }

    pub fn set_player_eye_height(&mut self, height: f32) {
        self.player.eye_height = height.max(0.0);
    }

    pub fn player_distance(&self) -> f32 {
        self.player.distance
    }

    pub fn set_player_distance(&mut self, distance: f32) {
        self.player.distance = distance.max(0.0);
    }

    pub fn player_gravity_pull(&self) -> f32 {
        self.player.gravity_pull
    }

    pub fn set_player_gravity_pull(&mut self, pull: f32) {
        self.player.gravity_pull = pull;
    }

    pub fn player_jump_strength(&self) -> f32 {
        self.player.jump_strength
    }

    pub fn set_player_jump_strength(&mut self, strength: f32) {
        self.player.jump_strength = strength;
    }

    pub fn player_is_grounded(&self) -> bool {
        self.player.grounded
    }

    pub fn player_body(&self) -> Option<NodeId> {
        self.player.body
    }

    /// Attaches the object drawn as the player. The scene moves it, so whatever
    /// position it had before is overwritten on the next step.
    pub fn set_player_body(&mut self, body: Option<NodeId>) -> Result<(), SceneError> {
        if let Some(body) = body {
            self.entity(body)?;
            if self
                .world
                .get::<crate::ui::Ui>(self.entity(body)?)
                .is_some()
            {
                return Err(SceneError::NotAUi);
            }
        }
        self.player.body = body;
        self.show_body_for_mode();
        Ok(())
    }

    /// A first person camera sits inside the body, where it would only fill the
    /// screen with the inside of a cube, so the body wears it out of sight. An
    /// unattached or switched-off player hides nothing.
    fn show_body_for_mode(&mut self) {
        let Some(body) = self.player.body else {
            return;
        };
        let visible = !self.player.enabled || self.player.mode != PlayerMode::FirstPerson;
        let _ = self.set_visible(body, visible);
    }

    /// Whether the engine still wants the pointer for looking around.
    pub fn wants_pointer_grab(&self) -> bool {
        self.player.enabled && self.player.grab_mouse
    }

    /// The script taking, or handing back, the mouse.
    pub fn set_pointer_grab(&mut self, grab: bool) {
        self.player.grab_mouse = grab && self.player.enabled;
    }

    /// Pushes the player off the ground, if it is standing on it. A player sitting in a
    /// seat is standing on nothing of its own, so the key does nothing until it gets out.
    pub fn player_jump(&mut self) {
        if self.player.grounded && !self.rides_somewhere() {
            self.player.velocity.z = self.player.jump_strength;
            self.player.grounded = false;
        }
    }

    /// Walks the player by `dt` seconds from whatever the input says, then puts
    /// the camera where that leaves the eyes.
    pub(crate) fn step_player(&mut self, dt: f32) {
        if !self.player.enabled {
            return;
        }
        // A player sitting in a seat has no feet to walk with. Its keys still arrive -
        // a script reads them for the throttle - but the walk maths, the jump and the
        // gravity belong to the car now, and the step puts the eyes on the seat after
        // the solver has moved it.
        if self.rides_somewhere() {
            return;
        }
        let mut input = self.input.clone();
        let mut player = self.player;

        if player.grounded && input.pressed(Key::Space) {
            player.velocity.z = player.jump_strength;
            player.grounded = false;
            input.consume_press(Key::Space);
        }

        let wanted = bool_axis(input.key(Key::W), input.key(Key::S), player.forward())
            + bool_axis(input.key(Key::D), input.key(Key::A), player.right());
        let wanted = wanted.normalize_or_zero() * player.speed;

        // Keys set the target velocity and the body chases it, so releasing a key
        // slides to a stop instead of snapping.
        let chase = 1.0 - (-12.0 * dt).exp();
        player.velocity.x += (wanted.x - player.velocity.x) * chase;
        player.velocity.y += (wanted.y - player.velocity.y) * chase;
        player.velocity.z -= player.gravity_pull * dt;

        player.position += player.velocity * dt;

        if player.position.z <= GROUND_HEIGHT {
            player.position.z = GROUND_HEIGHT;
            if player.velocity.z < 0.0 {
                player.velocity.z = 0.0;
            }
            player.grounded = true;
        } else {
            player.grounded = false;
        }

        self.player = player;
        self.input = input;
        self.follow_camera();
    }

    /// Whether the player's body is seated in something, which is the only way a
    /// player stops being a walker.
    pub(crate) fn rides_somewhere(&self) -> bool {
        self.player.body.is_some_and(|body| self.rides_a_seat(body))
    }

    /// The place the seat under the player has in the world, or `None` while the player
    /// stands on its own feet.
    fn seat_place(&self) -> Option<Vec3> {
        let body = self.player.body?;
        let seat = self.seat_of(body).ok().flatten()?;
        Some(self.world_place(seat)?.0)
    }

    /// Which way the vehicle the player rides points along the ground. A player walking,
    /// or sitting in a seat that drives nothing, has no answer for a chase camera.
    fn ridden_nose(&self) -> Option<Vec3> {
        let body = self.player.body?;
        let seat = self.seat_of(body).ok().flatten()?;
        let car = self.seat_vehicle(seat).ok().flatten()?;
        let (_, turning) = self.world_place(car)?;
        crate::vehicle::ground_direction(turning * glam::Vec3::X)
    }

    /// Puts the player's feet on the seat its body rides in, after the solver has moved
    /// the vehicle that seat hangs on. From here the eyes, the drawn body and the
    /// camera all read the car's place rather than the ground it last walked on.
    pub(crate) fn ride_seat(&mut self) {
        let Some(at) = self.seat_place() else {
            return;
        };
        let mut player = self.player;
        player.position = at;
        player.velocity = Vec3::ZERO;
        // The seat holds them up: a rider is not falling, and the key that steps a
        // player off a kerb does not throw them out of a car.
        player.grounded = true;
        self.player = player;
    }

    /// Keeps the camera on the player: in the eyes, or behind the body. The view is
    /// the only thing a quake is allowed to move without moving the body, so the
    /// shaking is added here and never to `player.position` — the collider walks the
    /// floor while the eyes rattle above it.
    ///
    /// Only while the player is being driven: a script that has put the camera
    /// somewhere itself owns that camera, and a step has no business taking it back.
    pub(crate) fn follow_camera(&mut self) {
        if !self.player.enabled {
            return;
        }
        let player = self.player;
        let look = player.look();
        let eyes = player.position + UP * player.eye_height + self.shake;

        match player.mode {
            PlayerMode::FirstPerson => {
                self.set_camera_position(eyes);
                self.set_camera_target(eyes + self.swing(look));
            }
            PlayerMode::ThirdPerson => {
                let head = player.position + UP * (player.eye_height * 0.9);
                // Ridden by a seat, the camera keeps behind the chassis rather than
                // behind the way the driver's mouse happens to point; on foot, it chases
                // the player's own view.
                let line = self.ridden_nose().unwrap_or(look);
                let mut back = head - line * player.distance + self.shake;
                // A camera under the floor shows the world from inside it.
                back.z = back.z.max(GROUND_HEIGHT + 0.1);
                self.set_camera_position(back);
                self.set_camera_target(head);
            }
        }
    }

    /// Where the eyes actually point while the ground is moving: the same jitter
    /// that displaces them leans the direction they look, along the view's own
    /// right and up so a shove east swings the frame east however the player is
    /// facing. A third person camera needs none of this - it slides around a body
    /// that stays put, and the parallax turns the view for it.
    fn swing(&self, look: Vec3) -> Vec3 {
        let right = look.cross(UP).try_normalize().unwrap_or(Vec3::ZERO);
        let up = right.cross(look);
        let sideways = (right * self.shake.dot(right) + up * self.shake.dot(up))
            * crate::disaster::CAMERA_SWING_PER_METER;
        let worst = crate::disaster::MAX_CAMERA_SWING;
        let turned = look
            + if sideways.length() > worst {
                sideways * (worst / sideways.length())
            } else {
                sideways
            };
        turned.try_normalize().unwrap_or(look)
    }

    /// Moves the attached body with the player: bottom face on the player's feet,
    /// turned to face the way the player looks. A body that has been destroyed is
    /// simply forgotten.
    pub(crate) fn place_body(&mut self) {
        let Some(body) = self.player.body else {
            return;
        };
        if self.entity(body).is_err() {
            self.player.body = None;
            return;
        }
        // A body seated in a seat belongs to the seat: the welding put it there this
        // step, and standing it back on the floor would pull it out of the car.
        if self.rides_a_seat(body) {
            return;
        }

        let size = self.vec_field(body, VecField::Size).unwrap_or(Vec3::ONE);
        let bottom = self
            .mesh(body)
            .ok()
            .and_then(|mesh| mesh.bounds())
            .map(|(min, _)| min.z)
            .unwrap_or(0.0);
        let position = Vec3::new(
            self.player.position.x,
            self.player.position.y,
            self.player.position.z - bottom * size.z,
        );
        let _ = self.set_vec_field(body, VecField::Position, position);
        let _ = self.set_rotation(
            body,
            glam::Quat::from_rotation_z(self.player.yaw.to_radians()),
        );
    }
}

/// `+axis` while the first key is held, `-axis` while the second is.
fn bool_axis(forward: bool, back: bool, direction: Vec3) -> Vec3 {
    let ahead = if forward { direction } else { Vec3::ZERO };
    let behind = if back { -direction } else { Vec3::ZERO };
    ahead + behind
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::FrameInput;

    fn walking(mut scene: Scene) -> Scene {
        scene.set_player_enabled(true);
        scene.set_player_mode(PlayerMode::ThirdPerson);
        scene
    }

    #[test]
    fn a_player_starts_still_and_harmless() {
        let scene = Scene::new();
        let player = scene.player();

        assert!(!player.enabled);
        assert_eq!(player.mode, PlayerMode::FirstPerson);
        assert_eq!(player.position, Vec3::ZERO);
        assert!(player.grounded);
        assert_eq!(player.speed, WALK_SPEED);
        // Nothing grabs the pointer until a script asks for the player.
        assert!(!scene.wants_pointer_grab());
    }

    #[test]
    fn holding_w_walks_forward_at_the_set_speed() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        scene.set_player_speed(6.0);

        for _ in 0..60 {
            scene.update_input(&FrameInput {
                keys: vec![(Key::W, true)],
                ..Default::default()
            });
            scene.step(1.0 / 60.0);
        }

        let position = scene.player_position();
        assert!(
            (5.4..=6.05).contains(&position.x),
            "one second at six metres a second, less the short ramp up to speed: {position}"
        );
        assert!(position.y.abs() < 0.01);
        assert_eq!(position.z, GROUND_HEIGHT);
        assert!(scene.player_is_grounded());
    }

    #[test]
    fn turning_left_points_the_walk_direction_left() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        scene.set_player_speed(6.0);
        scene.set_player_yaw(90.0);

        for _ in 0..60 {
            scene.update_input(&FrameInput {
                keys: vec![(Key::W, true)],
                ..Default::default()
            });
            scene.step(1.0 / 60.0);
        }

        let position = scene.player_position();
        assert!(
            position.x.abs() < 0.3,
            "forward is +y at 90 degrees: {position}"
        );
        assert!((position.y - 6.0).abs() < 0.6);
    }

    #[test]
    fn the_mouse_turns_the_player_and_the_pitch_refuses_to_flip() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        scene.set_player_sensitivity(0.2);

        scene.update_input(&FrameInput {
            mouse_delta: Vec2::new(50.0, 0.0),
            ..Default::default()
        });
        // Ten degrees off a yaw of zero, and yaw keeps counting up from there.
        assert_eq!(scene.player_yaw(), 350.0, "mouse right turns right");

        for _ in 0..20 {
            scene.update_input(&FrameInput {
                mouse_delta: Vec2::new(0.0, -100.0),
                ..Default::default()
            });
        }
        assert_eq!(scene.player_pitch(), PITCH_LIMIT);

        for _ in 0..40 {
            scene.update_input(&FrameInput {
                mouse_delta: Vec2::new(0.0, 100.0),
                ..Default::default()
            });
        }
        assert_eq!(scene.player_pitch(), -PITCH_LIMIT);
    }

    #[test]
    fn a_disabled_player_ignores_the_keyboard_and_the_mouse() {
        let mut scene = Scene::new();
        scene.set_player_enabled(false);

        scene.update_input(&FrameInput {
            keys: vec![(Key::W, true)],
            mouse_delta: Vec2::new(100.0, 0.0),
            ..Default::default()
        });
        scene.step(1.0);

        assert_eq!(scene.player_position(), Vec3::ZERO);
        assert_eq!(scene.player_yaw(), 0.0);
    }

    #[test]
    fn space_jumps_and_gravity_brings_the_player_back_down() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        scene.set_player_jump_strength(5.0);

        scene.update_input(&FrameInput {
            keys: vec![(Key::Space, true)],
            ..Default::default()
        });
        scene.step(0.1);
        assert!(!scene.player_is_grounded());
        assert!(scene.player_position().z > 0.3, "one tenth of a jump");

        for _ in 0..120 {
            scene.step(1.0 / 60.0);
        }
        assert_eq!(scene.player_position().z, GROUND_HEIGHT);
        assert!(scene.player_is_grounded());
    }

    #[test]
    fn a_jump_only_launches_from_the_ground() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        scene.set_player_position(Vec3::new(0.0, 0.0, 4.0));

        scene.update_input(&FrameInput {
            keys: vec![(Key::Space, true)],
            ..Default::default()
        });
        scene.step(0.1);
        assert!(
            scene.player().velocity.z <= 0.0,
            "mid-air, so no second jump"
        );
    }

    #[test]
    fn the_camera_sits_in_the_eyes_or_behind_the_body() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        scene.set_player_eye_height(1.6);

        scene.step(0.0);
        let first = scene.camera();
        assert_eq!(first.position, Vec3::new(0.0, 0.0, 1.6));
        assert!((first.target - first.position).length() - 1.0 < 1e-4);

        scene.set_player_mode(PlayerMode::ThirdPerson);
        scene.set_player_distance(4.0);
        scene.step(0.0);
        let third = scene.camera();
        // Looking along +x, so the camera parks four metres back at the height of
        // the head it is following.
        let near = |a: Vec3, b: Vec3| (a - b).length() < 1e-4;
        assert!(
            near(third.position, Vec3::new(-4.0, 0.0, 1.44)),
            "{}",
            third.position
        );
        assert!(near(third.target, Vec3::new(0.0, 0.0, 1.44)));
    }

    #[test]
    fn a_camera_a_script_owns_survives_a_step() {
        let mut scene = Scene::new();
        scene.set_camera_position(Vec3::new(0.0, -4.0, 1.0));
        scene.set_camera_target(Vec3::ZERO);

        for _ in 0..10 {
            scene.step(1.0 / 60.0);
        }

        // No player is being driven, so there are no eyes to put the lens back on,
        // and the framing the script wrote is still the framing it asked for.
        assert_eq!(scene.camera().position, Vec3::new(0.0, -4.0, 1.0));
        assert_eq!(scene.camera().target, Vec3::ZERO);
    }

    #[test]
    fn a_third_person_camera_never_dives_under_the_floor() {
        let mut scene = walking(Scene::new());
        scene.set_player_pitch(80.0);
        scene.step(0.0);
        assert!(scene.camera().position.z > GROUND_HEIGHT);
    }

    #[test]
    fn the_body_is_dropped_onto_the_player_and_turned_with_it() {
        let mut scene = Scene::new();
        let body = scene.create("cube", Some("Body".into()), None).unwrap();
        scene
            .set_vec_field(body, VecField::Size, Vec3::new(1.0, 1.0, 2.0))
            .unwrap();
        scene.set_player_body(Some(body)).unwrap();
        scene.set_player_enabled(true);
        scene.set_player_yaw(90.0);

        scene.step(0.0);
        let placed = scene.vec_field(body, VecField::Position).unwrap();
        assert_eq!(
            placed,
            Vec3::new(0.0, 0.0, 1.0),
            "a two metre cube stands on its bottom face"
        );
        let turned = scene.vec_field(body, VecField::RotationEuler).unwrap();
        assert!(
            (turned.z - 90.0).abs() < 1e-3,
            "the body faces the way the player looks: {turned}"
        );
    }

    #[test]
    fn a_first_person_body_wears_itself_out_of_sight() {
        let mut scene = Scene::new();
        let body = scene.create("cube", None, None).unwrap();
        scene.set_player_body(Some(body)).unwrap();

        // Nothing is hidden while no one is driving the camera.
        assert!(scene.is_visible(body).unwrap());

        scene.set_player_enabled(true);
        assert!(
            !scene.is_visible(body).unwrap(),
            "the eyes are inside the cube"
        );

        scene.set_player_mode(PlayerMode::ThirdPerson);
        assert!(scene.is_visible(body).unwrap(), "the camera is behind it");

        scene.set_player_enabled(false);
        assert!(scene.is_visible(body).unwrap(), "an ordinary object again");
    }

    #[test]
    fn a_destroyed_body_is_forgotten_rather_than_written_to() {
        let mut scene = Scene::new();
        let body = scene.create("cube", None, None).unwrap();
        scene.set_player_body(Some(body)).unwrap();
        scene.set_player_enabled(true);
        scene.step(0.0);
        scene.destroy(body).unwrap();

        scene.step(1.0 / 60.0);
        assert_eq!(scene.player_body(), None);
    }

    #[test]
    fn a_ui_element_cannot_be_worn_as_a_body() {
        let mut scene = Scene::new();
        let button = scene.create_ui("square", None, None).unwrap();

        assert!(matches!(
            scene.set_player_body(Some(button)),
            Err(SceneError::NotAUi)
        ));
        assert_eq!(scene.player_body(), None);
    }

    #[test]
    fn escape_gives_the_mouse_back_and_a_click_asks_for_it_again() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        assert!(scene.wants_pointer_grab());

        scene.update_input(&FrameInput {
            keys: vec![(Key::Escape, true)],
            ..Default::default()
        });
        assert!(!scene.wants_pointer_grab());
        assert!(scene.input().pressed(Key::Escape));

        scene.update_input(&FrameInput {
            buttons: vec![(crate::input::MouseButton::Left, true)],
            ..Default::default()
        });
        assert!(scene.wants_pointer_grab());
    }

    #[test]
    fn a_teleport_arrives_at_rest() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        scene.set_player_speed(6.0);

        for _ in 0..60 {
            scene.update_input(&FrameInput {
                keys: vec![(Key::W, true)],
                ..Default::default()
            });
            scene.step(1.0 / 60.0);
        }
        assert!(scene.player_velocity().x > 5.0);

        scene.set_player_position(Vec3::new(0.0, 20.0, 5.0));
        assert_eq!(
            scene.player_velocity(),
            Vec3::ZERO,
            "no momentum carried in"
        );
        assert!(!scene.player_is_grounded(), "left above the floor");

        scene.step(1.0 / 60.0);
        assert!(scene.player_velocity().z < 0.0, "and so gravity takes over");
    }

    #[test]
    fn turning_the_player_off_releases_the_mouse() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        scene.set_player_enabled(false);
        assert!(!scene.wants_pointer_grab());

        scene.set_player_enabled(true);
        assert!(scene.wants_pointer_grab(), "taking over again asks again");
    }

    #[test]
    fn step_survives_a_stall_without_losing_the_player() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        scene.set_player_position(Vec3::new(0.0, 0.0, 20.0));

        scene.step(10.0);
        assert!(scene.player_position().z.is_finite());
    }

    /// A car settled on the floor with a driver's seat, and the player's own body
    /// buckled into it: the world a driver starts in.
    fn driven(mode: PlayerMode) -> (Scene, NodeId, NodeId, NodeId) {
        let mut scene = Scene::new();
        let car = scene.create("vehicle", None, None).unwrap();
        let seat = scene.create("DriverSeat", None, Some(car)).unwrap();
        let body = scene.create("cube", Some("Driver".into()), None).unwrap();
        scene.set_player_body(Some(body)).unwrap();
        scene.set_player_enabled(true);
        scene.set_player_mode(mode);
        // Out of the way of the car while it falls to rest, so the only thing left
        // holding the driver up is the seat.
        scene.set_player_position(Vec3::new(0.0, 30.0, GROUND_HEIGHT));

        for _ in 0..120 {
            scene.step(1.0 / 60.0);
        }
        scene.sit(body, seat).unwrap();
        scene.step(0.0);

        (scene, car, seat, body)
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn a_seated_player_has_no_feet_to_walk_with() {
        let (mut scene, _, seat, body) = driven(PlayerMode::ThirdPerson);
        let in_the_car = scene.world_place(seat).unwrap().0;

        for _ in 0..60 {
            scene.update_input(&FrameInput {
                keys: vec![(Key::W, true)],
                ..Default::default()
            });
            scene.step(1.0 / 60.0);
        }

        assert!(scene.is_seated(body).unwrap(), "still in the car");
        assert!(
            near(scene.player_position(), in_the_car),
            "the eyes never left the seat: {}",
            scene.player_position()
        );
        assert_eq!(scene.player_velocity(), Vec3::ZERO);
        assert!(scene.player_is_grounded(), "a seat holds them up");
    }

    #[test]
    fn a_driven_car_carries_its_driver_along_with_it() {
        let (mut scene, car, seat, body) = driven(PlayerMode::ThirdPerson);
        scene.set_throttle(car, 1.0).unwrap();

        for _ in 0..120 {
            scene.step(1.0 / 60.0);
        }

        let gone = scene.world_place(car).unwrap().0;
        assert!(
            Vec3::new(gone.x, gone.y, 0.0).length() > 5.0,
            "a car with its foot down went nowhere: {gone}"
        );
        let there = scene.world_place(seat).unwrap().0;
        assert!(
            near(scene.player_position(), there),
            "the eyes are on the seat, not on the last kerb they stood at"
        );
        assert!(
            near(scene.world_place(body).unwrap().0, there),
            "and the drawn body is in the car too"
        );
    }

    #[test]
    fn a_rider_takes_up_no_room_the_solver_has_to_push() {
        let (mut scene, car, _, body) = driven(PlayerMode::ThirdPerson);
        let parked_at = scene.world_place(car).unwrap().0;

        for _ in 0..30 {
            scene.step(1.0 / 60.0);
        }

        assert!(scene.touches(body).is_empty(), "a person welded in a seat");
        assert!(
            near(scene.world_place(car).unwrap().0, parked_at),
            "inside its own car is nothing the solver has to squeeze out"
        );

        // The same body standing on the same spot is a person, and a person inside a
        // car is two bodies the solver keeps apart.
        scene.set_player_position(parked_at);
        scene.step(1.0 / 60.0);
        assert!(
            scene.touches(car).contains(&body),
            "on foot, the hull and the chassis overlap: {:?}",
            scene.touches(car)
        );
    }

    #[test]
    fn a_chase_camera_watches_the_car_it_rides_in() {
        let (mut scene, _, _, _) = driven(PlayerMode::ThirdPerson);
        // Looking off to the left. On foot the chase lens would park behind that
        // gaze; ridden, it keeps behind the chassis the driver is in.
        scene.set_player_yaw(90.0);
        scene.step(1.0 / 60.0);

        let head = scene.player_position() + Vec3::Z * (scene.player_eye_height() * 0.9);
        let back = head - Vec3::X * scene.player_distance();
        let camera = scene.camera();
        assert!(
            near(camera.position, back),
            "the car faces +x, so the camera should too: {} vs {back}",
            camera.position
        );
        assert!(near(camera.target, head));
    }

    #[test]
    fn the_jump_key_leaves_a_rider_in_its_seat() {
        let (mut scene, _, _, body) = driven(PlayerMode::ThirdPerson);

        scene.update_input(&FrameInput {
            keys: vec![(Key::Space, true)],
            ..Default::default()
        });
        scene.step(1.0 / 60.0);
        scene.player_jump();

        assert!(
            scene.player().velocity == Vec3::ZERO,
            "a seat, not a launch: {:?}",
            scene.player().velocity
        );
        assert!(scene.player_is_grounded());
        assert!(scene.is_seated(body).unwrap());
    }

    #[test]
    fn a_teleport_sets_a_rider_back_on_its_feet() {
        let (mut scene, _, seat, body) = driven(PlayerMode::ThirdPerson);

        scene.set_player_position(Vec3::new(0.0, 12.0, GROUND_HEIGHT));

        assert!(
            !scene.is_seated(body).unwrap(),
            "being told where to stand is how you get out"
        );
        assert_eq!(scene.seated_in(seat).unwrap(), None, "and the seat is free");

        for _ in 0..60 {
            scene.update_input(&FrameInput {
                keys: vec![(Key::W, true)],
                ..Default::default()
            });
            scene.step(1.0 / 60.0);
        }
        assert!(
            scene.player_position().x > 3.0,
            "the feet work again: {}",
            scene.player_position()
        );
    }
}
