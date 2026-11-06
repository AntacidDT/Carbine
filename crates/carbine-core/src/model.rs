//! A model file, hung into the scene as nodes.
//!
//! An import is not one mesh welded into the shape of a vehicle: every node the file names
//! becomes a node of the scene, standing where the file put it, so a turret is still under its
//! tank and a seat is still a place to hang an avatar on. What a file holds in Y-up is read into
//! Carbine's Z-up by [`carbine_glb`], which is where that crossing is done once.
//!
//! A node wears a model by path, and the path is remembered: writing a new one takes the old
//! branch out of the world first, and writing `None` leaves the node standing as it was.

use bevy_ecs::prelude::Component;
use glam::Vec3;

use crate::draw::local_matrix;
use crate::mesh::MeshData;
use crate::motion::Motion;
use crate::node::{
    Children, DEFAULT_COLOR, MAX_ALPHA, NodeId, NodeKind, NodeMesh, NodeName, NodeTexture,
    ObjectColor, Opacity, Parent, Scene, SceneError, TextureTiling, Transform, Visible,
};

/// The kind word for a node that is there to wear a model.
pub const KIND: &str = "model";

/// Which file a node wears, and which branch of it was pulled out.
#[derive(Clone, Component)]
pub(crate) struct ModelSource {
    pub(crate) path: String,
    pub(crate) node: Option<String>,
}

/// A node the import put in the world, as opposed to one a script wrote by hand. Taking a
/// model off a node takes these out with it, and never a node the script named itself.
#[derive(Clone, Copy, Component)]
pub(crate) struct Adopted;

/// `file.glb`, or `file.glb#Heavy Tank` for one branch of a file that holds a whole hangar.
fn split(spec: &str) -> (&str, Option<&str>) {
    match spec.rsplit_once('#') {
        Some((path, node)) if !node.is_empty() => (path, Some(node)),
        _ => (spec, None),
    }
}

/// The model the path asks for, or the reason it is not one.
fn read(path: &str, node: Option<&str>) -> Result<carbine_glb::Model, String> {
    if path.to_lowercase().ends_with(".fbx") {
        return Err(
            "it is FBX, which Carbine does not read; export the model as glTF Binary (.glb) \
             instead"
                .to_owned(),
        );
    }
    let Ok(bytes) = std::fs::read(path) else {
        return Err("there is no file at that path".to_owned());
    };
    if !carbine_glb::is_glb(&bytes) {
        return Err("it is not a GLB file".to_owned());
    }
    carbine_glb::read_scene(&bytes, node).map_err(|why| why.to_string())
}

fn surface(part: &carbine_glb::Part) -> MeshData {
    MeshData {
        positions: part.positions.clone(),
        normals: part.normals.clone(),
        uvs: part.uvs.clone(),
        indices: part.indices.clone(),
        // Nothing a file holds is a shell the way Carbine's own sphere is, and a
        // file's picture is already laid out the way the modelling program chose.
        latlong: false,
    }
}

fn worn(transform: &carbine_glb::Transform) -> Transform {
    Transform {
        position: transform.position,
        rotation: transform.rotation,
        scale: transform.scale,
    }
}

impl Scene {
    /// A node with no body of its own, waiting for a model to be hung on it.
    pub fn create_model(
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
                NodeName(name.unwrap_or_else(|| "Model".to_owned())),
                NodeKind(KIND.to_owned()),
                Transform::default(),
                ObjectColor(DEFAULT_COLOR),
                Opacity::default(),
                NodeTexture::default(),
                TextureTiling::default(),
                Visible::default(),
                Motion::default(),
                Parent(parent_id),
                Children::default(),
            ))
            .id();
        self.insert(id, entity);
        self.attach(id, parent_id);

        Ok(id)
    }

    /// The model a node wears, written back the way it was asked for.
    pub fn model(&self, id: NodeId) -> Result<Option<String>, SceneError> {
        self.entity(id)?;
        Ok(self
            .component::<ModelSource>(id)
            .ok()
            .map(|source| match &source.node {
                Some(node) => format!("{}#{node}", source.path),
                None => source.path.clone(),
            }))
    }

    /// Hangs a model file on the node, or takes the one it wears off.
    ///
    /// The file's roots come under the node as children, so the node stays the anchor a script
    /// moves and everything the file named stays addressable beneath it.
    pub fn set_model(&mut self, id: NodeId, spec: Option<String>) -> Result<(), SceneError> {
        self.entity(id)?;
        self.branch_allows(id, false)?;
        self.drop_model(id);

        let Some(spec) = spec else { return Ok(()) };
        let (path, node) = split(&spec);
        let model = read(path, node).map_err(|why| SceneError::BadModel {
            path: path.to_owned(),
            why,
        })?;

        let owner = self.entity(id)?;
        self.world.entity_mut(owner).insert(ModelSource {
            path: path.to_owned(),
            node: node.map(str::to_owned),
        });
        for root in model.roots.clone() {
            self.hang(id, &model, root, None);
        }
        Ok(())
    }

    /// The branch the last import hung here, taken out of the world again.
    fn drop_model(&mut self, id: NodeId) {
        for child in self.adopted_children(id) {
            let _ = self.destroy(child);
        }
        if let Ok(owner) = self.entity(id) {
            self.world.entity_mut(owner).remove::<ModelSource>();
        }
    }

    fn is_adopted(&self, id: NodeId) -> bool {
        let Ok(entity) = self.entity(id) else {
            return false;
        };
        self.world.get::<Adopted>(entity).is_some()
    }

    /// One node of the file, with its surfaces and everything under it.
    ///
    /// A node holding more than one surface keeps the first as its own body and hangs the rest
    /// beside its children, named after it: a node carries one mesh, and the pair of them still
    /// measures as the one node the file described.
    fn hang(
        &mut self,
        parent: NodeId,
        model: &carbine_glb::Model,
        index: usize,
        base: Option<String>,
    ) -> NodeId {
        let node = &model.nodes[index];
        let name = node.name.clone().or(base).unwrap_or_else(|| "Model".into());
        let id = self.hang_node(
            parent,
            name.clone(),
            worn(&node.transform),
            node.parts.first(),
        );

        for (extra, part) in node.parts.iter().enumerate().skip(1) {
            self.hang_node(
                id,
                format!("{name}.{extra}"),
                Transform::default(),
                Some(part),
            );
        }
        for child in node.children.clone() {
            self.hang(id, model, child, Some(name.clone()));
        }
        id
    }

    #[allow(clippy::needless_pass_by_value)]
    fn hang_node(
        &mut self,
        parent: NodeId,
        name: String,
        transform: Transform,
        part: Option<&carbine_glb::Part>,
    ) -> NodeId {
        let id = self.reserve();
        let handle = part.map(|_| self.next_mesh_handle());
        let (color, alpha) = part.map_or((DEFAULT_COLOR, 1.0), |part| {
            let [red, green, blue, alpha] = part.material.color;
            (Vec3::new(red, green, blue), alpha)
        });
        let mut spawned = self.world.spawn((
            NodeName(name),
            NodeKind(KIND.to_owned()),
            transform,
            ObjectColor(color),
            Opacity(alpha * MAX_ALPHA),
            NodeTexture::default(),
            // A surface the file drew keeps the picture where the file put it: its own
            // UVs are already the artist's layout, so nothing is measured out for it
            // until a script asks for tiles by writing a metre count on the part.
            TextureTiling(0.0),
            Visible::default(),
            Motion::default(),
            Parent(parent),
            Children::default(),
            Adopted,
        ));
        if let (Some(part), Some(handle)) = (part, handle) {
            spawned.insert(NodeMesh {
                handle,
                data: surface(part),
            });
        }
        let entity = spawned.id();
        self.insert(id, entity);
        self.attach(id, parent);
        id
    }

    /// The first node under this one carrying the name the file gave it, searched down the
    /// branch rather than one level at a time, because a turret sits under its tank.
    pub fn descendant_named(&self, id: NodeId, name: &str) -> Result<Option<NodeId>, SceneError> {
        self.entity(id)?;
        let mut queue = self.children(id)?;
        while let Some(current) = queue.pop() {
            if self.name(current)? == name {
                return Ok(Some(current));
            }
            queue.extend(self.children(current)?);
        }
        Ok(None)
    }

    /// Every surface a model reaches, folded into this node's own local metres.
    ///
    /// This is what makes `size` mean the whole vehicle rather than whichever surface the node
    /// wears itself, and what a collider on the anchor is cut from.
    pub(crate) fn model_bounds(&self, id: NodeId) -> Option<(Vec3, Vec3)> {
        let entity = self.entity(id).ok()?;
        let wears = self.world.get::<ModelSource>(entity).is_some()
            || self.world.get::<Adopted>(entity).is_some();
        if !wears {
            return None;
        }

        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        let mut found = false;
        for (node, into) in self.model_walk(id) {
            let Ok(body) = self.component::<NodeMesh>(node) else {
                continue;
            };
            if let Some((own_min, own_max)) = body.data.bounds() {
                // All eight corners, not the two the box names: a file's node can be turned
                // any way the modelling program left it, and a turned box's reach is not its
                // own corners' reach.
                for x in [own_min.x, own_max.x] {
                    for y in [own_min.y, own_max.y] {
                        for z in [own_min.z, own_max.z] {
                            let place = into.transform_point3(Vec3::new(x, y, z));
                            min = min.min(place);
                            max = max.max(place);
                        }
                    }
                }
                found = true;
            }
        }
        found.then_some((min, max))
    }

    /// The model's dimensions at scale one, with the same care a flat shape gets from a
    /// primitive: an axis the model does not reach along stays a metre.
    pub(crate) fn model_extent(&self, id: NodeId) -> Option<Vec3> {
        let (min, max) = self.model_bounds(id)?;
        let reach = max - min;
        Some(Vec3::new(
            axis_extent(reach.x),
            axis_extent(reach.y),
            axis_extent(reach.z),
        ))
    }

    /// The points a mesh collider is cut from: one pile per surface the model drew, each
    /// in this node's metres. A surface stays its own pile rather than being folded into
    /// the ones beside it, because that is the whole point of asking for a mesh: a turret,
    /// a hull and a wheel are three solids to the solver, and the air between them is air.
    pub(crate) fn model_pieces(&self, id: NodeId) -> Vec<Vec<Vec3>> {
        let mut pieces = Vec::new();
        for (node, into) in self.model_walk(id) {
            let Ok(body) = self.component::<NodeMesh>(node) else {
                continue;
            };
            if body.data.positions.is_empty() {
                continue;
            }
            pieces.push(
                body.data
                    .positions
                    .iter()
                    .map(|position| into.transform_point3(*position))
                    .collect(),
            );
        }
        pieces
    }

    /// The cheap twin of [`Scene::model_pieces`], asked at every step of a scene that
    /// keeps its cuts: each surface's own node, where it sits under this one, and how
    /// many points it brings. A file put on or taken off, a part lifted clear of its
    /// hull and a body's own mesh all move this list, and none of them cost a point.
    pub(crate) fn model_surface_key(&self, id: NodeId) -> Vec<(NodeId, [f32; 16], usize)> {
        self.model_walk(id)
            .into_iter()
            .filter_map(|(node, into)| {
                let body = self.component::<NodeMesh>(node).ok()?;
                let points = body.data.positions.len();
                (points > 0).then(|| (node, into.to_cols_array(), points))
            })
            .collect()
    }

    /// Whether a script gave this node a file to wear. The file's own nodes hanging
    /// under it are not anchors of one, and only an anchor's box has to be measured
    /// across a whole branch.
    pub(crate) fn wears_file(&self, id: NodeId) -> bool {
        self.entity(id)
            .map(|entity| self.world.get::<ModelSource>(entity).is_some())
            .unwrap_or(false)
    }

    /// This node and the branch an import hung under it, each with the matrix that carries its
    /// local metres into this node's own. A body the script wrote itself is not walked into.
    ///
    /// The node's own matrix is not part of any of them: what comes back is in the node's local
    /// metres, the same place a plain node's mesh bounds live, so `size` stays the model at
    /// scale one rather than the model measured through the scale it is asking about.
    fn model_walk(&self, id: NodeId) -> Vec<(NodeId, glam::Mat4)> {
        let mut out = vec![(id, glam::Mat4::IDENTITY)];
        let mut stack: Vec<(NodeId, glam::Mat4)> = self
            .adopted_children(id)
            .into_iter()
            .map(|child| {
                let local = self.transform(child).unwrap_or_default();
                (child, local_matrix(local))
            })
            .collect();
        while let Some((current, into)) = stack.pop() {
            out.push((current, into));
            for child in self.adopted_children(current) {
                let local = self.transform(child).unwrap_or_default();
                stack.push((child, into * local_matrix(local)));
            }
        }
        out
    }

    /// The children of a node that an import put there, the only ones a model walk enters.
    fn adopted_children(&self, id: NodeId) -> Vec<NodeId> {
        self.children(id)
            .unwrap_or_default()
            .into_iter()
            .filter(|child| self.is_adopted(*child))
            .collect()
    }
}

/// An axis a model does not reach along stays a metre, the way a flat primitive keeps its
/// remaining axes authorable.
fn axis_extent(reach: f32) -> f32 {
    if reach > f32::EPSILON { reach } else { 1.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::CollisionType;
    use crate::geometry::Solid;
    use crate::node::VecField;
    use crate::player::GROUND_HEIGHT;
    use glam::Vec2;

    /// A square sheet, `half` from its centre to each edge, lying flat in its own node.
    fn sheet(half: f32, color: [f32; 4]) -> carbine_glb::Part {
        carbine_glb::Part {
            positions: vec![
                Vec3::new(-half, -half, 0.0),
                Vec3::new(half, -half, 0.0),
                Vec3::new(half, half, 0.0),
                Vec3::new(-half, half, 0.0),
            ],
            normals: vec![Vec3::Z; 4],
            uvs: vec![Vec2::ZERO, Vec2::X, Vec2::Y, Vec2::ONE],
            indices: vec![0, 1, 2, 0, 2, 3],
            material: carbine_glb::Material { name: None, color },
        }
    }

    fn place(x: f32, y: f32, z: f32) -> carbine_glb::Transform {
        carbine_glb::Transform {
            position: Vec3::new(x, y, z),
            ..carbine_glb::Transform::IDENTITY
        }
    }

    /// A tank of two surfaces with a turret on top, written out as the only format Carbine
    /// reads and left on disk for the tests below to open.
    fn hangar() -> String {
        let hull = [0.6, 0.2, 0.1, 1.0];
        let skirt = [0.1, 0.1, 0.5, 0.6];
        let barrel = [0.9, 0.9, 0.9, 1.0];
        let model = carbine_glb::Model::new(
            vec![
                carbine_glb::Node {
                    name: Some("Tank".to_owned()),
                    transform: place(0.0, 0.0, 0.5),
                    children: vec![1],
                    parts: vec![sheet(1.0, hull), sheet(0.5, skirt)],
                },
                carbine_glb::Node {
                    name: Some("Turret".to_owned()),
                    transform: place(0.0, 0.25, 1.0),
                    children: Vec::new(),
                    parts: vec![sheet(0.25, barrel)],
                },
            ],
            vec![0],
        );

        let path = std::env::temp_dir().join("carbine-model-hangar.glb");
        std::fs::write(&path, carbine_glb::write(&model)).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn anchor(scene: &mut Scene) -> NodeId {
        let id = scene
            .create("model", Some("Import".to_owned()), None)
            .unwrap();
        scene.set_model(id, Some(hangar())).unwrap();
        id
    }

    /// The file's node, one step under the anchor a script wrote on.
    fn tank(scene: &Scene, id: NodeId) -> NodeId {
        *scene.children(id).unwrap().first().unwrap()
    }

    /// How high in the world the body's cut reaches: the roofline of whichever piece
    /// stands tallest.
    fn roof(scene: &Scene, id: NodeId) -> f32 {
        scene
            .collider(id)
            .iter()
            .map(|piece| piece.aabb().1.z)
            .fold(f32::MIN, f32::max)
    }

    #[test]
    fn an_import_arrives_as_nodes_and_not_as_one_welded_mesh() {
        let mut scene = Scene::new();
        let id = anchor(&mut scene);

        assert_eq!(scene.kind(id).unwrap(), KIND);
        assert!(
            scene.mesh(id).is_err(),
            "the anchor keeps no body of its own"
        );

        let hull = tank(&scene, id);
        assert_eq!(scene.name(hull).unwrap(), "Tank");
        assert_eq!(scene.kind(hull).unwrap(), KIND);
        assert_eq!(scene.parent(hull).unwrap(), Some(id));
        assert!(!scene.mesh(hull).unwrap().positions.is_empty());

        let turret = scene.descendant_named(id, "Turret").unwrap().unwrap();
        assert_eq!(scene.parent(turret).unwrap(), Some(hull));

        // Standing where the file put them: the turret is a metre up the hull's own metres.
        assert_eq!(
            scene.transform(hull).unwrap().position,
            Vec3::new(0.0, 0.0, 0.5)
        );
        assert_eq!(
            scene.transform(turret).unwrap().position,
            Vec3::new(0.0, 0.25, 1.0)
        );

        // The whole branch is content the script can count, and the path is remembered.
        assert_eq!(scene.node_count(), 4);
        assert_eq!(scene.model(id).unwrap(), Some(hangar()));
    }

    #[test]
    fn a_surface_and_a_colour_come_over_with_each_node() {
        let mut scene = Scene::new();
        let id = anchor(&mut scene);
        let hull = tank(&scene, id);

        let [red, green, blue] = scene.color(hull).unwrap().to_array();
        assert!((red - 0.6).abs() < 1e-4, "{red}");
        assert!((green - 0.2).abs() < 1e-4, "{green}");
        assert!((blue - 0.1).abs() < 1e-4, "{blue}");
        assert_eq!(scene.opacity(hull).unwrap(), MAX_ALPHA);

        // A node carries one mesh, so a second surface is hung beside its children under
        // its own name.
        let kids = scene.children(hull).unwrap();
        assert_eq!(scene.name(kids[0]).unwrap(), "Tank.1");
        assert_eq!(scene.name(kids[1]).unwrap(), "Turret");
        let extra = kids[0];
        assert_eq!(scene.transform(extra).unwrap().position, Vec3::ZERO);
        assert_eq!(scene.transform(extra).unwrap().scale, Vec3::ONE);
        assert!((scene.color(extra).unwrap().z - 0.5).abs() < 1e-4);
        assert!((scene.opacity(extra).unwrap() - 0.6 * MAX_ALPHA).abs() < 1e-4);
    }

    #[test]
    fn a_hung_part_keeps_the_pictures_the_file_laid_out() {
        let mut scene = Scene::new();
        let id = anchor(&mut scene);
        let hull = tank(&scene, id);
        let turret = scene.descendant_named(id, "Turret").unwrap().unwrap();

        // A file's UVs are already the layout the modelling program chose, so a hung
        // part arrives wearing the whole picture across each face rather than one
        // tile per metre.
        assert_eq!(scene.tiling(hull).unwrap(), 0.0);
        assert_eq!(scene.tiling(turret).unwrap(), 0.0);

        // The anchor is Carbine's own node, so it keeps the default metre, and so does
        // a body built by hand.
        assert_eq!(scene.tiling(id).unwrap(), 1.0);
        let brick = scene.create("cube", None, None).unwrap();
        assert_eq!(scene.tiling(brick).unwrap(), 1.0);

        // And a script can still measure out any part it can reach.
        scene.set_tiling(turret, 2.0).unwrap();
        assert_eq!(scene.tiling(turret).unwrap(), 2.0);
        assert_eq!(scene.tiling(hull).unwrap(), 0.0);
    }

    #[test]
    fn the_anchor_measures_the_whole_model() {
        let mut scene = Scene::new();
        let id = anchor(&mut scene);

        // Hull spans a metre each way at half a metre up; the turret adds the last half metre.
        assert_eq!(
            scene.model_bounds(id).unwrap(),
            (Vec3::new(-1.0, -1.0, 0.5), Vec3::new(1.0, 1.0, 1.5))
        );
        assert_eq!(scene.natural_extent(id).unwrap(), Vec3::new(2.0, 2.0, 1.0));

        // What the anchor is scaled to stays out of it: size is the model at scale one.
        scene
            .set_vec_field(id, VecField::Scale, Vec3::new(2.0, 2.0, 2.0))
            .unwrap();
        assert_eq!(scene.natural_extent(id).unwrap(), Vec3::new(2.0, 2.0, 1.0));
        scene
            .set_vec_field(id, VecField::Size, Vec3::new(4.0, 4.0, 2.0))
            .unwrap();
        assert_eq!(scene.natural_extent(id).unwrap(), Vec3::new(2.0, 2.0, 1.0));
    }

    #[test]
    fn a_box_collider_on_an_import_is_the_brick_round_the_whole_model() {
        let mut scene = Scene::new();
        let id = anchor(&mut scene);

        scene.set_collision_type(id, CollisionType::Box).unwrap();
        let body = scene.collider(id);
        assert_eq!(body.len(), 1, "a box is one piece");
        assert_eq!(
            body[0].aabb(),
            (Vec3::new(-1.0, -1.0, 0.5), Vec3::new(1.0, 1.0, 1.5))
        );
    }

    #[test]
    fn a_mesh_collider_on_an_import_is_one_piece_per_surface() {
        let mut scene = Scene::new();
        let id = anchor(&mut scene);

        // Every surface the import hung, each its own solid: the hull, its skirt and
        // the turret. Twelve corners between them, four each.
        scene.set_collision_type(id, CollisionType::Mesh).unwrap();
        let body = scene.collider(id);
        assert_eq!(body.len(), 3);
        assert_eq!(
            body.iter().map(Solid::point_count).sum::<usize>(),
            12,
            "every surface, not just the one the anchor wears itself"
        );

        // And the metre of air between the deck and the turret is empty, which is the
        // whole reason a mesh is offered: no piece of the body crosses it.
        let mut floors: Vec<(f32, f32)> = body
            .iter()
            .map(|piece| {
                let (min, max) = piece.aabb();
                (min.z, max.z)
            })
            .collect();
        floors.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(
            floors,
            vec![(0.5, 0.5), (0.5, 0.5), (1.5, 1.5)],
            "three separate solids, not one brick around them"
        );
    }

    #[test]
    fn a_neighbour_can_be_put_in_the_room_inside_a_mesh_body() {
        let mut scene = Scene::new();
        let id = anchor(&mut scene);
        scene.set_collision_type(id, CollisionType::Mesh).unwrap();
        scene.set_anchored(id, true).unwrap();

        // A crate stood in the metre of air between the tank's deck and its turret,
        // and left to gravity so the solver is free to move it.
        let crate_ = scene.create("cube", Some("Crate".into()), None).unwrap();
        scene
            .set_vec_field(crate_, VecField::Size, Vec3::splat(0.2))
            .unwrap();
        scene
            .set_vec_field(crate_, VecField::Position, Vec3::new(0.0, 0.0, 1.0))
            .unwrap();
        scene.set_collision(crate_, true).unwrap();
        scene.set_uses_gravity(crate_, true).unwrap();

        scene.step(1.0 / 60.0);
        let at = scene.vec_field(crate_, VecField::Position).unwrap();
        assert!(
            (at - Vec3::new(0.0, 0.0, 1.0)).length() < 1e-2,
            "nothing of the tank is in its way: {at}"
        );

        // The same place cut as a box is inside the brick around the whole model, and
        // the solver spends the step pulling the crate back out of it.
        scene.set_collision_type(id, CollisionType::Box).unwrap();
        for _ in 0..3 {
            scene.step(1.0 / 60.0);
        }
        let moved = scene.vec_field(crate_, VecField::Position).unwrap();
        assert!(
            (moved.z - 1.0).abs() > 0.1,
            "the box would not let the crate stand there: {moved}"
        );
    }

    #[test]
    fn a_part_lifted_off_its_hull_is_cut_from_where_it_hangs_now() {
        let mut scene = Scene::new();
        let id = anchor(&mut scene);
        scene.set_collision_type(id, CollisionType::Mesh).unwrap();
        scene.set_anchored(id, true).unwrap();

        // The turret's cut stands a metre and a half up, and a step is spent with it
        // there - which is where the answer below is kept from.
        assert!(
            (roof(&scene, id) - 1.5).abs() < 1e-5,
            "the turret is tallest"
        );
        scene.step(1.0 / 60.0);
        assert!((roof(&scene, id) - 1.5).abs() < 1e-5, "still where it hung");

        // A script lifts the part clear of the deck. The body is cut again from where
        // the part now hangs, rather than going on answering for where it stood.
        let turret = scene.descendant_named(id, "Turret").unwrap().unwrap();
        scene
            .set_vec_field(turret, VecField::Position, Vec3::new(0.0, 0.25, 2.5))
            .unwrap();
        scene.step(1.0 / 60.0);

        let lifted = roof(&scene, id);
        assert!(
            (lifted - 3.0).abs() < 1e-5,
            "the lifted turret should travel with the cut: {lifted}"
        );

        // And a step in which nothing at all moved keeps answering for the same body,
        // which is the whole reason the cut is kept.
        scene.step(1.0 / 60.0);
        assert!((roof(&scene, id) - 3.0).abs() < 1e-5);
    }

    #[test]
    fn a_different_file_on_the_same_anchor_is_cut_from_scratch() {
        let mut scene = Scene::new();
        let id = anchor(&mut scene);
        scene.set_collision_type(id, CollisionType::Mesh).unwrap();
        scene.set_anchored(id, true).unwrap();

        // The hangar is stepped once, so a three-piece answer is kept for this anchor.
        scene.step(1.0 / 60.0);
        assert_eq!(scene.collider(id).len(), 3);

        // A smaller file takes its place on the very same node. The kept cut names the
        // parts it was made of, so it is refused and the new branch is cut instead.
        scene
            .set_model(id, Some(format!("{}#Turret", hangar())))
            .unwrap();
        scene.step(1.0 / 60.0);

        let body = scene.collider(id);
        assert_eq!(body.len(), 1, "only the turret is hung now");
        assert!(
            (roof(&scene, id) - 1.0).abs() < 1e-5,
            "the turret where the file leaves it, not the hull's roofline kept alive: {}",
            roof(&scene, id)
        );
    }

    #[test]
    fn a_named_branch_of_a_hangar_file_is_the_only_thing_hung() {
        let mut scene = Scene::new();
        let id = scene
            .create("model", Some("Turret".to_owned()), None)
            .unwrap();
        scene
            .set_model(id, Some(format!("{}#Turret", hangar())))
            .unwrap();

        let hull = tank(&scene, id);
        assert_eq!(scene.name(hull).unwrap(), "Turret");
        assert_eq!(scene.children(id).unwrap().len(), 1);
        assert!(scene.descendant_named(id, "Tank").unwrap().is_none());
        assert_eq!(
            scene.model(id).unwrap(),
            Some(format!("{}#Turret", hangar()))
        );

        // And the rest of the hangar is still reachable by asking for it.
        scene.set_model(id, Some(hangar())).unwrap();
        assert_eq!(scene.children(id).unwrap().len(), 1);
        assert_eq!(scene.name(tank(&scene, id)).unwrap(), "Tank");
    }

    #[test]
    fn taking_a_model_off_leaves_what_the_script_wrote() {
        let mut scene = Scene::new();
        let id = anchor(&mut scene);
        let box_ = scene
            .create("cube", Some("Crate".to_owned()), Some(id))
            .unwrap();
        assert_eq!(scene.node_count(), 5);

        scene.set_model(id, None).unwrap();

        assert_eq!(scene.model(id).unwrap(), None);
        assert_eq!(scene.children(id).unwrap(), vec![box_]);
        assert_eq!(scene.node_count(), 2);
        assert_eq!(scene.natural_extent(id).unwrap(), Vec3::ONE);
        assert!(scene.collider(id).is_empty());

        // The anchor itself survives, standing where it was.
        assert_eq!(scene.name(id).unwrap(), "Import");
        assert_eq!(scene.kind(id).unwrap(), KIND);
    }

    #[test]
    fn a_file_that_is_not_a_glb_is_refused_with_the_reason() {
        let mut scene = Scene::new();
        let id = scene.create("model", None, None).unwrap();

        let fbx = "it is not really a model.fbx";
        let why = scene.set_model(id, Some(fbx.to_owned())).unwrap_err();
        assert!(
            why.to_string().contains("export the model as glTF"),
            "{why}"
        );

        let missing = std::env::temp_dir().join("carbine-model-nowhere.glb");
        let why = scene
            .set_model(id, Some(missing.to_string_lossy().into_owned()))
            .unwrap_err();
        assert!(why.to_string().contains("no file"), "{why}");

        let not_glb = std::env::temp_dir().join("carbine-model-notaglfb.glb");
        std::fs::write(&not_glb, b"this is a wavefront obj pretending").unwrap();
        let why = scene
            .set_model(id, Some(not_glb.to_string_lossy().into_owned()))
            .unwrap_err();
        assert!(why.to_string().contains("not a GLB"), "{why}");

        assert!(
            scene.children(id).unwrap().is_empty(),
            "a refused import hangs nothing"
        );
    }

    #[test]
    fn a_copy_of_an_import_carries_its_branch() {
        let mut scene = Scene::new();
        let id = anchor(&mut scene);
        let copy = *scene.duplicate(id, 1).unwrap().first().unwrap();

        assert_ne!(copy, id);
        assert_eq!(scene.model(copy).unwrap(), scene.model(id).unwrap());
        let hull = tank(&scene, copy);
        assert_eq!(scene.name(hull).unwrap(), "Tank");
        assert_eq!(
            scene.natural_extent(copy).unwrap(),
            Vec3::new(2.0, 2.0, 1.0)
        );
        assert_eq!(scene.node_count(), 8);

        // Taking the model off one copy leaves the other standing.
        scene.set_model(copy, None).unwrap();
        assert_eq!(scene.children(id).unwrap().len(), 1);
        assert!(scene.children(copy).unwrap().is_empty());
    }

    #[test]
    fn a_plain_node_never_reads_as_a_model() {
        let mut scene = Scene::new();
        let cube = scene.create("cube", None, None).unwrap();
        let empty = scene.create("model", None, None).unwrap();

        assert!(scene.model_bounds(cube).is_none());
        assert!(scene.model_bounds(empty).is_none());
        assert_eq!(scene.natural_extent(cube).unwrap(), Vec3::ONE);
        assert_eq!(scene.natural_extent(empty).unwrap(), Vec3::ONE);
        scene.set_collision(cube, true).unwrap();
        assert_eq!(scene.collider(cube)[0].point_count(), 8);
    }

    #[test]
    fn a_model_under_a_shape_measures_as_the_pair() {
        let mut scene = Scene::new();
        let cube = scene.create("cube", Some("Both".to_owned()), None).unwrap();
        scene.set_model(cube, Some(hangar())).unwrap();

        // The node's own unit body and the branch under it, together.
        assert_eq!(
            scene.model_bounds(cube).unwrap(),
            (Vec3::new(-1.0, -1.0, -0.5), Vec3::new(1.0, 1.0, 1.5))
        );
        assert_eq!(
            scene.natural_extent(cube).unwrap(),
            Vec3::new(2.0, 2.0, 2.0)
        );
    }

    #[test]
    fn a_turned_surface_is_measured_from_the_corners_it_reaches() {
        let model = carbine_glb::Model::new(
            vec![carbine_glb::Node {
                name: Some("Ramp".to_owned()),
                transform: carbine_glb::Transform {
                    rotation: glam::Quat::from_rotation_z(45.0_f32.to_radians()),
                    ..carbine_glb::Transform::IDENTITY
                },
                children: Vec::new(),
                parts: vec![sheet(1.0, [0.6, 0.2, 0.1, 1.0])],
            }],
            vec![0],
        );
        let path = std::env::temp_dir().join("carbine-model-ramp.glb");
        std::fs::write(&path, carbine_glb::write(&model)).unwrap();

        let mut scene = Scene::new();
        let id = scene
            .create("model", Some("Ramp".to_owned()), None)
            .unwrap();
        scene
            .set_model(id, Some(path.to_string_lossy().into_owned()))
            .unwrap();

        // A square two metres each way, turned an eighth turn: its corners reach root-two
        // out along both axes, which the two corners a box names would say are a metre.
        let reach = std::f32::consts::SQRT_2;
        let (min, max) = scene.model_bounds(id).unwrap();
        for (found, want) in [
            (min.x, -reach),
            (min.y, -reach),
            (max.x, reach),
            (max.y, reach),
        ] {
            assert!((found - want).abs() < 1e-5, "{min:?} -> {max:?}");
        }
        let extent = scene.natural_extent(id).unwrap();
        assert!((extent.x - 2.0 * reach).abs() < 1e-4, "{extent:?}");
        assert!((extent.y - 2.0 * reach).abs() < 1e-4, "{extent:?}");
        assert_eq!(
            extent.z, 1.0,
            "an axis the sheet does not reach along stays a metre"
        );
    }

    #[test]
    fn a_child_the_script_wrote_is_not_part_of_the_measure() {
        let mut scene = Scene::new();
        let id = anchor(&mut scene);
        let far = scene
            .create("cube", Some("Far".to_owned()), Some(tank(&scene, id)))
            .unwrap();
        scene
            .set_vec_field(far, VecField::Position, Vec3::new(0.0, 0.0, 40.0))
            .unwrap();

        let bounds = scene.model_bounds(id).unwrap();
        assert_eq!(
            bounds.1.z, 1.5,
            "the crate is the script's own, not the model's"
        );
    }

    /// A file that stands its geometry below the pivot, the way a real vehicle whose
    /// wheels touch the road does.
    fn sunk() -> String {
        let model = carbine_glb::Model::new(
            vec![carbine_glb::Node {
                name: Some("Skid plate".to_owned()),
                transform: place(0.0, 0.0, -0.5),
                children: Vec::new(),
                parts: vec![sheet(1.0, [0.4, 0.4, 0.4, 1.0])],
            }],
            vec![0],
        );

        let path = std::env::temp_dir().join("carbine-model-sunk.glb");
        std::fs::write(&path, carbine_glb::write(&model)).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn a_loaded_model_comes_to_rest_on_its_lowest_point_and_not_on_its_pivot() {
        let mut scene = Scene::new();
        let id = scene
            .create("model", Some("Sunk".to_owned()), None)
            .unwrap();
        scene.set_model(id, Some(sunk())).unwrap();
        scene.set_collision(id, true).unwrap();
        scene.set_uses_gravity(id, true).unwrap();
        scene
            .set_vec_field(id, VecField::Position, Vec3::new(0.0, 0.0, 4.0))
            .unwrap();

        // The branch hangs half a metre under the pivot, so a pivot on the floor is a
        // body buried to its axle in it.
        assert_eq!(scene.model_bounds(id).unwrap().0.z, -0.5);

        for _ in 0..300 {
            scene.step(1.0 / 60.0);
        }

        let position = scene.vec_field(id, VecField::Position).unwrap();
        assert!(
            (position.z - 0.5).abs() < 1e-3,
            "the pivot stops half a metre up, where the file left it: {position:?}"
        );
        let floor = scene.collider(id)[0].aabb().0.z;
        assert!(
            (floor - GROUND_HEIGHT).abs() < 1e-3,
            "and the collider rests on the floor with it: {floor}"
        );
    }
}
