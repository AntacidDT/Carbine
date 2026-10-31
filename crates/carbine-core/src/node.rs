use std::collections::HashMap;
use std::fmt;
use std::io::Read;

use bevy_ecs::component::Mutable;
use bevy_ecs::prelude::*;
use glam::{EulerRot, Quat, Vec3};

use crate::camera::Camera;
use crate::mesh::{MeshData, MeshHandle};
use crate::motion::Motion;
use crate::primitives;

/// Carbine orange (sRGB #eb7a34), the default colour of a new object.
pub const DEFAULT_COLOR: Vec3 = Vec3::new(0.921, 0.478, 0.204);

/// Alpha is an eight bit count: 255 is opaque and 0 is invisible.
pub const MAX_ALPHA: f32 = 255.0;

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

/// How transparent the node is, in bytes: 255 opaque down to 0 invisible. The
/// world and the UI overlay both read it, so `transparent` means the same thing
/// on a cube as it does on a button.
#[derive(Component)]
pub(crate) struct Opacity(pub(crate) f32);

impl Default for Opacity {
    fn default() -> Self {
        Self(MAX_ALPHA)
    }
}

/// The image a node is skinned with, kept as a path. Decoding the file is the
/// renderer's job; the scene only remembers which image was asked for.
#[derive(Component, Default)]
pub(crate) struct NodeTexture(pub(crate) Option<String>);

/// Whether the node is drawn at all. A first person player hides its own body
/// with this, and a script uses it for anything it wants out of the frame.
#[derive(Component)]
pub(crate) struct Visible(pub(crate) bool);

impl Default for Visible {
    fn default() -> Self {
        Self(true)
    }
}

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
    CannotCopyRoot,
    Cycle,
    CrossedTree,
    NotAChild,
    DuplicateChild,
    MissingComponent,
    NotAnEntity,
    NotAUi,
    NotAParticles,
    UiNotCollidable,
    UnknownShape(String),
    UnknownParticleShape(String),
    BadColor(String),
    UnknownDisaster(String),
    NoSuchDisaster,
    UnknownAtmosphere(String),
    UnknownLightQuality(String),
    BadTime(String),
    BadTexture { path: String, why: String },
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
            Self::CannotCopyRoot => {
                write!(
                    formatter,
                    "the scene root holds the world; it is not a thing to copy"
                )
            }
            Self::Cycle => write!(formatter, "reparenting would create a cycle"),
            Self::CrossedTree => write!(
                formatter,
                "a UI element and a world object cannot share a branch: one is measured in \
                 pixels, the other in metres. Hang them both on the scene root instead."
            ),
            Self::NotAChild => write!(formatter, "that node is not a child of this one"),
            Self::DuplicateChild => {
                write!(formatter, "a node cannot appear in a child list twice")
            }
            Self::MissingComponent => write!(formatter, "the object is missing a component"),
            Self::NotAnEntity => write!(formatter, "the object is not an entity"),
            Self::NotAUi => write!(formatter, "the object is not a UI element"),
            Self::NotAParticles => {
                write!(formatter, "the object is not a particle system")
            }
            Self::UiNotCollidable => {
                write!(
                    formatter,
                    "a UI element is laid out in pixels and never collides"
                )
            }
            Self::UnknownShape(shape) => write!(
                formatter,
                "unknown collider shape \"{shape}\" (available: {})",
                crate::collision::ColliderShape::NAMES.join(", ")
            ),
            Self::UnknownParticleShape(shape) => write!(
                formatter,
                "unknown particle shape \"{shape}\" (available: {})",
                crate::particles::ParticleShape::NAMES.join(", ")
            ),
            Self::BadColor(color) => write!(
                formatter,
                "\"{color}\" is not a colour; use #rgb, #rgba, #rrggbb or #rrggbbaa"
            ),
            Self::UnknownDisaster(kind) => write!(
                formatter,
                "unknown disaster kind \"{kind}\" (available: {})",
                crate::disaster::DisasterKind::NAMES.join(", ")
            ),
            Self::NoSuchDisaster => write!(formatter, "no such disaster"),
            Self::UnknownAtmosphere(kind) => write!(
                formatter,
                "unknown atmosphere kind \"{kind}\" (available: {}; fog is a knob on \
                 `atmosphere`, not something that falls)",
                crate::atmosphere::Precipitation::NAMES.join(", ")
            ),
            Self::UnknownLightQuality(quality) => write!(
                formatter,
                "unknown light quality \"{quality}\" (available: {})",
                crate::atmosphere::LightQuality::NAMES.join(", ")
            ),
            Self::BadTime(clock) => write!(
                formatter,
                "\"{clock}\" is not a time; write \"07:30\" or the hours past midnight"
            ),
            Self::BadTexture { path, why } => write!(
                formatter,
                "\"{path}\" cannot skin an object: {why}. Images come to Carbine as .ktx2, so \
                 pack it first with carbine.packager.image(\"{path}\") and skin the object with \
                 the path that hands back."
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
    /// Which nodes were written as linked copies of the same thing, grouped by the
    /// number each of them carries in its `Link`. A configuration write to one
    /// member lands on all of them; see [`crate::duplicate`].
    pub(crate) links: HashMap<u64, Vec<NodeId>>,
    pub(crate) next_link: u64,
    pub(crate) camera: Camera,
    pub(crate) sky: crate::sky::Sky,
    pub(crate) input: crate::input::Input,
    pub(crate) player: crate::player::PlayerController,
    /// The pairs that rode into each other during the last step. The solver
    /// writes it and the damage pass reads it, so a script can answer a touch.
    pub(crate) contacts: Vec<(NodeId, NodeId)>,
    /// Weather, in the order a script wrote it. A disaster is a rule the solver
    /// asks about every step, not a node, so it has its own little register.
    pub(crate) disasters: Vec<crate::disaster::Disaster>,
    pub(crate) next_disaster: u64,
    /// What the air looks like: how thick the fog is and what clock the world is
    /// lit for. Neither of these is a node, so the scene holds them itself.
    pub(crate) atmosphere: crate::atmosphere::Atmosphere,
    /// The graphics device the renderer was given, as soon as it knows. Until it
    /// says, an `auto` light quality has nothing to be auto about.
    pub(crate) gpu: Option<crate::atmosphere::Gpu>,
    /// How far the view is allowed to sit away from the player's eyes this frame,
    /// written by the disasters and added by the follow camera.
    pub(crate) shake: Vec3,
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
            links: HashMap::new(),
            next_link: 0,
            camera: Camera::default(),
            sky: crate::sky::Sky::default(),
            input: crate::input::Input::default(),
            player: crate::player::PlayerController::default(),
            contacts: Vec::new(),
            disasters: Vec::new(),
            next_disaster: 0,
            atmosphere: crate::atmosphere::Atmosphere::default(),
            gpu: None,
            shake: Vec3::ZERO,
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
        if let Some(parent) = parent {
            self.entity(parent)?;
            self.branch_allows(parent, false)?;
        }

        Ok(self.spawn(kind, name, parent, data))
    }

    /// Attaches a node holding `data` to the tree. Shared by world objects and UI
    /// elements, which differ in the mesh they get rather than in anything else.
    pub(crate) fn spawn(
        &mut self,
        kind: &str,
        name: Option<String>,
        parent: Option<NodeId>,
        data: MeshData,
    ) -> NodeId {
        let parent_id = parent.unwrap_or(self.root);
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
                Opacity::default(),
                NodeTexture::default(),
                Visible::default(),
                Motion::default(),
                Parent(parent_id),
                Children::default(),
            ))
            .id();
        self.ids.insert(id, entity);
        self.attach(id, parent_id);

        id
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

    /// Throws away everything the script authored: every node under the root, with
    /// the subtree hanging off it, and every disaster still written into the world.
    /// The camera, the backdrop, the light and the player are settings a game tuned
    /// rather than things it made, so they stay where they were left; `reset` is
    /// what wipes those too.
    pub fn clear(&mut self) {
        for id in self.children(self.root).unwrap_or_default() {
            let _ = self.destroy(id);
        }
        self.links.clear();
        self.remove_all_disasters();
    }

    /// The scene as it was before anything touched it: no nodes, no weather, the
    /// camera and the player back at their defaults, the input forgotten. What the
    /// renderer reported about the machine it is drawing on is not content and
    /// stays.
    pub fn reset(&mut self) {
        // What the machine under the world is does not change when the world is
        // cleared, so the device reading outlives a reset.
        let gpu = self.gpu;
        *self = Self::new();
        self.gpu = gpu;
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
        let in_ui = self.is_ui(id)?;
        self.branch_allows(new_parent, in_ui)?;

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

    /// Hangs `child` under `parent`, at the end of its children. `parent` keeps
    /// its own transform, so the child starts moving with the branch.
    pub fn add_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), SceneError> {
        self.entity(parent)?;
        self.reparent(child, Some(parent))
    }

    /// Takes `child` off `parent` and hangs it on the scene root, where it stops
    /// moving with the branch it came off. Removing a child never destroys it:
    /// `destroy` is the call that means that.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), SceneError> {
        self.entity(parent)?;
        if !self.children(parent)?.contains(&child) {
            return Err(SceneError::NotAChild);
        }
        self.reparent(child, None)
    }

    /// Replaces a node's child list. The order given is the order the branch is
    /// walked in, which is also the order UI elements are painted, so it is how a
    /// script decides what sits on top.
    ///
    /// Children already attached but left out of the list move to the scene root:
    /// they are detached, never destroyed, and everything under them comes with
    /// them. New children are hung on, so this adopts as well as reorders.
    pub fn set_children(&mut self, parent: NodeId, kids: &[NodeId]) -> Result<(), SceneError> {
        self.entity(parent)?;

        let mut ordered: Vec<NodeId> = Vec::with_capacity(kids.len());
        for kid in kids {
            self.entity(*kid)?;
            if *kid == self.root {
                return Err(SceneError::CannotReparentRoot);
            }
            // The parent itself would close the branch on a loop, and so would any
            // node the parent already hangs under.
            if *kid == parent || self.is_ancestor(*kid, parent)? {
                return Err(SceneError::Cycle);
            }
            if ordered.contains(kid) {
                return Err(SceneError::DuplicateChild);
            }
            // Checked here rather than left to the moves below, so a list naming a
            // world node under a panel is refused before it detaches anything.
            self.branch_allows(parent, self.is_ui(*kid)?)?;
            ordered.push(*kid);
        }

        for old in self.children(parent)? {
            if !ordered.contains(&old) {
                self.reparent(old, None)?;
            }
        }
        for kid in &ordered {
            self.reparent(*kid, Some(parent))?;
        }

        // A child that already hung here answers `reparent` with "no change", which
        // would leave it wherever it was, so the list is written out rather than
        // trusted to the appends above.
        let parent_entity = self.entity(parent)?;
        if let Some(mut children) = self.world.get_mut::<Children>(parent_entity) {
            children.0 = ordered;
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
            return self.set_color(id, value);
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
            return self.set_color(id, color);
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
        Ok(hex_of(self.color(id)?))
    }

    /// Accepts `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`. A trailing alpha sets
    /// the node's transparency, which both the world and the UI overlay honour.
    pub fn set_color_hex(&mut self, id: NodeId, hex: &str) -> Result<(), SceneError> {
        let (color, alpha) =
            parse_hex_color(hex).ok_or_else(|| SceneError::BadColor(hex.to_owned()))?;
        self.set_color(id, color)?;
        if let Some(alpha) = alpha {
            self.set_opacity(id, alpha)?;
        }
        Ok(())
    }

    /// The colour on one node, with no group asked about it.
    pub(crate) fn paint(&mut self, id: NodeId, color: Vec3) -> Result<(), SceneError> {
        self.component_mut::<ObjectColor>(id)?.0 = color;
        Ok(())
    }

    /// What a thing is painted is part of what it is, so a linked copy is painted
    /// with its group. A node nobody linked is a group of one, and the write lands
    /// on it alone.
    pub fn set_color(&mut self, id: NodeId, color: Vec3) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| scene.paint(member, color))
    }

    /// Transparency in bytes: 255 is opaque, 0 is invisible.
    pub fn opacity(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.component::<Opacity>(id)?.0)
    }

    pub(crate) fn fade(&mut self, id: NodeId, alpha: f32) -> Result<(), SceneError> {
        self.component_mut::<Opacity>(id)?.0 = alpha.clamp(0.0, MAX_ALPHA);
        Ok(())
    }

    pub fn set_opacity(&mut self, id: NodeId, alpha: f32) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| scene.fade(member, alpha))
    }

    /// The image path a node is skinned with, or `None` for a flat colour.
    pub fn texture(&self, id: NodeId) -> Result<Option<String>, SceneError> {
        Ok(self.component::<NodeTexture>(id)?.0.clone())
    }

    pub(crate) fn skin(&mut self, id: NodeId, path: Option<String>) -> Result<(), SceneError> {
        self.component_mut::<NodeTexture>(id)?.0 = path;
        Ok(())
    }

    pub fn set_texture(&mut self, id: NodeId, path: Option<String>) -> Result<(), SceneError> {
        if let Some(path) = &path
            && let Err(why) = packed(path)
        {
            return Err(SceneError::BadTexture {
                path: path.clone(),
                why,
            });
        }

        let shared = path;
        self.spread(id, move |scene, member| scene.skin(member, shared.clone()))
    }

    pub fn is_visible(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.component::<Visible>(id)?.0)
    }

    pub fn set_visible(&mut self, id: NodeId, visible: bool) -> Result<(), SceneError> {
        self.component_mut::<Visible>(id)?.0 = visible;
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

    /// Whether `parent` may hold a node of `child_ui` kind. The overlay is measured
    /// in pixels and the world in metres, so a branch never crosses between them -
    /// except at the scene root, which is where both kinds are hung.
    pub(crate) fn branch_allows(&self, parent: NodeId, child_ui: bool) -> Result<(), SceneError> {
        if parent == self.root {
            return Ok(());
        }
        if self.is_ui(parent)? == child_ui {
            Ok(())
        } else {
            Err(SceneError::CrossedTree)
        }
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

/// Channels in 0..1 written the way a design tool writes them: `#rrggbb`, lowercase.
/// Anything that wants a colour out of the engine asks for it in this form, so a
/// preset and a script's own write go through the same door.
pub(crate) fn hex_of(color: Vec3) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(color.x),
        channel(color.y),
        channel(color.z)
    )
}

pub(crate) fn parse_hex_color(hex: &str) -> Option<(Vec3, Option<f32>)> {
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

/// Whether a path is a file Carbine's one image loader can read. Only the header is
/// opened: the renderer checks the whole container when it puts the skin on the GPU,
/// and a script that names a PNG hears about it here rather than watching an object
/// draw in a flat colour.
fn packed(path: &str) -> Result<(), String> {
    let mut head = [0u8; carbine_ktx::MAGIC.len()];
    let Ok(mut file) = std::fs::File::open(path) else {
        return Err("there is no file at that path".to_owned());
    };
    if file.read_exact(&mut head).is_err() {
        return Err("it is shorter than an image".to_owned());
    }
    if carbine_ktx::is_ktx2(&head) {
        Ok(())
    } else {
        Err("it is not a KTX2 file".to_owned())
    }
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
    fn a_child_list_reorders_adopts_and_detaches_the_rest() {
        let mut scene = Scene::new();
        let panel = scene.create("rectangle", None, None).unwrap();
        let first = scene.create("square", None, Some(panel)).unwrap();
        let second = scene.create("square", None, Some(panel)).unwrap();
        let third = scene.create("square", None, Some(panel)).unwrap();
        let stranger = scene.create("square", None, None).unwrap();

        scene
            .set_children(panel, &[third, first, stranger])
            .unwrap();
        assert_eq!(scene.children(panel).unwrap(), vec![third, first, stranger]);
        assert_eq!(
            scene.parent(stranger).unwrap(),
            Some(panel),
            "a node listed but not yet hung here is adopted"
        );
        assert_eq!(
            scene.parent(second).unwrap(),
            Some(scene.root()),
            "a child left out is detached, not destroyed"
        );
        assert_eq!(scene.node_count(), 5, "and every node is still standing");
        assert!(scene.name(second).is_ok());

        scene.set_children(panel, &[first]).unwrap();
        assert_eq!(scene.children(panel).unwrap(), vec![first]);
        assert!(scene.name(third).is_ok());
    }

    #[test]
    fn a_child_list_refuses_loops_and_repeats_without_changing_anything() {
        let mut scene = Scene::new();
        let root = scene.root();
        let parent = scene.create("rectangle", None, None).unwrap();
        let child = scene.create("rectangle", None, Some(parent)).unwrap();
        let grandchild = scene.create("rectangle", None, Some(child)).unwrap();

        assert!(matches!(
            scene.set_children(parent, &[parent]),
            Err(SceneError::Cycle)
        ));
        assert!(matches!(
            scene.set_children(grandchild, &[parent]),
            Err(SceneError::Cycle)
        ));
        assert!(matches!(
            scene.set_children(parent, &[child, child]),
            Err(SceneError::DuplicateChild)
        ));
        assert!(matches!(
            scene.set_children(parent, &[root]),
            Err(SceneError::CannotReparentRoot)
        ));
        assert_eq!(scene.children(parent).unwrap(), vec![child]);
        assert_eq!(scene.children(child).unwrap(), vec![grandchild]);

        // A node deeper in the branch is not a loop: this just pulls it up a level.
        scene.set_children(parent, &[grandchild]).unwrap();
        assert_eq!(scene.children(child).unwrap(), Vec::<NodeId>::new());
        assert_eq!(scene.parent(grandchild).unwrap(), Some(parent));
    }

    #[test]
    fn adding_and_removing_a_child_moves_one_node() {
        let mut scene = Scene::new();
        let parent = scene.create("rectangle", None, None).unwrap();
        let first = scene.create("square", None, Some(parent)).unwrap();
        let second = scene.create("square", None, None).unwrap();

        scene.add_child(parent, second).unwrap();
        assert_eq!(
            scene.children(parent).unwrap(),
            vec![first, second],
            "a fresh child goes last"
        );

        scene.remove_child(parent, first).unwrap();
        assert_eq!(scene.children(parent).unwrap(), vec![second]);
        assert_eq!(scene.parent(first).unwrap(), Some(scene.root()));
        assert!(
            scene.name(first).is_ok(),
            "removing a child never destroys it"
        );

        assert!(matches!(
            scene.remove_child(parent, first),
            Err(SceneError::NotAChild)
        ));
        assert!(matches!(
            scene.add_child(parent, parent),
            Err(SceneError::Cycle)
        ));
    }

    #[test]
    fn the_overlay_and_the_world_never_share_a_branch() {
        let mut scene = Scene::new();
        let cube = scene.create("cube", None, None).unwrap();
        let panel = scene.create_ui("rectangle", None, None).unwrap();

        assert!(matches!(
            scene.create_ui("circle", None, Some(cube)),
            Err(SceneError::CrossedTree)
        ));
        assert!(matches!(
            scene.create("cube", None, Some(panel)),
            Err(SceneError::CrossedTree)
        ));
        assert!(matches!(
            scene.spawn_entity(Some("cube"), None, Some(panel), None, 1.0, 1.0),
            Err(SceneError::CrossedTree)
        ));
        assert!(matches!(
            scene.reparent(cube, Some(panel)),
            Err(SceneError::CrossedTree)
        ));
        assert!(matches!(
            scene.reparent(panel, Some(cube)),
            Err(SceneError::CrossedTree)
        ));
        assert!(matches!(
            scene.set_children(panel, &[cube]),
            Err(SceneError::CrossedTree)
        ));

        // Each branch stays in its own units, and the root still holds both.
        let badge = scene.create_ui("circle", None, Some(panel)).unwrap();
        let lid = scene.create("cube", None, Some(cube)).unwrap();
        assert_eq!(scene.children(panel).unwrap(), vec![badge]);
        assert_eq!(scene.children(cube).unwrap(), vec![lid]);
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
        assert_eq!(scene.opacity(card).unwrap(), MAX_ALPHA);

        scene.set_color_hex(card, "#0f0f0f40").unwrap();
        assert_eq!(scene.opacity(card).unwrap(), 64.0);
        assert_eq!(scene.color_hex(card).unwrap(), "#0f0f0f");
    }

    #[test]
    fn a_new_node_is_opaque_and_untextured() {
        let mut scene = Scene::new();
        let crate_ = scene.create("cube", None, None).unwrap();

        assert_eq!(scene.opacity(crate_).unwrap(), MAX_ALPHA);
        assert_eq!(scene.texture(crate_).unwrap(), None);
    }

    #[test]
    fn a_texture_is_remembered_as_a_path() {
        let mut scene = Scene::new();
        let crate_ = scene.create("cube", None, None).unwrap();

        let wood = crate::test_skin("node-wood");
        scene.set_texture(crate_, Some(wood.clone())).unwrap();
        assert_eq!(
            scene.texture(crate_).unwrap().as_deref(),
            Some(wood.as_str())
        );

        scene.set_texture(crate_, None).unwrap();
        assert_eq!(scene.texture(crate_).unwrap(), None);
    }

    #[test]
    fn a_skin_has_to_be_a_packed_image() {
        let mut scene = Scene::new();
        let crate_ = scene.create("cube", None, None).unwrap();

        let missing = scene.set_texture(crate_, Some("assets/wood.png".to_owned()));
        assert!(
            matches!(&missing, Err(SceneError::BadTexture { why, .. })
                if why == "there is no file at that path"),
            "{missing:?}"
        );

        let lying = std::env::temp_dir().join("carbine-node-lying-skin.png");
        std::fs::write(
            &lying,
            [
                0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
            ],
        )
        .unwrap();
        let wrong = scene.set_texture(crate_, Some(lying.to_str().unwrap().to_owned()));
        assert!(
            matches!(&wrong, Err(SceneError::BadTexture { why, .. })
                if why == "it is not a KTX2 file"),
            "{wrong:?}"
        );
        assert_eq!(
            scene.texture(crate_).unwrap(),
            None,
            "a refused path leaves the object unskinned"
        );
        assert!(
            wrong
                .unwrap_err()
                .to_string()
                .contains("carbine.packager.image"),
            "the refusal has to name the tool that fixes it"
        );

        std::fs::remove_file(&lying).ok();
    }

    #[test]
    fn transparency_is_an_eight_bit_count_on_world_objects_too() {
        let mut scene = Scene::new();
        let crate_ = scene.create("cube", None, None).unwrap();

        scene.set_opacity(crate_, 300.0).unwrap();
        assert_eq!(scene.opacity(crate_).unwrap(), MAX_ALPHA);

        scene.set_opacity(crate_, -5.0).unwrap();
        assert_eq!(scene.opacity(crate_).unwrap(), 0.0);

        scene.set_opacity(crate_, 120.0).unwrap();
        assert_eq!(scene.opacity(crate_).unwrap(), 120.0);
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

    #[test]
    fn clearing_the_scene_takes_the_tree_and_the_weather_with_it() {
        let mut scene = Scene::new();
        let crate_ = scene.create("cube", None, None).unwrap();
        let bolt = scene.create("sphere", None, Some(crate_)).unwrap();
        let card = scene.create_ui("rectangle", None, None).unwrap();
        let crowd = scene.create_particles(None, None).unwrap();
        let rain = scene.create_precipitation("rain", None).unwrap();
        let quake = scene.create_disaster("earthquake", None).unwrap();
        scene.start_disaster(quake).unwrap();

        scene.clear();

        assert_eq!(scene.node_count(), 0);
        assert_eq!(scene.children(scene.root()).unwrap(), vec![]);
        for node in [crate_, bolt, card, crowd, rain] {
            assert_eq!(scene.name(node), Err(SceneError::NodeDestroyed));
        }
        assert_eq!(scene.disaster_ids(), vec![]);
    }

    #[test]
    fn clearing_leaves_the_look_of_the_world_alone() {
        let mut scene = Scene::new();
        scene.set_fog(40.0);
        scene.camera.position = Vec3::new(3.0, 4.0, 5.0);
        scene.set_player_enabled(true);

        scene.clear();

        assert_eq!(scene.atmosphere().fog, 40.0);
        assert_eq!(scene.camera.position, Vec3::new(3.0, 4.0, 5.0));
        assert!(scene.player_is_enabled());
    }

    #[test]
    fn a_reset_scene_is_the_one_the_process_started_with() {
        let mut scene = Scene::new();
        let crate_ = scene.create("cube", None, None).unwrap();
        let rain = scene.create_precipitation("snow", None).unwrap();
        scene.set_fog(40.0);
        scene.camera.position = Vec3::new(3.0, 4.0, 5.0);
        scene.set_player_enabled(true);
        scene.set_light_quality_name("extra").unwrap();
        scene.report_gpu(crate::atmosphere::DeviceKind::Discrete, Some(8 << 30));

        scene.reset();

        assert_eq!(scene.node_count(), 0);
        assert_eq!(scene.name(crate_), Err(SceneError::NodeDestroyed));
        assert_eq!(scene.name(rain), Err(SceneError::NodeDestroyed));
        assert_eq!(scene.disaster_ids(), vec![]);
        assert_eq!(scene.atmosphere().fog, 0.0);
        assert_eq!(scene.camera.position, Vec3::new(0.0, -6.0, 4.0));
        assert!(!scene.player_is_enabled());
        // A script's rung is authored content, so it goes; the machine the world
        // is drawn on is not, so it stays.
        assert_eq!(
            scene.written_light_quality(),
            crate::atmosphere::LightQuality::Auto
        );
        assert_eq!(scene.light().quality, crate::atmosphere::LightQuality::High);
        assert!(scene.gpu().is_some());
    }
}
