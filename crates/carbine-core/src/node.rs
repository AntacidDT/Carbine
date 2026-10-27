use std::collections::HashMap;
use std::fmt;

use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::*;
use glam::{EulerRot, Quat, Vec3};

use crate::camera::Camera;
use crate::mesh::{MeshData, MeshHandle};
use crate::motion::Motion;
use crate::primitives;

/// Carbine orange (sRGB #eb7a34), the default colour of a new object.
pub const DEFAULT_COLOR: Vec3 = Vec3::new(0.921, 0.478, 0.204);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NodeId(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    X,
    Y,
    Z,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VecField {
    Position,
    Scale,
    Size,
    RotationEuler,
    Color,
}

#[derive(Clone, Copy, Debug, Component)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }
}

#[derive(Component)]
pub(crate) struct NodeName(pub(crate) String);

#[derive(Component)]
pub(crate) struct NodeKind(pub(crate) String);

#[derive(Component)]
pub(crate) struct NodeMesh {
    pub(crate) handle: MeshHandle,
    pub(crate) data: MeshData,
}

#[derive(Component)]
pub(crate) struct ObjectColor(pub(crate) Vec3);

#[derive(Component)]
pub(crate) struct Parent(pub(crate) NodeId);

#[derive(Component, Default)]
pub(crate) struct Children(pub(crate) Vec<NodeId>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneError {
    NodeDestroyed,
    UnknownKind(String),
    CannotDestroyRoot,
    CannotReparentRoot,
    Cycle,
    MissingComponent,
    NotAnEntity,
    NotAUi,
    BadColor(String),
}

impl fmt::Display for SceneError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NodeDestroyed => write!(formatter, "the object has been destroyed"),
            Self::UnknownKind(kind) => write!(
                formatter,
                "unknown object kind \"{kind}\" (available: {})",
                primitives::KINDS.join(", ")
            ),
            Self::CannotDestroyRoot => write!(formatter, "the scene root cannot be destroyed"),
            Self::CannotReparentRoot => write!(formatter, "the scene root cannot be reparented"),
            Self::Cycle => write!(formatter, "reparenting would create a cycle"),
            Self::MissingComponent => write!(formatter, "the object is missing a component"),
            Self::NotAnEntity => write!(formatter, "the object is not an entity"),
            Self::NotAUi => write!(formatter, "the object is not a UI element"),
            Self::BadColor(color) => write!(
                formatter,
                "\"{color}\" is not a colour; use #rgb, #rgba, #rrggbb or #rrggbbaa"
            ),
        }
    }
}

impl std::error::Error for SceneError {}

pub struct Scene {
    pub(crate) world: World,
    pub(crate) ids: HashMap<NodeId, Entity>,
    pub(crate) next_id: u64,
    pub(crate) next_mesh: u64,
    pub(crate) camera: Camera,
    root: NodeId,
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene {
    pub fn new() -> Self {
        let mut world = World::new();
        let root = NodeId(0);
        let root_entity = world
            .spawn((
                NodeName("Scene".to_owned()),
                NodeKind("root".to_owned()),
                Transform::default(),
                Children::default(),
            ))
            .id();

        let mut ids = HashMap::new();
        ids.insert(root, root_entity);

        Self {
            world,
            ids,
            next_id: 1,
            next_mesh: 1,
            camera: Camera::default(),
            root,
        }
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn node_count(&self) -> usize {
        self.ids.len().saturating_sub(1)
    }

    pub fn create(
        &mut self,
        kind: &str,
        name: Option<String>,
        parent: Option<NodeId>,
    ) -> Result<NodeId, SceneError> {
        let data =
            primitives::build(kind).ok_or_else(|| SceneError::UnknownKind(kind.to_owned()))?;
        let parent_id = parent.unwrap_or(self.root);
        self.entity(parent_id)?;

        let id = self.reserve();
        let handle = self.next_mesh_handle();
        let entity = self
            .world
            .spawn((
                NodeName(name.unwrap_or_else(|| primitives::default_name(kind))),
                NodeKind(kind.to_owned()),
                Transform::default(),
                NodeMesh { handle, data },
                ObjectColor(DEFAULT_COLOR),
                Motion::default(),
                Parent(parent_id),
                Children::default(),
            ))
            .id();
        self.ids.insert(id, entity);
        self.attach(id, parent_id);

        Ok(id)
    }

    pub fn destroy(&mut self, id: NodeId) -> Result<(), SceneError> {
        if id == self.root {
            return Err(SceneError::CannotDestroyRoot);
        }
        self.entity(id)?;

        let mut pending = vec![id];
        let mut subtree = Vec::new();
        while let Some(current) = pending.pop() {
            subtree.push(current);
            pending.extend(self.children(current)?);
        }

        let parent_entity = self.parent(id)?.and_then(|parent| self.entity(parent).ok());
        if let Some(parent_entity) = parent_entity
            && let Some(mut children) = self.world.get_mut::<Children>(parent_entity)
        {
            children.0.retain(|child| *child != id);
        }

        for node in subtree {
            if let Some(entity) = self.ids.remove(&node) {
                self.world.entity_mut(entity).despawn();
            }
        }

        Ok(())
    }

    pub fn reparent(&mut self, id: NodeId, new_parent: Option<NodeId>) -> Result<(), SceneError> {
        if id == self.root {
            return Err(SceneError::CannotReparentRoot);
        }
        let new_parent = new_parent.unwrap_or(self.root);
        self.entity(new_parent)?;
        if self.is_ancestor(id, new_parent)? {
            return Err(SceneError::Cycle);
        }

        let old_parent = self.parent(id)?.ok_or(SceneError::CannotReparentRoot)?;
        if old_parent == new_parent {
            return Ok(());
        }

        let old_parent_entity = self.entity(old_parent)?;
        let new_parent_entity = self.entity(new_parent)?;
        let entity = self.entity(id)?;

        if let Some(mut children) = self.world.get_mut::<Children>(old_parent_entity) {
            children.0.retain(|child| *child != id);
        }
        if let Some(mut children) = self.world.get_mut::<Children>(new_parent_entity) {
            children.0.push(id);
        }
        if let Some(mut parent) = self.world.get_mut::<Parent>(entity) {
            parent.0 = new_parent;
        }

        Ok(())
    }

    pub fn name(&self, id: NodeId) -> Result<&str, SceneError> {
        Ok(&self.component::<NodeName>(id)?.0)
    }

    pub fn set_name(&mut self, id: NodeId, name: String) -> Result<(), SceneError> {
        self.component_mut::<NodeName>(id)?.0 = name;
        Ok(())
    }

    pub fn kind(&self, id: NodeId) -> Result<&str, SceneError> {
        Ok(&self.component::<NodeKind>(id)?.0)
    }

    pub fn parent(&self, id: NodeId) -> Result<Option<NodeId>, SceneError> {
        let entity = self.entity(id)?;
        Ok(self.world.get::<Parent>(entity).map(|parent| parent.0))
    }

    pub fn children(&self, id: NodeId) -> Result<Vec<NodeId>, SceneError> {
        Ok(self.component::<Children>(id)?.0.clone())
    }

    pub fn transform(&self, id: NodeId) -> Result<Transform, SceneError> {
        Ok(*self.component::<Transform>(id)?)
    }

    pub fn rotation(&self, id: NodeId) -> Result<Quat, SceneError> {
        Ok(self.transform(id)?.rotation)
    }

    pub fn set_rotation(&mut self, id: NodeId, rotation: Quat) -> Result<(), SceneError> {
        let rotation = if rotation.length_squared() > f32::EPSILON {
            rotation.normalize()
        } else {
            Quat::IDENTITY
        };
        self.component_mut::<Transform>(id)?.rotation = rotation;
        Ok(())
    }

    pub fn vec_field(&self, id: NodeId, field: VecField) -> Result<Vec3, SceneError> {
        if matches!(field, VecField::Color) {
            return Ok(self.component::<ObjectColor>(id)?.0);
        }

        let transform = self.transform(id)?;
        Ok(match field {
            VecField::Position => transform.position,
            VecField::Scale => transform.scale,
            VecField::Size => transform.scale * self.natural_extent(id)?,
            VecField::RotationEuler => euler_degrees(transform.rotation),
            VecField::Color => unreachable!("colour is handled above"),
        })
    }

    pub fn set_vec_field(
        &mut self,
        id: NodeId,
        field: VecField,
        value: Vec3,
    ) -> Result<(), SceneError> {
        if matches!(field, VecField::Color) {
            self.component_mut::<ObjectColor>(id)?.0 = value;
            return Ok(());
        }

        let extent = self.natural_extent(id)?;
        let mut transform = self.component_mut::<Transform>(id)?;
        match field {
            VecField::Position => transform.position = value,
            VecField::Scale => transform.scale = value,
            VecField::Size => transform.scale = value / extent,
            VecField::RotationEuler => transform.rotation = quat_from_euler_degrees(value),
            VecField::Color => unreachable!("colour is handled above"),
        }
        Ok(())
    }

    pub fn set_vec_field_axis(
        &mut self,
        id: NodeId,
        field: VecField,
        axis: Axis,
        value: f32,
    ) -> Result<(), SceneError> {
        if matches!(field, VecField::Color) {
            let mut color = self.component::<ObjectColor>(id)?.0;
            set_axis(&mut color, axis, value);
            self.component_mut::<ObjectColor>(id)?.0 = color;
            return Ok(());
        }

        let extent = self.natural_extent(id)?;
        let mut transform = *self.component::<Transform>(id)?;
        match field {
            VecField::Position => set_axis(&mut transform.position, axis, value),
            VecField::Scale => set_axis(&mut transform.scale, axis, value),
            VecField::Size => {
                let mut size = transform.scale * extent;
                set_axis(&mut size, axis, value);
                transform.scale = size / extent;
            }
            VecField::RotationEuler => {
                let mut euler = euler_degrees(transform.rotation);
                set_axis(&mut euler, axis, value);
                transform.rotation = quat_from_euler_degrees(euler);
            }
            VecField::Color => unreachable!("colour is handled above"),
        }
        *self.component_mut::<Transform>(id)? = transform;
        Ok(())
    }

    pub fn color(&self, id: NodeId) -> Result<Vec3, SceneError> {
        Ok(self.component::<ObjectColor>(id)?.0)
    }

    /// The colour the way a design tool writes it: `#rrggbb`, lowercase.
    pub fn color_hex(&self, id: NodeId) -> Result<String, SceneError> {
        let color = self.color(id)?;
        Ok(format!(
            "#{:02x}{:02x}{:02x}",
            channel(color.x),
            channel(color.y),
            channel(color.z)
        ))
    }

    /// Accepts `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`. A trailing alpha only
    /// means anything on a UI element, and is dropped elsewhere.
    pub fn set_color_hex(&mut self, id: NodeId, hex: &str) -> Result<(), SceneError> {
        let (color, alpha) =
            parse_hex_color(hex).ok_or_else(|| SceneError::BadColor(hex.to_owned()))?;
        self.component_mut::<ObjectColor>(id)?.0 = color;
        if let Some(alpha) = alpha
            && self.is_ui(id)?
        {
            self.set_alpha(id, alpha)?;
        }
        Ok(())
    }

    pub fn mesh(&self, id: NodeId) -> Result<MeshData, SceneError> {
        let mesh = self.component::<NodeMesh>(id)?;
        Ok(mesh.data.clone())
    }

    /// The mesh's dimensions in meters at scale one, or one on axes the mesh
    /// does not extend along.
    pub fn natural_extent(&self, id: NodeId) -> Result<Vec3, SceneError> {
        let entity = self.entity(id)?;
        Ok(self
            .world
            .get::<NodeMesh>(entity)
            .map(|mesh| mesh.data.natural_extent())
            .unwrap_or(Vec3::ONE))
    }

    pub(crate) fn reserve(&mut self) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        id
    }

    pub(crate) fn next_mesh_handle(&mut self) -> MeshHandle {
        let handle = MeshHandle(self.next_mesh);
        self.next_mesh += 1;
        handle
    }

    pub(crate) fn attach(&mut self, id: NodeId, parent: NodeId) {
        if let Some(parent_entity) = self.entity(parent).ok()
            && let Some(mut children) = self.world.get_mut::<Children>(parent_entity)
        {
            children.0.push(id);
        }
    }

    pub(crate) fn insert(&mut self, id: NodeId, entity: Entity) {
        self.ids.insert(id, entity);
    }

    pub(crate) fn entity(&self, id: NodeId) -> Result<Entity, SceneError> {
        self.ids.get(&id).copied().ok_or(SceneError::NodeDestroyed)
    }

    pub(crate) fn component<T: Component>(&self, id: NodeId) -> Result<&T, SceneError> {
        let entity = self.entity(id)?;
        self.world
            .get::<T>(entity)
            .ok_or(SceneError::MissingComponent)
    }

    pub(crate) fn component_mut<T: Component<Mutability = Mutable>>(
        &mut self,
        id: NodeId,
    ) -> Result<Mut<'_, T>, SceneError> {
        let entity = self.entity(id)?;
        self.world
            .get_mut::<T>(entity)
            .ok_or(SceneError::MissingComponent)
    }

    fn is_ancestor(&self, ancestor: NodeId, node: NodeId) -> Result<bool, SceneError> {
        let mut current = Some(node);
        while let Some(current_id) = current {
            if current_id == ancestor {
                return Ok(true);
            }
            current = self.parent(current_id)?;
        }
        Ok(false)
    }
}

fn channel(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn parse_hex_color(hex: &str) -> Option<(Vec3, Option<f32>)> {
    let digits = hex.strip_prefix('#').unwrap_or(hex);
    let pairs: Vec<Option<u32>> = match digits.len() {
        3 | 4 => digits
            .chars()
            .map(|d| d.to_digit(16).map(|v| v * 17))
            .collect(),
        6 | 8 => digits
            .as_bytes()
            .chunks(2)
            .map(|pair| {
                std::str::from_utf8(pair)
                    .ok()
                    .and_then(|text| u32::from_str_radix(text, 16).ok())
            })
            .collect(),
        _ => return None,
    };
    let channels = pairs.into_iter().collect::<Option<Vec<u32>>>()?;
    let color = Vec3::new(
        channels[0] as f32 / 255.0,
        channels[1] as f32 / 255.0,
        channels[2] as f32 / 255.0,
    );
    Some((color, channels.get(3).map(|&v| v as f32)))
}

fn set_axis(vector: &mut Vec3, axis: Axis, value: f32) {
    match axis {
        Axis::X => vector.x = value,
        Axis::Y => vector.y = value,
        Axis::Z => vector.z = value,
    }
}

fn euler_degrees(rotation: Quat) -> Vec3 {
    let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
    Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
}

fn quat_from_euler_degrees(euler: Vec3) -> Quat {
    Quat::from_euler(
        EulerRot::XYZ,
        euler.x.to_radians(),
        euler.y.to_radians(),
        euler.z.to_radians(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_scene_has_only_a_root() {
        let scene = Scene::new();
        assert_eq!(scene.node_count(), 0);
        assert_eq!(scene.name(scene.root()).unwrap(), "Scene");
        assert_eq!(scene.parent(scene.root()).unwrap(), None);
    }

    #[test]
    fn creating_a_rectangle_attaches_it_to_the_root() {
        let mut scene = Scene::new();
        let node = scene.create("rectangle", None, None).unwrap();

        assert_eq!(scene.node_count(), 1);
        assert_eq!(scene.name(node).unwrap(), "Rectangle");
        assert_eq!(scene.kind(node).unwrap(), "rectangle");
        assert_eq!(scene.parent(node).unwrap(), Some(scene.root()));
        assert_eq!(scene.children(scene.root()).unwrap(), vec![node]);
        assert_eq!(scene.mesh(node).unwrap().vertex_count(), 4);
        assert_eq!(scene.color(node).unwrap(), DEFAULT_COLOR);
    }

    #[test]
    fn transform_fields_write_through() {
        let mut scene = Scene::new();
        let node = scene.create("rectangle", None, None).unwrap();

        scene
            .set_vec_field(node, VecField::Position, Vec3::new(1.0, 2.0, 3.0))
            .unwrap();
        assert_eq!(
            scene.vec_field(node, VecField::Position).unwrap(),
            Vec3::new(1.0, 2.0, 3.0)
        );

        scene
            .set_vec_field_axis(node, VecField::Scale, Axis::Y, 4.0)
            .unwrap();
        assert_eq!(
            scene.vec_field(node, VecField::Scale).unwrap(),
            Vec3::new(1.0, 4.0, 1.0)
        );

        scene
            .set_vec_field_axis(node, VecField::RotationEuler, Axis::Z, 90.0)
            .unwrap();
        let euler = scene.vec_field(node, VecField::RotationEuler).unwrap();
        assert!((euler.z - 90.0).abs() < 1e-4);
    }

    #[test]
    fn size_is_meters_and_colour_is_write_through() {
        let mut scene = Scene::new();
        let node = scene.create("rectangle", None, None).unwrap();

        scene
            .set_vec_field(node, VecField::Size, Vec3::new(2.0, 3.0, 1.0))
            .unwrap();
        assert_eq!(
            scene.vec_field(node, VecField::Scale).unwrap(),
            Vec3::new(2.0, 3.0, 1.0)
        );
        assert_eq!(
            scene.vec_field(node, VecField::Size).unwrap(),
            Vec3::new(2.0, 3.0, 1.0)
        );

        scene
            .set_vec_field_axis(node, VecField::Size, Axis::X, 5.0)
            .unwrap();
        assert_eq!(
            scene.vec_field(node, VecField::Size).unwrap(),
            Vec3::new(5.0, 3.0, 1.0)
        );

        scene
            .set_vec_field_axis(node, VecField::Color, Axis::X, 0.0)
            .unwrap();
        assert_eq!(
            scene.vec_field(node, VecField::Color).unwrap(),
            Vec3::new(0.0, DEFAULT_COLOR.y, DEFAULT_COLOR.z)
        );
    }

    #[test]
    fn reparenting_moves_the_node_and_rejects_cycles() {
        let mut scene = Scene::new();
        let parent = scene.create("rectangle", None, None).unwrap();
        let child = scene.create("rectangle", None, Some(parent)).unwrap();

        assert_eq!(scene.children(parent).unwrap(), vec![child]);
        scene.reparent(child, None).unwrap();
        assert_eq!(scene.children(parent).unwrap(), Vec::<NodeId>::new());
        assert_eq!(scene.parent(child).unwrap(), Some(scene.root()));

        scene.reparent(child, Some(parent)).unwrap();
        assert!(matches!(
            scene.reparent(parent, Some(child)),
            Err(SceneError::Cycle)
        ));
    }

    #[test]
    fn destroying_a_node_destroys_its_subtree() {
        let mut scene = Scene::new();
        let parent = scene.create("rectangle", None, None).unwrap();
        let child = scene.create("rectangle", None, Some(parent)).unwrap();

        scene.destroy(parent).unwrap();
        assert_eq!(scene.node_count(), 0);
        assert!(matches!(scene.name(parent), Err(SceneError::NodeDestroyed)));
        assert!(matches!(scene.name(child), Err(SceneError::NodeDestroyed)));
    }

    #[test]
    fn the_root_is_protected() {
        let mut scene = Scene::new();
        assert!(matches!(
            scene.destroy(scene.root()),
            Err(SceneError::CannotDestroyRoot)
        ));
        assert!(matches!(
            scene.reparent(scene.root(), None),
            Err(SceneError::CannotReparentRoot)
        ));
    }

    #[test]
    fn unknown_kinds_are_rejected() {
        let mut scene = Scene::new();
        assert!(matches!(
            scene.create("dodecahedron", None, None),
            Err(SceneError::UnknownKind(kind)) if kind == "dodecahedron"
        ));
    }

    #[test]
    fn colours_are_written_as_hex_and_read_back() {
        let mut scene = Scene::new();
        let card = scene.create("rectangle", None, None).unwrap();

        scene.set_color_hex(card, "#eb7a34").unwrap();
        assert_eq!(scene.color_hex(card).unwrap(), "#eb7a34");
        assert_eq!(
            scene.color(card).unwrap(),
            Vec3::new(235.0 / 255.0, 122.0 / 255.0, 52.0 / 255.0)
        );

        scene.set_color_hex(card, "f00").unwrap();
        assert_eq!(scene.color(card).unwrap(), Vec3::new(1.0, 0.0, 0.0));

        scene.set_color_hex(card, "#0f0f0fff").unwrap();
        assert_eq!(scene.color_hex(card).unwrap(), "#0f0f0f");
    }

    #[test]
    fn bad_colours_are_rejected() {
        let mut scene = Scene::new();
        let card = scene.create("rectangle", None, None).unwrap();

        for bad in ["#ee7a3", "orange", "#gggggg", ""] {
            assert!(
                matches!(
                    scene.set_color_hex(card, bad),
                    Err(SceneError::BadColor(value)) if value == bad
                ),
                "{bad} should not parse"
            );
        }
    }
}
