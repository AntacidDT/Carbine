//! Disasters: an acceleration the whole world is asked about, once a step.
//!
//! A disaster is not a body. It has no collider, no position of its own in the
//! tree, and nothing to destroy by accident. It is a rule that says how hard, and
//! in which direction, the world is shoved at any given metre - which makes it the
//! same kind of thing as `gravityPull`, except turned on for a while and shaped
//! like weather. So it runs inside [`Scene::step`], where every other movement in
//! the engine is decided, and a script channel and an editor channel get the same
//! shaking without either of them writing maths.
//!
//! The one number a script sets is `magnitude`, and it behaves the way a news
//! report says it does: every step up the scale doubles the shove.

use glam::Vec3;

use crate::motion::Motion;
use crate::node::{NodeId, Scene, SceneError};

/// The shove a magnitude-5 event lays on the world, in meters per second squared.
/// A 6 is twice this, a 4 half of it, which is the doubling every scale that calls
/// itself magnitude means.
const BASE_ACCEL: f32 = 3.0;

/// The magnitude `BASE_ACCEL` belongs to.
const REFERENCE_MAGNITUDE: f32 = 5.0;

/// A magnitude is only ever asked for between these. Past ten the world is not
/// being shaken, it is being deleted, and the solver's step limit stops meaning
/// anything.
const MAX_MAGNITUDE: f32 = 10.0;

/// How many meters of camera jitter one meter per second squared of ground shake
/// is worth. A magnitude 6.8 quake shoves about eleven of those, so the view leaves
/// its post by roughly this much times eleven - enough to read as a rattle at eye
/// height rather than a drift, and the cap below keeps it short enough to see the
/// world through.
const CAMERA_SHAKE_PER_ACCEL: f32 = 0.012;

/// The most the view ever leaves the player's eyes, in meters. Past this a quake
/// is a strobe light rather than an earthquake.
const MAX_CAMERA_SHAKE: f32 = 0.35;

/// How many radians the first person view turns aside for every meter of that
/// jitter. Sliding the eyes a hand's width in the air barely moves the horizon,
/// which is why a shake alone reads as almost nothing; turning the direction they
/// point is what makes the same quake swing the whole frame.
pub(crate) const CAMERA_SWING_PER_METER: f32 = 0.5;

/// The most the view ever turns aside, in radians. A nine landed under the feet
/// would otherwise throw the horizon five degrees each way every few frames, and
/// that is a seizure rather than an earthquake.
pub(crate) const MAX_CAMERA_SWING: f32 = 0.09;

/// The kinds a script can ask for by name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DisasterKind {
    /// The ground jitters sideways and up and down, least where you are farthest
    /// from the epicentre.
    Earthquake,
    /// One way, with gusts, whatever `direction` says.
    Wind,
    /// Drawn in towards a centre and spun around it, and thrown up inside the
    /// funnel that reaches it.
    Tornado,
    /// Outward from one point, once, then over.
    Blast,
}

impl DisasterKind {
    /// The kinds a script can ask for, in the order they are listed when one is
    /// wrong. The shorter names a weather report would use, like `quake`, are
    /// accepted by `from_name` but not advertised: one name per kind is enough
    /// for a list, and a guess that works should not look like a fifth kind.
    pub const NAMES: &'static [&'static str] = &["blast", "earthquake", "tornado", "wind"];

    pub fn from_name(name: &str) -> Option<DisasterKind> {
        Some(match name.trim().to_lowercase().as_str() {
            "blast" | "explosion" => Self::Blast,
            "earthquake" | "quake" => Self::Earthquake,
            "tornado" | "twister" => Self::Tornado,
            "wind" | "windstorm" | "gale" => Self::Wind,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Blast => "blast",
            Self::Earthquake => "earthquake",
            Self::Tornado => "tornado",
            Self::Wind => "wind",
        }
    }

    /// How long one of these is worth by default, in seconds. Zero is a disaster
    /// that keeps going until a script stops it, which is what a stuck fan and a
    /// standing storm both are.
    fn default_duration(self) -> f32 {
        match self {
            Self::Blast => 0.4,
            Self::Earthquake => 8.0,
            Self::Tornado | Self::Wind => 0.0,
        }
    }
}

/// One disaster, as a script set it and as the solver last ran it.
#[derive(Clone, Debug)]
pub struct Disaster {
    id: u64,
    name: String,
    kind: DisasterKind,
    magnitude: f32,
    /// Seconds of it, or zero to keep going until stopped.
    duration: f32,
    /// How far it has run, which is also the clock its shaking is read from.
    elapsed: f32,
    /// Where it is worst, in meters. Overridden by `node`.
    epicentre: Vec3,
    /// How many meters away it stops being felt. Zero is the whole world.
    reach: f32,
    /// Which way a wind blows. Read back as given, normalised or not.
    direction: Vec3,
    /// The object this one rides: the centre is that node's place in the world, so
    /// a funnel bolted to a fan follows the fan.
    node: Option<NodeId>,
    running: bool,
}

impl Disaster {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn kind(&self) -> DisasterKind {
        self.kind
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn magnitude(&self) -> f32 {
        self.magnitude
    }

    pub fn duration(&self) -> f32 {
        self.duration
    }

    /// Seconds of it the solver has run. Reading it is how a script finds out how
    /// far into a storm it is standing.
    pub fn elapsed(&self) -> f32 {
        self.elapsed
    }

    pub fn epicentre(&self) -> Vec3 {
        self.epicentre
    }

    pub fn reach(&self) -> f32 {
        self.reach
    }

    pub fn direction(&self) -> Vec3 {
        self.direction
    }

    pub fn node(&self) -> Option<NodeId> {
        self.node
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Meters per second squared this event is worth at its centre.
    pub fn acceleration(&self) -> f32 {
        (BASE_ACCEL * 2.0f32.powf(self.magnitude - REFERENCE_MAGNITUDE)).max(0.0)
    }

    /// 1 at the centre, 0 at `reach` meters away, and 1 everywhere when the reach
    /// is zero. Linear, because a script can see what it is being told: half the
    /// distance left is half the shaking.
    fn weight(&self, centre: Vec3, at: Vec3, flat: bool) -> f32 {
        if self.reach <= 0.0 {
            return 1.0;
        }
        let away = if flat {
            Vec3::new(at.x - centre.x, at.y - centre.y, 0.0).length()
        } else {
            (at - centre).length()
        };
        (1.0 - away / self.reach).clamp(0.0, 1.0)
    }

    /// How hard, and which way, this disaster shoves at one point in the world.
    /// `centre` is where it sits, which is its epicentre or wherever the object it
    /// rides has got to.
    pub fn acceleration_at(&self, centre: Vec3, at: Vec3) -> Vec3 {
        let clock = self.elapsed;
        match self.kind {
            DisasterKind::Earthquake => {
                let shove = self.acceleration() * self.weight(centre, at, false);
                if shove <= 0.0 {
                    return Vec3::ZERO;
                }
                // Two waves that do not divide evenly, so the ground never settles
                // into a rhythm a player can read as a metronome.
                let beat = clock * (0.5 + 0.5 * self.magnitude.max(0.0)) * std::f32::consts::TAU;
                Vec3::new(
                    (beat * 0.8 + 1.1).sin(),
                    beat.sin(),
                    0.3 * (beat * 1.7).sin(),
                ) * shove
            }
            DisasterKind::Wind => {
                let shove = self.acceleration() * self.weight(centre, at, false);
                let gust = 0.75 + 0.25 * (clock * std::f32::consts::TAU).sin();
                self.direction.normalize_or_zero() * shove * gust
            }
            DisasterKind::Tornado => {
                // A funnel stands up, so how far you are from it is measured on the
                // ground rather than through the air.
                let shove = self.acceleration() * self.weight(centre, at, true);
                if shove <= 0.0 {
                    return Vec3::ZERO;
                }
                let towards = Vec3::new(centre.x - at.x, centre.y - at.y, 0.0);
                let away = towards.length();
                let inward = towards.normalize_or_zero();
                let around = Vec3::new(-inward.y, inward.x, 0.0);
                let funnel = if self.reach > 0.0 {
                    (self.reach * 0.25).max(0.5)
                } else {
                    1.0
                };
                // Inside the funnel the updraft beats the inward pull, which is what
                // throws whatever touches it up the tube.
                let lift = (1.0 - away / funnel).clamp(0.0, 1.0) * shove * 1.8;
                inward * shove * 0.7 + around * shove + Vec3::Z * lift
            }
            DisasterKind::Blast => {
                let shove = self.acceleration() * self.weight(centre, at, false);
                if shove <= 0.0 {
                    return Vec3::ZERO;
                }
                let outward = (at - centre).normalize_or_zero();
                // A blast that starts exactly on top of a body has no direction of
                // its own to throw it by, so it goes up.
                let outward = if outward.length_squared() > 0.0 {
                    outward
                } else {
                    Vec3::Z
                };
                (outward + Vec3::Z * 0.4).normalize_or_zero() * shove
            }
        }
    }
}

impl Scene {
    /// Writes a disaster into the world, switched off. `start` is what turns it on.
    pub fn create_disaster(&mut self, kind: &str, name: Option<String>) -> Result<u64, SceneError> {
        let kind = DisasterKind::from_name(kind)
            .ok_or_else(|| SceneError::UnknownDisaster(kind.to_owned()))?;
        let id = self.next_disaster;
        self.next_disaster += 1;
        self.disasters.push(Disaster {
            id,
            name: name.unwrap_or_else(|| kind.name().to_owned()),
            kind,
            magnitude: REFERENCE_MAGNITUDE,
            duration: kind.default_duration(),
            elapsed: 0.0,
            epicentre: Vec3::ZERO,
            reach: 0.0,
            direction: Vec3::X,
            node: None,
            running: false,
        });
        Ok(id)
    }

    /// A disaster as it stands, or `NoSuchDisaster` once it has been removed.
    pub fn disaster(&self, id: u64) -> Result<Disaster, SceneError> {
        let index = self.disaster_index(id)?;
        Ok(self.disasters[index].clone())
    }

    /// Every disaster written into the world, running or not.
    pub fn disaster_ids(&self) -> Vec<u64> {
        self.disasters.iter().map(|event| event.id).collect()
    }

    /// The ones currently shaking the world.
    pub fn running_disasters(&self) -> Vec<u64> {
        self.disasters
            .iter()
            .filter(|event| event.running)
            .map(|event| event.id)
            .collect()
    }

    /// Takes a disaster out of the world. What it already shoved stays shoved: this
    /// removes the rule, not the weather's mark on the scene.
    pub fn remove_disaster(&mut self, id: u64) -> Result<(), SceneError> {
        let index = self.disaster_index(id)?;
        self.disasters.remove(index);
        Ok(())
    }

    /// Sets it going. A disaster that already spent its length is wound back to the
    /// start, because a hand that switches a finished shock on again means a second
    /// shock and not a frame of nothing. One that is merely paused keeps its clock,
    /// so the shaking does not jump phase under a key that is tapped.
    pub fn start_disaster(&mut self, id: u64) -> Result<(), SceneError> {
        let event = self.disaster_mut(id)?;
        if event.duration > 0.0 && event.elapsed >= event.duration {
            event.elapsed = 0.0;
        }
        event.running = true;
        Ok(())
    }

    pub fn stop_disaster(&mut self, id: u64) -> Result<(), SceneError> {
        self.disaster_mut(id)?.running = false;
        Ok(())
    }

    /// Stops every disaster at once, which is what a test and an editor's panic
    /// button both want.
    pub fn stop_all_disasters(&mut self) {
        for event in &mut self.disasters {
            event.running = false;
        }
        self.shake = Vec3::ZERO;
    }

    /// Takes every disaster out of the world, stopped or not. A script that cleared
    /// its scene does not want the weather it wrote still waiting to be restarted.
    pub fn remove_all_disasters(&mut self) {
        self.disasters.clear();
        self.shake = Vec3::ZERO;
    }

    pub fn set_disaster_magnitude(&mut self, id: u64, magnitude: f32) -> Result<(), SceneError> {
        self.disaster_mut(id)?.magnitude = magnitude.clamp(0.0, MAX_MAGNITUDE);
        Ok(())
    }

    /// Seconds of it. Zero keeps going until `stop_disaster`, and a negative value
    /// is read as zero.
    pub fn set_disaster_duration(&mut self, id: u64, duration: f32) -> Result<(), SceneError> {
        self.disaster_mut(id)?.duration = duration.max(0.0);
        Ok(())
    }

    pub fn set_disaster_epicentre(&mut self, id: u64, at: Vec3) -> Result<(), SceneError> {
        self.disaster_mut(id)?.epicentre = at;
        Ok(())
    }

    /// Meters before it stops being felt. Zero, which is what an event starts with,
    /// means the whole world.
    pub fn set_disaster_reach(&mut self, id: u64, reach: f32) -> Result<(), SceneError> {
        self.disaster_mut(id)?.reach = reach.max(0.0);
        Ok(())
    }

    pub fn set_disaster_direction(&mut self, id: u64, direction: Vec3) -> Result<(), SceneError> {
        self.disaster_mut(id)?.direction = direction;
        Ok(())
    }

    /// Bolts the disaster to an object, so its centre is that node's place in the
    /// world and it follows the node about. `None` puts the epicentre back.
    pub fn set_disaster_node(&mut self, id: u64, node: Option<NodeId>) -> Result<(), SceneError> {
        if let Some(node) = node {
            self.entity(node)?;
        }
        self.disaster_mut(id)?.node = node;
        Ok(())
    }

    pub fn set_disaster_name(&mut self, id: u64, name: String) -> Result<(), SceneError> {
        self.disaster_mut(id)?.name = name;
        Ok(())
    }

    /// Where the view is allowed to be away from the player's eyes this frame, in
    /// meters. The follow camera adds it, so a quake is seen without being walked.
    pub fn camera_shake(&self) -> Vec3 {
        self.shake
    }

    /// Asks every disaster how hard it shoves at one point in the world.
    pub fn acceleration_at(&self, at: Vec3) -> Vec3 {
        let mut push = Vec3::ZERO;
        for event in self.disasters.iter().filter(|event| event.running) {
            push += event.acceleration_at(self.disaster_centre(event), at);
        }
        push
    }

    /// Asks one disaster the same question, from wherever it actually sits, and
    /// answers zero while it is switched off: a disaster that is not happening is
    /// not shoving anything.
    pub fn disaster_acceleration_at(&self, id: u64, at: Vec3) -> Result<Vec3, SceneError> {
        let index = self.disaster_index(id)?;
        let event = &self.disasters[index];
        if !event.running {
            return Ok(Vec3::ZERO);
        }
        Ok(event.acceleration_at(self.disaster_centre(event), at))
    }

    /// Runs every disaster for `dt` seconds: the clock moves, an event whose time
    /// is up is done, and what is left shoves the bodies the solver may move and
    /// the player along with them. Runs inside [`Scene::step`], after the walk, so
    /// a gust arrives as a shove on top of a step rather than replacing one.
    pub(crate) fn run_disasters(&mut self, dt: f32) {
        for event in &mut self.disasters {
            if !event.running {
                continue;
            }
            event.elapsed += dt;
            if event.duration > 0.0 && event.elapsed >= event.duration {
                event.running = false;
                event.elapsed = event.duration;
            }
        }

        // Centres once each, so a node walk never has to ask where the fan got to for
        // every body in the world.
        let active: Vec<(Vec3, Disaster)> = self
            .disasters
            .iter()
            .filter(|event| event.running)
            .map(|event| (self.disaster_centre(event), event.clone()))
            .collect();
        if active.is_empty() {
            self.shake = Vec3::ZERO;
            return;
        }

        // A body the solver is allowed to move and the player are shoved by the same
        // rule, at the world place the solver works in.
        for id in self.node_tree() {
            if !self.is_dynamic(id).unwrap_or(false) {
                continue;
            }
            let Some(at) = self.world_position(id) else {
                continue;
            };
            let shove = shove_at(&active, at);
            if shove.length_squared() <= 0.0 {
                continue;
            }
            let Ok(mut motion) = self.component_mut::<Motion>(id) else {
                continue;
            };
            motion.velocity += shove * dt;
        }

        if self.player.enabled {
            let mut player = self.player;
            player.velocity += shove_at(&active, player.position) * dt;
            self.player = player;
        }

        self.shake = self.quake_shake(&active);
    }

    fn disaster_centre(&self, event: &Disaster) -> Vec3 {
        match event.node {
            Some(node) => self.world_position(node).unwrap_or(event.epicentre),
            None => event.epicentre,
        }
    }

    /// Where a node actually stands, through every parent above it. The solver works
    /// in these meters, so a disaster asks where a body is rather than where its own
    /// `position` claims it is.
    fn world_position(&self, id: NodeId) -> Option<Vec3> {
        self.global_matrix(id)
            .ok()
            .map(|model| model.w_axis.truncate())
    }

    /// How far the view jumps this frame. Only an earthquake moves the camera: wind
    /// and a funnel shove the body you are standing in, which the eyes feel already.
    fn quake_shake(&self, active: &[(Vec3, Disaster)]) -> Vec3 {
        let eyes = self.eyes();
        let mut offset = Vec3::ZERO;
        for (centre, event) in active
            .iter()
            .filter(|(_, event)| event.kind == DisasterKind::Earthquake)
        {
            offset += event.acceleration_at(*centre, eyes) * CAMERA_SHAKE_PER_ACCEL;
        }
        let length = offset.length();
        if length > MAX_CAMERA_SHAKE {
            offset *= MAX_CAMERA_SHAKE / length;
        }
        offset
    }

    fn disaster_index(&self, id: u64) -> Result<usize, SceneError> {
        self.disasters
            .iter()
            .position(|event| event.id == id)
            .ok_or(SceneError::NoSuchDisaster)
    }

    fn disaster_mut(&mut self, id: u64) -> Result<&mut Disaster, SceneError> {
        let index = self.disaster_index(id)?;
        Ok(&mut self.disasters[index])
    }
}

/// What the running disasters shove at one point, each from its own centre.
fn shove_at(active: &[(Vec3, Disaster)], at: Vec3) -> Vec3 {
    active
        .iter()
        .map(|(centre, event)| event.acceleration_at(*centre, at))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::VecField;
    use crate::player::GROUND_HEIGHT;

    /// A weather event the world has been told about but not yet told to feel.
    fn written(scene: &mut Scene, kind: &str) -> u64 {
        scene.create_disaster(kind, None).unwrap()
    }

    /// A body that hangs rather than falls: still dynamic, so a disaster can shove
    /// it, but with no gravity pulling it into the floor while we watch.
    fn loose(scene: &mut Scene, at: Vec3) -> NodeId {
        let id = scene
            .create("cube", Some("Crate".to_owned()), None)
            .unwrap();
        scene.set_vec_field(id, VecField::Position, at).unwrap();
        scene.set_uses_gravity(id, true).unwrap();
        scene.set_gravity_pull(id, 0.0).unwrap();
        id
    }

    fn velocity(scene: &Scene, id: NodeId) -> Vec3 {
        scene.component::<Motion>(id).unwrap().velocity
    }

    fn place(scene: &Scene, id: NodeId) -> Vec3 {
        scene.transform(id).unwrap().position
    }

    #[test]
    fn a_disaster_is_written_switched_off() {
        let mut scene = Scene::new();
        let quake = written(&mut scene, "earthquake");

        let event = scene.disaster(quake).unwrap();
        assert_eq!(event.kind(), DisasterKind::Earthquake);
        assert_eq!(event.name(), "earthquake");
        assert!(
            !event.is_running(),
            "writing weather is not the same as having it"
        );
        assert_eq!(event.magnitude(), REFERENCE_MAGNITUDE);
        assert!(scene.running_disasters().is_empty());
        assert_eq!(scene.disaster_ids(), vec![quake]);

        scene.start_disaster(quake).unwrap();
        assert!(scene.disaster(quake).unwrap().is_running());
        assert_eq!(scene.running_disasters(), vec![quake]);

        scene.stop_disaster(quake).unwrap();
        assert!(!scene.disaster(quake).unwrap().is_running());
    }

    #[test]
    fn each_kind_comes_with_the_time_you_would_expect() {
        let mut scene = Scene::new();
        let blast = written(&mut scene, "blast");
        let quake = written(&mut scene, "earthquake");
        let wind = written(&mut scene, "wind");
        let tornado = written(&mut scene, "tornado");

        let length = |scene: &Scene, id: u64| scene.disaster(id).unwrap().duration();
        assert!(
            length(&scene, blast) > 0.0 && length(&scene, blast) < 1.0,
            "a blast is over at once"
        );
        assert!(length(&scene, quake) > 1.0, "a quake is worth watching");
        assert_eq!(
            (length(&scene, wind), length(&scene, tornado)),
            (0.0, 0.0),
            "a storm keeps going until a script stops it"
        );
    }

    #[test]
    fn one_step_on_the_scale_doubles_the_shove() {
        let mut scene = Scene::new();
        let quake = written(&mut scene, "earthquake");

        for (magnitude, expected) in [(4.0, 1.5), (5.0, 3.0), (6.0, 6.0), (7.0, 12.0)] {
            scene.set_disaster_magnitude(quake, magnitude).unwrap();
            let event = scene.disaster(quake).unwrap();
            assert!(
                (event.acceleration() - expected).abs() < 1e-4,
                "a magnitude {magnitude} event should be worth {expected}: {}",
                event.acceleration()
            );
        }

        scene.set_disaster_magnitude(quake, 99.0).unwrap();
        assert_eq!(scene.disaster(quake).unwrap().magnitude(), MAX_MAGNITUDE);
        scene.set_disaster_magnitude(quake, -3.0).unwrap();
        assert_eq!(scene.disaster(quake).unwrap().magnitude(), 0.0);
    }

    #[test]
    fn a_reach_makes_it_quiet_far_away() {
        let mut scene = Scene::new();
        let gale = written(&mut scene, "wind");
        scene.set_disaster_magnitude(gale, 5.0).unwrap();
        scene.set_disaster_direction(gale, Vec3::X).unwrap();
        scene.set_disaster_reach(gale, 10.0).unwrap();
        scene.start_disaster(gale).unwrap();

        let at = |scene: &Scene, away: f32| scene.acceleration_at(Vec3::new(away, 0.0, 0.0)).x;
        let at_centre = at(&scene, 0.0);
        assert!(at_centre > 0.0, "a wind pointed down +x shoves down +x");
        assert!(
            (at(&scene, 5.0) - at_centre * 0.5).abs() < 1e-4,
            "half the distance left is half the shove"
        );
        assert_eq!(
            at(&scene, 20.0),
            0.0,
            "past the reach it is not felt at all"
        );
    }

    #[test]
    fn an_earthquake_shoves_a_loose_body_but_not_a_pinned_one() {
        let mut scene = Scene::new();
        let crate_id = loose(&mut scene, Vec3::new(0.0, 0.0, 5.0));
        let wall = scene.create("cube", Some("Wall".to_owned()), None).unwrap();
        scene
            .set_vec_field(wall, VecField::Position, Vec3::new(0.0, 0.0, 5.0))
            .unwrap();

        let quake = written(&mut scene, "earthquake");
        scene.set_disaster_magnitude(quake, 7.0).unwrap();
        scene
            .set_disaster_epicentre(quake, Vec3::new(0.0, 0.0, 5.0))
            .unwrap();
        scene.start_disaster(quake).unwrap();

        scene.step(1.0 / 60.0);
        assert!(
            velocity(&scene, crate_id).length() > 0.0,
            "the floor moved under the crate"
        );
        assert_eq!(
            velocity(&scene, wall),
            Vec3::ZERO,
            "scenery never asked to be moved and is not"
        );

        // The shove wobbles rather than running away, and the view wobbles with it.
        for _ in 0..60 {
            scene.step(1.0 / 60.0);
        }
        let shaken = place(&scene, crate_id);
        assert!(shaken.length() < 10.0, "a quake rattles: {shaken}");
        let shake = scene.camera_shake();
        assert!(shake.length() > 0.0, "the eyes feel a quake");
        assert!(
            shake.length() <= MAX_CAMERA_SHAKE,
            "but not so much that the world is unreadable: {shake}"
        );
    }

    #[test]
    fn a_quake_turns_the_eyes_and_not_merely_slides_them() {
        // Sliding the head a hand's width in the air barely moves the horizon, so a
        // shake you can only measure on the eyes' position is a shake nobody sees.
        // The first person frame has to turn as well.
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        scene.step(1.0 / 60.0);
        let camera = scene.camera();
        let settled = (camera.target - camera.position).normalize();

        let quake = written(&mut scene, "earthquake");
        scene.set_disaster_magnitude(quake, 7.0).unwrap();
        scene
            .set_disaster_epicentre(quake, Vec3::new(0.0, 0.0, GROUND_HEIGHT))
            .unwrap();
        scene.start_disaster(quake).unwrap();

        let mut farthest = 0.0f32;
        for _ in 0..60 {
            scene.step(1.0 / 60.0);
            let camera = scene.camera();
            let facing = (camera.target - camera.position).normalize();
            farthest = farthest.max(facing.distance(settled));
        }
        assert!(
            farthest > 0.01,
            "the frame itself swings: {farthest} radians off the settled view"
        );
        assert!(
            farthest < 0.2,
            "and still shows a world you can read: {farthest} radians"
        );
    }

    #[test]
    fn a_wind_you_stand_in_drifts_you_without_lifting_you() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        let gale = written(&mut scene, "wind");
        scene.set_disaster_direction(gale, Vec3::X).unwrap();
        scene.set_disaster_magnitude(gale, 6.0).unwrap();
        scene.start_disaster(gale).unwrap();

        for _ in 0..120 {
            scene.step(1.0 / 60.0);
        }
        let walked = scene.player_position();
        assert!(walked.x > 0.1, "a wind you stand in drifts you: {walked}");
        assert_eq!(walked.z, GROUND_HEIGHT, "a horizontal wind never lifts you");
    }

    #[test]
    fn a_funnel_draws_things_in_and_throws_them_up() {
        let mut scene = Scene::new();
        let fan = scene
            .create("sphere", Some("Fan".to_owned()), None)
            .unwrap();
        scene
            .set_vec_field(fan, VecField::Position, Vec3::new(0.0, 0.0, 0.0))
            .unwrap();

        let twister = written(&mut scene, "tornado");
        scene.set_disaster_node(twister, Some(fan)).unwrap();
        scene.set_disaster_magnitude(twister, 7.0).unwrap();
        scene.start_disaster(twister).unwrap();
        assert_eq!(
            scene.disaster(twister).unwrap().node(),
            Some(fan),
            "the funnel is bolted to the fan"
        );

        // Standing half a metre from the fan, inside its funnel: pulled towards it,
        // and up.
        let near = scene.acceleration_at(Vec3::new(0.5, 0.0, 0.0));
        assert!(near.x < 0.0, "tornados do not push you away: {near}");
        assert!(near.z > 0.0, "the tube lifts whatever it reaches: {near}");

        // Move the fan and the weather moves with it.
        scene
            .set_vec_field(fan, VecField::Position, Vec3::new(20.0, 0.0, 0.0))
            .unwrap();
        let beside = scene.acceleration_at(Vec3::new(21.0, 0.0, 0.0));
        assert!(beside.x < 0.0, "the funnel followed the fan: {beside}");
    }

    #[test]
    fn a_funnel_lifts_the_player_off_the_floor() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        let twister = written(&mut scene, "tornado");
        scene.set_disaster_magnitude(twister, 7.0).unwrap();
        scene.start_disaster(twister).unwrap();

        for _ in 0..30 {
            scene.step(1.0 / 60.0);
        }
        assert!(
            scene.player_position().z > GROUND_HEIGHT,
            "a magnitude 7 updraft beats a magnitude 10 world"
        );
    }

    #[test]
    fn an_event_whose_time_is_up_stops_by_itself() {
        let mut scene = Scene::new();
        let blast = written(&mut scene, "blast");
        scene.set_disaster_duration(blast, 0.5).unwrap();
        scene.set_disaster_magnitude(blast, 7.0).unwrap();
        scene.start_disaster(blast).unwrap();

        scene.step(1.0 / 60.0);
        let event = scene.disaster(blast).unwrap();
        assert!(event.is_running());
        assert!(event.elapsed() > 0.0);

        for _ in 0..60 {
            scene.step(1.0 / 60.0);
        }
        let event = scene.disaster(blast).unwrap();
        assert!(!event.is_running(), "a blast does not go again");
        assert_eq!(event.elapsed(), 0.5, "it stops at its length, not past it");
        assert_eq!(scene.acceleration_at(Vec3::new(1.0, 0.0, 0.0)), Vec3::ZERO);
        assert_eq!(scene.camera_shake(), Vec3::ZERO);
    }

    #[test]
    fn switching_a_spent_event_on_again_means_another_one() {
        let mut scene = Scene::new();
        let quake = written(&mut scene, "earthquake");
        scene.set_disaster_duration(quake, 1.0).unwrap();
        scene.set_disaster_magnitude(quake, 7.0).unwrap();
        scene.start_disaster(quake).unwrap();

        for _ in 0..90 {
            scene.step(1.0 / 60.0);
        }
        assert!(!scene.disaster(quake).unwrap().is_running());

        scene.start_disaster(quake).unwrap();
        let event = scene.disaster(quake).unwrap();
        assert!(event.is_running(), "a switched-on shock is a shock");
        assert!(
            event.elapsed() < 0.1,
            "a spent one is wound back, not left at its length"
        );

        let mut loudest = Vec3::ZERO;
        for _ in 0..20 {
            scene.step(1.0 / 60.0);
            let shove = scene.acceleration_at(Vec3::ZERO);
            if shove.length() > loudest.length() {
                loudest = shove;
            }
        }
        assert_ne!(loudest, Vec3::ZERO, "the restarted ground moves again");
    }

    #[test]
    fn an_event_only_paused_keeps_its_clock_when_it_starts_again() {
        let mut scene = Scene::new();
        let quake = written(&mut scene, "earthquake");
        scene.set_disaster_duration(quake, 8.0).unwrap();
        scene.start_disaster(quake).unwrap();

        for _ in 0..30 {
            scene.step(1.0 / 60.0);
        }
        scene.stop_disaster(quake).unwrap();
        let paused_at = scene.disaster(quake).unwrap().elapsed();

        scene.start_disaster(quake).unwrap();
        let event = scene.disaster(quake).unwrap();
        assert!(event.is_running());
        assert_eq!(
            event.elapsed(),
            paused_at,
            "a hand on the switch must not jump the shaking to a new phase"
        );
    }

    #[test]
    fn stopping_everything_at_once_leaves_what_it_already_did() {
        let mut scene = Scene::new();
        let crate_id = loose(&mut scene, Vec3::new(0.0, 0.0, 5.0));
        let quake = written(&mut scene, "earthquake");
        let gale = written(&mut scene, "wind");
        scene.start_disaster(quake).unwrap();
        scene.start_disaster(gale).unwrap();

        scene.step(1.0 / 60.0);
        let moved = velocity(&scene, crate_id);
        assert_ne!(moved, Vec3::ZERO);

        scene.stop_all_disasters();
        assert!(scene.running_disasters().is_empty());
        assert_eq!(scene.camera_shake(), Vec3::ZERO);

        scene.step(1.0 / 60.0);
        assert_eq!(
            velocity(&scene, crate_id),
            moved,
            "the calm takes away the rule, not the crate's speed"
        );
    }

    #[test]
    fn a_weather_report_cannot_ask_for_a_hurricane() {
        let mut scene = Scene::new();
        let error = scene.create_disaster("hurricane", None).unwrap_err();
        assert_eq!(error, SceneError::UnknownDisaster("hurricane".to_owned()));
        let text = error.to_string();
        assert!(text.contains("hurricane"), "{text}");
        assert!(text.contains("earthquake"), "{text}");

        // The short names a script would actually type are read, not refused.
        assert_eq!(
            scene.create_disaster("Quake", None).unwrap(),
            0,
            "case and spare letters are forgiven"
        );
        assert_eq!(scene.disaster(0).unwrap().kind(), DisasterKind::Earthquake);
    }

    #[test]
    fn a_removed_disaster_is_not_the_same_as_a_stopped_one() {
        let mut scene = Scene::new();
        let quake = written(&mut scene, "earthquake");
        let gale = written(&mut scene, "wind");
        scene.start_disaster(quake).unwrap();

        scene.remove_disaster(quake).unwrap();
        assert!(
            matches!(scene.disaster(quake), Err(SceneError::NoSuchDisaster)),
            "it is gone, not resting"
        );
        assert_eq!(scene.disaster_ids(), vec![gale]);
        assert!(scene.running_disasters().is_empty());
        assert_eq!(
            scene.create_disaster("wind", None).unwrap(),
            2,
            "removing one never reuses its number"
        );
    }
}
