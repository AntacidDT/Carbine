//! Vehicles as things standing in the world, and the seats people ride in them from.
//!
//! A vehicle is an object wearing a drive: `carbine.new("Vehicle")` writes one, it is
//! drawn, collides and falls like any other body, and everything about how it handles
//! is a knob written on it. The solver moves it the way it moves a crate, which is why
//! a car can be shoved by a quake and stopped by a wall.
//!
//! A seat is a second thing and hangs on the object it belongs to. A `Seat` is simply
//! somewhere to sit: whoever rides in it rides along and drives nothing. A `DriverSeat`
//! is the seat that drives, and it drives the object directly above it - the person
//! sitting in it lends their hands to the parent's `throttle`, `steer` and `brake`.
//! That is the whole difference between the two, and it comes from the kind word a
//! script typed rather than from a switch a script has to remember to throw.

use bevy_ecs::prelude::*;
use glam::{Mat4, Quat, Vec3};

use crate::motion::Motion;
use crate::node::{
    Children, DEFAULT_COLOR, NodeId, NodeKind, NodeName, NodeTexture, ObjectColor, Opacity, Parent,
    Scene, SceneError, Transform, VecField, Visible,
};
use crate::primitives;

/// The kind word `carbine.new()` takes for a vehicle. It is a box the geometry already
/// knows, so unlike a light it is drawn and collides the way an object does.
pub const KIND: &str = "vehicle";

/// The two kind words for somewhere to sit. Only the second one drives.
pub const SEAT_KIND: &str = "seat";
pub const DRIVER_SEAT_KIND: &str = "driverSeat";

/// The chassis a vehicle is born with, in metres: a hatchback, because that is what
/// someone typing `new("Vehicle")` means. A script that wants a lorry writes `size`.
const CHASSIS: Vec3 = Vec3::new(4.4, 2.0, 1.2);

/// Metres per second a vehicle born without an answer can be driven up to - seventy
/// two kilometres an hour, the speed of a car in town.
const DEFAULT_TOP_SPEED: f32 = 20.0;

/// Degrees per second the nose swings at full lock: a U-turn in a bit over two seconds.
const DEFAULT_TURN_RATE: f32 = 120.0;

/// How fast a drift's sideways speed is bled away. Low enough that a car thrown into a
/// turn keeps sliding for a beat, high enough that it stops sliding eventually.
const DEFAULT_GRIP: f32 = 0.5;

/// Metres per second squared a full throttle gives a car. Reaching the top speed of a
/// hatchback takes a couple of seconds, which is about the feel a script expects.
const ACCELERATION: f32 = 8.0;

/// Metres per second squared at the hardest the brake can be stamped on. A brake
/// outranks an accelerator, the way one foot covers both pedals.
const BRAKING: f32 = 20.0;

/// Per second, how hard a drift car's tyres bite into a slide. The `grip` knob scales
/// it, so a car on ice - grip at nil - keeps a sideways speed indefinitely, and one at
/// full grip loses the bulk of a slide in a second. An arcade car never gets here at all.
const SIDE_RATE: f32 = 2.0;

/// How a vehicle answers the wheel. Chosen per vehicle, the way a light's edge is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandlingType {
    /// The tyres are glued: where the nose points is where the car goes, and any speed
    /// sideways is gone inside a step. A handbrake does nothing but stop the car.
    Arcade,
    /// The tyres have a grip and it can be beaten: the car keeps sliding along its old
    /// line while the nose swings, so pointing it across a turn is a way to turn.
    Drift,
}

impl HandlingType {
    /// The types a script reads from `carbine.handlingTypes()`, in the order listed.
    pub const NAMES: &'static [&'static str] = &["arcade", "drift"];

    /// The type a vehicle is born with. Sliding is something a script has to want.
    pub const DEFAULT: Self = Self::Arcade;

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.trim().to_lowercase().as_str() {
            "arcade" | "grip" | "street" | "normal" => Self::Arcade,
            "drift" | "slide" | "rally" => Self::Drift,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Arcade => "arcade",
            Self::Drift => "drift",
        }
    }
}

/// Which of the two seats a kind word asks for, or `None` for a word that is neither.
/// Spelling is worth as little as a script can give it, so `"Driver's Seat"` is read.
pub fn seat_kind(name: &str) -> Option<bool> {
    let written = name
        .trim()
        .to_lowercase()
        .replace([' ', '-', '_', '\'', '.'], "");
    match written.as_str() {
        "seat" | "passenger" | "passengerseat" | "chair" => Some(false),
        "driverseat" | "driver" | "pilot" | "piloseat" => Some(true),
        _ => None,
    }
}

/// What a vehicle node is, beyond the box it is drawn from.
#[derive(Clone, Copy, Component)]
pub(crate) struct Vehicle {
    handling: HandlingType,
    /// Metres per second a full throttle is chasing.
    top_speed: f32,
    /// Degrees per second the nose turns at full lock.
    turn_rate: f32,
    /// How much of a sideways shove the tyres keep hold of, nil to one. An arcade car
    /// reads it as one grip and never lets the car slide at all.
    grip: f32,
    /// The three things a driver asks for. Each is a wish, not a speed: the drive model
    /// works out from them and from the handling where the car goes.
    throttle: f32,
    steer: f32,
    brake: f32,
}

impl Default for Vehicle {
    fn default() -> Self {
        Self {
            handling: HandlingType::DEFAULT,
            top_speed: DEFAULT_TOP_SPEED,
            turn_rate: DEFAULT_TURN_RATE,
            grip: DEFAULT_GRIP,
            throttle: 0.0,
            steer: 0.0,
            brake: 0.0,
        }
    }
}

impl Vehicle {
    /// Whether anybody is asking this car for something right now. It is the difference
    /// between a vehicle and a crate with wheels: a driven car's tyres decide its ground
    /// speed, and an undriven one is left to the floor's own drag.
    pub(crate) fn being_driven(&self) -> bool {
        self.throttle != 0.0 || self.steer != 0.0 || self.brake != 0.0
    }
}

/// A place to sit: whether it drives, and who is in it.
#[derive(Clone, Copy, Default, Component)]
pub(crate) struct Seat {
    /// Read off the kind word and never written again: `new("DriverSeat")` is true,
    /// `new("Seat")` is false.
    pub(crate) drives: bool,
    /// The node riding here, or `None` for an empty seat.
    pub(crate) seated: Option<NodeId>,
}

impl Seat {
    pub(crate) fn new(drives: bool) -> Self {
        Self {
            drives,
            seated: None,
        }
    }
}

/// The seat a node is riding in. A rider wears this while it is seated, which is how
/// the solver knows to leave it alone: a body in a seat neither falls nor shoves,
/// because the step puts it where the seat is.
#[derive(Clone, Copy, Component)]
pub(crate) struct Seated {
    pub(crate) seat: NodeId,
}

/// Metres per second a rider is thrown clear of a wreck, on top of the speed the
/// wreck itself was going.
const EJECTION_SPEED: f32 = 4.0;

/// How a number a script wrote is kept: a wish between two limits, with anything the
/// maths cannot read thrown away rather than carried into the solver.
fn wish(value: f32, low: f32, high: f32) -> f32 {
    if value.is_finite() {
        value.clamp(low, high)
    } else {
        0.0
    }
}

/// Which way a body's nose points along the ground. A car on a slope still drives along
/// the floor rather than into it, so the tilt is flattened away, and a rotation that says
/// nothing about forwards has no answer to give.
pub(crate) fn ground_direction(facing: Vec3) -> Option<Vec3> {
    let flat = Vec3::new(facing.x, facing.y, 0.0);
    (flat.length_squared() > 1e-6).then(|| flat.normalize())
}

impl Scene {
    /// Creates a vehicle: a box the world draws and collides, wearing a drive.
    pub fn create_vehicle(
        &mut self,
        name: Option<String>,
        parent: Option<NodeId>,
    ) -> Result<NodeId, SceneError> {
        if let Some(parent) = parent {
            self.entity(parent)?;
            self.branch_allows(parent, false)?;
        }

        let id = self.spawn(KIND, name, parent, primitives::cube());
        self.world
            .entity_mut(self.entity(id)?)
            .insert(Vehicle::default());
        // A vehicle is a hatchback before a script says otherwise, and the size is
        // written rather than stored so `mass` and the collider follow it for free.
        self.set_vec_field(id, VecField::Size, CHASSIS)?;
        // A car somebody can drive through is no use to anyone, so a vehicle is in the
        // solver from the first moment; a script that wants a ghost of one can still
        // switch the collider off. Gravity is switched on for the same reason: only a
        // body the solver moves can answer a wheel.
        self.set_collision(id, true)?;
        self.set_uses_gravity(id, true)?;

        Ok(id)
    }

    /// Creates a seat. It draws nothing, the way a lamp does, and hangs on whatever it
    /// is told to - a vehicle it rides in, or a floor it sits on.
    pub fn create_seat(
        &mut self,
        name: Option<String>,
        parent: Option<NodeId>,
        drives: bool,
    ) -> Result<NodeId, SceneError> {
        let parent_id = parent.unwrap_or(self.root());
        self.entity(parent_id)?;
        self.branch_allows(parent_id, false)?;

        let kind = if drives { DRIVER_SEAT_KIND } else { SEAT_KIND };
        let id = self.reserve();
        let entity = self
            .world
            .spawn((
                NodeName(
                    name.unwrap_or_else(|| if drives { "DriverSeat" } else { "Seat" }.to_owned()),
                ),
                NodeKind(kind.to_owned()),
                Transform::default(),
                ObjectColor(DEFAULT_COLOR),
                Opacity::default(),
                NodeTexture::default(),
                Visible::default(),
                Motion::default(),
                Parent(parent_id),
                Children::default(),
                Seat::new(drives),
            ))
            .id();
        self.insert(id, entity);
        self.attach(id, parent_id);

        Ok(id)
    }

    pub fn is_vehicle(&self, id: NodeId) -> Result<bool, SceneError> {
        let entity = self.entity(id)?;
        Ok(self.world.get::<Vehicle>(entity).is_some())
    }

    pub fn is_seat(&self, id: NodeId) -> Result<bool, SceneError> {
        let entity = self.entity(id)?;
        Ok(self.world.get::<Seat>(entity).is_some())
    }

    /// Whether the rider here is the one driving. A read, because it is the kind word
    /// that says so and not a script.
    pub fn seat_drives(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.seat(id)?.drives)
    }

    /// The node riding in this seat, or `None` for an empty one.
    pub fn seated_in(&self, id: NodeId) -> Result<Option<NodeId>, SceneError> {
        Ok(self.seat(id)?.seated)
    }

    /// The vehicle a driver seat drives: the object directly above it, if that object
    /// is a vehicle. A seat on a chair, or a plain seat in a car, drives nothing.
    pub fn seat_vehicle(&self, id: NodeId) -> Result<Option<NodeId>, SceneError> {
        if !self.seat(id)?.drives {
            return Ok(None);
        }
        let Some(parent) = self.parent(id)? else {
            return Ok(None);
        };
        Ok(if self.is_vehicle(parent)? {
            Some(parent)
        } else {
            None
        })
    }

    /// The first driver seat of this vehicle with somebody in it, which is the one
    /// whose hands are on the wheel. A car with two drivers seated still has one
    /// driver: the seat a script made first.
    pub fn vehicle_driver(&self, vehicle: NodeId) -> Result<Option<NodeId>, SceneError> {
        if !self.is_vehicle(vehicle)? {
            return Err(SceneError::NotAVehicle);
        }
        for place in self.children(vehicle)? {
            let Ok(seat) = self.seat(place) else {
                continue;
            };
            if seat.drives && seat.seated.is_some() {
                return Ok(seat.seated);
            }
        }
        Ok(None)
    }

    /// Sits a node in a seat. The rider keeps its own place in the tree - its children,
    /// its name, its own everything - and the step holds it at the seat's place in the
    /// world for as long as it is seated. Nothing is re-parented, so a driver that gets
    /// out is still the node the script made.
    ///
    /// A seat holds one rider and whoever was in it before gets out; a rider holds one
    /// seat and sitting again carries it across.
    pub fn sit(&mut self, rider: NodeId, seat: NodeId) -> Result<(), SceneError> {
        self.entity(rider)?;
        if self.is_seat(rider)? {
            return Err(SceneError::SeatCannotSit);
        }
        let place = self.seat(seat)?;
        if let Some(before) = place.seated.filter(|who| *who != rider) {
            self.stand(before)?;
        }
        if let Some(other) = self.seat_of(rider)?
            && let Ok(mut last) = self.seat_mut(other)
        {
            last.seated = None;
        }
        self.seat_mut(seat)?.seated = Some(rider);
        let entity = self.entity(rider)?;
        self.world.entity_mut(entity).insert(Seated { seat });
        // The rider is at the seat from the moment it is asked to be, not from the next
        // step: a script that sits somebody and looks at them in the same frame sees
        // them in the car.
        self.weld(rider, seat);

        Ok(())
    }

    /// Gets a node out of whatever it was sitting in, and leaves it exactly where the
    /// seat had it. A rider that gets out of a moving car keeps the car's speed for a
    /// moment, the way a body does when it stops being carried.
    ///
    /// A node that was not sitting anywhere is left alone rather than refused: the same
    /// key a script offers for getting in is safe to offer for getting out.
    pub fn stand(&mut self, rider: NodeId) -> Result<(), SceneError> {
        let entity = self.entity(rider)?;
        let Some(where_it_sits) = self.world.get::<Seated>(entity).copied() else {
            return Ok(());
        };
        self.world.entity_mut(entity).remove::<Seated>();
        if let Some(place) = self.entity(where_it_sits.seat).ok()
            && let Some(mut seat) = self.world.get_mut::<Seat>(place)
            && seat.seated == Some(rider)
        {
            seat.seated = None;
        }
        let speed = self.ride_speed(where_it_sits.seat);
        if let Ok(mut motion) = self.component_mut::<Motion>(rider) {
            motion.velocity = speed;
        }

        Ok(())
    }

    /// Whether this node is riding in something.
    pub fn is_seated(&self, rider: NodeId) -> Result<bool, SceneError> {
        Ok(self.seat_of(rider)?.is_some())
    }

    /// The seat this node is riding in, or `None` for a node on its own feet.
    pub fn seat_of(&self, rider: NodeId) -> Result<Option<NodeId>, SceneError> {
        Ok(self
            .component::<Seated>(rider)
            .ok()
            .map(|riding| riding.seat))
    }

    /// Every seat and rider pair that the death of this node strands: the seat itself,
    /// or every seat hanging under the vehicle that died. Riders are stood out with the
    /// wreck's speed and a shove upwards, so a car that dies mid-turn throws its driver
    /// into the world instead of deleting them with it.
    pub(crate) fn eject_riders(&mut self, dying: NodeId) {
        let stranded: Vec<NodeId> = match self.seat(dying) {
            Ok(place) => place.seated.into_iter().collect(),
            Err(_) => self
                .children(dying)
                .unwrap_or_default()
                .iter()
                .filter_map(|place| self.seat(*place).ok())
                .filter_map(|place| place.seated)
                .collect(),
        };
        let speed = stranded
            .first()
            .and_then(|rider| self.seat_of(*rider).ok().flatten())
            .map(|seat| self.ride_speed(seat))
            .unwrap_or(Vec3::ZERO);

        for rider in stranded {
            let _ = self.stand(rider);
            if let Ok(mut motion) = self.component_mut::<Motion>(rider) {
                motion.velocity = speed + Vec3::Z * EJECTION_SPEED;
            }
        }
    }

    /// Where a branch leaves the world: every seat that lost its rider in there is
    /// emptied, and every rider whose seat was destroyed is on its own feet again. Both
    /// halves are needed, because a node can be destroyed as the seat, the vehicle or
    /// the rider, and a dangling link is a body welded to something that no longer is.
    pub(crate) fn release_seats(&mut self) {
        for id in self.node_tree() {
            let Ok(entity) = self.entity(id) else {
                continue;
            };
            if let Some(riding) = self.component::<Seated>(id).ok().copied()
                && !self.ids.contains_key(&riding.seat)
            {
                self.world.entity_mut(entity).remove::<Seated>();
            }
            if let Some(seat) = self.seat(id).ok().and_then(|place| place.seated)
                && !self.ids.contains_key(&seat)
                && let Some(mut place) = self.world.get_mut::<Seat>(entity)
            {
                place.seated = None;
            }
        }
    }

    /// Holds every seated node at the place its seat has in the world. This runs after
    /// the solver has moved everything, so a rider is drawn where its car actually
    /// ended up rather than where it started the step.
    pub(crate) fn place_seats(&mut self) {
        for id in self.node_tree() {
            let Some(rider) = self.seat(id).ok().and_then(|place| place.seated) else {
                continue;
            };
            self.weld(rider, id);
        }
    }

    /// Takes a dying vehicle's foot off everything: a wreck with a throttle still held
    /// down would go on driving itself, and the corpse of a car is scenery. Only a
    /// vehicle is asked, and the wishes are its own rather than a linked group's.
    pub(crate) fn let_go_of_the_wheel(&mut self, id: NodeId) {
        let Ok(mut driven) = self.vehicle_mut(id) else {
            return;
        };
        driven.throttle = 0.0;
        driven.steer = 0.0;
        driven.brake = 0.0;
    }

    fn weld(&mut self, rider: NodeId, seat: NodeId) {
        let Some((at, turning)) = self.bolt_to(rider, seat) else {
            return;
        };
        if self.set_vec_field(rider, VecField::Position, at).is_err() {
            return;
        }
        let _ = self.set_rotation(rider, turning);
    }

    /// The `position` and `rotation` a rider writes to stand exactly where its seat
    /// stands, through whatever the rider's own parents do to it. A chain squashed flat
    /// by a zero scale has no answer, and a rider is left where it is rather than put
    /// somewhere the maths cannot read.
    fn bolt_to(&self, rider: NodeId, seat: NodeId) -> Option<(Vec3, Quat)> {
        let standing = self.global_matrix(seat).ok()?;
        let local = match self.parent(rider).ok().flatten() {
            Some(parent) => self.global_matrix(parent).ok()?.try_inverse()? * standing,
            None => standing,
        };
        standing_place(local)
    }

    /// The speed of the vehicle a seat hangs on. A rider shares it while it sits, since
    /// the solver leaves a seated body's own speed alone.
    fn ride_speed(&self, seat: NodeId) -> Vec3 {
        self.parent(seat)
            .ok()
            .flatten()
            .and_then(|vehicle| self.component::<Motion>(vehicle).ok())
            .map(|motion| motion.velocity)
            .unwrap_or(Vec3::ZERO)
    }

    /// Where a node stands and which way it turns, through every parent above it. The
    /// eyes of a rider and a camera behind a car both ask their seat or chassis this.
    pub(crate) fn world_place(&self, id: NodeId) -> Option<(Vec3, Quat)> {
        standing_place(self.global_matrix(id).ok()?)
    }

    /// How this vehicle answers the wheel.
    pub fn handling(&self, id: NodeId) -> Result<HandlingType, SceneError> {
        Ok(self.vehicle(id)?.handling)
    }

    /// What a vehicle is - its handling, its top speed, the grip of its tyres - is part
    /// of what it is, so a linked copy is written with it.
    pub fn set_handling(&mut self, id: NodeId, handling: HandlingType) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.vehicle_mut(member)?.handling = handling;
            Ok(())
        })
    }

    pub fn set_handling_name(&mut self, id: NodeId, name: &str) -> Result<(), SceneError> {
        let handling = HandlingType::from_name(name)
            .ok_or_else(|| SceneError::UnknownHandling(name.trim().to_owned()))?;
        self.set_handling(id, handling)
    }

    /// Metres per second a full throttle drives this vehicle up to.
    pub fn top_speed(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.vehicle(id)?.top_speed)
    }

    pub fn set_top_speed(&mut self, id: NodeId, speed: f32) -> Result<(), SceneError> {
        let speed = if speed.is_finite() {
            speed.max(0.0)
        } else {
            0.0
        };
        self.spread(id, move |scene, member| {
            scene.vehicle_mut(member)?.top_speed = speed;
            Ok(())
        })
    }

    /// Degrees per second the nose swings with the wheel at full lock.
    pub fn turn_rate(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.vehicle(id)?.turn_rate)
    }

    pub fn set_turn_rate(&mut self, id: NodeId, rate: f32) -> Result<(), SceneError> {
        let rate = if rate.is_finite() { rate.max(0.0) } else { 0.0 };
        self.spread(id, move |scene, member| {
            scene.vehicle_mut(member)?.turn_rate = rate;
            Ok(())
        })
    }

    /// How much of a sideways speed the tyres keep hold of, from nil - ice - to one -
    /// glued. An arcade car slides at none of it whatever this says.
    pub fn grip(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.vehicle(id)?.grip)
    }

    pub fn set_grip(&mut self, id: NodeId, grip: f32) -> Result<(), SceneError> {
        let grip = grip.clamp(0.0, 1.0).max(0.0);
        self.spread(id, move |scene, member| {
            scene.vehicle_mut(member)?.grip = grip;
            Ok(())
        })
    }

    /// How hard the driver is standing on the accelerator, from full reverse to full
    /// throttle. Where a seat's own driver leaves this is the drive model's business;
    /// where a script writes it, it is the script's.
    pub fn throttle(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.vehicle(id)?.throttle)
    }

    /// A wish, not a state: one vehicle in a linked fleet does not decide what the
    /// others are asked to do, so intent is written on one body alone.
    pub fn set_throttle(&mut self, id: NodeId, throttle: f32) -> Result<(), SceneError> {
        let throttle = wish(throttle, -1.0, 1.0);
        self.vehicle_mut(id)?.throttle = throttle;
        Ok(())
    }

    /// Where the wheel is turned, from fully left to fully right.
    pub fn steer(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.vehicle(id)?.steer)
    }

    pub fn set_steer(&mut self, id: NodeId, steer: f32) -> Result<(), SceneError> {
        let steer = wish(steer, -1.0, 1.0);
        self.vehicle_mut(id)?.steer = steer;
        Ok(())
    }

    /// How hard the brake is on. In a drift car, all of it is the handbrake.
    pub fn brake(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.vehicle(id)?.brake)
    }

    pub fn set_brake(&mut self, id: NodeId, brake: f32) -> Result<(), SceneError> {
        let brake = wish(brake, 0.0, 1.0);
        self.vehicle_mut(id)?.brake = brake;
        Ok(())
    }

    /// Drives every vehicle the world is holding, just before the solver moves anything.
    /// The three wishes a driver wrote are turned into a speed along the nose and a turn
    /// of it; the solver then integrates the body the way it integrates a crate, which is
    /// why a wall the car has no way through stops it in the same place.
    pub(crate) fn step_vehicles(&mut self, dt: f32) {
        for id in self.node_tree() {
            self.drive(id, dt);
        }
    }

    /// Works one vehicle's answer out of what it is asked to do. A vehicle nobody is
    /// asking anything of is left alone, coasting: a shove from a quake keeps rolling it
    /// and the floor's own drag slows it, exactly as it does a crate.
    fn drive(&mut self, id: NodeId, dt: f32) {
        let Ok(driven) = self.vehicle(id) else {
            return;
        };
        // Only a body the solver is allowed to move can be driven: a car pinned to the
        // world, or one gravity has no claim on, has no way to answer the wheel.
        let Ok(motion) = self.component::<Motion>(id) else {
            return;
        };
        if motion.anchored || !motion.gravity {
            return;
        }
        let throttle = driven.throttle;
        let steer = driven.steer;
        let brake = driven.brake;
        if !driven.being_driven() {
            if let Ok(mut motion) = self.component_mut::<Motion>(id) {
                motion.spin = 0.0;
            }
            return;
        }

        let Ok(rotation) = self.rotation(id) else {
            return;
        };
        let Some(pointing) = ground_direction(rotation * Vec3::X) else {
            return;
        };
        // Only the ground plan is the tyres' business; whatever the car does about going
        // up or down belongs to gravity and to the floor.
        let speed = Vec3::new(motion.velocity.x, motion.velocity.y, 0.0);
        let top = driven.top_speed;

        // A wheel turned while the car stands still turns nothing: a car pivots on the
        // speed it arrives with, and reversing flips the wheel the way it does in a real
        // one. A drifting car is on the move even when it is going forwards hardly at all
        // - the whole of a slide swings the nose - but an arcade car only has its forwards
        // speed to turn on.
        let forwards = speed.dot(pointing);
        let carrying = match driven.handling {
            HandlingType::Arcade => forwards,
            HandlingType::Drift => {
                let ground = speed.length();
                if forwards < 0.0 { -ground } else { ground }
            }
        };
        let rolling = if top > 0.0 {
            (carrying / top).clamp(-1.0, 1.0)
        } else {
            0.0
        };
        // The world turns counter-clockwise about the upright axis, so a wheel turned to
        // the right is a negative turn of the nose.
        let spin = -steer * driven.turn_rate * rolling;

        // The tyres answer against the way the car will be pointing once the step has
        // turned it, not the way it points now: a step is a turn and a drive at once, and
        // an arcade car is meant to leave along the nose it ends on. A yaw of a level
        // direction is still a level direction, so this needs no flattening again.
        let nose = Quat::from_rotation_z(spin.to_radians() * dt) * pointing;
        let along = speed.dot(nose);
        let mut sideways = speed - nose * along;

        // A brake outranks an accelerator, the way one foot covers both pedals, and it
        // only ever asks for less of the speed the car already has. The foot can only
        // change the speed so much in one step, so a car gathers way over a couple of
        // seconds rather than snapping to the figure a script wrote.
        let desired = if brake > 0.0 { 0.0 } else { throttle * top };
        let limit = if brake > 0.0 {
            BRAKING * brake * dt
        } else {
            ACCELERATION * dt
        };
        let driven_along = along + (desired - along).clamp(-limit, limit);

        // Where the two handlings part company is sideways. An arcade car's tyres are
        // glued: the speed across the nose is gone in the step it appears, so the car goes
        // exactly where it points. A drift car keeps its grip as a grip, and the handbrake
        // - a brake stamped on in a car built to slide - takes that grip off the road, so
        // the car carries on along its old line while its nose swings away from it.
        match driven.handling {
            HandlingType::Arcade => sideways = Vec3::ZERO,
            HandlingType::Drift => {
                let bite = driven.grip * if brake > 0.0 { 1.0 - brake } else { 1.0 };
                sideways *= (-SIDE_RATE * bite * dt).exp();
            }
        }

        let Ok(mut motion) = self.component_mut::<Motion>(id) else {
            return;
        };
        motion.velocity.x = nose.x * driven_along + sideways.x;
        motion.velocity.y = nose.y * driven_along + sideways.y;
        motion.spin = spin;
    }

    /// Whether the tyres are the thing deciding this body's ground speed: a vehicle with
    /// a driver asking something of it, which is the difference between a car and a crate
    /// with wheels.
    pub(crate) fn is_driven(&self, id: NodeId) -> bool {
        self.vehicle(id).is_ok_and(|driven| driven.being_driven())
    }

    pub(crate) fn vehicle(&self, id: NodeId) -> Result<Vehicle, SceneError> {
        let entity = self.entity(id)?;
        self.world
            .get::<Vehicle>(entity)
            .copied()
            .ok_or(SceneError::NotAVehicle)
    }

    pub(crate) fn vehicle_mut(&mut self, id: NodeId) -> Result<Mut<'_, Vehicle>, SceneError> {
        let entity = self.entity(id)?;
        self.world
            .get_mut::<Vehicle>(entity)
            .ok_or(SceneError::NotAVehicle)
    }

    pub(crate) fn seat(&self, id: NodeId) -> Result<Seat, SceneError> {
        let entity = self.entity(id)?;
        self.world
            .get::<Seat>(entity)
            .copied()
            .ok_or(SceneError::NotASeat)
    }

    fn seat_mut(&mut self, id: NodeId) -> Result<Mut<'_, Seat>, SceneError> {
        let entity = self.entity(id)?;
        self.world
            .get_mut::<Seat>(entity)
            .ok_or(SceneError::NotASeat)
    }

    /// Whether the solver should leave this body where it is because it is riding in
    /// a seat: a passenger does not fall, and a driver does not shove the crates it
    /// runs into - the car it sits in takes the hit.
    pub(crate) fn rides_a_seat(&self, id: NodeId) -> bool {
        self.component::<Seated>(id).is_ok()
    }
}

/// Where a matrix puts a body and which way it turns it. The scale is dropped: a rider
/// keeps its own size, and only the place of the seat is borrowed. `None` when the
/// three axes say nothing about a direction, which is a squashed branch.
fn standing_place(model: Mat4) -> Option<(Vec3, Quat)> {
    let whole = [model.x_axis, model.y_axis, model.z_axis]
        .iter()
        .all(|column| column.truncate().length_squared() > 1e-12);
    if !whole {
        return None;
    }
    let (_, turning, at) = model.to_scale_rotation_translation();
    Some((at, turning))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scene holding one vehicle and nothing else.
    fn car() -> (Scene, NodeId) {
        let mut scene = Scene::new();
        let id = scene.create("vehicle", None, None).unwrap();
        (scene, id)
    }

    #[test]
    fn a_vehicle_is_an_object_wearing_a_drive() {
        let mut scene = Scene::new();
        let id = scene.create("vehicle", None, None).unwrap();

        assert_eq!(scene.kind(id).unwrap(), "vehicle");
        assert!(scene.is_vehicle(id).unwrap());
        assert!(!scene.is_seat(id).unwrap());
        // The hatchback it was born as, and so the weight that comes with it.
        assert_eq!(
            scene.vec_field(id, VecField::Size).unwrap(),
            Vec3::new(4.4, 2.0, 1.2)
        );
        assert_eq!(scene.handling(id).unwrap(), HandlingType::Arcade);
        assert_eq!(scene.top_speed(id).unwrap(), DEFAULT_TOP_SPEED);
        assert_eq!(scene.throttle(id).unwrap(), 0.0);
        // Born inside the solver, because a car you walk through is no use to anyone.
        assert!(scene.has_collision(id).unwrap());
        assert!(scene.uses_gravity(id).unwrap());
        assert_eq!(
            scene.node_count(),
            1,
            "a car is one node, not a car and a box"
        );
    }

    #[test]
    fn a_seat_sits_and_a_driver_seat_drives() {
        let mut scene = Scene::new();
        let car = scene.create("Vehicle", None, None).unwrap();
        let driver = scene.create("DriverSeat", None, Some(car)).unwrap();
        let passenger = scene.create("seat", None, Some(car)).unwrap();

        assert!(scene.is_seat(driver).unwrap());
        assert!(scene.seat_drives(driver).unwrap());
        assert!(!scene.seat_drives(passenger).unwrap());
        assert_eq!(scene.kind(driver).unwrap(), "driverSeat");
        assert_eq!(scene.kind(passenger).unwrap(), "seat");
        assert_eq!(scene.name(driver).unwrap(), "DriverSeat");

        // The driver seat drives the object it hangs on; the passenger seat only rides.
        assert_eq!(scene.seat_vehicle(driver).unwrap(), Some(car));
        assert_eq!(scene.seat_vehicle(passenger).unwrap(), None);
        assert!(
            scene.seated_in(driver).unwrap().is_none(),
            "and it is empty"
        );

        // Neither of them draws anything.
        assert!(scene.mesh(driver).is_err(), "a seat is not a shape");
    }

    #[test]
    fn a_driver_seat_on_a_thing_that_is_not_a_vehicle_drives_nothing() {
        let mut scene = Scene::new();
        let kitchen = scene.create("cube", None, None).unwrap();
        let chair = scene.create("DriverSeat", None, Some(kitchen)).unwrap();

        assert!(scene.seat_drives(chair).unwrap());
        assert_eq!(scene.seat_vehicle(chair).unwrap(), None);

        // A seat nobody hung on anything is a chair in the middle of the room.
        let alone = scene.create("Seat", None, None).unwrap();
        assert_eq!(scene.parent(alone).unwrap(), Some(scene.root()));
    }

    #[test]
    fn the_handling_is_chosen_by_name_the_way_a_light_is() {
        let (mut scene, id) = car();

        scene.set_handling_name(id, "Drift").unwrap();
        assert_eq!(scene.handling(id).unwrap(), HandlingType::Drift);

        scene.set_handling_name(id, "  slide  ").unwrap();
        assert_eq!(scene.handling(id).unwrap(), HandlingType::Drift);

        scene.set_handling_name(id, "arcade").unwrap();
        assert_eq!(scene.handling(id).unwrap(), HandlingType::Arcade);

        let error = scene.set_handling_name(id, "bouncy").unwrap_err();
        assert_eq!(error, SceneError::UnknownHandling("bouncy".to_owned()));
        assert!(format!("{error}").contains("bouncy"), "{error}");
        assert!(format!("{error}").contains("arcade"), "{error}");
    }

    #[test]
    fn the_knobs_are_kept_where_they_were_put() {
        let (mut scene, id) = car();

        scene.set_top_speed(id, 60.0).unwrap();
        assert_eq!(scene.top_speed(id).unwrap(), 60.0);
        scene.set_top_speed(id, -5.0).unwrap();
        assert_eq!(scene.top_speed(id).unwrap(), 0.0);

        scene.set_turn_rate(id, 45.0).unwrap();
        assert_eq!(scene.turn_rate(id).unwrap(), 45.0);

        scene.set_grip(id, 2.0).unwrap();
        assert_eq!(scene.grip(id).unwrap(), 1.0);
        scene.set_grip(id, -1.0).unwrap();
        assert_eq!(scene.grip(id).unwrap(), 0.0);

        // A wish is a wish: backwards throttle and no wheel at all.
        scene.set_throttle(id, -3.0).unwrap();
        assert_eq!(scene.throttle(id).unwrap(), -1.0);
        scene.set_steer(id, 0.25).unwrap();
        assert_eq!(scene.steer(id).unwrap(), 0.25);
        scene.set_brake(id, 9.0).unwrap();
        assert_eq!(scene.brake(id).unwrap(), 1.0);
        scene.set_brake(id, -9.0).unwrap();
        assert_eq!(scene.brake(id).unwrap(), 0.0);
    }

    #[test]
    fn what_a_vehicle_is_spreads_and_what_it_is_asked_to_do_does_not() {
        let mut scene = Scene::new();
        let id = scene.create("vehicle", None, None).unwrap();
        scene.set_top_speed(id, 80.0).unwrap();
        scene.set_handling(id, HandlingType::Drift).unwrap();
        scene.set_throttle(id, 1.0).unwrap();

        let twin = scene.link_duplicate(id, 1).unwrap()[0];
        assert_eq!(
            scene.top_speed(twin).unwrap(),
            80.0,
            "a fleet is built the same"
        );
        assert_eq!(scene.handling(twin).unwrap(), HandlingType::Drift);

        scene.set_throttle(twin, 0.0).unwrap();
        assert_eq!(
            scene.throttle(id).unwrap(),
            1.0,
            "one driver's foot is their own"
        );
    }

    #[test]
    fn a_vehicle_knob_on_a_thing_that_is_not_one_is_refused() {
        let mut scene = Scene::new();
        let box_id = scene.create("cube", None, None).unwrap();
        let seat = scene.create("seat", None, None).unwrap();

        for error in [
            scene.top_speed(box_id).err(),
            scene.set_throttle(box_id, 1.0).err(),
            scene.handling(seat).err(),
            scene.set_grip(seat, 1.0).err(),
        ]
        .into_iter()
        .flatten()
        {
            assert_eq!(error, SceneError::NotAVehicle, "{error}");
        }

        assert_eq!(scene.seat_drives(box_id).unwrap_err(), SceneError::NotASeat);
        assert_eq!(
            scene.seat_vehicle(box_id).unwrap_err(),
            SceneError::NotASeat,
            "a cube has no seat to ask"
        );
    }

    #[test]
    fn a_seat_survives_being_moved_and_destroyed_like_any_node() {
        let mut scene = Scene::new();
        let car = scene.create("vehicle", None, None).unwrap();
        let driver = scene.create("DriverSeat", None, Some(car)).unwrap();

        scene.remove_child(car, driver).unwrap();
        assert_eq!(scene.parent(driver).unwrap(), Some(scene.root()));
        assert_eq!(
            scene.seat_vehicle(driver).unwrap(),
            None,
            "a driver seat off its car drives nothing"
        );

        scene.destroy(driver).unwrap();
        assert_eq!(
            scene.seat_drives(driver).unwrap_err(),
            SceneError::NodeDestroyed
        );
    }

    /// A car standing still on the floor, the way it is before a driver gets in.
    fn parked() -> (Scene, NodeId) {
        let mut scene = Scene::new();
        let id = scene.create("vehicle", None, None).unwrap();

        for _ in 0..120 {
            scene.step(1.0 / 60.0);
        }
        assert_eq!(scene.throttle(id).unwrap(), 0.0);
        (scene, id)
    }

    /// What the driver is asking the car to do.
    fn ask(scene: &mut Scene, id: NodeId, throttle: f32, steer: f32, brake: f32) {
        scene.set_throttle(id, throttle).unwrap();
        scene.set_steer(id, steer).unwrap();
        scene.set_brake(id, brake).unwrap();
    }

    fn rolling(scene: &Scene, id: NodeId) -> Vec3 {
        scene.component::<Motion>(id).unwrap().velocity
    }

    fn shove(scene: &mut Scene, id: NodeId, speed: Vec3) {
        scene.component_mut::<Motion>(id).unwrap().velocity = speed;
    }

    fn yaw(scene: &Scene, id: NodeId) -> f32 {
        scene.vec_field(id, VecField::RotationEuler).unwrap().z
    }

    fn place(scene: &mut Scene, id: NodeId, at: Vec3) {
        scene.set_vec_field(id, VecField::Position, at).unwrap();
    }

    #[test]
    fn a_throttle_drives_a_car_along_its_nose() {
        let (mut scene, id) = parked();
        ask(&mut scene, id, 1.0, 0.0, 0.0);

        for _ in 0..60 {
            scene.step(1.0 / 60.0);
        }

        let at = scene.vec_field(id, VecField::Position).unwrap();
        assert!(at.x > 3.5, "{at} after a second of full throttle");
        assert!(
            at.y.abs() < 1e-3,
            "straight ahead, and the floor holds it up"
        );
        let speed = rolling(&scene, id);
        assert!(
            speed.x > 7.5 && speed.x < 8.2,
            "{speed}: eight metres a second gained in a second"
        );
    }

    #[test]
    fn the_top_speed_is_the_ceiling_a_throttle_chases() {
        let (mut scene, id) = parked();
        scene.set_top_speed(id, 5.0).unwrap();
        ask(&mut scene, id, 1.0, 0.0, 0.0);

        for _ in 0..300 {
            scene.step(1.0 / 60.0);
        }

        let speed = rolling(&scene, id);
        assert!(speed.x > 4.9 && speed.x < 5.1, "{speed}, and no further");
    }

    #[test]
    fn a_reversed_throttle_drives_backwards() {
        let (mut scene, id) = parked();
        ask(&mut scene, id, -1.0, 0.0, 0.0);

        for _ in 0..60 {
            scene.step(1.0 / 60.0);
        }

        assert!(scene.vec_field(id, VecField::Position).unwrap().x < -3.5);
        assert!(rolling(&scene, id).x < -7.5);
    }

    #[test]
    fn the_brake_outranks_the_accelerator() {
        let (mut scene, id) = parked();
        ask(&mut scene, id, 1.0, 0.0, 0.0);
        for _ in 0..120 {
            scene.step(1.0 / 60.0);
        }
        assert!(rolling(&scene, id).x > 15.0, "up to speed first");

        ask(&mut scene, id, 1.0, 0.0, 1.0);
        for _ in 0..60 {
            scene.step(1.0 / 60.0);
        }

        let speed = rolling(&scene, id);
        assert!(
            speed.x.abs() < 1e-3,
            "{speed}: both pedals at once is a stop"
        );
    }

    #[test]
    fn a_wheel_turns_the_nose_only_while_the_car_is_rolling() {
        let (mut scene, id) = parked();

        // Parked with the wheel hard over: nothing moves, because a car turns on the
        // speed it is already carrying.
        ask(&mut scene, id, 0.0, 1.0, 0.0);
        for _ in 0..60 {
            scene.step(1.0 / 60.0);
        }
        assert_eq!(yaw(&scene, id), 0.0);
        assert_eq!(scene.vec_field(id, VecField::Position).unwrap().x, 0.0);

        // Now driving, the same wheel swings the nose to the right, which in a world
        // that turns counter-clockwise about the upright is a negative yaw.
        ask(&mut scene, id, 1.0, 1.0, 0.0);
        for _ in 0..120 {
            scene.step(1.0 / 60.0);
        }
        assert!(yaw(&scene, id) < -20.0, "{}", yaw(&scene, id));
    }

    #[test]
    fn nobody_at_the_wheel_leaves_a_shoved_car_coasting() {
        let (mut scene, id) = parked();
        shove(&mut scene, id, Vec3::new(5.0, 0.0, 0.0));

        scene.step(1.0 / 60.0);

        let speed = rolling(&scene, id);
        assert!(
            speed.x > 4.5,
            "{speed}: the drive model never takes a foot off the gas for a driver \
             who has not asked for one; only the floor's drag does"
        );
        assert_eq!(scene.throttle(id).unwrap(), 0.0);
    }

    #[test]
    fn an_anchored_car_does_not_answer_the_wheel() {
        let (mut scene, id) = parked();
        scene.set_anchored(id, true).unwrap();
        ask(&mut scene, id, 1.0, 1.0, 0.0);

        for _ in 0..60 {
            scene.step(1.0 / 60.0);
        }

        assert_eq!(scene.vec_field(id, VecField::Position).unwrap().x, 0.0);
        assert_eq!(yaw(&scene, id), 0.0);
    }

    #[test]
    fn an_arcade_car_kills_a_slide_and_a_drift_car_keeps_it() {
        let (mut scene, id) = parked();
        ask(&mut scene, id, 1.0, 0.0, 0.0);
        // Thrown sideways while it is being driven: half along the nose, half across it.
        shove(&mut scene, id, Vec3::new(5.0, 5.0, 0.0));
        scene.step(1.0 / 60.0);
        assert_eq!(
            rolling(&scene, id).y,
            0.0,
            "an arcade car goes where it points, the instant it is told to"
        );

        scene.set_handling_name(id, "drift").unwrap();
        scene.set_grip(id, 0.0).unwrap();
        ask(&mut scene, id, 1.0, 0.0, 0.0);
        shove(&mut scene, id, Vec3::new(5.0, 5.0, 0.0));
        scene.step(1.0 / 60.0);
        assert!(
            rolling(&scene, id).y > 4.5,
            "on ice, a drift car keeps sliding the way it was thrown"
        );
    }

    /// How far the nose points away from where the car is actually going, in degrees.
    /// Nothing at all means a car sitting on its line; something means it is sliding.
    fn slip(scene: &Scene, id: NodeId) -> f32 {
        let rolling = rolling(scene, id);
        let ground = Vec3::new(rolling.x, rolling.y, 0.0);
        if ground.length_squared() < 1e-6 {
            return 0.0;
        }
        let nose = ground_direction(scene.rotation(id).unwrap() * Vec3::X).unwrap();
        nose.dot(ground.normalize())
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
    }

    #[test]
    fn an_arcade_car_sits_on_its_line_and_a_drift_car_does_not() {
        let (mut scene, id) = parked();
        ask(&mut scene, id, 1.0, 1.0, 0.0);
        for _ in 0..60 {
            scene.step(1.0 / 60.0);
        }
        assert!(
            slip(&scene, id) < 0.1,
            "an arcade car goes exactly where it is pointed: {}",
            slip(&scene, id)
        );

        let (mut scene, id) = parked();
        scene.set_handling_name(id, "drift").unwrap();
        ask(&mut scene, id, 1.0, 1.0, 0.0);
        for _ in 0..60 {
            scene.step(1.0 / 60.0);
        }
        assert!(
            slip(&scene, id) > 5.0,
            "the same wheel in the same car, and the tail has come round: {}",
            slip(&scene, id)
        );
    }

    #[test]
    fn a_grip_of_ice_slides_and_a_grip_of_glue_does_not() {
        let (mut icy, ice) = parked();
        icy.set_handling_name(ice, "drift").unwrap();
        icy.set_grip(ice, 0.05).unwrap();
        ask(&mut icy, ice, 1.0, 1.0, 0.0);
        for _ in 0..60 {
            icy.step(1.0 / 60.0);
        }

        let (mut glued, glue) = parked();
        glued.set_handling_name(glue, "drift").unwrap();
        glued.set_grip(glue, 1.0).unwrap();
        ask(&mut glued, glue, 1.0, 1.0, 0.0);
        for _ in 0..60 {
            glued.step(1.0 / 60.0);
        }

        let slide = slip(&icy, ice);
        let bite = slip(&glued, glue);
        assert!(
            slide > bite + 5.0,
            "{slide} degrees on ice against {bite} with the tyres glued"
        );
        assert!(slide > 15.0, "{slide}");
    }

    #[test]
    fn the_handbrake_is_the_brake_of_a_drift_car() {
        let (mut going_straight, wheel) = parked();
        going_straight.set_handling_name(wheel, "drift").unwrap();
        ask(&mut going_straight, wheel, 1.0, 0.0, 0.0);
        for _ in 0..120 {
            going_straight.step(1.0 / 60.0);
        }
        let straight = rolling(&going_straight, wheel).length();

        let (mut sliding, handbrake) = parked();
        sliding.set_handling_name(handbrake, "drift").unwrap();
        ask(&mut sliding, handbrake, 1.0, 0.0, 0.0);
        for _ in 0..120 {
            sliding.step(1.0 / 60.0);
        }

        // The same speed, the same wheel, and one car with its handbrake stamped on.
        ask(&mut going_straight, wheel, 1.0, 1.0, 0.0);
        for _ in 0..30 {
            going_straight.step(1.0 / 60.0);
        }
        ask(&mut sliding, handbrake, 1.0, 1.0, 1.0);
        for _ in 0..30 {
            sliding.step(1.0 / 60.0);
        }

        assert!(
            slip(&sliding, handbrake) > slip(&going_straight, wheel) + 5.0,
            "{} against {}",
            slip(&sliding, handbrake),
            slip(&going_straight, wheel)
        );
        assert!(
            rolling(&sliding, handbrake).length() < straight,
            "and a handbrake is still a brake: {} against {straight}",
            rolling(&sliding, handbrake).length()
        );
    }

    #[test]
    fn a_wall_stops_a_car_driven_into_it() {
        let (mut scene, id) = parked();
        let wall = scene.create("cube", Some("Wall".into()), None).unwrap();
        place(&mut scene, wall, Vec3::new(6.0, 0.0, 1.5));
        scene
            .set_vec_field(wall, VecField::Size, Vec3::new(1.0, 20.0, 3.0))
            .unwrap();
        scene.set_collision(wall, true).unwrap();
        scene.set_anchored(wall, true).unwrap();

        ask(&mut scene, id, 1.0, 0.0, 0.0);
        for _ in 0..180 {
            scene.step(1.0 / 60.0);
        }

        let at = scene.vec_field(id, VecField::Position).unwrap();
        assert!(
            at.x < 3.7,
            "{at}: the nose meets the wall at five and a half, and the car is four \
             and a half long"
        );
    }

    #[test]
    fn an_arcade_car_never_minds_what_its_grip_says() {
        let (mut scene, id) = parked();
        scene.set_grip(id, 0.0).unwrap();
        ask(&mut scene, id, 1.0, 0.0, 0.0);
        shove(&mut scene, id, Vec3::new(5.0, 5.0, 0.0));

        scene.step(1.0 / 60.0);

        assert_eq!(
            rolling(&scene, id).y,
            0.0,
            "glued is glued, however slippery the tyres were told to be"
        );
    }

    /// Something with feet, hung on the root and standing on its own until it is sat
    /// down - the way a script makes an avatar before it has a car.
    fn rider(scene: &mut Scene) -> NodeId {
        scene.create("cube", Some("Driver".into()), None).unwrap()
    }

    /// A car with a seat for a driver and a seat for anybody else, settled on the floor.
    fn fitted() -> (Scene, NodeId, NodeId, NodeId) {
        let (mut scene, car) = parked();
        let driver = scene.create("DriverSeat", None, Some(car)).unwrap();
        let passenger = scene.create("Seat", None, Some(car)).unwrap();
        (scene, car, driver, passenger)
    }

    /// Two places the same to the solver, given the round trip through a matrix.
    fn stands_together(scene: &Scene, a: NodeId, seat: NodeId) -> bool {
        let Some((at, turning)) = scene.world_place(a) else {
            return false;
        };
        let Some((there, there_turns)) = scene.world_place(seat) else {
            return false;
        };
        (at - there).length() < 1e-4 && turning.dot(there_turns).abs() > 0.9999
    }

    #[test]
    fn sitting_puts_a_rider_in_the_car_and_standing_takes_it_out() {
        let (mut scene, _, driver, _) = fitted();
        let who = rider(&mut scene);
        place(&mut scene, who, Vec3::new(0.0, 0.0, 20.0));

        scene.sit(who, driver).unwrap();

        assert_eq!(scene.seated_in(driver).unwrap(), Some(who));
        assert_eq!(scene.seat_of(who).unwrap(), Some(driver));
        assert!(scene.is_seated(who).unwrap());
        assert!(
            stands_together(&scene, who, driver),
            "at the seat, in the same frame it was asked"
        );
        assert_eq!(
            scene.parent(who).unwrap(),
            Some(scene.root()),
            "and still a node of its own, not a piece of the car"
        );

        scene.stand(who).unwrap();

        assert_eq!(scene.seated_in(driver).unwrap(), None);
        assert!(!scene.is_seated(who).unwrap());
        assert!(
            stands_together(&scene, who, driver),
            "left standing exactly where the seat had it"
        );
    }

    #[test]
    fn a_rider_goes_where_the_car_does() {
        let (mut scene, car, driver, _) = fitted();
        let who = rider(&mut scene);
        scene.sit(who, driver).unwrap();

        ask(&mut scene, car, 1.0, 1.0, 0.0);
        for _ in 0..60 {
            scene.step(1.0 / 60.0);
        }

        let gone = scene.world_place(car).unwrap().0;
        assert!(
            Vec3::new(gone.x, gone.y, 0.0).length() > 3.0,
            "the car drove off without waiting: {gone}"
        );
        assert!(
            stands_together(&scene, who, driver),
            "and the driver went with it: {:?}",
            scene.world_place(who).unwrap().0
        );
    }

    #[test]
    fn a_seated_body_is_nothing_the_solver_moves() {
        let (mut scene, _, driver, _) = fitted();
        let who = rider(&mut scene);
        scene.set_uses_gravity(who, true).unwrap();
        scene.sit(who, driver).unwrap();

        let at_seat = scene.world_place(who).unwrap().0;
        for _ in 0..120 {
            scene.step(1.0 / 60.0);
        }

        assert!(
            stands_together(&scene, who, driver),
            "gravity had hold of it and it stayed in the car: {at_seat} against {:?}",
            scene.world_place(who).unwrap().0
        );
        assert_eq!(
            rolling(&scene, who),
            Vec3::ZERO,
            "and it never gathered a speed"
        );
    }

    #[test]
    fn a_seat_holds_one_rider_and_a_rider_holds_one_seat() {
        let (mut scene, _, driver, passenger) = fitted();
        let first = rider(&mut scene);
        let second = scene.create("cube", Some("Second".into()), None).unwrap();

        scene.sit(first, driver).unwrap();
        scene.sit(second, driver).unwrap();
        assert_eq!(scene.seated_in(driver).unwrap(), Some(second));
        assert!(
            !scene.is_seated(first).unwrap(),
            "and whoever was in it before got out"
        );

        scene.sit(second, passenger).unwrap();
        assert_eq!(scene.seat_of(second).unwrap(), Some(passenger));
        assert_eq!(
            scene.seated_in(driver).unwrap(),
            None,
            "the seat it left is empty again"
        );
    }

    #[test]
    fn a_seat_is_not_something_that_sits() {
        let (mut scene, _, driver, passenger) = fitted();

        let error = scene.sit(driver, passenger).unwrap_err();
        assert_eq!(error, SceneError::SeatCannotSit);
        assert!(format!("{error}").contains("seat"), "{error}");
    }

    #[test]
    fn destroying_a_car_leaves_nobody_welded_to_nothing() {
        let (mut scene, car, driver, _) = fitted();
        let who = rider(&mut scene);
        scene.sit(who, driver).unwrap();

        // The seat hangs on the car and goes down with it.
        scene.destroy(car).unwrap();

        assert_eq!(scene.name(who).unwrap(), "Driver", "the rider survived");
        assert!(!scene.is_seated(who).unwrap(), "and is on its own feet");
    }

    #[test]
    fn a_wreck_throws_its_driver_clear() {
        let (mut scene, car, driver, _) = fitted();
        ask(&mut scene, car, 1.0, 0.0, 0.0);
        for _ in 0..30 {
            scene.step(1.0 / 60.0);
        }
        let car_speed = rolling(&scene, car);
        assert!(car_speed.x > 1.0, "a car under way: {car_speed}");

        let who = rider(&mut scene);
        scene.sit(who, driver).unwrap();
        scene.eject_riders(car);

        assert!(!scene.is_seated(who).unwrap());
        assert_eq!(scene.seated_in(driver).unwrap(), None);
        let flung = rolling(&scene, who);
        assert!(
            flung.x > 1.0 && flung.z > 0.0,
            "still going the way the car went, and up out of it: {flung}"
        );
    }

    #[test]
    fn getting_out_carries_the_speed_the_car_had() {
        let (mut scene, car, driver, _) = fitted();
        ask(&mut scene, car, 1.0, 0.0, 0.0);
        for _ in 0..30 {
            scene.step(1.0 / 60.0);
        }
        let car_speed = rolling(&scene, car);

        let who = rider(&mut scene);
        scene.sit(who, driver).unwrap();
        scene.stand(who).unwrap();

        assert_eq!(
            rolling(&scene, who),
            car_speed,
            "a body that stops being carried keeps going"
        );
    }

    #[test]
    fn the_first_seat_with_somebody_in_it_is_the_one_driving() {
        let (mut scene, car, driver, passenger) = fitted();
        let who = rider(&mut scene);
        assert_eq!(scene.vehicle_driver(car).unwrap(), None);

        scene.sit(who, passenger).unwrap();
        assert_eq!(
            scene.vehicle_driver(car).unwrap(),
            None,
            "a passenger lends no hands"
        );

        let second = scene.create("cube", Some("Second".into()), None).unwrap();
        scene.sit(second, driver).unwrap();
        assert_eq!(scene.vehicle_driver(car).unwrap(), Some(second));
    }

    #[test]
    fn a_dead_car_takes_its_foot_off_the_pedals() {
        let (mut scene, car) = parked();
        ask(&mut scene, car, 1.0, 0.5, 0.25);

        scene.let_go_of_the_wheel(car);

        assert_eq!(scene.throttle(car).unwrap(), 0.0);
        assert_eq!(scene.steer(car).unwrap(), 0.0);
        assert_eq!(scene.brake(car).unwrap(), 0.0);

        // A thing that is not a vehicle is not asked.
        let crate_box = scene.create("cube", None, None).unwrap();
        scene.let_go_of_the_wheel(crate_box);
        assert!(!scene.is_vehicle(crate_box).unwrap());
    }
}
