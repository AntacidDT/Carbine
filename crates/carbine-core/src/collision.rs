//! Colliders: what a body means to the solver.
//!
//! Nothing collides until a script says so. Turning it on cuts a collider from
//! the body the node already has, in the same meters the renderer draws it in,
//! so `pillar.collision = True` is the whole of the work.

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::{Mat4, Vec3};

use crate::draw::local_matrix;
use crate::geometry::Solid;
use crate::node::{NodeId, NodeKind, NodeMesh, Scene, SceneError};

/// The three ways a body can be read. A box or a ball is the one solid the solver
/// answers with at once, which is all the detail a stack of crates needs; a mesh is
/// a solid per surface the body is drawn from, which is the only way a body with a
/// hollow in it - a vehicle, a chair, a room - gets a collider that is not a brick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CollisionType {
    /// The box the body's `size` describes.
    Box,
    /// A ball as wide as the body's widest dimension.
    Sphere,
    /// The body's own surfaces, each of them a solid of its own.
    Mesh,
}

impl CollisionType {
    /// The names a script writes, in the order they are listed when one is wrong.
    pub const NAMES: &'static [&'static str] = &["box", "sphere", "mesh"];

    pub fn from_name(name: &str) -> Option<CollisionType> {
        Some(match name.trim().to_lowercase().as_str() {
            "box" | "cube" => Self::Box,
            "sphere" | "ball" => Self::Sphere,
            "mesh" => Self::Mesh,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Box => "box",
            Self::Sphere => "sphere",
            Self::Mesh => "mesh",
        }
    }

    /// The type a body of this kind reads as. A ball is round; everything else
    /// is the box its `size` describes, which is the shape a solver is fastest at.
    fn for_kind(kind: &str) -> CollisionType {
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
    pub(crate) shape: CollisionType,
}

/// What one body is made of, kept from the step it was cut in.
///
/// A mesh is a thousand points per surface and a parked fleet is the same points every
/// step it stands still, so the walk that carries them into world metres is done when a
/// body moves - or the file under it moves - rather than at every pass of the solver.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Cut {
    key: CutKey,
    /// The body's own box in its local metres, which is what the floor rests it on.
    bounds: Option<(Vec3, Vec3)>,
    /// The body in world metres, one solid per piece it is cut into.
    pieces: Vec<Solid>,
}

/// The cheap description of a body, which is what a cached cut is looked up by: every
/// surface is named by the node it belongs to, where it sits under the body and how many
/// points it brings, so nothing here grows with the size of the model.
#[derive(Clone, Debug, PartialEq)]
struct CutKey {
    /// Where the body stands in the world, or nothing when the scene cannot say.
    at: Option<[f32; 16]>,
    /// Whether the body is in the solver, and how it is read.
    collider: Option<(bool, CollisionType)>,
    /// Each surface the body is drawn from, in the order the branch is walked.
    surfaces: Vec<(NodeId, [f32; 16], usize)>,
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
                shape: CollisionType::for_kind(&kind),
            });
        }
        Ok(())
    }

    /// The type the body is cut as. A node that never had a collider answers with
    /// the type its kind reads as, so a script can ask before it switches
    /// collisions on.
    pub fn collision_type(&self, id: NodeId) -> Result<CollisionType, SceneError> {
        if let Ok(collider) = self.component::<Collider>(id) {
            return Ok(collider.shape);
        }
        if self.is_ui(id)? {
            return Err(SceneError::UiNotCollidable);
        }
        Ok(CollisionType::for_kind(&self.component::<NodeKind>(id)?.0))
    }

    /// Cuts the collider a different way. Asking for a type turns collisions on,
    /// since a type nobody collides with is not something anyone writes on
    /// purpose.
    pub fn set_collision_type(
        &mut self,
        id: NodeId,
        shape: CollisionType,
    ) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.set_collision_one(member, true)?;
            scene.component_mut::<Collider>(member)?.shape = shape;
            Ok(())
        })
    }

    /// The type the way a script writes it, so the mistake comes back as the
    /// list of what could have been written instead.
    pub fn set_collision_type_name(&mut self, id: NodeId, name: &str) -> Result<(), SceneError> {
        let shape = CollisionType::from_name(name)
            .ok_or_else(|| SceneError::UnknownCollisionType(name.to_owned()))?;
        self.set_collision_type(id, shape)
    }

    /// The node's collider in world meters, as the pieces it is cut into, and nothing
    /// at all when it does not collide. A hidden body still blocks its way: an
    /// invisible floor is a floor.
    ///
    /// A box is one piece and a sphere is one piece, because that is what they are. A
    /// mesh is one piece per surface the body is drawn from - the node's own mesh, or
    /// every part an import hung under it - so the space between a tank's wheels is
    /// empty to the solver instead of filled in by the brick around the whole vehicle.
    ///
    /// The pieces come from the body's cut when it is still the body it was, which is
    /// what `recut` keeps standing; a scene that has not stepped yet cuts it here.
    pub(crate) fn collider(&self, id: NodeId) -> Vec<Solid> {
        if let Some(cut) = self.cuts.get(&id)
            && cut.key == self.cut_key(id)
        {
            return cut.pieces.clone();
        }
        self.pieces_now(id)
    }

    /// The same body cut the long way: its geometry walked and carried into world
    /// metres, with nothing kept of it. This is what a scene that has never stepped
    /// answers with, and what `recut` keeps a cut of.
    fn pieces_now(&self, id: NodeId) -> Vec<Solid> {
        let Ok(shape) = self.component::<Collider>(id) else {
            return Vec::new();
        };
        if !shape.enabled {
            return Vec::new();
        }
        let Ok(model) = self.global_matrix(id) else {
            return Vec::new();
        };
        let Some((min, max)) = self.local_bounds(id) else {
            return Vec::new();
        };

        match shape.shape {
            CollisionType::Box => Solid::new(corners(min, max, &model), 0.0)
                .into_iter()
                .collect(),
            CollisionType::Sphere => {
                let centre = model.transform_point3((min + max) * 0.5);
                // A body squashed unevenly is still one ball to the solver: the
                // widest of the three ways it reaches, so the collider never
                // sits inside the drawn body.
                let radius = lengths(&model, min, max).into_iter().fold(0.0, f32::max);
                Solid::ball(centre, radius).into_iter().collect()
            }
            CollisionType::Mesh => {
                let mut pieces: Vec<Solid> = self
                    .local_surfaces(id)
                    .into_iter()
                    .filter_map(|points| Solid::new(transformed(&points, &model), 0.0))
                    .collect();
                if pieces.is_empty() {
                    // A body with no surface of its own to cut is still the box its
                    // `size` describes, which is what it draws as.
                    pieces = Solid::new(corners(min, max, &model), 0.0)
                        .into_iter()
                        .collect();
                }
                pieces
            }
        }
    }

    /// The key this body's cut is kept under, asked of the cheap parts of it only.
    fn cut_key(&self, id: NodeId) -> CutKey {
        CutKey {
            at: self
                .global_matrix(id)
                .ok()
                .map(|model| model.to_cols_array()),
            collider: self
                .component::<Collider>(id)
                .ok()
                .map(|collider| (collider.enabled, collider.shape)),
            surfaces: self.model_surface_key(id),
        }
    }

    /// Keeps every body the solver and the floor ask about cut to where it now stands,
    /// so a step that parks four vehicles turns their thousands of points into world
    /// metres once instead of at every pass. A body that has not moved, and whose file
    /// has not moved under it, is the same body it was.
    ///
    /// A cut is only ever carried forward between steps of the same scene, which is why
    /// the walk below moves the old cuts rather than copying them: a node that is no
    /// longer in the tree leaves with the tree it left.
    pub(crate) fn recut(&mut self) {
        let tree = self.node_tree();
        let mut kept = std::mem::take(&mut self.cuts);
        let mut current = HashMap::with_capacity(kept.len());

        for id in tree {
            let collider = self
                .component::<Collider>(id)
                .ok()
                .map(|collider| (collider.enabled, collider.shape));
            if collider.is_none() && !self.wears_file(id) {
                continue;
            }

            let key = self.cut_key(id);
            let cut = match kept.remove(&id) {
                Some(found) if found.key == key => found,
                _ => Cut {
                    bounds: self.measured_bounds(id),
                    pieces: self.pieces_now(id),
                    key,
                },
            };
            current.insert(id, cut);
        }

        self.cuts = current;
    }

    /// Every node the solver should reckon with, with its collider in hand.
    /// Parents come before children, the same order the step walks the tree in.
    pub(crate) fn colliders(&self) -> Vec<(NodeId, Vec<Solid>)> {
        let mut list = Vec::new();
        let mut stack = vec![self.root()];

        while let Some(id) = stack.pop() {
            if id != self.root() {
                let pieces = self.collider(id);
                if !pieces.is_empty() {
                    list.push((id, pieces));
                }
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
    /// A node wearing a model is cut from the whole model, not from one surface.
    ///
    /// Only the surfaces decide this box, so a body that has been walked along the
    /// floor by its own gravity keeps the answer it was cut with.
    pub(crate) fn local_bounds(&self, id: NodeId) -> Option<(Vec3, Vec3)> {
        if let Some(cut) = self.cuts.get(&id)
            && cut.key.surfaces == self.model_surface_key(id)
        {
            return cut.bounds;
        }
        self.measured_bounds(id)
    }

    /// The same box, measured off the geometry the long way.
    fn measured_bounds(&self, id: NodeId) -> Option<(Vec3, Vec3)> {
        if let Some(bounds) = self.model_bounds(id) {
            return Some(bounds);
        }
        let entity = self.entity(id).ok()?;
        Some(
            self.world
                .get::<NodeMesh>(entity)
                .and_then(|mesh| mesh.data.bounds())
                .unwrap_or((Vec3::splat(-0.5), Vec3::splat(0.5))),
        )
    }

    /// The surfaces the body is drawn from, each as its own pile of points in the node's
    /// local metres: every part of an import separately, or a plain node's one mesh.
    /// Empty for a body that draws nothing, which is cut as its box instead.
    fn local_surfaces(&self, id: NodeId) -> Vec<Vec<Vec3>> {
        let pieces = self.model_pieces(id);
        if !pieces.is_empty() {
            return pieces;
        }
        let Ok(entity) = self.entity(id) else {
            return Vec::new();
        };
        self.world
            .get::<NodeMesh>(entity)
            .map(|mesh| vec![mesh.data.positions.clone()])
            .unwrap_or_default()
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

    const SECOND: f32 = 1.0 / 60.0;

    fn cube(scene: &mut Scene, size: f32, at: Vec3) -> NodeId {
        let id = scene.create("cube", None, None).unwrap();
        scene
            .set_vec_field(id, VecField::Size, Vec3::splat(size))
            .unwrap();
        scene.set_vec_field(id, VecField::Position, at).unwrap();
        id
    }

    /// The one piece of a body that is only ever cut as one: a box, a ball, or a
    /// plain node's own mesh.
    fn piece(scene: &Scene, id: NodeId) -> Solid {
        let mut pieces = scene.collider(id);
        assert_eq!(pieces.len(), 1, "a body with one surface has one piece");
        pieces.pop().unwrap()
    }

    #[test]
    fn nothing_collides_until_a_script_says_so() {
        let mut scene = Scene::new();
        let id = cube(&mut scene, 2.0, Vec3::ZERO);

        assert!(!scene.has_collision(id).unwrap());
        assert!(scene.collider(id).is_empty());
        assert!(scene.colliders().is_empty());

        scene.set_collision(id, true).unwrap();
        assert!(scene.has_collision(id).unwrap());
        assert_eq!(scene.colliders().len(), 1);

        scene.set_collision(id, false).unwrap();
        assert!(!scene.has_collision(id).unwrap());
        assert!(scene.collider(id).is_empty());
    }

    #[test]
    fn a_box_collides_as_the_box_its_size_describes() {
        let mut scene = Scene::new();
        let id = cube(&mut scene, 2.0, Vec3::new(1.0, 0.0, 0.0));
        scene.set_collision(id, true).unwrap();

        let (min, max) = piece(&scene, id).aabb();
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

        let (min, max) = piece(&scene, id).aabb();
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
        assert_eq!(scene.collision_type(ball).unwrap(), CollisionType::Sphere);
        assert_eq!(scene.collision_type(block).unwrap(), CollisionType::Box);

        let (min, max) = piece(&scene, ball).aabb();
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

        let (min, max) = piece(&scene, child).aabb();
        assert!((min - Vec3::splat(-2.0)).length() < 1e-5, "{min:?}");
        assert!((max - Vec3::splat(2.0)).length() < 1e-5, "{max:?}");
    }

    #[test]
    fn a_mesh_collides_with_every_corner_the_body_has() {
        let mut scene = Scene::new();
        let id = scene.create("sphere", None, None).unwrap();
        scene
            .set_vec_field(id, VecField::Size, Vec3::splat(2.0))
            .unwrap();
        scene.set_collision_type(id, CollisionType::Mesh).unwrap();

        let solid = piece(&scene, id);
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
    fn a_mesh_body_with_no_surface_of_its_own_is_cut_as_its_box() {
        let mut scene = Scene::new();
        let id = scene
            .spawn_entity(None, None, None, Some(Vec3::Z * 2.0), 10.0, 1.0)
            .unwrap();
        scene
            .set_vec_field(id, VecField::Size, Vec3::new(2.0, 1.0, 4.0))
            .unwrap();
        scene.set_collision_type(id, CollisionType::Mesh).unwrap();

        // Nothing to cut the pieces from, so the body the solver answers with is the
        // box the node draws as.
        let (min, max) = piece(&scene, id).aabb();
        assert!(
            (min - Vec3::new(-1.0, -0.5, 0.0)).length() < 1e-5,
            "{min:?}"
        );
        assert!((max - Vec3::new(1.0, 0.5, 4.0)).length() < 1e-5, "{max:?}");
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
        let (id, pieces) = scene.colliders().pop().unwrap();
        assert_eq!(id, child);
        let (min, max) = pieces[0].aabb();
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

        let (min, max) = piece(&scene, id).aabb();
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
    fn a_type_is_written_by_name() {
        let mut scene = Scene::new();
        let id = cube(&mut scene, 1.0, Vec3::ZERO);

        for name in CollisionType::NAMES {
            scene.set_collision_type_name(id, name).unwrap();
            assert_eq!(
                scene.collision_type(id).unwrap().name(),
                *name,
                "each name round-trips"
            );
        }

        scene.set_collision_type_name(id, "  BALL ").unwrap();
        assert_eq!(scene.collision_type(id).unwrap(), CollisionType::Sphere);

        let unknown = scene.set_collision_type_name(id, "octahedron");
        assert!(matches!(unknown, Err(SceneError::UnknownCollisionType(_))));
        let message = unknown.unwrap_err().to_string();
        assert!(message.contains("box, sphere, mesh"), "{message}");
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
    fn a_type_can_be_read_before_anything_collides() {
        let mut scene = Scene::new();
        let ball = scene.create("sphere", None, None).unwrap();
        let crate_ = scene.create("cube", None, None).unwrap();
        let button = scene.create_ui("square", None, None).unwrap();

        assert!(!scene.has_collision(ball).unwrap());
        assert_eq!(scene.collision_type(ball).unwrap(), CollisionType::Sphere);
        assert_eq!(
            scene.collision_type(crate_).unwrap(),
            CollisionType::Box,
            "the type the body would be cut as the moment it is switched on"
        );
        assert!(
            matches!(
                scene.collision_type(button),
                Err(SceneError::UiNotCollidable)
            ),
            "and no type at all for an element pinned to the screen"
        );
    }

    #[test]
    fn a_body_is_cut_where_it_stands_after_the_step_that_kept_its_cut() {
        let mut scene = Scene::new();
        let id = cube(&mut scene, 1.0, Vec3::new(0.0, 0.0, 1.0));
        scene.set_collision(id, true).unwrap();

        // The first step keeps the body's cut; the second is the one that has to answer
        // for the body somewhere else.
        scene.step(SECOND);
        scene
            .set_vec_field(id, VecField::Position, Vec3::new(0.0, 0.0, 9.0))
            .unwrap();
        scene.step(SECOND);

        let (min, max) = piece(&scene, id).aabb();
        assert!(
            (min - Vec3::new(-0.5, -0.5, 8.5)).length() < 1e-5
                && (max - Vec3::new(0.5, 0.5, 9.5)).length() < 1e-5,
            "a kept cut is not a stale one: {min:?} .. {max:?}"
        );
    }

    #[test]
    fn a_body_pushes_its_neighbour_from_where_it_stands_now() {
        let mut scene = Scene::new();
        let wall = cube(&mut scene, 2.0, Vec3::new(0.0, 0.0, 10.0));
        scene.set_collision(wall, true).unwrap();
        scene.set_anchored(wall, true).unwrap();

        let neighbour = cube(&mut scene, 1.0, Vec3::new(0.0, 0.0, 10.0));
        scene.set_collision(neighbour, true).unwrap();
        scene.set_uses_gravity(neighbour, true).unwrap();

        // Parked apart, the two bodies have nothing to say to each other.
        scene
            .set_vec_field(neighbour, VecField::Position, Vec3::new(6.0, 0.0, 10.0))
            .unwrap();
        for _ in 0..3 {
            scene.step(SECOND);
        }
        let apart = scene
            .vec_field(neighbour, VecField::Position)
            .unwrap()
            .to_array();
        assert!(
            (apart[0] - 6.0).abs() < 1e-3,
            "nothing was in the way: {apart:?}"
        );

        // Walked into the wall, the cut the step keeps has to follow the body in, or
        // the solver spends the step pushing the neighbour away from nothing.
        scene
            .set_vec_field(neighbour, VecField::Position, Vec3::new(1.2, 0.0, 10.0))
            .unwrap();
        for _ in 0..3 {
            scene.step(SECOND);
        }
        let gone = scene
            .vec_field(neighbour, VecField::Position)
            .unwrap()
            .to_array();
        assert!(gone[0] > 1.4, "the wall should have refused it: {gone:?}");
    }
}
