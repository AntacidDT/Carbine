use glam::Vec3;

use crate::node::Scene;
use crate::node::{SceneError, parse_hex_color};

/// The backdrop behind the world: a horizon to zenith gradient with a faint
/// grain, plus the one directional light every object is shaded by.
///
/// Colours are sRGB channels in 0..1, the same way an object's colour is stored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sky {
    pub enabled: bool,
    /// Straight up.
    pub zenith: Vec3,
    /// The line where the sky meets the ground.
    pub horizon: Vec3,
    /// How much grain the gradient carries, 0..1.
    pub noise: f32,
    /// The direction the light arrives from, in world space. Normals facing it
    /// get full colour, normals facing away get ambient.
    pub sun: Vec3,
    /// How much of the colour survives on a face that points away from the sun.
    pub ambient: f32,
}

impl Default for Sky {
    fn default() -> Self {
        Self {
            enabled: true,
            zenith: Vec3::new(0.184, 0.424, 0.702),
            horizon: Vec3::new(0.812, 0.882, 0.933),
            noise: 0.035,
            // A mid-afternoon sun: high, and off to one side so that a cube
            // shows three differently lit faces.
            sun: Vec3::new(-0.387, -0.531, 0.754),
            ambient: 0.30,
        }
    }
}

/// A backdrop colour written the way an object colour is written.
fn sky_color(hex: &str) -> Result<Vec3, SceneError> {
    parse_hex_color(hex)
        .map(|(color, _)| color)
        .ok_or_else(|| SceneError::BadColor(hex.to_owned()))
}

impl Scene {
    pub fn sky(&self) -> Sky {
        self.sky
    }

    pub fn set_sky_enabled(&mut self, enabled: bool) {
        self.sky.enabled = enabled;
    }

    /// The colour at the top of the sky.
    pub fn set_sky_zenith(&mut self, color: Vec3) {
        self.sky.zenith = color;
    }

    pub fn set_sky_horizon(&mut self, color: Vec3) {
        self.sky.horizon = color;
    }

    /// The two backdrop colours from `#rrggbb` style strings, the same forms an
    /// object colour accepts.
    pub fn set_sky_colors_hex(&mut self, zenith: &str, horizon: &str) -> Result<(), SceneError> {
        let zenith = sky_color(zenith)?;
        let horizon = sky_color(horizon)?;
        self.sky.zenith = zenith;
        self.sky.horizon = horizon;
        Ok(())
    }

    pub fn set_sky_zenith_hex(&mut self, hex: &str) -> Result<(), SceneError> {
        self.sky.zenith = sky_color(hex)?;
        Ok(())
    }

    pub fn set_sky_horizon_hex(&mut self, hex: &str) -> Result<(), SceneError> {
        self.sky.horizon = sky_color(hex)?;
        Ok(())
    }

    /// Grain strength, clamped to the range a backdrop can actually show.
    pub fn set_sky_noise(&mut self, noise: f32) {
        self.sky.noise = noise.clamp(0.0, 1.0);
    }

    /// Points the light. A zero or near-zero vector would leave a face with no
    /// direction to shade toward, so it falls back to the default sun.
    pub fn set_sky_sun(&mut self, direction: Vec3) {
        self.sky.sun = if direction.length_squared() > 1e-12 {
            direction.normalize()
        } else {
            Sky::default().sun
        };
    }

    /// Brightness of a face that points away from the sun, 0..1.
    pub fn set_sky_ambient(&mut self, ambient: f32) {
        self.sky.ambient = ambient.clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sky_is_on_and_blue_by_default() {
        let scene = Scene::new();
        let sky = scene.sky();

        assert!(sky.enabled);
        // Blue above, pale at the horizon.
        assert!(sky.zenith.z > sky.zenith.x);
        assert!(sky.horizon.y > sky.horizon.x);
        assert!(sky.noise > 0.0 && sky.noise < 0.2);
        assert!(sky.ambient > 0.0 && sky.ambient < 1.0);
    }

    #[test]
    fn the_sun_is_a_direction_not_a_distance() {
        let mut scene = Scene::new();

        scene.set_sky_sun(Vec3::new(0.0, 0.0, 10.0));
        assert_eq!(scene.sky().sun, Vec3::Z);

        scene.set_sky_sun(Vec3::new(2.0, 0.0, 0.0));
        assert!((scene.sky().sun - Vec3::X).length() < 1e-6);

        // A direction with nothing in it would divide to NaN, so it is refused.
        let default = Sky::default();
        scene.set_sky_sun(Vec3::ZERO);
        assert_eq!(scene.sky().sun, default.sun);
    }

    #[test]
    fn sky_colours_round_trip_and_grain_stays_visible() {
        let mut scene = Scene::new();

        scene.set_sky_zenith(Vec3::new(0.1, 0.2, 0.3));
        scene.set_sky_horizon(Vec3::new(0.9, 0.8, 0.7));
        assert_eq!(scene.sky().zenith, Vec3::new(0.1, 0.2, 0.3));
        assert_eq!(scene.sky().horizon, Vec3::new(0.9, 0.8, 0.7));

        scene.set_sky_noise(9.0);
        assert_eq!(scene.sky().noise, 1.0);
        scene.set_sky_noise(-1.0);
        assert_eq!(scene.sky().noise, 0.0);

        scene.set_sky_ambient(1.5);
        assert_eq!(scene.sky().ambient, 1.0);

        scene.set_sky_enabled(false);
        assert!(!scene.sky().enabled);
    }

    #[test]
    fn the_backdrop_takes_hex_colours_the_way_an_object_does() {
        let mut scene = Scene::new();

        scene.set_sky_colors_hex("#2f6cb3", "#e0eaf2").unwrap();
        let sky = scene.sky();
        assert!(
            (sky.zenith - Vec3::new(47.0 / 255.0, 108.0 / 255.0, 179.0 / 255.0)).length() < 1e-6
        );
        assert!(
            (sky.horizon - Vec3::new(224.0 / 255.0, 234.0 / 255.0, 242.0 / 255.0)).length() < 1e-6
        );

        assert!(matches!(
            scene.set_sky_colors_hex("not a colour", "#000000"),
            Err(SceneError::BadColor(_))
        ));
    }

    #[test]
    fn a_backdrop_colour_can_be_changed_on_its_own() {
        let mut scene = Scene::new();
        let before = scene.sky().horizon;

        scene.set_sky_zenith_hex("#0000ff").unwrap();
        assert_eq!(scene.sky().zenith, Vec3::new(0.0, 0.0, 1.0));
        assert_eq!(scene.sky().horizon, before);

        scene.set_sky_horizon_hex("#ff0000").unwrap();
        assert_eq!(scene.sky().horizon, Vec3::new(1.0, 0.0, 0.0));

        assert!(matches!(
            scene.set_sky_zenith_hex("#12345"),
            Err(SceneError::BadColor(_))
        ));
    }
}
