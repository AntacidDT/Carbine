//! A map: ground that is somewhere to stand instead of a flat plane.
//!
//! The generator lays a heightfield — a grid of heights summed out of the same seeded
//! noise every other generator rolls from — and hangs it in the tree as one node wearing
//! the mesh it draws. The ground a walker or a falling body meets is no longer the one
//! flat `GROUND_HEIGHT`: it is the height of the terrain under that spot, asked of the
//! grid analytically rather than by cutting the whole map into thousands of collider
//! pieces the solver would spend the step digging out of.
//!
//! A terrain is scenery: it holds its ground, is not a body the solver shoves, and is
//! walked on by anything the floor already catches. Same seed, same hills, on this
//! machine and on the four others in a session.

use bevy_ecs::prelude::{Component, Mut};
use glam::{Vec2, Vec3};

use crate::mesh::MeshData;
use crate::motion::Motion;
use crate::node::{
    Children, DEFAULT_COLOR, NodeId, NodeKind, NodeMesh, NodeName, NodeTexture, ObjectColor,
    Opacity, Parent, Scene, SceneError, TextureTiling, Transform, Visible,
};
use crate::player::GROUND_HEIGHT;
use crate::procgen::{Rng, Seed, fbm2, salt};

/// The kind word for a node that is ground to walk on.
pub const KIND: &str = "terrain";

/// How finely the map is laid: the grid across it and the octaves of noise summed into
/// each height. A finer grid is the same hills seen closer, the way a texture's quality
/// is the same pattern counted smaller.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum TerrainQuality {
    Low,
    #[default]
    Medium,
    High,
}

impl TerrainQuality {
    pub const NAMES: &[&str] = &["low", "medium", "high"];

    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_lowercase().as_str() {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Cells across one side of the map.
    fn cells(self) -> usize {
        match self {
            Self::Low => 32,
            Self::Medium => 64,
            Self::High => 128,
        }
    }

    /// Octaves of noise summed into a height.
    fn octaves(self) -> u32 {
        match self {
            Self::Low => 3,
            Self::Medium => 4,
            Self::High => 5,
        }
    }
}

/// The kind of country a map lays. A biome is the shape of the ground and what grows
/// on it, not a palette: the same seed in the same biome lays the same hills for anyone.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Biome {
    #[default]
    Meadow,
    Forest,
    Desert,
    Alpine,
}

impl Biome {
    pub const NAMES: &[&str] = &["meadow", "forest", "desert", "alpine"];

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.trim().to_lowercase().as_str() {
            "meadow" | "plain" | "plains" | "grass" => Self::Meadow,
            "forest" | "wood" | "woods" | "woodland" => Self::Forest,
            "desert" | "dune" | "dunes" | "sand" => Self::Desert,
            "alpine" | "mountain" | "mountains" | "peak" => Self::Alpine,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Meadow => "meadow",
            Self::Forest => "forest",
            Self::Desert => "desert",
            Self::Alpine => "alpine",
        }
    }

    /// How many trees a biome plants when a script does not name a count. A desert
    /// grows none and a forest grows a stand; the other two are in between.
    pub fn trees(self) -> usize {
        match self {
            Self::Meadow => 4,
            Self::Forest => 24,
            Self::Desert => 0,
            Self::Alpine => 2,
        }
    }

    /// The height field this country is made of. Every shape stays in 0..relief so a
    /// valley sits at the node and no peak stands taller than the relief the script
    /// named, whichever biome was asked for.
    fn height(self, field: f32, u: f32, _v: f32) -> f32 {
        let shaped = match self {
            Self::Meadow => field.powf(1.4),
            Self::Forest => (field * 0.9).powf(1.2),
            Self::Desert => {
                let ridge = (u * FEATURES * 2.0 + field * 3.0).sin() * 0.5 + 0.5;
                field * 0.25 + ridge * 0.75
            }
            Self::Alpine => field.powf(2.2),
        };
        shaped.clamp(0.0, 1.0)
    }
}

/// The ground a map lays: its reach, how tall its hills are, how finely it is counted,
/// the dice it was rolled from, and the heights that came out. The heights are kept so
/// the floor under a walker is read straight off the grid rather than summed again.
#[derive(Clone, Component)]
pub(crate) struct Terrain {
    pub(crate) size: Vec2,
    pub(crate) relief: f32,
    pub(crate) quality: TerrainQuality,
    pub(crate) seed: Seed,
    pub(crate) biome: Biome,
    pub(crate) trees: usize,
    /// The node holding the generated stand, so a relayout can regrow only the trees
    /// and leave anything a script hung on the map where it was.
    pub(crate) forest: Option<NodeId>,
    pub(crate) heights: Vec<f32>,
}

impl Terrain {
    fn cells(&self) -> usize {
        self.quality.cells()
    }

    fn half(&self) -> Vec2 {
        self.size * 0.5
    }

    /// The height of the ground at a place in the node's own metres, or `None` outside
    /// the map. Between grid points the four around it are blended by how close it is,
    /// which is the same smooth the mesh is drawn with.
    fn sample(&self, x: f32, y: f32) -> Option<f32> {
        let half = self.half();
        if x < -half.x || x > half.x || y < -half.y || y > half.y {
            return None;
        }
        let cells = self.cells() as f32;
        let gx = ((x + half.x) / self.size.x * cells).clamp(0.0, cells);
        let gy = ((y + half.y) / self.size.y * cells).clamp(0.0, cells);
        let x0 = gx.floor() as usize;
        let y0 = gy.floor() as usize;
        let x1 = (x0 + 1).min(self.cells());
        let y1 = (y0 + 1).min(self.cells());
        let fx = gx - x0 as f32;
        let fy = gy - y0 as f32;
        let stride = self.cells() + 1;
        let at = |i: usize, j: usize| self.heights[j * stride + i];
        let low = at(x0, y0) + (at(x1, y0) - at(x0, y0)) * fx;
        let high = at(x0, y1) + (at(x1, y1) - at(x0, y1)) * fx;
        Some(low + (high - low) * fy)
    }
}

/// How many hills lie across the map. The noise is counted in these rather than in
/// metres, so the same seed names the same shape whichever size it is stretched to.
const FEATURES: f32 = 3.0;

/// The ceiling a generator may plant on one map.
const MAX_TREES: usize = 300;

/// The heights a map is laid from: rolling hills out of the seeded field, lifted by the
/// relief so the valleys sit at the node and the peaks rise above it.
fn heights(relief: f32, quality: TerrainQuality, seed: Seed, biome: Biome) -> Vec<f32> {
    let cells = quality.cells();
    let octaves = quality.octaves();
    let mut out = Vec::with_capacity((cells + 1) * (cells + 1));
    for j in 0..=cells {
        for i in 0..=cells {
            let u = i as f32 / cells as f32;
            let v = j as f32 / cells as f32;
            let field = fbm2(u * FEATURES, v * FEATURES, seed, octaves);
            // Valleys settle towards the node and hills stand up out of them, so a map
            // reads as ground rising rather than a sheet hung halfway through the air.
            out.push(biome.height(field, u, v) * relief.max(0.0));
        }
    }
    out
}

/// The mesh the map draws: one vertex per grid point, its normal leaned by the slope of
/// the heights around it, and two triangles to a cell wound to face the sky.
fn mesh(size: Vec2, quality: TerrainQuality, heights: &[f32]) -> MeshData {
    let cells = quality.cells();
    let stride = cells + 1;
    let half = size * 0.5;
    let step = Vec2::new(size.x / cells as f32, size.y / cells as f32);
    let at = |i: usize, j: usize| heights[j * stride + i];

    let mut positions = Vec::with_capacity(stride * stride);
    let mut normals = Vec::with_capacity(stride * stride);
    let mut uvs = Vec::with_capacity(stride * stride);
    for j in 0..=cells {
        for i in 0..=cells {
            let x = -half.x + step.x * i as f32;
            let y = -half.y + step.y * j as f32;
            positions.push(Vec3::new(x, y, at(i, j)));
            uvs.push(Vec2::new(i as f32 / cells as f32, j as f32 / cells as f32));

            let left = at(i.saturating_sub(1), j);
            let right = at((i + 1).min(cells), j);
            let down = at(i, j.saturating_sub(1));
            let up = at(i, (j + 1).min(cells));
            let dx = if i == 0 || i == cells {
                step.x
            } else {
                step.x * 2.0
            };
            let dy = if j == 0 || j == cells {
                step.y
            } else {
                step.y * 2.0
            };
            normals.push(Vec3::new(-(right - left) / dx, -(up - down) / dy, 1.0).normalize());
        }
    }

    let mut indices = Vec::with_capacity(cells * cells * 6);
    for j in 0..cells {
        for i in 0..cells {
            let a = (j * stride + i) as u32;
            let b = a + 1;
            let c = a + stride as u32 + 1;
            let d = a + stride as u32;
            indices.extend_from_slice(&[a, b, c, a, c, d]);
        }
    }

    MeshData {
        positions,
        normals,
        uvs,
        indices,
        latlong: false,
    }
}

impl Scene {
    /// Lays a map into the tree and hands back its node. Every knob has a default, so
    /// `create_terrain` alone is a walkable hill field; the generator writes the size,
    /// relief, quality and seed a script asked for and the ground is the one they name.
    pub fn create_terrain(
        &mut self,
        name: Option<String>,
        parent: Option<NodeId>,
    ) -> Result<NodeId, SceneError> {
        self.create_terrain_full(
            name,
            parent,
            Vec2::new(64.0, 64.0),
            6.0,
            TerrainQuality::default(),
            Seed::default(),
            Biome::default(),
            0,
        )
    }

    /// The whole map in one ask, so the ground is laid once with every knob it was given
    /// rather than rebuilt as each is written.
    pub fn create_terrain_with(
        &mut self,
        name: Option<String>,
        parent: Option<NodeId>,
        size: Vec2,
        relief: f32,
        quality: TerrainQuality,
        seed: Seed,
    ) -> Result<NodeId, SceneError> {
        let biome = Biome::default();
        self.create_terrain_full(
            name,
            parent,
            size,
            relief,
            quality,
            seed,
            biome,
            biome.trees(),
        )
    }

    /// The whole map in one ask, including the country it is made of and how many
    /// trees grow on it. Every knob is written before the ground or the stand is laid.
    #[allow(clippy::too_many_arguments)]
    pub fn create_terrain_full(
        &mut self,
        name: Option<String>,
        parent: Option<NodeId>,
        size: Vec2,
        relief: f32,
        quality: TerrainQuality,
        seed: Seed,
        biome: Biome,
        trees: usize,
    ) -> Result<NodeId, SceneError> {
        let parent_id = parent.unwrap_or(self.root());
        self.entity(parent_id)?;
        self.branch_allows(parent_id, false)?;

        let size = sane_size(size);
        let relief = if relief.is_finite() {
            relief.max(0.0)
        } else {
            6.0
        };
        let field = heights(relief, quality, seed, biome);
        let data = mesh(size, quality, &field);
        let trees = trees.min(MAX_TREES);

        let id = self.reserve();
        let handle = self.next_mesh_handle();
        let entity = self
            .world
            .spawn((
                NodeName(name.unwrap_or_else(|| "Terrain".to_owned())),
                NodeKind(KIND.to_owned()),
                Transform::default(),
                NodeMesh { handle, data },
                ObjectColor(DEFAULT_COLOR),
                Opacity::default(),
                NodeTexture::default(),
                TextureTiling::default(),
                Visible::default(),
                // Ground holds its place: it is not a body gravity owns or the solver
                // shoves, only the floor everything else is caught by.
                Motion {
                    anchored: true,
                    ..Motion::default()
                },
                Parent(parent_id),
                Children::default(),
                Terrain {
                    size,
                    relief,
                    quality,
                    seed,
                    biome,
                    trees,
                    forest: None,
                    heights: field,
                },
            ))
            .id();
        self.insert(id, entity);
        self.attach(id, parent_id);
        self.grow_forest(id)?;
        Ok(id)
    }

    /// How many trees a generator may plant on one map. A script can ask for more and
    /// gets this ceiling instead of a scene with ten thousand nodes in it.
    /// Grows the stand again after the ground beneath it changed. The trees hang under
    /// one group node, so only the generated ones go; a script's own children stay.
    pub(crate) fn grow_forest(&mut self, id: NodeId) -> Result<(), SceneError> {
        let (old, count, size, seed) = {
            let field = self.terrain(id)?;
            (field.forest, field.trees, field.size, field.seed)
        };
        if let Some(forest) = old {
            self.destroy(forest)?;
        }
        if count == 0 {
            self.terrain_mut(id)?.forest = None;
            return Ok(());
        }

        let group = self.create_model(Some("Trees".to_owned()), Some(id))?;
        for index in 0..count {
            let mut dice = Rng::new(salt(seed, 1_000 + index));
            let place = dice.inside(Vec3::new(size.x * 0.88, size.y * 0.88, 0.0));
            let height = self.terrain(id)?.sample(place.x, place.y).unwrap_or(0.0);
            let tall = 2.0 + dice.between(0.0, 1.4);
            let trunk = self.create("cube", Some(format!("Tree {}", index + 1)), Some(group))?;
            self.set_vec_field(
                trunk,
                crate::node::VecField::Size,
                Vec3::new(0.28, 0.28, tall),
            )?;
            self.set_vec_field(
                trunk,
                crate::node::VecField::Position,
                Vec3::new(place.x, place.y, height + tall * 0.5),
            )?;
            self.set_color(trunk, Vec3::new(0.647, 0.165, 0.165))?;
            self.set_anchored(trunk, true)?;

            let crown = self.create("sphere", Some("Crown".to_owned()), Some(trunk))?;
            self.set_vec_field(crown, crate::node::VecField::Size, Vec3::new(1.5, 1.5, 1.4))?;
            self.set_vec_field(
                crown,
                crate::node::VecField::Position,
                Vec3::new(0.0, 0.0, tall * 0.5),
            )?;
            self.set_color(crown, Vec3::new(0.0, 0.502, 0.0))?;
        }
        self.terrain_mut(id)?.forest = Some(group);
        Ok(())
    }

    pub fn is_terrain(&self, id: NodeId) -> Result<bool, SceneError> {
        let entity = self.entity(id)?;
        Ok(self.world.get::<Terrain>(entity).is_some())
    }

    fn terrain(&self, id: NodeId) -> Result<&Terrain, SceneError> {
        self.component::<Terrain>(id)
    }

    /// Metres the map reaches along the ground, length and width.
    pub fn terrain_size(&self, id: NodeId) -> Result<Vec2, SceneError> {
        Ok(self.terrain(id)?.size)
    }

    pub fn set_terrain_size(&mut self, id: NodeId, size: Vec2) -> Result<(), SceneError> {
        let size = sane_size(size);
        self.spread(id, move |scene, member| {
            scene.terrain_mut(member)?.size = size;
            scene.relayout(member)
        })
    }

    /// How tall the hills stand above the node, in metres.
    pub fn terrain_relief(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.terrain(id)?.relief)
    }

    pub fn set_terrain_relief(&mut self, id: NodeId, relief: f32) -> Result<(), SceneError> {
        let relief = if relief.is_finite() {
            relief.max(0.0)
        } else {
            0.0
        };
        self.spread(id, move |scene, member| {
            scene.terrain_mut(member)?.relief = relief;
            scene.relayout(member)
        })
    }

    pub fn terrain_quality(&self, id: NodeId) -> Result<TerrainQuality, SceneError> {
        Ok(self.terrain(id)?.quality)
    }

    pub fn set_terrain_quality(
        &mut self,
        id: NodeId,
        quality: TerrainQuality,
    ) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.terrain_mut(member)?.quality = quality;
            scene.relayout(member)
        })
    }

    pub fn set_terrain_quality_name(&mut self, id: NodeId, name: &str) -> Result<(), SceneError> {
        let quality = TerrainQuality::from_name(name)
            .ok_or_else(|| SceneError::UnknownTerrainQuality(name.trim().to_owned()))?;
        self.set_terrain_quality(id, quality)
    }

    /// The dice the hills were rolled from. Writing a new one lays the map that seed
    /// names, which is how the same ground comes back.
    pub fn terrain_seed(&self, id: NodeId) -> Result<u64, SceneError> {
        Ok(self.terrain(id)?.seed.0)
    }

    pub fn set_terrain_seed(&mut self, id: NodeId, seed: Seed) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.terrain_mut(member)?.seed = seed;
            scene.relayout(member)
        })
    }

    /// The country this map lays: meadow, forest, desert or alpine.
    pub fn terrain_biome(&self, id: NodeId) -> Result<Biome, SceneError> {
        Ok(self.terrain(id)?.biome)
    }

    pub fn set_terrain_biome(&mut self, id: NodeId, biome: Biome) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.terrain_mut(member)?.biome = biome;
            scene.relayout(member)
        })
    }

    pub fn set_terrain_biome_name(&mut self, id: NodeId, name: &str) -> Result<(), SceneError> {
        let biome = Biome::from_name(name)
            .ok_or_else(|| SceneError::UnknownBiome(name.trim().to_owned()))?;
        self.set_terrain_biome(id, biome)
    }

    /// How many seeded trees grow on this map. Changing it regrows only the generated
    /// stand; what a script added to the map stays put.
    pub fn terrain_tree_count(&self, id: NodeId) -> Result<usize, SceneError> {
        Ok(self.terrain(id)?.trees)
    }

    pub fn set_terrain_tree_count(&mut self, id: NodeId, count: usize) -> Result<(), SceneError> {
        let count = count.min(MAX_TREES);
        self.spread(id, move |scene, member| {
            scene.terrain_mut(member)?.trees = count;
            scene.grow_forest(member)
        })
    }

    fn terrain_mut(&mut self, id: NodeId) -> Result<Mut<'_, Terrain>, SceneError> {
        self.component_mut::<Terrain>(id)
    }

    /// Sum the heights again and draw the mesh they name, on a fresh handle so the
    /// renderer uploads the new ground rather than keeping the old one cached.
    fn relayout(&mut self, id: NodeId) -> Result<(), SceneError> {
        let (size, relief, quality, seed, biome) = {
            let field = self.terrain(id)?;
            (
                field.size,
                field.relief,
                field.quality,
                field.seed,
                field.biome,
            )
        };
        let field = heights(relief, quality, seed, biome);
        let data = mesh(size, quality, &field);
        let handle = self.next_mesh_handle();
        self.terrain_mut(id)?.heights = field;
        let entity = self.entity(id)?;
        self.world
            .entity_mut(entity)
            .insert(NodeMesh { handle, data });
        self.grow_forest(id)?;
        Ok(())
    }

    /// The floor under a spot in the world: the flat ground every world is born with, or
    /// the top of whichever map reaches that spot, whichever is higher. This is what a
    /// walker's feet and a falling body's underside are caught by, so a map is walked on
    /// without being cut into a collider.
    pub(crate) fn floor_at(&self, x: f32, y: f32) -> f32 {
        let mut floor = GROUND_HEIGHT;
        for id in self.node_tree() {
            let Ok(field) = self.component::<Terrain>(id) else {
                continue;
            };
            let Ok(model) = self.global_matrix(id) else {
                continue;
            };
            let local = model.inverse().transform_point3(Vec3::new(x, y, 0.0));
            let Some(height) = field.sample(local.x, local.y) else {
                continue;
            };
            let world = model.transform_point3(Vec3::new(local.x, local.y, height));
            floor = floor.max(world.z);
        }
        floor
    }
}

/// A map has to reach somewhere to be ground: a nil or negative side is folded to a
/// metre so a stray knob never lays a map with no area to stand on.
fn sane_size(size: Vec2) -> Vec2 {
    Vec2::new(
        if size.x.is_finite() && size.x > 0.0 {
            size.x
        } else {
            1.0
        },
        if size.y.is_finite() && size.y > 0.0 {
            size.y
        } else {
            1.0
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::VecField;
    use crate::procgen::Seed;

    fn map(scene: &mut Scene, seed: Seed) -> NodeId {
        scene
            .create_terrain_with(
                Some("Map".to_owned()),
                None,
                Vec2::new(32.0, 32.0),
                8.0,
                TerrainQuality::Medium,
                seed,
            )
            .unwrap()
    }

    #[test]
    fn a_map_is_a_node_wearing_the_ground_it_draws() {
        let mut scene = Scene::new();
        let id = map(&mut scene, Seed::of("hills"));

        assert_eq!(scene.kind(id).unwrap(), KIND);
        assert!(scene.is_terrain(id).unwrap());
        let mesh = scene.mesh(id).unwrap();
        assert_eq!(mesh.vertex_count(), 65 * 65);
        assert_eq!(mesh.triangle_count(), 64 * 64 * 2);
        // The map reaches the size it was asked for, so `size` reads it back.
        assert_eq!(scene.natural_extent(id).unwrap().x, 32.0);
        assert_eq!(scene.natural_extent(id).unwrap().y, 32.0);
    }

    #[test]
    fn the_same_seed_lays_the_same_hills() {
        let mut scene = Scene::new();
        let first = map(&mut scene, Seed::of("valley"));
        let again = map(&mut scene, Seed::of("valley"));
        let other = map(&mut scene, Seed::of("vallez"));

        assert_eq!(
            scene.mesh(first).unwrap().positions,
            scene.mesh(again).unwrap().positions
        );
        assert_ne!(
            scene.mesh(first).unwrap().positions,
            scene.mesh(other).unwrap().positions
        );
        assert_eq!(scene.terrain_seed(first).unwrap(), Seed::of("valley").0);
    }

    #[test]
    fn the_ground_rises_out_of_the_node_and_stays_under_the_relief() {
        let mut scene = Scene::new();
        let id = map(&mut scene, Seed::of("peaks"));
        let mesh = scene.mesh(id).unwrap();
        let (min, max) = mesh.bounds().unwrap();
        assert!(min.z >= -1e-4, "valleys sit at the node: {min:?}");
        assert!(
            max.z <= 8.0 + 1e-3,
            "no peak taller than the relief: {max:?}"
        );
        assert!(max.z > 1.0, "a flat sheet is not a map: {max:?}");
    }

    #[test]
    fn the_floor_under_a_spot_is_the_map_there() {
        let mut scene = Scene::new();
        let id = map(&mut scene, Seed::of("floor"));
        // Raise the whole map a metre so the answer is clearly the map's and not the
        // flat ground it stands on.
        scene
            .set_vec_field(id, VecField::Position, Vec3::new(0.0, 0.0, 1.0))
            .unwrap();

        let mesh = scene.mesh(id).unwrap();
        // A grid point's height is the mesh's own, plus the metre the node was lifted.
        let centre = mesh.positions[32 * 65 + 32].z;
        let floor = scene.floor_at(0.0, 0.0);
        assert!(
            (floor - (centre + 1.0)).abs() < 1e-3,
            "{floor} vs {}",
            centre + 1.0
        );

        // Outside the map the flat ground answers instead.
        assert_eq!(scene.floor_at(100.0, 100.0), GROUND_HEIGHT);
    }

    #[test]
    fn a_walker_is_held_up_by_the_hill_it_stands_on() {
        let mut scene = Scene::new();
        let id = map(&mut scene, Seed::of("walk"));
        scene.player.enabled = true;
        let floor = scene.floor_at(0.0, 0.0);
        scene.player.position = Vec3::new(0.0, 0.0, floor + 3.0);

        for _ in 0..240 {
            scene.step(1.0 / 60.0);
        }
        let at = scene.player.position;
        assert!(
            (at.z - scene.floor_at(at.x, at.y)).abs() < 1e-2,
            "the walker settled onto the ground under it: {at:?}"
        );
        assert!(scene.player.grounded);
        assert!(
            at.z > GROUND_HEIGHT,
            "the hill holds them above the flat floor"
        );
        let _ = id;
    }

    #[test]
    fn a_falling_body_rests_on_the_map_not_through_it() {
        let mut scene = Scene::new();
        map(&mut scene, Seed::of("crate"));
        let crate_ = scene.create("cube", Some("Crate".into()), None).unwrap();
        scene
            .set_vec_field(crate_, VecField::Size, Vec3::splat(0.5))
            .unwrap();
        scene
            .set_vec_field(crate_, VecField::Position, Vec3::new(2.0, 2.0, 20.0))
            .unwrap();
        scene.set_collision(crate_, true).unwrap();
        scene.set_uses_gravity(crate_, true).unwrap();

        for _ in 0..400 {
            scene.step(1.0 / 60.0);
        }
        let at = scene.vec_field(crate_, VecField::Position).unwrap();
        let floor = scene.floor_at(at.x, at.y);
        // The crate's underside is half a metre below its pivot.
        assert!(
            (at.z - 0.25 - floor).abs() < 1e-2,
            "the crate rests on the hill: {at:?} floor {floor}"
        );
    }

    #[test]
    fn writing_a_knob_relayouts_the_ground() {
        let mut scene = Scene::new();
        let id = map(&mut scene, Seed::of("knobs"));
        let before = scene.mesh(id).unwrap();

        scene.set_terrain_relief(id, 2.0).unwrap();
        assert_eq!(scene.terrain_relief(id).unwrap(), 2.0);
        let lower = scene.mesh(id).unwrap();
        assert!(lower.bounds().unwrap().1.z < before.bounds().unwrap().1.z);

        scene.set_terrain_size(id, Vec2::new(16.0, 16.0)).unwrap();
        assert_eq!(scene.terrain_size(id).unwrap(), Vec2::new(16.0, 16.0));
        assert_eq!(scene.natural_extent(id).unwrap().x, 16.0);

        scene.set_terrain_quality_name(id, "low").unwrap();
        assert_eq!(scene.terrain_quality(id).unwrap(), TerrainQuality::Low);
        assert_eq!(scene.mesh(id).unwrap().vertex_count(), 33 * 33);

        let seed = Seed::of("elsewhere");
        scene.set_terrain_seed(id, seed).unwrap();
        assert_eq!(scene.terrain_seed(id).unwrap(), seed.0);
    }

    #[test]
    fn a_biome_lays_its_own_ground_and_grows_a_stand() {
        let mut scene = Scene::new();
        let forest = scene
            .create_terrain_full(
                Some("Wood".to_owned()),
                None,
                Vec2::new(32.0, 32.0),
                8.0,
                TerrainQuality::Low,
                Seed::of("woods"),
                Biome::Forest,
                6,
            )
            .unwrap();
        assert_eq!(scene.terrain_biome(forest).unwrap(), Biome::Forest);
        assert_eq!(scene.terrain_tree_count(forest).unwrap(), 6);

        let stand = scene.children(forest).unwrap();
        assert_eq!(stand.len(), 1, "one group holds the generated trees");
        assert_eq!(scene.name(stand[0]).unwrap(), "Trees");
        let trunks = scene.children(stand[0]).unwrap();
        assert_eq!(trunks.len(), 6);
        for trunk in trunks {
            assert_eq!(
                scene.children(trunk).unwrap().len(),
                1,
                "a crown under each trunk"
            );
        }

        // The same seed in another country lays different hills.
        let meadow = scene
            .create_terrain_full(
                None,
                None,
                Vec2::new(32.0, 32.0),
                8.0,
                TerrainQuality::Low,
                Seed::of("woods"),
                Biome::Meadow,
                0,
            )
            .unwrap();
        assert_ne!(
            scene.mesh(forest).unwrap().positions,
            scene.mesh(meadow).unwrap().positions
        );
        assert!(scene.children(meadow).unwrap().is_empty());
    }

    #[test]
    fn changing_the_stand_regrows_only_the_trees() {
        let mut scene = Scene::new();
        let id = map(&mut scene, Seed::of("stand"));
        scene.set_terrain_tree_count(id, 5).unwrap();
        let old_stand = scene.children(id).unwrap()[0];

        // A thing a script hung on the map is not part of the stand.
        let kept = scene
            .create("cube", Some("Kept".to_owned()), Some(id))
            .unwrap();
        scene.set_terrain_tree_count(id, 2).unwrap();

        let children = scene.children(id).unwrap();
        assert_eq!(children.len(), 2, "the kept node and a new stand");
        assert!(children.contains(&kept));
        assert!(!children.contains(&old_stand));
        let new_stand = children.into_iter().find(|child| *child != kept).unwrap();
        assert_eq!(scene.children(new_stand).unwrap().len(), 2);

        scene.set_terrain_biome_name(id, "desert").unwrap();
        assert_eq!(scene.terrain_biome(id).unwrap(), Biome::Desert);
        assert_eq!(scene.terrain_tree_count(id).unwrap(), 2);
        let error = scene.set_terrain_biome_name(id, "tundra").unwrap_err();
        assert!(error.to_string().contains("tundra"), "{error}");
    }

    #[test]
    fn a_quality_that_is_not_one_is_refused() {
        let mut scene = Scene::new();
        let id = map(&mut scene, Seed::of("quality"));
        let error = scene.set_terrain_quality_name(id, "ultra").unwrap_err();
        assert!(error.to_string().contains("ultra"), "{error}");
    }

    #[test]
    fn a_map_is_not_a_body_the_solver_shoves() {
        let mut scene = Scene::new();
        let id = map(&mut scene, Seed::of("scenery"));
        // No collider is cut for ground: it is the floor, not a thing in the way.
        assert!(scene.collider(id).is_empty());
        scene.step(1.0 / 60.0);
        assert_eq!(
            scene.vec_field(id, VecField::Position).unwrap(),
            Vec3::ZERO,
            "the ground holds its place"
        );
    }

    #[test]
    fn a_copy_of_a_map_is_ground_too() {
        let mut scene = Scene::new();
        let id = map(&mut scene, Seed::of("copy"));
        let copies = scene.duplicate(id, 1).unwrap();
        let copy = copies[0];

        assert!(
            scene.is_terrain(copy).unwrap(),
            "the copy is a map, not just a picture"
        );
        assert_eq!(scene.terrain_seed(copy).unwrap(), Seed::of("copy").0);
        assert_eq!(scene.terrain_relief(copy).unwrap(), 8.0);
        assert_eq!(
            scene.mesh(copy).unwrap().positions,
            scene.mesh(id).unwrap().positions,
            "the copy draws the same hills"
        );
        // Lift the copy and ask the floor under its centre: the copy's own ground
        // answers, so a walker would stand on it.
        scene
            .set_vec_field(copy, VecField::Position, Vec3::new(40.0, 0.0, 2.0))
            .unwrap();
        assert!(scene.floor_at(40.0, 0.0) >= 2.0);
    }

    #[test]
    fn linked_maps_share_their_knobs_but_not_their_place() {
        let mut scene = Scene::new();
        let id = map(&mut scene, Seed::of("twins"));
        let twins = scene.link_duplicate(id, 1).unwrap();
        let twin = twins[0];
        assert!(scene.is_linked(id).unwrap());

        // A knob is what the ground *is*, so it lands on both.
        scene.set_terrain_relief(id, 3.0).unwrap();
        assert_eq!(scene.terrain_relief(id).unwrap(), 3.0);
        assert_eq!(scene.terrain_relief(twin).unwrap(), 3.0);
        scene.set_terrain_seed(twin, Seed::of("elsewhere")).unwrap();
        assert_eq!(scene.terrain_seed(id).unwrap(), Seed::of("elsewhere").0);

        // Where a member stands stays its own.
        scene
            .set_vec_field(twin, VecField::Position, Vec3::new(10.0, 0.0, 0.0))
            .unwrap();
        assert_eq!(scene.vec_field(id, VecField::Position).unwrap(), Vec3::ZERO);
    }
}
