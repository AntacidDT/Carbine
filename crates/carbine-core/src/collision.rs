//! Colliders: what a body means to the solver.
//!
//! Nothing collides until a script says so. Turning it on cuts a collider from
//! the body the node already has, in the same meters the renderer draws it in,
//! so `pillar.collision = True` is the whole of the work.

use bevy_ecs::prelude::*;
use glam::{Mat4, Vec3};

use crate::draw::local_matrix;
use crate::geometry::Solid;
use crate::node::{NodeId, NodeKind, NodeMesh, Scene, SceneError};

/// The four ways a body can be read. A collider is chosen from what the body
/// looks like, not from what it is made of, because a mesh's triangles are far
/// more detail than a solver needs to decide who is standing on what.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColliderShape {
    /// The box the body's `size` describes.
    Box,
    /// A ball as wide as the body's widest dimension.
    Sphere,
    /// A sausage along the body's longest dimension.
    Capsule,
    /// Every corner the body actually has.
    Mesh,
}

impl ColliderShape {
    /// The names a script writes, in the order they are listed when one is wrong.
    pub const NAMES: &'static [&'static str] = &["box", "sphere", "capsule", "mesh"];

    pub fn from_name(name: &str) -> Option<ColliderShape> {
        Some(match name.trim().to_lowercase().as_str() {
            "box" | "cube" => Self::Box,
            "sphere" | "ball" => Self::Sphere,
            "capsule" => Self::Capsule,
            "mesh" => Self::Mesh,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Box => "box",
            Self::Sphere => "sphere",
            Self::Capsule => "capsule",
            Self::Mesh => "mesh",
        }
    }

    /// The shape a body of this kind reads as. A ball is round; everything else
    /// is the box its `size` describes, which is the shape a solver is fastest at.
    fn for_kind(kind: &str) -> ColliderShape {
        match kind {
            "sphere" => Self::Sphere,
            _ => Self::Box,
        }
    }
}

/// A node's invitation to the solver. It is not there at all until the script
/// asks for it, so an untouched scene has nothing to walk past, and switching
/// collisions off keeps the shape for the next time.
#[derive(Clone, Copy, Component)]
pub(crate) struct Collider {
    pub(crate) enabled: bool,
    pub(crate) shape: ColliderShape,
}

impl Scene {
    pub fn has_collision(&self, id: NodeId) -> Result<bool, SceneError> {
        let entity = self.entity(id)?;
        Ok(self
            .world
            .get::<Collider>(entity)
            .is_some_and(|collider| collider.enabled))
    }

    /// Puts the body in the solver or takes it out. A body that has never had a
    /// collider gets the shape its kind reads as. Being a body the solver answers
    /// to is part of what a thing is, so the whole link group is told together.
    pub fn set_collision(&mut self, id: NodeId, enabled: bool) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.set_collision_one(member, enabled)
        })
    }

    /// The same, for the one body the engine is on about itself - a corpse that has
    /// stopped being in anyone's way, which its twins have no reason to hear about.
    pub(crate) fn set_collision_one(
        &mut self,
        id: NodeId,
        enabled: bool,
    ) -> Result<(), SceneError> {
        if self.is_ui(id)? {
            return Err(SceneError::UiNotCollidable);
        }
        let entity = self.entity(id)?;
        if let Some(mut collider) = self.world.get_mut::<Collider>(entity) {
            collider.enabled = enabled;
            return Ok(());
        }

        if enabled {
            let kind = self.component::<NodeKind>(id)?.0.clone();
            self.world.entity_mut(entity).insert(Collider {
                enabled: true,
                shape: ColliderShape::for_kind(&kind),
            });
        }
        Ok(())
    }

    /// The shape the body is cut as. A node that never had a collider answers with
    /// the shape its kind reads as, so a script can ask before it switches
    /// collisions on.
    pub fn collision_shape(&self, id: NodeId) -> Result<ColliderShape, SceneError> {
        if let Ok(collider) = self.component::<Collider>(id) {
            return Ok(collider.shape);
        }
        if self.is_ui(id)? {
            return Err(SceneError::UiNotCollidable);
        }
        Ok(ColliderShape::for_kind(&self.component::<NodeKind>(id)?.0))
    }

    /// Cuts the collider a different way. Asking for a shape turns collisions on,
    /// since a shape nobody collides with is not something anyone writes on
    /// purpose.
    pub fn set_collision_shape(
        &mut self,
        id: NodeId,
        shape: ColliderShape,
    ) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.set_collision_one(member, true)?;
            scene.component_mut::<Collider>(member)?.shape = shape;
            Ok(())
        })
    }

    /// The shape the way a script writes it, so the mistake comes back as the
    /// list of what could have been written instead.
    pub fn set_collision_shape_name(&mut self, id: NodeId, name: &str) -> Result<(), SceneError> {
        let shape = ColliderShape::from_name(name)
            .ok_or_else(|| SceneError::UnknownShape(name.to_owned()))?;
        self.set_collision_shape(id, shape)
    }

    /// The node's collider in world meters, or nothing when it does not collide.
    /// A hidden body still blocks its way: an invisible floor is a floor.
    pub(crate) fn collider(&self, id: NodeId) -> Option<Solid> {
        let shape = self.component::<Collider>(id).ok()?;
        if !shape.enabled {
            return None;
        }
        let model = self.global_matrix(id).ok()?;
        let (min, max) = self.local_bounds(id)?;

        match shape.shape {
            ColliderShape::Box => Solid::new(corners(min, max, &model), 0.0),
            ColliderShape::Mesh => match self.mesh_points(id) {
                Some(points) => Solid::new(transformed(&points, &model), 0.0),
                None => Solid::new(corners(min, max, &model), 0.0),
            },
            ColliderShape::Sphere => {
                let centre = model.transform_point3((min + max) * 0.5);
                // A body squashed unevenly is still one ball to the solver: the
                // widest of the three ways it reaches, so the collider never
                // sits inside the drawn body.
                let radius = lengths(&model, min, max).into_iter().fold(0.0, f32::max);
                Solid::ball(centre, radius)
            }
            ColliderShape::Capsule => {
                let centre = model.transform_point3((min + max) * 0.5);
                let arms = arms(&model, min, max);
                let long = arms
                    .iter()
                    .max_by(|a, b| a.length().total_cmp(&b.length()))
                    .copied()
                    .unwrap_or(Vec3::Z);
                let thick = arms
                    .iter()
                    .filter(|arm| **arm != long)
                    .map(|arm| arm.length())
                    .min_by(|a, b| a.total_cmp(b))
                    .unwrap_or(0.0);
                // The ends of the segment are where the round part stops, so the
                // whole thing is exactly as long as the body.
                let along = long.normalize_or_zero() * (long.length() - thick).max(0.0);
                Solid::capsule([centre - along, centre + along], thick)
            }
        }
    }

    /// Every node the solver should reckon with, with its collider in hand.
    /// Parents come before children, the same order the step walks the tree in.
    pub(crate) fn colliders(&self) -> Vec<(NodeId, Solid)> {
        let mut list = Vec::new();
        let mut stack = vec![self.root()];

        while let Some(id) = stack.pop() {
            if id != self.root()
                && let Some(solid) = self.collider(id)
            {
                list.push((id, solid));
            }
            if let Ok(children) = self.children(id) {
                stack.extend(children.iter().rev().copied());
            }
        }
        list
    }

    /// The node's transform as the world sees it: its own matrix, then every
    /// parent's, composed from the root down. This is the chain the renderer
    /// walks, so a body collides exactly where it is drawn.
    pub(crate) fn global_matrix(&self, id: NodeId) -> Result<Mat4, SceneError> {
        let mut chain = vec![self.transform(id)?];
        let mut current = self.parent(id)?;
        while let Some(parent) = current {
            chain.push(self.transform(parent)?);
            current = self.parent(parent)?;
        }

        let mut model = Mat4::IDENTITY;
        for transform in chain.into_iter().rev() {
            model *= local_matrix(transform);
        }
        Ok(model)
    }

    /// The body's own box in its local meters: the mesh's bounds, or a unit cube
    /// for a node with no geometry, whose `size` is then read as a box of sides.
    fn local_bounds(&self, id: NodeId) -> Option<(Vec3, Vec3)> {
        let entity = self.entity(id).ok()?;
        Some(
            self.world
                .get::<NodeMesh>(entity)
                .and_then(|mesh| mesh.data.bounds())
                .unwrap_or((Vec3::splat(-0.5), Vec3::splat(0.5))),
        )
    }

    fn mesh_points(&self, id: NodeId) -> Option<Vec<Vec3>> {
        let entity = self.entity(id).ok()?;
        self.world
            .get::<NodeMesh>(entity)
            .map(|mesh| mesh.data.positions.clone())
    }
}

/// The eight corners of a box, carried into the world.
fn corners(min: Vec3, max: Vec3, model: &Mat4) -> Vec<Vec3> {
    let mut points = Vec::with_capacity(8);
    for corner in [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ] {
        points.push(model.transform_point3(corner));
    }
    points
}

fn transformed(points: &[Vec3], model: &Mat4) -> Vec<Vec3> {
    points
        .iter()
        .map(|point| model.transform_point3(*point))
        .collect()
}

/// A body's reach along each of its own axes, as a world vector: the mesh's half
/// extent, turned and stretched by everything the hierarchy put above it. A round
/// collider has to be cut from these, not from the mesh, because a small body
/// under a large parent is a large body to the solver.
fn arms(model: &Mat4, min: Vec3, max: Vec3) -> [Vec3; 3] {
    let half = (max - min) * 0.5;
    [
        model.transform_vector3(Vec3::new(half.x, 0.0, 0.0)),
        model.transform_vector3(Vec3::new(0.0, half.y, 0.0)),
        model.transform_vector3(Vec3::new(0.0, 0.0, half.z)),
    ]
}

fn lengths(model: &Mat4, min: Vec3, max: Vec3) -> [f32; 3] {
    let [x, y, z] = arms(model, min, max);
    [x.length(), y.length(), z.length()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::VecField;

    fn cube(scene: &mut Scene, size: f32, at: Vec3) -> NodeId {
        let id = scene.create("cube", None, None).unwrap();
        scene
            .set_vec_field(id, VecField::Size, Vec3::splat(size))
            .unwrap();
        scene.set_vec_field(id, VecField::Position, at).unwrap();
        id
    }

    #[test]
    fn nothing_collides_until_a_script_says_so() {
        let mut scene = Scene::new();
        let id = cube(&mut scene, 2.0, Vec3::ZERO);

        assert!(!scene.has_collision(id).unwrap());
        assert!(scene.collider(id).is_none());
        assert!(scene.colliders().is_empty());

        scene.set_collision(id, true).unwrap();
        assert!(scene.has_collision(id).unwrap());
        assert_eq!(scene.colliders().len(), 1);

        scene.set_collision(id, false).unwrap();
        assert!(!scene.has_collision(id).unwrap());
        assert!(scene.collider(id).is_none());
    }

    #[test]
    fn a_box_collides_as_the_box_its_size_describes() {
        let mut scene = Scene::new();
        let id = cube(&mut scene, 2.0, Vec3::new(1.0, 0.0, 0.0));
        scene.set_collision(id, true).unwrap();

        let (_, solid) = scene.colliders().pop().unwrap();
        let (min, max) = solid.aabb();
        assert!(
            (min - Vec3::new(0.0, -1.0, -1.0)).length() < 1e-5,
            "{min:?}"
        );
        assert!((max - Vec3::new(2.0, 1.0, 1.0)).length() < 1e-5, "{max:?}");
    }

    #[test]
    fn a_bodiless_node_collides_as_its_size() {
        let mut scene = Scene::new();
        let id = scene
            .spawn_entity(None, None, None, Some(Vec3::Y * 3.0), 10.0, 1.0)
            .unwrap();
        scene
            .set_vec_field(id, VecField::Size, Vec3::new(1.0, 1.0, 2.0))
            .unwrap();
        scene.set_collision(id, true).unwrap();

        let (_, solid) = scene.colliders().pop().unwrap();
        let (min, max) = solid.aabb();
        assert!(
            (min - Vec3::new(-0.5, 2.5, -1.0)).length() < 1e-5,
            "{min:?}"
        );
        assert!((max - Vec3::new(0.5, 3.5, 1.0)).length() < 1e-5, "{max:?}");
    }

    #[test]
    fn a_ball_body_gets_a_round_collider_and_a_cube_does_not() {
        let mut scene = Scene::new();
        let ball = scene.create("sphere", None, None).unwrap();
        scene
            .set_vec_field(ball, VecField::Size, Vec3::splat(2.0))
            .unwrap();
        let block = cube(&mut scene, 2.0, Vec3::ZERO);

        scene.set_collision(ball, true).unwrap();
        scene.set_collision(block, true).unwrap();
        assert_eq!(scene.collision_shape(ball).unwrap(), ColliderShape::Sphere);
        assert_eq!(scene.collision_shape(block).unwrap(), ColliderShape::Box);

        let (_, solid) = scene
            .colliders()
            .iter()
            .find(|(id, _)| *id == ball)
            .unwrap()
            .clone();
        let (min, max) = solid.aabb();
        assert!(
            (min - Vec3::splat(-1.0)).length() < 1e-5,
            "a ball as wide as its body: {min:?}"
        );
        assert!((max - Vec3::splat(1.0)).length() < 1e-5, "{max:?}");
    }

    #[test]
    fn a_round_collider_measures_the_body_where_it_is_not_the_mesh_it_was_built_with() {
        let mut scene = Scene::new();
        // The parent's size scales the child, so the ball the solver sees is twice
        // as wide as the mesh the renderer was given.
        let parent = cube(&mut scene, 4.0, Vec3::ZERO);
        let child = scene.create("sphere", None, Some(parent)).unwrap();
        scene.set_collision(child, true).unwrap();

        let (min, max) = scene.collider(child).unwrap().aabb();
        assert!((min - Vec3::splat(-2.0)).length() < 1e-5, "{min:?}");
        assert!((max - Vec3::splat(2.0)).length() < 1e-5, "{max:?}");
    }

    #[test]
    fn a_capsule_is_a_sausage_along_the_longest_way() {
        let mut scene = Scene::new();
        let id = scene.create("cube", None, None).unwrap();
        scene
            .set_vec_field(id, VecField::Size, Vec3::new(0.6, 0.6, 2.0))
            .unwrap();
        scene
            .set_collision_shape(id, ColliderShape::Capsule)
            .unwrap();

        let solid = scene.collider(id).unwrap();
        let (min, max) = solid.aabb();
        assert!((min.z - (-1.0)).abs() < 1e-5, "{min:?}");
        assert!((max.z - 1.0).abs() < 1e-5, "{max:?}");
        assert!((min.x - (-0.3)).abs() < 1e-5, "{min:?}");
        assert!((max.y - 0.3).abs() < 1e-5, "{max:?}");
        assert!(scene.has_collision(id).unwrap(), "a shape turns it on");
    }

    #[test]
    fn a_mesh_collides_with_every_corner_the_body_has() {
        let mut scene = Scene::new();
        let id = scene.create("sphere", None, None).unwrap();
        scene
            .set_vec_field(id, VecField::Size, Vec3::splat(2.0))
            .unwrap();
        scene.set_collision_shape(id, ColliderShape::Mesh).unwrap();

        let solid = scene.collider(id).unwrap();
        assert_eq!(
            solid.point_count(),
            scene.mesh(id).unwrap().vertex_count(),
            "the collider is the body"
        );
        let (min, max) = solid.aabb();
        assert!((min - Vec3::splat(-1.0)).length() < 1e-5, "{min:?}");
        assert!((max - Vec3::splat(1.0)).length() < 1e-5, "{max:?}");
    }

    #[test]
    fn a_child_collides_where_it_is_drawn() {
        let mut scene = Scene::new();
        let parent = cube(&mut scene, 2.0, Vec3::new(3.0, 0.0, 0.0));
        let child = scene.create("cube", None, Some(parent)).unwrap();
        scene
            .set_vec_field(child, VecField::Size, Vec3::splat(1.0))
            .unwrap();
        scene
            .set_vec_field(child, VecField::Position, Vec3::new(0.5, 0.0, 0.0))
            .unwrap();
        scene.set_collision(child, true).unwrap();

        // The parent's size scales both the offset and the body, exactly as the
        // renderer composes them.
        let (id, solid) = scene.colliders().pop().unwrap();
        assert_eq!(id, child);
        let (min, max) = solid.aabb();
        assert!((min.x - 3.0).abs() < 1e-5, "{min:?}");
        assert!((max.x - 5.0).abs() < 1e-5, "{max:?}");

        let drawn = scene
            .draw_items()
            .into_iter()
            .find(|item| item.node == child)
            .unwrap();
        let centre = (min + max) * 0.5;
        assert!(
            (drawn.model.w_axis.trunc() - centre.extend(1.0)).length() < 1e-4,
            "the collider sits where the body is drawn"
        );
    }

    #[test]
    fn a_turned_body_turns_its_collider_with_it() {
        let mut scene = Scene::new();
        let id = scene.create("cube", None, None).unwrap();
        scene
            .set_vec_field(id, VecField::Size, Vec3::new(2.0, 0.5, 0.5))
            .unwrap();
        scene
            .set_vec_field(id, VecField::RotationEuler, Vec3::new(0.0, 0.0, 90.0))
            .unwrap();
        scene.set_collision(id, true).unwrap();

        let (min, max) = scene.collider(id).unwrap().aabb();
        assert!((min.y - (-1.0)).abs() < 1e-5, "{min:?}");
        assert!((max.y - 1.0).abs() < 1e-5, "{max:?}");
        assert!((max.x - 0.25).abs() < 1e-5, "{max:?}");
    }

    #[test]
    fn overlay_never_gets_in_the_way() {
        let mut scene = Scene::new();
        let button = scene.create_ui("square", None, None).unwrap();

        assert!(matches!(
            scene.set_collision(button, true),
            Err(SceneError::UiNotCollidable)
        ));
        assert!(scene.colliders().is_empty());
    }

    #[test]
    fn a_shape_is_written_by_name() {
        let mut scene = Scene::new();
        let id = cube(&mut scene, 1.0, Vec3::ZERO);

        for name in ColliderShape::NAMES {
            scene.set_collision_shape_name(id, name).unwrap();
            assert_eq!(
                scene.collision_shape(id).unwrap().name(),
                *name,
                "each name round-trips"
            );
        }

        scene.set_collision_shape_name(id, "  BALL ").unwrap();
        assert_eq!(scene.collision_shape(id).unwrap(), ColliderShape::Sphere);

        let unknown = scene.set_collision_shape_name(id, "octahedron");
        assert!(matches!(unknown, Err(SceneError::UnknownShape(_))));
        let message = unknown.unwrap_err().to_string();
        assert!(message.contains("box, sphere, capsule, mesh"), "{message}");
    }

    #[test]
    fn a_destroyed_node_has_no_collisions_to_answer_for() {
        let mut scene = Scene::new();
        let id = cube(&mut scene, 1.0, Vec3::ZERO);
        scene.set_collision(id, true).unwrap();
        scene.destroy(id).unwrap();

        assert!(matches!(
            scene.has_collision(id),
            Err(SceneError::NodeDestroyed)
        ));
        assert!(scene.colliders().is_empty());
    }

    #[test]
    fn a_shape_can_be_read_before_anything_collides() {
        let mut scene = Scene::new();
        let ball = scene.create("sphere", None, None).unwrap();
        let crate_ = scene.create("cube", None, None).unwrap();
        let button = scene.create_ui("square", None, None).unwrap();

        assert!(!scene.has_collision(ball).unwrap());
        assert_eq!(scene.collision_shape(ball).unwrap(), ColliderShape::Sphere);
        assert_eq!(
            scene.collision_shape(crate_).unwrap(),
            ColliderShape::Box,
            "the shape the body would be cut as the moment it is switched on"
        );
        assert!(
            matches!(
                scene.collision_shape(button),
                Err(SceneError::UiNotCollidable)
            ),
            "and no shape at all for an element pinned to the screen"
        );
    }
}
