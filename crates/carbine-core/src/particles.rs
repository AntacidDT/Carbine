//! Particle systems: a crowd of small things, flown by one node.
//!
//! A system is an object. It has a place, a parent to hang on, a colour, a
//! transparency and a visibility like anything else in the tree, and it draws
//! nothing of its own - what it draws is the crowd it is carrying. Hang one on a
//! chimney and it smokes with the chimney; put one on the sky, with
//! [`Scene::set_particles_on_sky`], and its box rides with whatever the frame is
//! showing, which is the only way a downpour can follow you a mile.
//!
//! The crowd is kept in world meters, not in the node's pocket. A drop already
//! falling carries on falling where it was born and only a new one is placed
//! around wherever the node has got to, which is what makes weather out of a
//! moving parent instead of a smear of one. Moving or turning a system therefore
//! takes a restart: switch it off and on and the veil is laid again from where it
//! now stands.
//!
//! The one rule about hitting things: a drop is a place with a skin, not a body.
//! The hull maths is asked one place at a time and no more than
//! [`COLLISION_BUDGET`] times a step, so a storm over a city costs a set amount
//! instead of the square of both.

use std::mem;

use bevy_ecs::prelude::*;
use glam::{Quat, Vec3};

use crate::geometry::{self, Solid};
use crate::motion::Motion;
use crate::node::{
    Children, DEFAULT_COLOR, MAX_ALPHA, NodeId, NodeKind, NodeName, NodeTexture, ObjectColor,
    Opacity, Parent, Scene, SceneError, Transform, Visible,
};
use crate::player::GROUND_HEIGHT;
use crate::procgen::{Rng, Seed, salt};

/// The most drops one system may fly. Past this a script is not making weather, it
/// is making a wall, and the frame cost is the player's.
pub const MAX_PARTICLES: usize = 20_000;

/// The most splash droplets a system may have in the air at one time. Impacts are
/// cheap only until the crowd they throw is bigger than the crowd that made them.
pub const MAX_SPLASH_DROPS: usize = 4_096;

/// How many times one step asks whether a drop is inside a body. When the budget
/// runs out the rest of the crowd falls through for this frame: a storm that
/// overshoots by a frame is unnoticed, a storm that takes a frame to answer is not.
pub const COLLISION_BUDGET: usize = 2_048;

/// The most splash droplets one impact may throw.
const MAX_SPLASH_PER_HIT: usize = 32;

/// Seconds a splash droplet is thrown for.
const SPLASH_LIFETIME: f32 = 0.3;

/// Meters per second squared the splash droplets are pulled by - their own little
/// arc, so a splash reads as a flick of water and not as rain starting early.
const SPLASH_GRAVITY: f32 = 6.0;

/// Meters per second a drop is walked sideways by, per meter per second squared
/// the disasters answer at the crowd. A drop is not a body and is not asked twice:
/// the air's shove is read once, as the pace a gust of that strength carries it
/// along, so a magnitude-6 gale leans rain about thirty degrees and a calm air
/// leaves it falling straight down the way it was written.
const AIR_PER_ACCEL: f32 = 0.9;

/// How one thing in the crowd is cut.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParticleShape {
    /// A line stretched along the way it travels: rain, sleet, a driving spray.
    Streak,
    /// A hard-edged quad. Ash, paper, a chip of something.
    Square,
    /// A soft round dot. Snow, dust, pollen, spark.
    Circle,
}

impl ParticleShape {
    /// The shapes a script can write, in the order they are listed when one is
    /// wrong. The extra names [`from_name`](Self::from_name) forgives are not
    /// advertised, the same way a weather report's `mist` is not a kind of its own.
    pub const NAMES: &'static [&'static str] = &["circle", "square", "streak"];

    pub fn from_name(name: &str) -> Option<ParticleShape> {
        Some(match name.trim().to_lowercase().as_str() {
            "circle" | "disc" | "dot" | "round" | "snow" | "spark" => Self::Circle,
            "square" | "box" | "quad" | "ash" | "leaf" => Self::Square,
            "streak" | "dash" | "line" | "drop" | "rain" => Self::Streak,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Circle => "circle",
            Self::Square => "square",
            Self::Streak => "streak",
        }
    }

    /// How long this shape is along the way it travels, as a multiple of how wide
    /// it is. A dot and a chip are as long as they are wide; rain is not.
    pub fn stretch(self) -> f32 {
        match self {
            Self::Streak => 6.0,
            Self::Circle | Self::Square => 1.0,
        }
    }
}

/// One thing in the air, in world meters, ready to be drawn.
pub struct ParticleItem<'a> {
    pub node: NodeId,
    pub at: Vec3,
    /// Meters across. A streak is [`ParticleShape::stretch`] times this long along
    /// `velocity`.
    pub size: f32,
    /// Meters per second in world space, which is also the way it points.
    pub velocity: Vec3,
    pub shape: ParticleShape,
    pub color: [f32; 3],
    /// Alpha in 0..1, ready for blending.
    pub alpha: f32,
    /// The image the renderer should skin this with, by path.
    pub texture: Option<&'a str>,
}

/// One thing the system itself is flying: it goes where the system points.
#[derive(Clone, Copy)]
struct Drop {
    at: Vec3,
    /// Seconds before this one is spent.
    left: f32,
    /// How much bigger or smaller than the system's size this one is.
    scale: f32,
    /// How much of the system's colour this one keeps, 0..1.
    shade: f32,
    /// How much slower or faster than the system's speed this one goes.
    drag: f32,
}

/// One droplet thrown clear by an impact. It flies on its own and never splashes
/// again: a splash that splashes is a hang waiting to happen.
#[derive(Clone, Copy)]
struct Burst {
    at: Vec3,
    velocity: Vec3,
    left: f32,
    scale: f32,
}

/// A body a drop can be stopped by, its box kept beside it so the box can be asked
/// a thousand times a frame and the hull only now and then.
struct Blocker {
    min: Vec3,
    max: Vec3,
    solid: Solid,
}

#[derive(Component)]
struct Particles {
    shape: ParticleShape,
    count: usize,
    size: f32,
    speed: f32,
    direction: Vec3,
    area: Vec3,
    lifetime: f32,
    seed: Seed,
    active: bool,
    collide: bool,
    splash: usize,
    recycle: bool,
    /// Whether the box the crowd is laid in rides with whatever is looking, rather
    /// than staying at the node's own place. This is what makes a system the air:
    /// walk a mile and the rain comes with you instead of falling on where you were.
    on_sky: bool,
    /// The dice for everything after the first veil. The places a system starts
    /// with come from its seed by index; its replacements follow the frame.
    rng: Rng,
    live: Vec<Drop>,
    bursts: Vec<Burst>,
    /// How many drops the last step brought up short, for a script that wants to
    /// know whether its weather is hitting anything.
    impacts: usize,
}

impl Default for Particles {
    fn default() -> Self {
        Self {
            shape: ParticleShape::Streak,
            count: 400,
            size: 0.05,
            speed: 9.0,
            direction: Vec3::NEG_Z,
            area: Vec3::new(24.0, 24.0, 16.0),
            lifetime: 2.0,
            seed: Seed::DEFAULT,
            active: true,
            collide: true,
            splash: 0,
            recycle: true,
            on_sky: false,
            rng: Rng::new(Seed::DEFAULT),
            live: Vec::new(),
            bursts: Vec::new(),
            impacts: 0,
        }
    }
}

impl Scene {
    /// Puts an empty particle system in the tree. It has no body of its own - the
    /// crowd is what it draws - so it shows nothing until it is given a place: its
    /// `position` is the middle of the box it flies its crowd in.
    pub fn create_particles(
        &mut self,
        name: Option<String>,
        parent: Option<NodeId>,
    ) -> Result<NodeId, SceneError> {
        let parent_id = parent.unwrap_or(self.root());
        self.entity(parent_id)?;
        self.branch_allows(parent_id, false)?;

        let id = self.reserve();
        let entity = self
            .world
            .spawn((
                NodeName(name.unwrap_or_else(|| "Particles".to_owned())),
                NodeKind("particles".to_owned()),
                Transform::default(),
                ObjectColor(DEFAULT_COLOR),
                Opacity::default(),
                NodeTexture::default(),
                Visible::default(),
                Motion::default(),
                Parent(parent_id),
                Children::default(),
                Particles::default(),
            ))
            .id();
        self.insert(id, entity);
        self.attach(id, parent_id);
        self.prefill(id)?;

        Ok(id)
    }

    pub fn is_particle_system(&self, id: NodeId) -> Result<bool, SceneError> {
        let entity = self.entity(id)?;
        Ok(self.world.get::<Particles>(entity).is_some())
    }

    pub fn particle_shape(&self, id: NodeId) -> Result<ParticleShape, SceneError> {
        Ok(self.particles(id)?.shape)
    }

    /// What each thing in the crowd is cut as: `streak`, `square` or `circle`.
    /// Every knob of a system is part of what the thing is, so a linked copy is
    /// written with its group; what is already in the air is each system's own.
    pub fn set_particle_shape(&mut self, id: NodeId, shape: &str) -> Result<(), SceneError> {
        let shape = ParticleShape::from_name(shape)
            .ok_or_else(|| SceneError::UnknownParticleShape(shape.to_owned()))?;
        self.spread(id, move |scene, member| {
            scene.particles_mut(member)?.shape = shape;
            Ok(())
        })
    }

    pub fn particle_count(&self, id: NodeId) -> Result<usize, SceneError> {
        Ok(self.particles(id)?.count)
    }

    /// How many things the system is asked to fly. Growing it lays the new drops
    /// at their seeded places, so the ones already in the air keep the places they
    /// had; shrinking it takes the last ones off.
    pub fn set_particle_count(&mut self, id: NodeId, count: usize) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.set_particle_count_one(member, count)
        })
    }

    fn set_particle_count_one(&mut self, id: NodeId, count: usize) -> Result<(), SceneError> {
        let count = count.min(MAX_PARTICLES);
        let (area, lifetime, seed, active) = {
            let particles = self.particles(id)?;
            (
                particles.area,
                particles.lifetime,
                particles.seed,
                particles.active,
            )
        };
        self.particles_mut(id)?.count = count;
        if !active {
            return Ok(());
        }

        let origin = self.spawn_origin(id)?;
        let mut particles = self.particles_mut(id)?;
        if count < particles.live.len() {
            particles.live.truncate(count);
        } else {
            let from = particles.live.len();
            particles
                .live
                .extend((from..count).map(|index| born(index, origin, area, lifetime, seed)));
        }

        Ok(())
    }

    pub fn particle_size(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.particles(id)?.size)
    }

    /// Meters across. Every drop already flying takes up the new size at once.
    pub fn set_particle_size(&mut self, id: NodeId, size: f32) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.particles_mut(member)?.size = size.max(0.0);
            Ok(())
        })
    }

    pub fn particle_speed(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.particles(id)?.speed)
    }

    /// Meters per second along the way the system points.
    pub fn set_particle_speed(&mut self, id: NodeId, speed: f32) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.particles_mut(member)?.speed = speed.max(0.0);
            Ok(())
        })
    }

    pub fn particle_direction(&self, id: NodeId) -> Result<Vec3, SceneError> {
        Ok(self.particles(id)?.direction)
    }

    /// The way the crowd travels, as written on the node. A system turned sideways
    /// flies sideways, and one hung on a slope sprays along it. Asking for no
    /// direction at all means straight down rather than nowhere.
    pub fn set_particle_direction(
        &mut self,
        id: NodeId,
        direction: Vec3,
    ) -> Result<(), SceneError> {
        let direction = direction.normalize_or_zero();
        let direction = if direction == Vec3::ZERO {
            Vec3::NEG_Z
        } else {
            direction
        };
        self.spread(id, move |scene, member| {
            scene.particles_mut(member)?.direction = direction;
            Ok(())
        })
    }

    pub fn particle_area(&self, id: NodeId) -> Result<Vec3, SceneError> {
        Ok(self.particles(id)?.area)
    }

    /// The box new drops are placed in, meters, centred on the node. Like the
    /// lifetime, it bites on the drops the system lays next.
    pub fn set_particle_area(&mut self, id: NodeId, area: Vec3) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.particles_mut(member)?.area = area.max(Vec3::ZERO);
            Ok(())
        })
    }

    pub fn particle_lifetime(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.particles(id)?.lifetime)
    }

    /// Seconds a drop lives for. Short, and the crowd only ever shows a slice of
    /// its box; long, and it crosses the whole of it. This is the age a drop is
    /// *given*, so it bites on the ones the system lays next - restart it to lay
    /// the whole crowd over.
    pub fn set_particle_lifetime(&mut self, id: NodeId, seconds: f32) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.particles_mut(member)?.lifetime = seconds.max(0.0);
            Ok(())
        })
    }

    pub fn particle_seed(&self, id: NodeId) -> Result<u64, SceneError> {
        Ok(self.particles(id)?.seed.0)
    }

    /// The dice the crowd is laid out with. Setting it throws the veil away and
    /// lays a new one, which is the point: the same seed is the same weather.
    pub fn set_particle_seed(&mut self, id: NodeId, seed: Seed) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.set_particle_seed_one(member, seed)
        })
    }

    fn set_particle_seed_one(&mut self, id: NodeId, seed: Seed) -> Result<(), SceneError> {
        self.particles_mut(id)?.seed = seed;
        self.particles_mut(id)?.rng = Rng::new(seed);
        self.prefill(id)
    }

    pub fn particles_are_active(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.particles(id)?.active)
    }

    /// Whether the crowd is flying. An inactive system keeps its place, its name
    /// and every knob it was given, and shows and does nothing: switched off, not
    /// deleted. Switching it back on lays the veil again from where it stands.
    pub fn set_particles_active(&mut self, id: NodeId, active: bool) -> Result<(), SceneError> {
        if !active {
            let mut particles = self.particles_mut(id)?;
            particles.active = false;
            particles.live.clear();
            particles.bursts.clear();
            particles.impacts = 0;
            return Ok(());
        }

        self.particles_mut(id)?.active = true;
        self.prefill(id)
    }

    pub fn particles_collide(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.particles(id)?.collide)
    }

    /// Whether the floor and the bodies with colliders switched on stop this
    /// crowd. Off is the flat way to wear weather: the drops go straight through
    /// everything and only their own time runs them out.
    pub fn set_particles_collide(&mut self, id: NodeId, collide: bool) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.particles_mut(member)?.collide = collide;
            Ok(())
        })
    }

    pub fn particle_splash(&self, id: NodeId) -> Result<usize, SceneError> {
        Ok(self.particles(id)?.splash)
    }

    /// How many droplets an impact throws, or zero for a drop that simply stops.
    /// A splash is the last generation: what it throws does not splash in turn.
    pub fn set_particle_splash(&mut self, id: NodeId, count: usize) -> Result<(), SceneError> {
        let count = count.min(MAX_SPLASH_PER_HIT);
        self.spread(id, move |scene, member| {
            scene.particles_mut(member)?.splash = count;
            Ok(())
        })
    }

    pub fn particles_recycle(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.particles(id)?.recycle)
    }

    /// Whether a spent drop is born again. Off makes the system a burst: one
    /// crowd, thrown once, and then the air is clear.
    pub fn set_particles_recycle(&mut self, id: NodeId, recycle: bool) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.particles_mut(member)?.recycle = recycle;
            Ok(())
        })
    }

    pub fn particles_on_sky(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.particles(id)?.on_sky)
    }
    /// Whether the crowd is laid around the eyes rather than around the node. On,
    /// the system is the air over whatever the frame shows, which is how
    /// [`Scene::create_precipitation`] rains everywhere; off, it is a local effect
    /// at a place, like smoke off a chimney. The box moves at once, because a veil
    /// left behind where the player started is not weather.
    pub fn set_particles_on_sky(&mut self, id: NodeId, on_sky: bool) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.particles_mut(member)?.on_sky = on_sky;
            scene.prefill(member)
        })
    }

    /// Throws the crowd away and lays a new one from where the node stands now,
    /// which is what a script calls after it has moved, turned or re-hung a system:
    /// a drop already flying keeps the world it was born into, so a veil only comes
    /// to its new place a lifetime at a time unless it is laid again. A system that
    /// was switched off stays switched off.
    pub fn restart_particles(&mut self, id: NodeId) -> Result<(), SceneError> {
        let was_flying = self.particles_are_active(id)?;
        self.set_particles_active(id, false)?;
        if was_flying {
            self.set_particles_active(id, true)?;
        }
        Ok(())
    }

    /// How many things are in the air right now, the splash counted in. A system
    /// that ran out without recycling answers zero with its knobs still set.
    pub fn particles_in_air(&self, id: NodeId) -> Result<usize, SceneError> {
        let particles = self.particles(id)?;
        Ok(particles.live.len() + particles.bursts.len())
    }

    /// How many drops the last step brought up short.
    pub fn particle_impacts(&self, id: NodeId) -> Result<usize, SceneError> {
        Ok(self.particles(id)?.impacts)
    }

    /// The crowd as the frame should show it. A hidden system, and anything under
    /// one, is out of the frame; a switched-off one has nothing to show.
    pub fn particle_items(&self) -> Vec<ParticleItem<'_>> {
        let mut items = Vec::new();
        let mut stack = vec![(self.root(), false)];

        while let Some((id, hidden)) = stack.pop() {
            let Ok(entity) = self.entity(id) else {
                continue;
            };
            let hidden = hidden || !self.is_visible(id).unwrap_or(true);

            if !hidden
                && let Some(particles) = self.world.get::<Particles>(entity)
                && particles.active
            {
                let color = self
                    .world
                    .get::<ObjectColor>(entity)
                    .map(|color| color.0)
                    .unwrap_or(DEFAULT_COLOR);
                let alpha = self
                    .world
                    .get::<Opacity>(entity)
                    .map(|opacity| opacity.0)
                    .unwrap_or(MAX_ALPHA)
                    / MAX_ALPHA;
                let texture = self
                    .world
                    .get::<NodeTexture>(entity)
                    .and_then(|texture| texture.0.as_deref());
                let fall = self.world_rotation(id) * particles.direction;
                // The same lean the step moves the crowd with, so a streak points
                // the way it is actually going rather than the way it was written.
                let air = self.carried_by_the_air(self.spawn_origin(id).unwrap_or(Vec3::ZERO));

                for drop in &particles.live {
                    items.push(ParticleItem {
                        node: id,
                        at: drop.at,
                        size: particles.size * drop.scale,
                        velocity: fall * (particles.speed * drop.drag) + air,
                        shape: particles.shape,
                        color: (color * drop.shade).to_array(),
                        alpha,
                        texture,
                    });
                }
                for burst in &particles.bursts {
                    items.push(ParticleItem {
                        node: id,
                        at: burst.at,
                        size: particles.size * burst.scale,
                        velocity: burst.velocity,
                        shape: particles.shape,
                        color: color.to_array(),
                        alpha: alpha * (burst.left / SPLASH_LIFETIME).clamp(0.0, 1.0),
                        texture,
                    });
                }
            }

            if let Some(children) = self.world.get::<Children>(entity) {
                stack.extend(children.0.iter().rev().map(|child| (*child, hidden)));
            }
        }

        items
    }

    /// Runs every system that is flying: ages its crowd, moves it, asks whether a
    /// drop has been stopped, and throws the splash a system said it throws.
    pub(crate) fn run_particles(&mut self, dt: f32) {
        let tree = self.node_tree();
        let needs_blockers = tree.iter().any(|id| {
            self.particles(*id)
                .is_ok_and(|particles| particles.active && particles.collide)
        });
        let blockers = if needs_blockers {
            self.blockers()
        } else {
            Vec::new()
        };

        let mut budget = COLLISION_BUDGET;
        for id in tree {
            self.step_particles(id, dt, &blockers, &mut budget);
        }
    }

    /// The bodies a drop can be stopped by. UI is measured in pixels, so it is
    /// never one of them, and a body with no collider has nothing to hit.
    fn blockers(&self) -> Vec<Blocker> {
        self.colliders()
            .into_iter()
            .map(|(_, solid)| {
                let (min, max) = solid.aabb();
                Blocker { min, max, solid }
            })
            .collect()
    }

    fn step_particles(&mut self, id: NodeId, dt: f32, blockers: &[Blocker], budget: &mut usize) {
        let Ok(entity) = self.entity(id) else {
            return;
        };
        let Ok(particles) = self.particles(id) else {
            return;
        };
        if !particles.active {
            return;
        }
        let count = particles.count;
        let size = particles.size;
        let speed = particles.speed;
        let area = particles.area;
        let lifetime = particles.lifetime;
        let direction = particles.direction;
        let collide = particles.collide;
        let splash = particles.splash;
        let recycle = particles.recycle;

        let origin = self.spawn_origin(id).unwrap_or(Vec3::ZERO);
        let fall = self.world_rotation(id) * direction;
        let air = self.carried_by_the_air(origin);

        let Some(mut particles) = self.world.get_mut::<Particles>(entity) else {
            return;
        };
        let mut rng = particles.rng;
        particles.impacts = 0;

        let mut kept: Vec<Drop> = Vec::with_capacity(particles.live.len());
        for mut drop in mem::take(&mut particles.live) {
            drop.left -= dt;
            let velocity = fall * (speed * drop.drag) + air;
            drop.at += velocity * dt;
            let skin = size * drop.scale * 0.5;

            let hit = if collide {
                stopped(drop.at, skin, velocity, blockers, budget)
            } else {
                None
            };

            if let Some((normal, rest)) = hit {
                particles.impacts += 1;
                let room =
                    splash.min(MAX_SPLASH_DROPS - particles.bursts.len().min(MAX_SPLASH_DROPS));
                for _ in 0..room {
                    let thrown = normal * (speed * 0.35) + rng.inside(Vec3::splat(speed * 0.6));
                    particles.bursts.push(Burst {
                        at: rest,
                        velocity: thrown,
                        left: SPLASH_LIFETIME,
                        scale: (drop.scale * 0.4).min(1.0),
                    });
                }
                if recycle && count > 0 {
                    kept.push(reborn(&mut rng, origin, area, lifetime));
                }
            } else if drop.left <= 0.0 {
                if recycle && count > 0 {
                    kept.push(reborn(&mut rng, origin, area, lifetime));
                }
            } else {
                kept.push(drop);
            }
        }
        particles.live = kept;

        let mut alive: Vec<Burst> = Vec::with_capacity(particles.bursts.len());
        for mut burst in mem::take(&mut particles.bursts) {
            burst.left -= dt;
            if burst.left <= 0.0 {
                continue;
            }
            burst.velocity.z -= SPLASH_GRAVITY * dt;
            burst.at += burst.velocity * dt;
            // A thrown droplet does not sink into the ground it was thrown off.
            if burst.at.z < GROUND_HEIGHT {
                burst.at.z = GROUND_HEIGHT;
                burst.velocity.z = 0.0;
            }
            alive.push(burst);
        }
        particles.bursts = alive;
        particles.rng = rng;
    }

    /// The crowd written again onto a fresh node: the same knobs, the same seed,
    /// and nothing carried over from the air. What is already flying belongs to the
    /// system it was born in, so the copy lays its own veil at its own place.
    pub(crate) fn copy_particles(&mut self, from: NodeId, into: NodeId) -> Result<(), SceneError> {
        let copied = {
            let source = self.particles(from)?;
            Particles {
                shape: source.shape,
                count: source.count,
                size: source.size,
                speed: source.speed,
                direction: source.direction,
                area: source.area,
                lifetime: source.lifetime,
                seed: source.seed,
                active: source.active,
                collide: source.collide,
                splash: source.splash,
                recycle: source.recycle,
                on_sky: source.on_sky,
                rng: Rng::new(source.seed),
                live: Vec::new(),
                bursts: Vec::new(),
                impacts: 0,
            }
        };
        let entity = self.entity(into)?;
        self.world.entity_mut(entity).insert(copied);
        self.prefill(into)
    }

    /// Lays the whole veil at once, each drop part way through its life, so a
    /// system that is switched on is weather immediately instead of waiting one
    /// lifetime for the first drops to reach the middle of the box.
    fn prefill(&mut self, id: NodeId) -> Result<(), SceneError> {
        let (count, area, lifetime, seed, active) = {
            let particles = self.particles(id)?;
            (
                particles.count,
                particles.area,
                particles.lifetime,
                particles.seed,
                particles.active,
            )
        };

        let origin = self.spawn_origin(id)?;
        let mut particles = self.particles_mut(id)?;
        particles.live.clear();
        particles.bursts.clear();
        particles.impacts = 0;
        if !active || count == 0 {
            return Ok(());
        }

        particles
            .live
            .extend((0..count).map(|index| born(index, origin, area, lifetime, seed)));

        Ok(())
    }

    /// How the air this crowd stands in is being shoved by whatever disasters are
    /// running, as a pace in meters a second. Asked once at the crowd's middle
    /// rather than drop by drop: a gale leans a veil, it does not tumble it, and a
    /// calm air answers zero so the drops fall the way they were written.
    fn carried_by_the_air(&self, at: Vec3) -> Vec3 {
        self.acceleration_at(at) * AIR_PER_ACCEL
    }

    /// Where the crowd is laid around: the node's own place in the world, with
    /// every parent's transform put to it first - or the eyes, for a system that is
    /// the air rather than a thing at a place.
    fn spawn_origin(&self, id: NodeId) -> Result<Vec3, SceneError> {
        let particles = self.particles(id)?;
        if particles.on_sky {
            // The crowd is laid *over* the eyes, not through them: a box centred on
            // the viewer buries the whole lower half of the sky, and every drop in it
            // splashes on the ground the frame the system is meant to be falling in.
            return Ok(self.eyes() + Vec3::new(0.0, 0.0, particles.area.z * 0.5));
        }
        Ok(self.global_matrix(id)?.transform_point3(Vec3::ZERO))
    }

    /// The node's turning as the world sees it, parents composed from the root
    /// down. Only the turning: a system's box is in authored meters, so a stretched
    /// parent points the fall without inflating the area.
    fn world_rotation(&self, id: NodeId) -> Quat {
        let mut rotation = self
            .transform(id)
            .map(|transform| transform.rotation)
            .unwrap_or(Quat::IDENTITY);

        let mut current = self.parent(id).ok().flatten();
        while let Some(parent) = current {
            if let Ok(transform) = self.transform(parent) {
                rotation = transform.rotation * rotation;
            }
            current = self.parent(parent).ok().flatten();
        }

        rotation
    }

    fn particles(&self, id: NodeId) -> Result<&Particles, SceneError> {
        let entity = self.entity(id)?;
        self.world
            .get::<Particles>(entity)
            .ok_or(SceneError::NotAParticles)
    }

    fn particles_mut(&mut self, id: NodeId) -> Result<Mut<'_, Particles>, SceneError> {
        let entity = self.entity(id)?;
        self.world
            .get_mut::<Particles>(entity)
            .ok_or(SceneError::NotAParticles)
    }
}

/// Where the drop of index `index` first stands, with its own size, brightness and
/// pace rolled after it. The same index from the same seed is the same drop, which
/// is what [`scatter`](crate::procgen::scatter) promises of a set of places.
fn born(index: usize, origin: Vec3, area: Vec3, lifetime: f32, seed: Seed) -> Drop {
    let mut rng = Rng::new(salt(seed, index));
    Drop {
        at: origin + rng.inside(area),
        left: rng.between(0.0, lifetime),
        scale: rng.between(0.6, 1.4),
        shade: rng.between(0.7, 1.0),
        drag: rng.between(0.8, 1.2),
    }
}

/// A drop born from the frame's own dice rather than from its index: it has no
/// place a script could have predicted, only a place in the box, which is wherever
/// the node stands now.
fn reborn(rng: &mut Rng, origin: Vec3, area: Vec3, lifetime: f32) -> Drop {
    Drop {
        at: origin + rng.inside(area),
        left: lifetime,
        scale: rng.between(0.6, 1.4),
        shade: rng.between(0.7, 1.0),
        drag: rng.between(0.8, 1.2),
    }
}

/// Whether a drop has been stopped, answering with the way out of what it hit and
/// the place it should be left at. The floor is one of the things it can hit,
/// collider or no collider, because the world's floor is everyone's.
fn stopped(
    at: Vec3,
    skin: f32,
    velocity: Vec3,
    blockers: &[Blocker],
    budget: &mut usize,
) -> Option<(Vec3, Vec3)> {
    if velocity.z < 0.0 && at.z - skin <= GROUND_HEIGHT {
        return Some((Vec3::Z, Vec3::new(at.x, at.y, GROUND_HEIGHT + skin)));
    }

    for blocker in blockers {
        if at.x < blocker.min.x - skin
            || at.x > blocker.max.x + skin
            || at.y < blocker.min.y - skin
            || at.y > blocker.max.y + skin
            || at.z < blocker.min.z - skin
            || at.z > blocker.max.z + skin
        {
            continue;
        }
        if *budget == 0 {
            return None;
        }
        *budget -= 1;

        let Some(probe) = Solid::ball(at, skin) else {
            continue;
        };
        if let Some(contact) = geometry::contact(&blocker.solid, &probe) {
            return Some((contact.normal, at + contact.normal * contact.depth));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::VecField;

    const SECOND: f32 = 1.0 / 60.0;

    /// Switches a system off and back on, which is how a script moves a veil to
    /// where its node now stands: a drop already flying keeps its world.
    fn restart(scene: &mut Scene, id: NodeId) {
        scene.set_particles_active(id, false).unwrap();
        scene.set_particles_active(id, true).unwrap();
    }

    /// Whether every thing in the crowd is inside the box it was given, allowing
    /// for however far a whole lifetime of falling could have taken it out.
    fn inside_the_box(items: &[ParticleItem], origin: Vec3, area: Vec3, fallen: f32) -> bool {
        items.iter().all(|item| {
            let drift = (area * 0.5) + Vec3::splat(fallen + item.size);
            (item.at - origin).abs().cmple(drift).all()
        })
    }

    fn places(scene: &Scene) -> Vec<Vec3> {
        scene.particle_items().iter().map(|item| item.at).collect()
    }

    #[test]
    fn a_particle_system_is_a_node_and_not_a_register() {
        let mut scene = Scene::new();
        let rain = scene.create_particles(None, None).unwrap();

        assert_eq!(scene.kind(rain).unwrap(), "particles");
        assert_eq!(scene.name(rain).unwrap(), "Particles");
        assert_eq!(scene.parent(rain).unwrap(), Some(scene.root()));
        assert_eq!(scene.children(scene.root()).unwrap(), vec![rain]);
        assert!(scene.is_particle_system(rain).unwrap());
        assert!(
            !scene.has_body(rain).unwrap(),
            "the crowd is what a system draws, not a body of its own"
        );
        assert!(
            scene.draw_items().is_empty(),
            "a system contributes no mesh to the frame"
        );

        let chimney = scene.create("cube", None, None).unwrap();
        let smoke = scene
            .create_particles(Some("Smoke".to_owned()), Some(chimney))
            .unwrap();
        assert_eq!(scene.children(chimney).unwrap(), vec![smoke]);
        assert_eq!(scene.node_count(), 3);

        assert!(matches!(
            scene.particle_count(chimney),
            Err(SceneError::NotAParticles)
        ));
    }

    #[test]
    fn a_system_switched_on_is_weather_at_once() {
        let mut scene = Scene::new();
        let rain = scene.create_particles(None, None).unwrap();
        scene
            .set_vec_field(rain, VecField::Position, Vec3::new(0.0, 0.0, 20.0))
            .unwrap();
        restart(&mut scene, rain);

        assert_eq!(scene.particles_in_air(rain).unwrap(), 400);
        let items = scene.particle_items();
        assert_eq!(items.len(), 400);
        assert!(
            inside_the_box(
                &items,
                Vec3::new(0.0, 0.0, 20.0),
                Vec3::new(24.0, 24.0, 16.0),
                0.0
            ),
            "the first veil was laid outside the box it was given"
        );
        assert_eq!(
            scene.particle_impacts(rain).unwrap(),
            0,
            "nothing has stepped yet"
        );
    }

    #[test]
    fn the_crowd_falls_every_frame() {
        let mut scene = Scene::new();
        let rain = scene.create_particles(None, None).unwrap();
        scene
            .set_vec_field(rain, VecField::Position, Vec3::new(0.0, 0.0, 40.0))
            .unwrap();
        scene.set_particles_collide(rain, false).unwrap();
        scene.set_particles_recycle(rain, false).unwrap();
        scene.set_particle_lifetime(rain, 60.0).unwrap();
        restart(&mut scene, rain);

        let before = places(&scene);
        for _ in 0..10 {
            scene.step(SECOND);
        }
        let after = places(&scene);

        // A drop that ran out of its sixty seconds in the meantime is gone, and
        // one that was laid a hundredth above another keeps its x and y.
        assert!(
            after.len() >= before.len() - 3,
            "{} of {} drops went missing in a sixth of a second",
            before.len() - after.len(),
            before.len()
        );
        for place in &after {
            assert!(
                before
                    .iter()
                    .any(|from| from.x == place.x && from.y == place.y && place.z < from.z - 0.5),
                "a drop did not fall the way its system points: {place}"
            );
        }
        assert_eq!(scene.particles_in_air(rain).unwrap(), after.len());
    }

    #[test]
    fn a_spent_drop_is_born_again_until_the_system_stops_recycling() {
        let mut scene = Scene::new();
        let puff = scene.create_particles(None, None).unwrap();
        scene.set_particle_lifetime(puff, 0.2).unwrap();
        scene.set_particles_collide(puff, false).unwrap();
        scene.set_particles_recycle(puff, false).unwrap();
        restart(&mut scene, puff);

        for _ in 0..30 {
            scene.step(SECOND);
        }
        assert_eq!(
            scene.particles_in_air(puff).unwrap(),
            0,
            "a crowd that is not recycled came back on its own"
        );

        scene.set_particles_recycle(puff, true).unwrap();
        scene.set_particle_count(puff, 120).unwrap();
        assert_eq!(scene.particles_in_air(puff).unwrap(), 120);
        for _ in 0..120 {
            scene.step(SECOND);
        }
        assert_eq!(
            scene.particles_in_air(puff).unwrap(),
            120,
            "a recycled crowd thinned out instead of being laid again"
        );
    }

    #[test]
    fn switching_a_system_off_leaves_it_there_but_empty() {
        let mut scene = Scene::new();
        let rain = scene.create_particles(None, None).unwrap();
        assert_eq!(scene.particles_in_air(rain).unwrap(), 400);

        scene.set_particles_active(rain, false).unwrap();
        assert_eq!(scene.particle_count(rain).unwrap(), 400);
        assert!(!scene.particles_are_active(rain).unwrap());
        assert_eq!(scene.particles_in_air(rain).unwrap(), 0);
        assert!(scene.particle_items().is_empty());
        assert_eq!(scene.name(rain).unwrap(), "Particles");

        scene.step(SECOND);
        assert!(scene.particle_items().is_empty());
        assert_eq!(scene.particles_in_air(rain).unwrap(), 0);

        scene.set_particles_active(rain, true).unwrap();
        assert_eq!(scene.particles_in_air(rain).unwrap(), 400);
    }

    #[test]
    fn a_drop_brought_up_short_by_the_floor_is_counted_and_throws_its_splash() {
        let mut scene = Scene::new();
        let rain = scene.create_particles(None, None).unwrap();
        scene
            .set_vec_field(rain, VecField::Position, Vec3::new(0.0, 0.0, 3.0))
            .unwrap();
        scene
            .set_particle_area(rain, Vec3::new(20.0, 20.0, 1.0))
            .unwrap();
        restart(&mut scene, rain);

        for _ in 0..60 {
            scene.step(SECOND);
        }
        assert!(
            scene.particle_impacts(rain).unwrap() > 0,
            "a minute of rain never reached the ground"
        );
        assert_eq!(
            scene.particles_in_air(rain).unwrap(),
            400,
            "no splash was asked for, so nothing should be thrown"
        );

        scene.set_particle_splash(rain, 3).unwrap();
        for _ in 0..12 {
            scene.step(SECOND);
        }
        assert!(
            scene.particles_in_air(rain).unwrap() > 400,
            "the impacts threw no droplets"
        );

        let under_the_floor = scene
            .particle_items()
            .iter()
            .filter(|item| item.at.z < GROUND_HEIGHT)
            .count();
        assert_eq!(
            under_the_floor, 0,
            "{under_the_floor} things went under the floor of a system that collides"
        );
    }

    #[test]
    fn a_wall_stops_the_crowd_and_turning_collision_off_does_not() {
        let mut scene = Scene::new();
        let roof = scene.create("cube", None, None).unwrap();
        scene
            .set_vec_field(roof, VecField::Size, Vec3::new(8.0, 8.0, 0.5))
            .unwrap();
        scene
            .set_vec_field(roof, VecField::Position, Vec3::new(0.0, 0.0, 6.0))
            .unwrap();
        scene.set_collision(roof, true).unwrap();

        let rain = scene.create_particles(None, None).unwrap();
        scene
            .set_vec_field(rain, VecField::Position, Vec3::new(0.0, 0.0, 10.0))
            .unwrap();
        scene
            .set_particle_area(rain, Vec3::new(8.0, 8.0, 4.0))
            .unwrap();
        scene.set_particles_collide(rain, false).unwrap();
        restart(&mut scene, rain);

        for _ in 0..120 {
            scene.step(SECOND);
        }
        let through_the_roof = scene
            .particle_items()
            .iter()
            .filter(|item| item.at.z < 5.7 && item.at.x.abs() < 4.0 && item.at.y.abs() < 4.0)
            .count();
        assert!(
            through_the_roof > 20,
            "with collision off nothing fell through the roof: {through_the_roof} things"
        );

        scene.set_particles_collide(rain, true).unwrap();
        restart(&mut scene, rain);
        for _ in 0..120 {
            scene.step(SECOND);
        }
        // Strictly inside the slab: a droplet resting on the roof is legitimately
        // a skin's width above 6.25, and that is a contact, not a leak.
        let inside_the_roof = scene
            .particle_items()
            .iter()
            .filter(|item| {
                item.at.x.abs() < 4.0 && item.at.y.abs() < 4.0 && item.at.z > 5.8 && item.at.z < 6.2
            })
            .count();
        assert_eq!(
            inside_the_roof, 0,
            "a thing was left inside the body it collides with"
        );
    }

    #[test]
    fn the_same_seed_lays_the_same_crowd() {
        let mut first = Scene::new();
        let mut second = Scene::new();
        let a = first.create_particles(None, None).unwrap();
        let b = second.create_particles(None, None).unwrap();

        first.set_particle_seed(a, Seed::of("storm")).unwrap();
        second.set_particle_seed(b, Seed::of("storm")).unwrap();
        assert_eq!(places(&first), places(&second));

        second.set_particle_seed(b, Seed::of("storms")).unwrap();
        assert_ne!(
            places(&first),
            places(&second),
            "another seed laid the same weather"
        );

        // The same questions asked of the same steps get the same answers, which
        // is what a two player session needs from one seed.
        let mut third = Scene::new();
        let c = third.create_particles(None, None).unwrap();
        third.set_particle_seed(c, Seed::of("storm")).unwrap();
        for _ in 0..10 {
            first.step(SECOND);
            third.step(SECOND);
        }
        assert_eq!(places(&first), places(&third));
    }

    #[test]
    fn a_bigger_veil_keeps_the_drops_it_had() {
        let mut grown = Scene::new();
        let mut whole = Scene::new();
        let thin = grown.create_particles(None, None).unwrap();
        let thick = whole.create_particles(None, None).unwrap();

        grown.set_particle_count(thin, 100).unwrap();
        whole.set_particle_count(thick, 300).unwrap();
        let first_hundred = places(&grown);
        assert_eq!(first_hundred.len(), 100);

        grown.set_particle_count(thin, 300).unwrap();
        assert_eq!(
            places(&grown),
            places(&whole),
            "growing the veil moved the drops that were already hanging"
        );

        grown.set_particle_count(thin, 50).unwrap();
        assert_eq!(places(&grown)[..50], first_hundred[..50]);
    }

    #[test]
    fn the_count_is_stopped_before_it_eats_the_frame() {
        let mut scene = Scene::new();
        let rain = scene.create_particles(None, None).unwrap();
        scene.set_particle_count(rain, 1_000_000).unwrap();

        assert_eq!(scene.particle_count(rain).unwrap(), MAX_PARTICLES);
        assert_eq!(scene.particles_in_air(rain).unwrap(), MAX_PARTICLES);
        assert_eq!(scene.particle_items().len(), MAX_PARTICLES);
    }

    #[test]
    fn a_hidden_system_and_its_crowd_leave_the_frame() {
        let mut scene = Scene::new();
        let rain = scene.create_particles(None, None).unwrap();
        assert_eq!(scene.particle_items().len(), 400);

        scene.set_visible(rain, false).unwrap();
        assert!(scene.particle_items().is_empty());
        assert_eq!(scene.particles_in_air(rain).unwrap(), 400);

        scene.set_visible(rain, true).unwrap();
        assert_eq!(scene.particle_items().len(), 400);
    }

    #[test]
    fn colour_transparency_image_and_shape_reach_the_renderer() {
        let mut scene = Scene::new();
        let snow = scene.create_particles(None, None).unwrap();
        scene.set_particle_shape(snow, "circle").unwrap();
        scene.set_color_hex(snow, "#dff3ff").unwrap();
        scene.set_opacity(snow, 128.0).unwrap();
        let flake = crate::test_skin("particles-flake");
        scene.set_texture(snow, Some(flake.clone())).unwrap();

        let items = scene.particle_items();
        let shape = items[0].shape;
        assert_eq!(shape, ParticleShape::Circle);
        assert_eq!(shape.name(), "circle");
        assert_eq!(shape.stretch(), 1.0);
        assert_eq!(ParticleShape::Streak.stretch(), 6.0);
        assert_eq!(items[0].texture, Some(flake.as_str()));
        assert!((items[0].alpha - 128.0 / MAX_ALPHA).abs() < 1e-6);
        assert!(
            items
                .iter()
                .all(|item| item.color[0] <= 223.0 / 255.0 + 1e-6 && item.color[0] > 0.5),
            "the crowd is not the colour it was given"
        );
        assert!(
            items.iter().any(|item| item.size != items[0].size),
            "every drop is exactly the same size"
        );

        assert!(matches!(
            scene.set_particle_shape(snow, "hexagon"),
            Err(SceneError::UnknownParticleShape(_))
        ));
    }

    #[test]
    fn the_crowd_points_the_way_its_node_is_turned() {
        let mut scene = Scene::new();
        let rain = scene.create_particles(None, None).unwrap();
        scene.set_particles_collide(rain, false).unwrap();
        scene
            .set_vec_field(rain, VecField::RotationEuler, Vec3::new(0.0, -90.0, 0.0))
            .unwrap();

        let items = scene.particle_items();
        let first = &items[0];
        assert!(
            first.velocity.x > first.velocity.length() * 0.9,
            "a system turned onto its side still flies down: {:?}",
            first.velocity
        );
        assert!(first.velocity.z.abs() < 1e-3);

        let speeds: Vec<f32> = items.iter().map(|item| item.velocity.length()).collect();
        assert!(
            speeds
                .iter()
                .all(|speed| (speed - 9.0).abs() < 9.0 * 0.2 + 1e-3)
        );
        assert!(
            speeds.windows(2).any(|pair| pair[0] != pair[1]),
            "every drop flies at exactly the same pace"
        );

        scene.set_particle_speed(rain, 20.0).unwrap();
        assert!(
            scene.particle_items()[0].velocity.length() >= 20.0 * 0.8 - 1e-3,
            "a faster system kept its old crowd"
        );
    }

    #[test]
    fn a_parent_moves_the_box_and_leans_the_whole_crowd() {
        let mut scene = Scene::new();
        let mast = scene.create("cube", None, None).unwrap();
        let rain = scene.create_particles(None, Some(mast)).unwrap();
        scene.set_particles_collide(rain, false).unwrap();
        scene
            .set_vec_field(mast, VecField::Position, Vec3::new(4.0, 0.0, 12.0))
            .unwrap();
        restart(&mut scene, rain);

        let items = scene.particle_items();
        assert!(
            items
                .iter()
                .all(|item| (item.at.x - 4.0).abs() <= 12.0 + item.size
                    && item.at.z <= 20.0 + item.size),
            "the box did not move with the node it hangs on"
        );

        scene
            .set_vec_field(mast, VecField::RotationEuler, Vec3::new(0.0, 60.0, 0.0))
            .unwrap();
        let tilted = scene.particle_items();
        assert!(
            tilted
                .iter()
                .all(|item| item.velocity.x.abs() > item.velocity.z.abs()),
            "the crowd did not lean with its parent"
        );
    }

    #[test]
    fn destroying_a_system_takes_its_crowd_with_it() {
        let mut scene = Scene::new();
        let rain = scene.create_particles(None, None).unwrap();
        assert_eq!(scene.particle_items().len(), 400);

        scene.destroy(rain).unwrap();
        assert!(scene.particle_items().is_empty());
        scene.step(SECOND);
        assert!(matches!(
            scene.particles_in_air(rain),
            Err(SceneError::NodeDestroyed)
        ));
    }

    #[test]
    fn a_crowd_can_be_thrown_the_other_way_and_up_a_slope() {
        let mut scene = Scene::new();
        let spray = scene.create_particles(None, None).unwrap();
        scene
            .set_particle_direction(spray, Vec3::new(0.0, 0.0, 4.0))
            .unwrap();
        assert_eq!(scene.particle_direction(spray).unwrap(), Vec3::Z);
        scene.set_particles_collide(spray, false).unwrap();
        scene.set_particles_recycle(spray, false).unwrap();
        scene.set_particle_lifetime(spray, 60.0).unwrap();
        scene
            .set_vec_field(spray, VecField::Position, Vec3::new(0.0, 0.0, 1.0))
            .unwrap();
        scene
            .set_particle_area(spray, Vec3::new(2.0, 2.0, 1.0))
            .unwrap();
        restart(&mut scene, spray);

        let before = places(&scene);
        for _ in 0..10 {
            scene.step(SECOND);
        }
        let after = places(&scene);
        // A drop whose sixty seconds ran out in the meantime is gone; the rest are
        // the same crowd, thrown up.
        assert!(
            after.len() >= before.len() - 3,
            "{} of {} drops went missing in a sixth of a second",
            before.len() - after.len(),
            before.len()
        );
        // A spray pointed straight up keeps its x and y, so every thing that came
        // back is the same thing, higher.
        for place in &after {
            assert!(
                before
                    .iter()
                    .any(|from| from.x == place.x && from.y == place.y && place.z > from.z),
                "a system pointed up still fell down: {place}"
            );
        }

        scene.set_particle_direction(spray, Vec3::ZERO).unwrap();
        assert_eq!(
            scene.particle_direction(spray).unwrap(),
            Vec3::NEG_Z,
            "no direction asked for should mean nowhere, not down"
        );
    }

    /// A crowd is not a body and is never pushed for a living, but the air it hangs
    /// in is being shoved by whatever the disasters answer, so a running wind leans
    /// the way it travels. Nothing new is asked of a script to make it happen.
    #[test]
    fn a_running_wind_walks_the_crowd_sideways() {
        let mut scene = Scene::new();
        let rain = scene.create_particles(None, None).unwrap();
        scene
            .set_vec_field(rain, VecField::Position, Vec3::new(0.0, 0.0, 20.0))
            .unwrap();
        scene.set_particles_collide(rain, false).unwrap();
        scene.restart_particles(rain).unwrap();

        let calm = scene.particle_items()[0].velocity;
        assert_eq!(calm.x, 0.0);
        assert!(calm.z < 0.0, "rain that will not fall down");

        let gale = scene
            .create_disaster("wind", Some("Gale".to_owned()))
            .unwrap();
        scene.set_disaster_direction(gale, Vec3::X).unwrap();
        scene.set_disaster_magnitude(gale, 6.0).unwrap();
        scene.set_disaster_reach(gale, 200.0).unwrap();
        scene.start_disaster(gale).unwrap();

        let blown = scene.particle_items()[0].velocity;
        assert!(blown.x > 1.0, "a magnitude 6 gale leaned it by {}", blown.x);
        assert_eq!(
            blown.z, calm.z,
            "the wind should walk the crowd sideways, not speed it up on the way down"
        );

        // And the crowd actually goes where it is drawn as going.
        let before = scene.particle_items()[0].at;
        scene.step(SECOND);
        let after = scene.particle_items()[0].at;
        assert!(after.x > before.x, "the drops never leaned");
        assert!(after.z < before.z);
    }
}
