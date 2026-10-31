mod atmosphere;
mod camera;
mod collision;
mod disaster;
mod draw;
mod duplicate;
mod entity;
mod geometry;
mod input;
mod mesh;
mod motion;
mod node;
mod particles;
mod physics;
mod player;
mod primitives;
mod procgen;
mod sky;
mod ui;

pub use atmosphere::{
    Atmosphere, DeviceKind, Gpu, Light, LightQuality, Precipitation, auto_quality,
};
pub use camera::{Camera, CameraMode, UP};
pub use collision::ColliderShape;
pub use disaster::{Disaster, DisasterKind};
pub use draw::DrawItem;
pub use geometry::{Contact, Solid};
pub use glam::{Mat4, Quat, Vec2, Vec3, Vec4};
pub use input::{FrameInput, Input, KEYS, Key, MouseButton};
pub use mesh::{MeshData, MeshHandle};
pub use motion::EARTH_GRAVITY_PULL;
pub use node::{Axis, DEFAULT_COLOR, MAX_ALPHA, NodeId, Scene, SceneError, Transform, VecField};
pub use particles::{
    COLLISION_BUDGET, MAX_PARTICLES, MAX_SPLASH_DROPS, ParticleItem, ParticleShape,
};
pub use physics::MAX_STEP;
pub use player::{PlayerController, PlayerMode};
pub use primitives::KINDS as PRIMITIVE_KINDS;
pub use primitives::UI_KINDS as UI_SHAPE_KINDS;
pub use procgen::{Rng, Seed, fbm2, fbm3, hash, noise2, noise3, salt, scatter};
pub use sky::Sky;
pub use ui::UiItem;

/// A packed skin written to the temporary directory, for tests that only care that a
/// scene remembers a path. A skin has to be a real file now that the scene checks one.
#[cfg(test)]
pub(crate) fn test_skin(name: &str) -> String {
    let path = std::env::temp_dir().join(format!("carbine-{name}.ktx2"));
    let white = vec![255u8; 4 * 4 * 4];
    std::fs::write(&path, carbine_ktx::pack(4, 4, &white, true).unwrap()).unwrap();
    path.to_str().unwrap().to_owned()
}
