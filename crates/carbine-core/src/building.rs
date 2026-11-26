//! The building generator: a seeded node tree standing on the ground.
//!
//! A building is not one welded mesh. It is a root with a floor, four walls, windows
//! and a roof hanging under it as ordinary nodes, so a script can move a window, skin
//! a wall or hide a floor the same way it would on an authored model. Everything is
//! rolled from one seed before the first node is spawned, so the same word raises the
//! same house for anybody.

use bevy_ecs::prelude::Component;
use glam::Vec3;

use crate::node::{NodeId, Scene, SceneError};
use crate::procgen::{Rng, Seed, salt};

/// The kind word on the building root's component, for a script that asks what it is.
pub const KIND: &str = "building";

/// How many metres one storey stands.
const FLOOR_HEIGHT: f32 = 3.0;

/// The silhouettes the generator knows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildingStyle {
    House,
    Tower,
    Cabin,
}

impl BuildingStyle {
    pub const NAMES: &[&str] = &["house", "tower", "cabin"];

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.trim().to_lowercase().as_str() {
            "house" | "home" | "cottage" => Self::House,
            "tower" | "keep" | "block" => Self::Tower,
            "cabin" | "hut" | "shack" => Self::Cabin,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::House => "house",
            Self::Tower => "tower",
            Self::Cabin => "cabin",
        }
    }

    /// Wall, roof and floor, as display-encoded colours the renderer takes directly.
    fn palette(self) -> (Vec3, Vec3, Vec3) {
        match self {
            Self::House => (
                Vec3::new(0.91, 0.86, 0.78),
                Vec3::new(0.55, 0.25, 0.18),
                Vec3::new(0.72, 0.70, 0.65),
            ),
            Self::Tower => (
                Vec3::new(0.60, 0.65, 0.70),
                Vec3::new(0.29, 0.33, 0.39),
                Vec3::new(0.48, 0.53, 0.58),
            ),
            Self::Cabin => (
                Vec3::new(0.48, 0.35, 0.22),
                Vec3::new(0.29, 0.21, 0.15),
                Vec3::new(0.42, 0.29, 0.19),
            ),
        }
    }
}

/// What was rolled before the first node was spawned, kept on the root so a script
/// can read back what stands in the world.
#[derive(Clone, Copy, Component)]
pub struct Building {
    pub(crate) style: BuildingStyle,
    pub(crate) floors: usize,
    pub(crate) width: f32,
    pub(crate) depth: f32,
    pub(crate) seed: Seed,
}

impl Scene {
    /// Raises a building: a root with one floor slab, four walls and a couple of
    /// windows per storey, and a roof over the lot. Every knob is written before the
    /// first node is spawned, so the whole thing arrives as one shape.
    #[allow(clippy::too_many_arguments)]
    pub fn create_building(
        &mut self,
        name: Option<String>,
        parent: Option<NodeId>,
        position: Vec3,
        floors: usize,
        width: f32,
        depth: f32,
        style: BuildingStyle,
        seed: Seed,
    ) -> Result<NodeId, SceneError> {
        let floors = floors.clamp(1, 20);
        let width = if width.is_finite() {
            width.max(1.0)
        } else {
            8.0
        };
        let depth = if depth.is_finite() {
            depth.max(1.0)
        } else {
            6.0
        };
        let (wall, roof, slab) = style.palette();
        let glass = Vec3::new(0.36, 0.62, 0.78);

        let root_name = name.unwrap_or_else(|| {
            format!(
                "{} Building",
                style.name().chars().next().unwrap_or('b').to_uppercase()
            )
        });
        let root = self.create_model(Some(root_name), parent)?;
        self.set_vec_field(root, crate::node::VecField::Position, position)?;
        self.set_anchored(root, true)?;

        for floor in 0..floors {
            let base = floor as f32 * FLOOR_HEIGHT;
            let level = self.create("cube", Some(format!("Floor {}", floor + 1)), Some(root))?;
            self.piece(
                level,
                Vec3::new(0.0, 0.0, base),
                Vec3::new(width, depth, 0.22),
                slab,
            )?;

            let panels = [
                (
                    "Front",
                    Vec3::new(0.0, -depth * 0.5, FLOOR_HEIGHT * 0.5),
                    Vec3::new(width, 0.18, FLOOR_HEIGHT - 0.22),
                ),
                (
                    "Back",
                    Vec3::new(0.0, depth * 0.5, FLOOR_HEIGHT * 0.5),
                    Vec3::new(width, 0.18, FLOOR_HEIGHT - 0.22),
                ),
                (
                    "Left",
                    Vec3::new(-width * 0.5, 0.0, FLOOR_HEIGHT * 0.5),
                    Vec3::new(0.18, depth, FLOOR_HEIGHT - 0.22),
                ),
                (
                    "Right",
                    Vec3::new(width * 0.5, 0.0, FLOOR_HEIGHT * 0.5),
                    Vec3::new(0.18, depth, FLOOR_HEIGHT - 0.22),
                ),
            ];
            for (side, at, size) in panels {
                let panel =
                    self.create("cube", Some(format!("{side} {}", floor + 1)), Some(level))?;
                self.piece(panel, at, size, wall)?;
            }

            // A dice per storey: how many windows and how far apart they stand.
            let mut dice = Rng::new(salt(seed, 100 + floor));
            let windows = dice.count(2, 4) as usize;
            let wobble = dice.between(-0.4, 0.4);
            for window in 0..windows {
                let across = (window as f32 + 1.0) / (windows as f32 + 1.0);
                let x = (across - 0.5) * width * 0.8 + wobble;
                let pane = self.create(
                    "cube",
                    Some(format!("Window {}.{}", floor + 1, window + 1)),
                    Some(level),
                )?;
                self.piece(
                    pane,
                    Vec3::new(x, -depth * 0.5 - 0.05, FLOOR_HEIGHT * 0.55),
                    Vec3::new(0.7, 0.06, 1.1),
                    glass,
                )?;
            }
        }

        let top = self.create("cube", Some("Roof".to_owned()), Some(root))?;
        self.piece(
            top,
            Vec3::new(0.0, 0.0, floors as f32 * FLOOR_HEIGHT),
            Vec3::new(width + 0.6, depth + 0.6, 0.32),
            roof,
        )?;

        let entity = self.entity(root)?;
        self.world.entity_mut(entity).insert(Building {
            style,
            floors,
            width,
            depth,
            seed,
        });
        Ok(root)
    }

    /// Writes one box of a building: its place relative to the floor it hangs on, its
    /// size in metres, its colour, and its pinned place so the solver leaves it be.
    fn piece(&mut self, id: NodeId, at: Vec3, size: Vec3, color: Vec3) -> Result<(), SceneError> {
        self.set_vec_field(id, crate::node::VecField::Position, at)?;
        self.set_vec_field(id, crate::node::VecField::Size, size)?;
        self.set_color(id, color)?;
        self.set_anchored(id, true)
    }

    pub fn is_building(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.component::<Building>(id).is_ok())
    }

    pub fn building_style(&self, id: NodeId) -> Result<BuildingStyle, SceneError> {
        Ok(self.component::<Building>(id)?.style)
    }

    pub fn building_floors(&self, id: NodeId) -> Result<usize, SceneError> {
        Ok(self.component::<Building>(id)?.floors)
    }

    pub fn building_seed(&self, id: NodeId) -> Result<Seed, SceneError> {
        Ok(self.component::<Building>(id)?.seed)
    }

    pub fn building_dimensions(&self, id: NodeId) -> Result<Vec3, SceneError> {
        let shape = self.component::<Building>(id)?;
        Ok(Vec3::new(shape.width, shape.depth, shape.floors as f32))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raised(style: BuildingStyle, seed: Seed) -> (Scene, NodeId) {
        let mut scene = Scene::new();
        let id = scene
            .create_building(
                Some("Block".to_owned()),
                None,
                Vec3::new(4.0, 5.0, 0.0),
                3,
                8.0,
                6.0,
                style,
                seed,
            )
            .unwrap();
        (scene, id)
    }

    #[test]
    fn a_building_is_a_tree_of_ordinary_nodes() {
        let (scene, root) = raised(BuildingStyle::House, Seed::of("home"));

        assert!(scene.is_building(root).unwrap());
        assert_eq!(scene.building_style(root).unwrap(), BuildingStyle::House);
        assert_eq!(scene.building_floors(root).unwrap(), 3);
        assert_eq!(
            scene.building_dimensions(root).unwrap(),
            Vec3::new(8.0, 6.0, 3.0)
        );
        assert_eq!(
            scene
                .vec_field(root, crate::node::VecField::Position)
                .unwrap(),
            Vec3::new(4.0, 5.0, 0.0)
        );

        let children = scene.children(root).unwrap();
        // Three floors, four walls and two to four windows each, then a roof.
        assert!(children.len() >= 4);
        assert!(children.iter().any(|id| scene.name(*id).unwrap() == "Roof"));
        let first = children
            .iter()
            .find(|id| scene.name(**id).unwrap() == "Floor 1")
            .unwrap();
        assert!(
            scene
                .children(*first)
                .unwrap()
                .iter()
                .any(|id| scene.name(*id).unwrap() == "Front 1")
        );
        // Every piece holds its place rather than being shoved by the solver.
        assert!(scene.is_anchored(*first).unwrap());
    }

    #[test]
    fn the_same_seed_raises_the_same_windows() {
        let (first, root) = raised(BuildingStyle::House, Seed::of("windows"));
        let (again, again_root) = raised(BuildingStyle::House, Seed::of("windows"));
        let (other, other_root) = raised(BuildingStyle::House, Seed::of("windowz"));

        let names = |scene: &Scene, root: NodeId| -> Vec<String> {
            let mut found = Vec::new();
            for floor in scene.children(root).unwrap() {
                for piece in scene.children(floor).unwrap() {
                    let name = scene.name(piece).unwrap();
                    if name.starts_with("Window") {
                        let x = scene
                            .vec_field(piece, crate::node::VecField::Position)
                            .unwrap()
                            .x;
                        found.push(format!("{name}@{x:.3}"));
                    }
                }
            }
            found
        };
        assert_eq!(names(&first, root), names(&again, again_root));
        assert_ne!(names(&first, root), names(&other, other_root));
    }

    #[test]
    fn a_style_that_is_not_one_is_refused() {
        assert_eq!(BuildingStyle::from_name("hut"), Some(BuildingStyle::Cabin));
        assert_eq!(BuildingStyle::from_name("keep"), Some(BuildingStyle::Tower));
        assert_eq!(BuildingStyle::from_name("castle"), None);
    }
}
