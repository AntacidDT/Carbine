//! The air: how the environment looks, as the engine owns it.
//!
//! Three things make a frame feel like a place rather than a void with objects in
//! it: what the backdrop is, how thick the air is, and what is falling through it.
//! All three are the renderer's business, so they live here once and a script
//! channel and an editor channel get the same afternoon.
//!
//! Only the falling part is content, and it is not modelled twice: rain, snow,
//! dust and ash are presets on the *particle generator*, so
//! [`Scene::create_precipitation`] writes an ordinary particle system with the
//! numbers that kind of weather wants. `carbine.atmosphere.new("rain")` is the
//! fast way to get a downpour; [`crate::particles`] is what it is made of, and
//! anything a preset does a script can do itself.
//!
//! Fog is deliberately not a particle system. It is not made of anything you could
//! catch: it is the air refusing to show far things, so it is one percent on the
//! register and a haze the world pass mixes toward by how far each pixel stands
//! from the eyes.

use glam::Vec3;

use crate::node::{NodeId, Scene, SceneError, hex_of, parse_hex_color};
use crate::particles::ParticleShape;
use crate::primitives::default_name;

/// The foggiest air there is, in haze per meter: at this thickness the air has
/// mostly given up by six meters, which is what a whiteout actually is.
const HAZE_PER_METRE_AT_FULL: f32 = 0.5;

/// The most fog a script can ask for, read as a percent of that thickness.
const MAX_FOG: f32 = 100.0;

/// The thickest air a name-less atmosphere starts with - no fog at all, which is
/// what a world with nothing written over it is.
const NO_FOG: f32 = 0.0;

/// Hours past midnight the sun is on the horizon going up.
const SUNRISE: f32 = 6.0;

/// ... and going down. Between these two the day runs.
const SUNSET: f32 = 18.0;

/// The lean that keeps the sun off a straight vertical line, so a cube at midday
/// still shows three differently lit faces.
const SUN_LEAN: f32 = -0.35;

const NIGHT_ZENITH: Vec3 = Vec3::new(0.024, 0.031, 0.071);
const NIGHT_HORIZON: Vec3 = Vec3::new(0.086, 0.094, 0.137);
const DAY_ZENITH: Vec3 = Vec3::new(0.184, 0.424, 0.702);
const DAY_HORIZON: Vec3 = Vec3::new(0.812, 0.882, 0.933);
/// What the band along the horizon turns into when the sun is on it.
const DUSK_HORIZON: Vec3 = Vec3::new(0.945, 0.478, 0.235);

/// What one of the falling kinds is made of: the shape, thickness, pace and colour
/// the particle generator is written with. These are the numbers an atmosphere is
/// supposed to spare a script, so they are the whole of a preset.
struct Preset {
    shape: ParticleShape,
    count: usize,
    size: f32,
    speed: f32,
    area: Vec3,
    lifetime: f32,
    color: Vec3,
    splash: usize,
}

/// What can fall out of the sky, as the fast path names it. Every one of these is
/// a particle system afterwards - a real node with a crowd around it, which can be
/// hung on the sky or moved somewhere else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Precipitation {
    /// Streaks, straight down when the air is calm and slanted when it is not.
    Rain,
    /// Flakes, slow enough to be blown about.
    Snow,
    /// Small motes, drifting.
    Dust,
    /// Larger chips, and dark.
    Ash,
}

impl Precipitation {
    /// The kinds `atmosphere.new()` accepts, in the order they are listed when one
    /// is wrong. Fog is not among them because fog does not fall.
    pub const NAMES: &'static [&'static str] = &["ash", "dust", "rain", "snow"];

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.trim().to_lowercase().as_str() {
            "ash" | "cinder" | "emberfall" => Self::Ash,
            "dust" | "sand" | "grit" => Self::Dust,
            "rain" | "drizzle" | "shower" | "downpour" => Self::Rain,
            "snow" | "blizzard" | "sleet" => Self::Snow,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Ash => "ash",
            Self::Dust => "dust",
            Self::Rain => "rain",
            Self::Snow => "snow",
        }
    }

    /// How far a drop of this kind falls in one life. The lifetime is the box
    /// height over the pace, given a bit of room, so the crowd is spread through
    /// the whole of its box instead of piling up at the top of it.
    fn preset(self) -> Preset {
        match self {
            Self::Rain => Preset {
                shape: ParticleShape::Streak,
                count: 1400,
                size: 0.035,
                speed: 9.0,
                area: Vec3::new(70.0, 70.0, 45.0),
                lifetime: 5.5,
                color: Vec3::new(0.725, 0.827, 0.902),
                splash: 2,
            },
            Self::Snow => Preset {
                shape: ParticleShape::Circle,
                count: 900,
                size: 0.07,
                speed: 1.6,
                area: Vec3::new(60.0, 60.0, 40.0),
                lifetime: 28.0,
                color: Vec3::new(0.949, 0.965, 0.980),
                splash: 0,
            },
            Self::Dust => Preset {
                shape: ParticleShape::Circle,
                count: 700,
                size: 0.045,
                speed: 1.0,
                area: Vec3::new(50.0, 50.0, 30.0),
                lifetime: 34.0,
                color: Vec3::new(0.784, 0.639, 0.455),
                splash: 0,
            },
            Self::Ash => Preset {
                shape: ParticleShape::Square,
                count: 600,
                size: 0.09,
                speed: 0.8,
                area: Vec3::new(50.0, 50.0, 35.0),
                lifetime: 45.0,
                color: Vec3::new(0.561, 0.561, 0.588),
                splash: 0,
            },
        }
    }
}

/// How much work the engine is allowed to do lighting a frame.
///
/// A ladder, not a bag of switches: a script picks a rung and the rungs below it
/// are what that rung costs on a worse machine. `auto` is not a rung of its own -
/// it is the engine reading the graphics device it got and standing on whichever
/// rung that device can hold, which is what a fresh world is standing on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LightQuality {
    /// Whatever this machine can hold. See [`auto_quality`].
    Auto,
    /// Only the sun direction and an ambient term: the whole world in one pass
    /// with nothing to pre-render. This is the cheapest a lit scene can be.
    Low,
    /// The sun casts, the sky lends a little light back, and a face pointing at
    /// the sun carries a hint of sheen. One small pre-pass per frame.
    Medium,
    /// The same as [`Self::Medium`] with a larger cast, a wider reach and a
    /// softer edge to the shade.
    High,
    /// The full reach and the finest cast the engine will build, for a machine
    /// that has memory to spare.
    Extra,
}

impl LightQuality {
    /// The rungs `atmosphere.lightQuality` accepts, in the order they are listed
    /// when one is wrong.
    pub const NAMES: &'static [&'static str] = &["auto", "extra", "high", "low", "medium"];

    /// The ladder from cheapest to dearest, which is the order a quality menu
    /// lists it in. `auto` is last because it is not a rung of its own.
    pub const LADDER: &'static [Self] =
        &[Self::Low, Self::Medium, Self::High, Self::Extra, Self::Auto];

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.trim().to_lowercase().as_str() {
            "auto" | "best" | "default" => Self::Auto,
            "extra" | "max" | "ultra" => Self::Extra,
            "high" | "great" => Self::High,
            "low" | "fast" => Self::Low,
            "medium" | "mid" | "balanced" => Self::Medium,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Extra => "extra",
            Self::High => "high",
            Self::Low => "low",
            Self::Medium => "medium",
        }
    }
}

/// What the light pass is told to do, once a rung has been resolved.
///
/// These are engine numbers, not a script's: a script moves a rung and this is
/// what moves with it. Every field has to be answerable on any machine, because
/// an editor draws its quality menu out of here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    /// Which rung these numbers came from. Always a real rung, never `auto`.
    pub quality: LightQuality,
    /// Whether the sun pre-renders the world to cast with. False only at `low`.
    pub shadows: bool,
    /// Texels on a side of the cast, or 0 when there is no cast.
    pub shadow_map: u32,
    /// How many readings a pixel takes around its place in the cast. One is a
    /// hard edge; more is a shade that fades where it ends.
    pub softness: u32,
    /// Metres around the eyes the cast covers. Anything outside it is lit but
    /// casts nothing, which is how a big reach stays cheap.
    pub reach: f32,
    /// How much of the sun a face throws back as a sheen, 0..1. 0 is chalk.
    pub gloss: f32,
    /// Whether the light the sun leaves out comes from the backdrop itself,
    /// so a north face is sky-blue at noon and starry at midnight.
    pub sky_fill: bool,
}

impl Light {
    /// The numbers a rung is worth. Two rungs are load-bearing and cannot move:
    /// [`LightQuality::Low`] is the bare lambert world every pixel test was
    /// measured against, and [`LightQuality::Medium`] is what a plain laptop gets.
    pub fn of(quality: LightQuality) -> Self {
        let (shadows, shadow_map, softness, reach, gloss, sky_fill) = match quality {
            LightQuality::Low | LightQuality::Auto => (false, 0, 0, 0.0, 0.0, false),
            LightQuality::Medium => (true, 1024, 4, 60.0, 0.25, true),
            LightQuality::High => (true, 2048, 8, 90.0, 0.45, true),
            LightQuality::Extra => (true, 4096, 16, 160.0, 0.6, true),
        };
        Self {
            quality,
            shadows,
            shadow_map,
            softness,
            reach,
            gloss,
            sky_fill,
        }
    }
}

/// The kind of graphics device the engine was given, as the core is told it.
///
/// The core does not know what wgpu is, so the renderer names the class and the
/// core only sorts it. `Integrated` is the interesting one: it owns no memory of
/// its own and borrows the machine's instead, which is the first band.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceKind {
    Cpu,
    Integrated,
    Discrete,
    Virtual,
    Other,
}

impl DeviceKind {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.trim().to_lowercase().as_str() {
            "cpu" | "software" | "llvmpipe" => Self::Cpu,
            "integrated" => Self::Integrated,
            "discrete" => Self::Discrete,
            "virtual" => Self::Virtual,
            "other" => Self::Other,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Integrated => "integrated",
            Self::Discrete => "discrete",
            Self::Virtual => "virtual",
            Self::Other => "other",
        }
    }
}

/// A card that owns less than this has nothing worth casting with; four gigabytes
/// is where a card has room for a big cast and a crowded world both, and a card
/// only takes the high rung once it owns more than that.
const SMALL_CARD: u64 = 100 * 1024 * 1024;
const BIG_CARD: u64 = 4 * 1024 * 1024 * 1024;

/// The rung a machine can hold, from what the renderer found out about it.
///
/// The rule a script would otherwise have to guess at: a card owning more than
/// four gigabytes takes the high rung; one owning from a tenth of a gigabyte up
/// to four takes the middle; anything smaller, and anything that owns nothing and
/// borrows the machine's memory, takes the lowest. A discrete card that will not
/// say how big it is still gets the middle, because it is a card.
///
/// A machine the renderer has not described at all is not this function's
/// business: [`Scene::light`] answers that with the middle.
///
/// `extra` is never the answer. A script has to ask for it by name, because the
/// engine cannot tell from a memory count that a card wants a four-thousand-texel
/// cast thrown over its shoulder all day.
pub fn auto_quality(kind: DeviceKind, vram: Option<u64>) -> LightQuality {
    match kind {
        DeviceKind::Discrete => match vram {
            Some(bytes) if bytes > BIG_CARD => LightQuality::High,
            Some(bytes) if bytes >= SMALL_CARD => LightQuality::Medium,
            Some(_) => LightQuality::Low,
            None => LightQuality::Medium,
        },
        DeviceKind::Cpu | DeviceKind::Integrated | DeviceKind::Virtual | DeviceKind::Other => {
            LightQuality::Low
        }
    }
}

/// What the renderer found out about the device it was handed.
///
/// A best-effort reading, not a guarantee: a machine that will not be read leaves
/// [`Gpu::vram`] empty and the engine judges it by its class alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gpu {
    pub kind: DeviceKind,
    /// Bytes the card owns outright. `None` for a card that borrows the machine's
    /// memory, and for one that will not say how much it has.
    pub vram: Option<u64>,
}

/// The air, as the renderer is told to see it.
///
/// Times are hours past midnight on the 0..24 clock, wrapped, so `25.5` is half
/// past one in the morning. Writing the time is what moves the sun and recolours
/// the backdrop; the sky keeps its own knobs afterwards, because a script may want
/// a red noon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    /// How much of the air is fog, 0 to 100.
    pub fog: f32,
    /// What the fog is coloured with, sRGB channels in 0..1.
    pub fog_color: Vec3,
    /// What the clock last said.
    pub time: f32,
    /// Which rung the light is worked at. `auto` until a script says otherwise,
    /// which is the rung this machine can hold.
    pub quality: LightQuality,
}

impl Default for Atmosphere {
    fn default() -> Self {
        Self {
            fog: NO_FOG,
            fog_color: Vec3::new(0.761, 0.800, 0.831),
            time: 15.0,
            quality: LightQuality::Auto,
        }
    }
}

/// Hours past midnight a `"HH:MM"` asks for. A bare hour count is accepted too, so
/// `"7"` and `"7.5"` both work, and anything the clock cannot be is refused.
fn hours_of(clock: &str) -> Option<f32> {
    let clock = clock.trim();
    if clock.is_empty() {
        return None;
    }
    if let Ok(hours) = clock.parse::<f32>() {
        return Some(hours).filter(|hours| hours.is_finite());
    }

    let (hours, minutes) = clock.split_once(':')?;
    let hours = hours.trim().parse::<f32>().ok()?;
    let minutes = minutes.trim().parse::<f32>().ok()?;
    if !hours.is_finite() || !minutes.is_finite() {
        return None;
    }
    if hours < 0.0 || hours > 24.0 || minutes < 0.0 || minutes >= 60.0 {
        return None;
    }
    Some(hours + minutes / 60.0)
}

/// The direction the light arrives from at this hour, always a unit one. It climbs
/// the sky from `SUNRISE` to noon and goes back down the other side of the world
/// to `SUNSET`; the rest of the twenty-four hours it is under the floor, which is
/// what makes a night face nothing lit at all.
fn sun_at(hours: f32) -> Vec3 {
    let phase = (hours - SUNRISE) / (SUNSET - SUNRISE) * std::f32::consts::PI;
    Vec3::new(SUN_LEAN, phase.cos(), phase.sin()).normalize()
}

/// How much of the day's light this height of the sun carries, 0..1. It reaches
/// full daylight well before the sun is overhead, because a sky at forty-five
/// degrees is already bright.
fn daylight_of(sun: Vec3) -> f32 {
    ((sun.z + 0.1) / 0.6).clamp(0.0, 1.0)
}

/// The backdrop and the ambient this hour is lit with: the night colours, the day
/// colours, and the band of dusk along the horizon where the two hand over.
fn colours_of(sun: Vec3) -> (Vec3, Vec3, f32) {
    let day = daylight_of(sun);
    let zenith = NIGHT_ZENITH + (DAY_ZENITH - NIGHT_ZENITH) * day;
    let horizon = NIGHT_HORIZON + (DAY_HORIZON - NIGHT_HORIZON) * day;
    // The glow only shows while the sun is near the line, on the way up or down.
    let glow = (1.0 - (sun.z / 0.35).abs()).clamp(0.0, 1.0) * 0.9;
    let horizon = horizon + (DUSK_HORIZON - horizon) * glow;
    (zenith, horizon, 0.12 + 0.18 * day)
}

impl Scene {
    pub fn atmosphere(&self) -> Atmosphere {
        self.atmosphere
    }

    /// How much of the air is fog, 0 to 100. Past either end the engine stops
    /// listening rather than inventing a fourth meaning for a number.
    pub fn set_fog(&mut self, fog: f32) {
        self.atmosphere.fog = fog.clamp(NO_FOG, MAX_FOG);
    }

    pub fn set_fog_color(&mut self, color: Vec3) {
        self.atmosphere.fog_color = color;
    }

    /// The fog colour written the way an object colour is written.
    pub fn set_fog_color_hex(&mut self, hex: &str) -> Result<(), SceneError> {
        parse_hex_color(hex)
            .map(|(color, _)| self.set_fog_color(color))
            .ok_or_else(|| SceneError::BadColor(hex.to_owned()))
    }

    /// How fast the air this fog fills loses sight of a distance, per meter, with
    /// its colour in the first three channels. Quadratic in the fog percent, so
    /// twice the fog is four times the thickness: a light 20 leaves the far end of
    /// a field visible where a 40 has swallowed it.
    ///
    /// A pixel two of these away from the eyes keeps a fifth of what it was.
    pub fn haze(&self) -> [f32; 4] {
        let fraction = self.atmosphere.fog / MAX_FOG;
        let density = HAZE_PER_METRE_AT_FULL * fraction * fraction;
        let color = self.atmosphere.fog_color;
        [color.x, color.y, color.z, density]
    }

    /// What the light pass is told to do this frame. This is what the renderer
    /// reads: a script never has to know which rung it got.
    pub fn light(&self) -> Light {
        Light::of(self.light_quality())
    }

    /// Which rung the light is worked at, with `auto` already answered. A machine
    /// the renderer has not described - a headless scene, a test - gets the middle
    /// rather than a guess about hardware that is not there.
    pub fn light_quality(&self) -> LightQuality {
        match self.atmosphere.quality {
            LightQuality::Auto => self
                .gpu
                .map(|gpu| auto_quality(gpu.kind, gpu.vram))
                .unwrap_or(LightQuality::Medium),
            rung => rung,
        }
    }

    /// The rung a script actually wrote, which may still be `auto`. An editor's
    /// menu reads this, so that it can show "auto" and not the rung behind it.
    pub fn written_light_quality(&self) -> LightQuality {
        self.atmosphere.quality
    }

    pub fn set_light_quality(&mut self, quality: LightQuality) {
        self.atmosphere.quality = quality;
    }

    /// The rung written the way a name is written, `"high"` and friends.
    pub fn set_light_quality_name(&mut self, name: &str) -> Result<(), SceneError> {
        let quality = LightQuality::from_name(name)
            .ok_or_else(|| SceneError::UnknownLightQuality(name.trim().to_owned()))?;
        self.set_light_quality(quality);
        Ok(())
    }

    /// Told by the renderer what graphics device it was actually given. This is
    /// what lets an unread world stand `auto` and still pick a sane rung.
    pub fn report_gpu(&mut self, kind: DeviceKind, vram: Option<u64>) {
        self.gpu = Some(Gpu { kind, vram });
    }

    /// What the renderer reported, or nothing before it has drawn a frame.
    pub fn gpu(&self) -> Option<Gpu> {
        self.gpu
    }

    /// Hours past midnight the world is lit for.
    pub fn time_of_day(&self) -> f32 {
        self.atmosphere.time
    }

    /// Puts the sun and the backdrop at this hour. Anything outside a day is read
    /// as the same clock again, so 30.0 is two in the morning.
    pub fn set_time_of_day(&mut self, hours: f32) {
        let hours = if hours.is_finite() {
            hours.rem_euclid(24.0)
        } else {
            self.atmosphere.time
        };
        self.atmosphere.time = hours;

        let sun = sun_at(hours);
        let (zenith, horizon, ambient) = colours_of(sun);
        self.sky.sun = sun;
        self.sky.zenith = zenith;
        self.sky.horizon = horizon;
        self.sky.ambient = ambient;
    }

    /// The same hour written the way a clock is: `"07:30"`, `"12"` or `7.5`.
    pub fn set_time_from_clock(&mut self, clock: &str) -> Result<(), SceneError> {
        let hours = hours_of(clock).ok_or_else(|| SceneError::BadTime(clock.to_owned()))?;
        self.set_time_of_day(hours);
        Ok(())
    }

    /// Writes one of the falling kinds as the particle system it is: the preset's
    /// shape, crowd, pace, colour and box, laid around whatever is looking, so
    /// `atmosphere.new("rain")` rains everywhere the sky stands over.
    pub fn create_precipitation(
        &mut self,
        kind: &str,
        name: Option<String>,
    ) -> Result<NodeId, SceneError> {
        let kind = Precipitation::from_name(kind)
            .ok_or_else(|| SceneError::UnknownAtmosphere(kind.trim().to_owned()))?;
        let preset = kind.preset();
        let id = self.create_particles(
            Some(name.unwrap_or_else(|| default_name(kind.name()))),
            None,
        )?;

        self.set_particle_shape(id, preset.shape.name())?;
        self.set_particle_size(id, preset.size)?;
        self.set_particle_speed(id, preset.speed)?;
        self.set_particle_area(id, preset.area)?;
        self.set_particle_lifetime(id, preset.lifetime)?;
        self.set_particle_splash(id, preset.splash)?;
        self.set_particle_count(id, preset.count)?;
        self.set_color_hex(id, &hex_of(preset.color))?;
        // Last, because it lays the veil: on the sky the box rides with the eyes,
        // and the crowd a preset wants is the one its own knobs named.
        self.set_particles_on_sky(id, true)?;
        Ok(id)
    }

    /// The weather a script has put on the sky and switched on, in the order it
    /// was written. These are ordinary nodes, so this is a look at the tree rather
    /// than at a register.
    pub fn precipitation_ids(&self) -> Vec<NodeId> {
        self.node_tree()
            .into_iter()
            .filter(|id| self.rains_over_the_view(*id))
            .collect()
    }

    /// Switches every sky-borne system off at once, which is what a test teardown
    /// and an editor's clear button both want. A system hung on a chimney is left
    /// alone: it is not the air.
    pub fn stop_all_precipitation(&mut self) {
        let ids = self.node_tree();
        for id in ids {
            if self.rains_over_the_view(id) {
                let _ = self.set_particles_active(id, false);
            }
        }
    }

    /// Whether this node is the air rather than a local effect: a crowd of its own,
    /// flying, and laid around the eyes instead of around its place.
    fn rains_over_the_view(&self, id: NodeId) -> bool {
        self.particles_on_sky(id).unwrap_or(false) && self.particles_are_active(id).unwrap_or(false)
    }

    /// Where the frame is being looked from: the player's feet when the engine is
    /// driving someone around, and the camera otherwise. Sky-borne weather is
    /// laid around this, so a downpour follows the eyes instead of raining on
    /// where the player happened to be standing at the start.
    pub(crate) fn eyes(&self) -> Vec3 {
        if self.player.enabled {
            self.player.position
        } else {
            self.camera.position
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::VecField;

    #[test]
    fn the_air_starts_clear_and_in_the_afternoon() {
        let scene = Scene::new();
        let air = scene.atmosphere();

        assert_eq!(air.fog, 0.0);
        assert_eq!(scene.haze()[3], 0.0);
        // The default light is the one the sky was drawn with, so a fresh world is
        // lit rather than night.
        assert!(scene.sky().sun.z > 0.4);
        assert!(air.time > 12.0 && air.time < 18.0);
    }

    #[test]
    fn fog_is_a_percent_that_stops_listening_at_both_ends() {
        let mut scene = Scene::new();

        scene.set_fog(20.0);
        assert_eq!(scene.atmosphere().fog, 20.0);
        scene.set_fog(500.0);
        assert_eq!(scene.atmosphere().fog, MAX_FOG);
        scene.set_fog(-3.0);
        assert_eq!(scene.atmosphere().fog, 0.0);
    }

    #[test]
    fn a_fog_gets_thicker_four_times_as_fast_as_it_gets_doubled() {
        let mut scene = Scene::new();

        scene.set_fog(20.0);
        let light = scene.haze()[3];
        scene.set_fog(40.0);
        let heavy = scene.haze()[3];

        assert!((heavy - 4.0 * light).abs() < 1e-6);
        scene.set_fog(100.0);
        assert!((scene.haze()[3] - HAZE_PER_METRE_AT_FULL).abs() < 1e-6);
    }

    #[test]
    fn the_fog_carries_its_own_colour_into_the_air() {
        let mut scene = Scene::new();
        scene.set_fog(40.0);

        scene.set_fog_color_hex("#2f6cb3").unwrap();
        assert!((scene.haze()[0] - 47.0 / 255.0).abs() < 1e-6);
        assert!((scene.haze()[2] - 179.0 / 255.0).abs() < 1e-6);
        assert!(matches!(
            scene.set_fog_color_hex("not a colour"),
            Err(SceneError::BadColor(_))
        ));
    }

    #[test]
    fn the_clock_is_a_string_a_person_can_write() {
        assert_eq!(hours_of("07:30"), Some(7.5));
        assert_eq!(hours_of(" 12:00 "), Some(12.0));
        assert_eq!(hours_of("00:00"), Some(0.0));
        assert_eq!(hours_of("9"), Some(9.0));
        assert_eq!(hours_of("7.5"), Some(7.5));

        assert_eq!(hours_of(""), None);
        assert_eq!(hours_of("half past six"), None);
        assert_eq!(hours_of("07:75"), None);
        assert_eq!(hours_of("25:00"), None);
    }

    #[test]
    fn setting_the_time_moves_the_sun_across_the_sky() {
        let mut scene = Scene::new();

        scene.set_time_from_clock("06:00").unwrap();
        assert!((scene.time_of_day() - 6.0).abs() < 1e-6);
        // On the horizon at sunrise: high enough to catch a wall, not a roof.
        assert!(scene.sky().sun.z.abs() < 1e-6);
        assert!(scene.sky().sun.y > 0.9);

        scene.set_time_from_clock("12:00").unwrap();
        assert!(scene.sky().sun.z > 0.9);

        scene.set_time_from_clock("18:00").unwrap();
        assert!(scene.sky().sun.z.abs() < 1e-6);
        assert!(scene.sky().sun.y < -0.9);

        scene.set_time_from_clock("00:00").unwrap();
        assert!(scene.sky().sun.z < -0.9);

        assert!(matches!(
            scene.set_time_from_clock("teatime"),
            Err(SceneError::BadTime(_))
        ));
    }

    #[test]
    fn a_day_gets_brighter_and_a_night_darker_as_the_clock_runs() {
        let mut scene = Scene::new();

        scene.set_time_of_day(12.0);
        let noon = scene.sky();
        scene.set_time_of_day(0.0);
        let midnight = scene.sky();

        assert!(noon.ambient > midnight.ambient + 0.1);
        assert!(noon.zenith.z > midnight.zenith.z + 0.3);
        assert!(noon.horizon.z > midnight.horizon.z + 0.3);

        // Dawn and dusk both put a band of colour on the line where the day ends.
        scene.set_time_of_day(6.5);
        assert!(scene.sky().horizon.x > scene.sky().horizon.z);
    }

    #[test]
    fn the_clock_wraps_rather_than_running_off_the_world() {
        let mut scene = Scene::new();

        scene.set_time_of_day(30.0);
        assert!((scene.time_of_day() - 6.0).abs() < 1e-6);
        scene.set_time_of_day(-6.0);
        assert!((scene.time_of_day() - 18.0).abs() < 1e-6);

        let before = scene.time_of_day();
        scene.set_time_of_day(f32::NAN);
        assert_eq!(scene.time_of_day(), before);
    }

    #[test]
    fn a_preset_is_a_particle_system_hung_on_the_sky() {
        let mut scene = Scene::new();
        let rain = scene.create_precipitation("rain", None).unwrap();

        assert_eq!(scene.kind(rain).unwrap(), "particles");
        assert_eq!(scene.name(rain).unwrap(), "Rain");
        assert_eq!(scene.particle_shape(rain).unwrap(), ParticleShape::Streak);
        assert_eq!(scene.particle_count(rain).unwrap(), 1400);
        assert!(scene.particles_are_active(rain).unwrap());
        assert!(scene.particles_on_sky(rain).unwrap());
        assert_eq!(scene.precipitation_ids(), vec![rain]);

        let second = scene
            .create_precipitation("Downpour", Some("Lash".to_owned()))
            .unwrap();
        assert_eq!(scene.name(second).unwrap(), "Lash");
        assert_eq!(scene.particle_count(second).unwrap(), 1400);

        scene.set_particles_active(second, false).unwrap();
        assert_eq!(scene.precipitation_ids(), vec![rain]);
        scene.stop_all_precipitation();
        assert!(scene.precipitation_ids().is_empty());
        assert!(!scene.particles_are_active(rain).unwrap());
    }

    #[test]
    fn each_kind_of_weather_falls_its_own_way() {
        let mut scene = Scene::new();
        let rain = scene.create_precipitation("rain", None).unwrap();
        let snow = scene.create_precipitation("snow", None).unwrap();
        let ash = scene.create_precipitation("ash", None).unwrap();

        assert!(scene.particle_speed(rain).unwrap() > scene.particle_speed(snow).unwrap());
        assert_eq!(scene.particle_shape(snow).unwrap(), ParticleShape::Circle);
        assert_eq!(scene.particle_shape(ash).unwrap(), ParticleShape::Square);
        assert!(scene.particle_splash(rain).unwrap() > 0);
        assert_eq!(scene.particle_splash(ash).unwrap(), 0);

        // The colour came through the ordinary colour door.
        assert!(scene.color(rain).unwrap().z > scene.color(rain).unwrap().x);
        assert!(scene.color(ash).unwrap().x < 0.7);
    }

    #[test]
    fn the_sky_lays_its_crowd_over_the_eyes() {
        let mut scene = Scene::new();
        let rain = scene.create_precipitation("rain", None).unwrap();

        // No player, so the eyes are the camera, and the box of a preset stands on
        // them: it reaches from the eyes up the whole 45 meters it is tall.
        let eyes = scene.camera().position;
        let overhead = eyes + Vec3::new(0.0, 0.0, 22.5);
        let items = scene.particle_items();
        assert_eq!(items.len(), 1400);
        assert!(
            items.iter().all(|item| (item.at - overhead)
                .abs()
                .cmple(Vec3::new(35.01, 35.01, 22.51))
                .all()),
            "a preset's drops were laid outside the box over the eyes"
        );

        scene.player.position = Vec3::new(100.0, -200.0, 1.0);
        scene.player.enabled = true;
        scene.set_particles_active(rain, false).unwrap();
        scene.set_particles_active(rain, true).unwrap();
        let away = scene
            .particle_items()
            .iter()
            .map(|item| item.at)
            .collect::<Vec<_>>();
        assert!(
            away.iter().all(|at| (at - Vec3::new(100.0, -200.0, 23.5))
                .abs()
                .cmple(Vec3::new(35.01, 35.01, 22.51))
                .all()),
            "the sky rained where the player used to stand"
        );
    }

    #[test]
    fn the_sky_keeps_up_with_the_eyes_and_a_local_system_does_not() {
        let mut scene = Scene::new();
        let rain = scene.create_precipitation("rain", None).unwrap();

        // A system of the same kind that is *not* on the sky, left hanging where it
        // was written so it cannot drift after the player.
        let smoke = scene
            .create_particles(Some("Smoke".to_owned()), None)
            .unwrap();
        scene.set_particle_area(smoke, Vec3::ZERO).unwrap();
        scene.set_particle_speed(smoke, 0.0).unwrap();
        scene.set_particles_collide(smoke, false).unwrap();
        scene.set_particle_lifetime(smoke, 600.0).unwrap();
        scene
            .set_vec_field(smoke, VecField::Position, Vec3::new(0.0, 0.0, 4.0))
            .unwrap();
        scene.set_particles_active(smoke, false).unwrap();
        scene.set_particles_active(smoke, true).unwrap();

        scene.player.position = Vec3::new(500.0, 500.0, 0.0);
        scene.player.enabled = true;
        for _ in 0..400 {
            scene.step(1.0 / 60.0);
        }

        let near_player = scene
            .particle_items()
            .iter()
            .filter(|item| item.node == rain)
            .all(|item| {
                (item.at - (scene.player.position + Vec3::Z * 22.5))
                    .abs()
                    .cmple(Vec3::new(35.01, 35.01, 22.51))
                    .all()
            });
        assert!(
            near_player,
            "the sky went on raining where the player started"
        );
        assert!(
            scene
                .particle_items()
                .iter()
                .filter(|item| item.node == smoke)
                .all(|item| (item.at - Vec3::new(0.0, 0.0, 4.0)).length() < 1.0),
            "a system that is not the air was dragged off with the player"
        );
    }

    #[test]
    fn an_atmosphere_name_that_does_not_fall_is_refused() {
        let mut scene = Scene::new();

        assert!(matches!(
            scene.create_precipitation("fog", None),
            Err(SceneError::UnknownAtmosphere(kind)) if kind == "fog"
        ));
        assert!(matches!(
            scene.create_precipitation("hail", None),
            Err(SceneError::UnknownAtmosphere(_))
        ));
        assert!(Precipitation::from_name(" blizzard ").is_some());
    }

    #[test]
    fn the_light_starts_auto_and_a_machine_that_said_nothing_is_medium() {
        let scene = Scene::new();

        // A fresh world does not choose a rung; it asks the machine to.
        assert_eq!(scene.atmosphere().quality, LightQuality::Auto);
        assert_eq!(scene.written_light_quality(), LightQuality::Auto);
        // Nothing has been reported, so this is a headless scene and the engine
        // declines to guess about hardware that is not there.
        assert_eq!(scene.light().quality, LightQuality::Medium);
        assert!(scene.gpu().is_none());
    }

    #[test]
    fn the_low_rung_is_the_bare_world_the_pixel_tests_were_measured_on() {
        let low = Light::of(LightQuality::Low);

        assert!(!low.shadows);
        assert_eq!(low.shadow_map, 0);
        assert_eq!(low.softness, 0);
        assert_eq!(low.reach, 0.0);
        assert_eq!(low.gloss, 0.0);
        assert!(!low.sky_fill);
        // Writing it is what a test of an exact colour does, so that the rung the
        // machine can hold never moves the numbers behind the assertion.
        let mut scene = Scene::new();
        scene.set_light_quality(LightQuality::Low);
        assert_eq!(scene.light(), low);
    }

    #[test]
    fn every_rung_above_costs_more_than_the_one_below_it() {
        let rungs = [
            LightQuality::Low,
            LightQuality::Medium,
            LightQuality::High,
            LightQuality::Extra,
        ];
        let lights: Vec<Light> = rungs.iter().map(|rung| Light::of(*rung)).collect();

        for pair in lights.windows(2) {
            let (less, more) = (pair[0], pair[1]);
            assert!(
                more.shadow_map >= less.shadow_map
                    && more.softness >= less.softness
                    && more.reach >= less.reach
                    && more.gloss >= less.gloss,
                "{} is not dearer than {}",
                more.quality.name(),
                less.quality.name()
            );
        }

        // Only the bottom rung is bare; everything above it casts and fills.
        assert!(
            lights[1..]
                .iter()
                .all(|light| light.shadows && light.sky_fill)
        );
        assert!(lights[1].shadow_map >= 1024 && lights[1].reach >= 40.0);
    }

    #[test]
    fn auto_reads_the_memory_a_card_actually_owns() {
        let big = 8 * 1024 * 1024 * 1024;
        let exactly_four = 4 * 1024 * 1024 * 1024;
        let a_bit_under = 3 * 1024 * 1024 * 1024;
        let a_tenth = 100 * 1024 * 1024;
        let tiny = 64 * 1024 * 1024;

        // More than four gigabytes of its own: the high rung.
        assert_eq!(
            auto_quality(DeviceKind::Discrete, Some(big)),
            LightQuality::High
        );
        // Four gigabytes on the nose is still the band a person would call mid,
        // and it is the same card the day after a driver tells the truth about it.
        assert_eq!(
            auto_quality(DeviceKind::Discrete, Some(exactly_four)),
            LightQuality::Medium
        );
        // From a tenth of a gigabyte up to four: the middle.
        assert_eq!(
            auto_quality(DeviceKind::Discrete, Some(a_bit_under)),
            LightQuality::Medium
        );
        assert_eq!(
            auto_quality(DeviceKind::Discrete, Some(a_tenth)),
            LightQuality::Medium
        );
        // Less than that, a card is not worth casting on.
        assert_eq!(
            auto_quality(DeviceKind::Discrete, Some(tiny)),
            LightQuality::Low
        );
        // A card that will not say how big it is is still a card.
        assert_eq!(
            auto_quality(DeviceKind::Discrete, None),
            LightQuality::Medium
        );

        // Anything that owns no memory and borrows the machine's takes the lowest
        // rung, however much of the machine it is allowed to borrow.
        for kind in [
            DeviceKind::Integrated,
            DeviceKind::Cpu,
            DeviceKind::Virtual,
            DeviceKind::Other,
        ] {
            assert_eq!(
                auto_quality(kind, Some(big)),
                LightQuality::Low,
                "{} has no memory of its own",
                kind.name()
            );
            assert_eq!(auto_quality(kind, None), LightQuality::Low);
        }
    }

    #[test]
    fn a_script_rung_beats_the_one_the_machine_was_read_as() {
        let mut scene = Scene::new();
        scene.report_gpu(DeviceKind::Discrete, Some(8 * 1024 * 1024 * 1024));
        assert_eq!(scene.light_quality(), LightQuality::High);

        scene.set_light_quality_name("low").unwrap();
        assert_eq!(scene.light().quality, LightQuality::Low);
        assert_eq!(scene.written_light_quality(), LightQuality::Low);

        // Back to asking the machine, and the reading has not been thrown away.
        scene.set_light_quality_name("auto").unwrap();
        assert_eq!(scene.light_quality(), LightQuality::High);
        let gpu = scene.gpu().unwrap();
        assert_eq!(gpu.kind, DeviceKind::Discrete);
        assert_eq!(gpu.vram, Some(8 * 1024 * 1024 * 1024));
    }

    #[test]
    fn a_rung_is_a_name_a_person_can_write() {
        for name in LightQuality::NAMES {
            let rung = LightQuality::from_name(name).unwrap();
            assert_eq!(rung.name(), *name);
        }
        assert_eq!(LightQuality::from_name(" HIGH "), Some(LightQuality::High));
        assert_eq!(
            LightQuality::from_name("balanced"),
            Some(LightQuality::Medium)
        );

        let mut scene = Scene::new();
        assert!(matches!(
            scene.set_light_quality_name("cinematic"),
            Err(SceneError::UnknownLightQuality(quality)) if quality == "cinematic"
        ));
        // Refused before anything moved.
        assert_eq!(scene.written_light_quality(), LightQuality::Auto);
    }
}
