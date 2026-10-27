use bevy_ecs::prelude::*;
use glam::Vec3;

use crate::mesh::MeshData;
use crate::motion::Motion;
use crate::node::{
    Children, DEFAULT_COLOR, NodeId, NodeKind, NodeMesh, NodeName, NodeTexture, ObjectColor,
    Opacity, Parent, Scene, SceneError, Transform,
};
use crate::primitives;

/// An actor: health and damage in hit points, the numbers a game reasons about.
#[derive(Component)]
pub(crate) struct EntityData {
    pub(crate) health: f32,
    pub(crate) damage: f32,
}

impl Scene {
    /// Spawns an entity. A `kind` gives it an object body with geometry; a
    /// bodiless entity is logic only, such as a controller or a camera anchor.
    pub fn spawn_entity(
        &mut self,
        kind: Option<&str>,
        name: Option<String>,
        parent: Option<NodeId>,
        position: Option<Vec3>,
        health: f32,
        damage: f32,
    ) -> Result<NodeId, SceneError> {
        let parent_id = parent.unwrap_or(self.root());
        self.entity(parent_id)?;

        let body: Option<MeshData> = match kind {
            Some(kind) => Some(
                primitives::build(kind).ok_or_else(|| SceneError::UnknownKind(kind.to_owned()))?,
            ),
            None => None,
        };
        let label = kind.unwrap_or("entity").to_owned();
        let name = name.unwrap_or_else(|| match kind {
            Some(kind) => primitives::default_name(kind),
            None => "Entity".to_owned(),
        });

        let id = self.reserve();
        let handle = self.next_mesh_handle();
        let mut entity = self.world.spawn((
            NodeName(name),
            NodeKind(label),
            Transform::default(),
            ObjectColor(DEFAULT_COLOR),
            Opacity::default(),
            NodeTexture::default(),
            EntityData { health, damage },
            Motion::default(),
            Parent(parent_id),
            Children::default(),
        ));
        if let Some(data) = body {
            entity.insert(NodeMesh { handle, data });
        }
        if let Some(position) = position
            && let Some(mut transform) = entity.get_mut::<Transform>()
        {
            transform.position = position;
        }

        let entity = entity.id();
        self.insert(id, entity);
        self.attach(id, parent_id);

        Ok(id)
    }

    /// Removes the actor but keeps its body in the world as a plain object.
    pub fn kill_entity(&mut self, id: NodeId) -> Result<(), SceneError> {
        if !self.is_entity(id)? {
            return Err(SceneError::NotAnEntity);
        }
        self.world
            .entity_mut(self.entity(id)?)
            .remove::<EntityData>();
        Ok(())
    }

    pub fn is_entity(&self, id: NodeId) -> Result<bool, SceneError> {
        let entity = self.entity(id)?;
        Ok(self.world.get::<EntityData>(entity).is_some())
    }

    /// Whether the node carries geometry, that is whether it has an object body.
    pub fn has_body(&self, id: NodeId) -> Result<bool, SceneError> {
        let entity = self.entity(id)?;
        Ok(self.world.get::<NodeMesh>(entity).is_some())
    }

    pub fn entity_count(&self) -> usize {
        self.ids
            .values()
            .filter(|entity| self.world.get::<EntityData>(**entity).is_some())
            .count()
    }

    pub fn health(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.component::<EntityData>(id)?.health)
    }

    pub fn set_health(&mut self, id: NodeId, health: f32) -> Result<(), SceneError> {
        self.component_mut::<EntityData>(id)?.health = health;
        Ok(())
    }

    pub fn damage(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.component::<EntityData>(id)?.damage)
    }

    pub fn set_damage(&mut self, id: NodeId, damage: f32) -> Result<(), SceneError> {
        self.component_mut::<EntityData>(id)?.damage = damage;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawning_an_entity_gives_it_a_body() {
        let mut scene = Scene::new();
        let id = scene
            .spawn_entity(Some("rectangle"), None, None, None, 40.0, 7.0)
            .unwrap();

        assert!(scene.is_entity(id).unwrap());
        assert!(scene.has_body(id).unwrap());
        assert_eq!(scene.entity_count(), 1);
        assert_eq!(scene.health(id).unwrap(), 40.0);
        assert_eq!(scene.damage(id).unwrap(), 7.0);
        assert_eq!(scene.kind(id).unwrap(), "rectangle");
        assert_eq!(scene.mesh(id).unwrap().vertex_count(), 4);
    }

    #[test]
    fn an_entity_carries_the_same_look_state_as_an_object() {
        let mut scene = Scene::new();
        let id = scene
            .spawn_entity(Some("cube"), None, None, None, 40.0, 7.0)
            .unwrap();

        assert_eq!(scene.alpha(id).unwrap(), crate::MAX_ALPHA);
        assert_eq!(scene.texture(id).unwrap(), None);

        scene
            .set_texture(id, Some("assets/moss.png".to_owned()))
            .unwrap();
        assert_eq!(
            scene.texture(id).unwrap().as_deref(),
            Some("assets/moss.png")
        );
    }

    #[test]
    fn an_entity_can_be_spawned_at_a_position_without_a_body() {
        let mut scene = Scene::new();
        let id = scene
            .spawn_entity(
                None,
                Some("controller".to_owned()),
                None,
                Some(Vec3::new(1.0, 2.0, 0.0)),
                1.0,
                0.0,
            )
            .unwrap();

        assert_eq!(scene.name(id).unwrap(), "controller");
        assert_eq!(scene.kind(id).unwrap(), "entity");
        assert_eq!(
            scene
                .vec_field(id, crate::node::VecField::Position)
                .unwrap(),
            Vec3::new(1.0, 2.0, 0.0)
        );
        assert!(matches!(scene.mesh(id), Err(SceneError::MissingComponent)));
    }

    #[test]
    fn killing_an_entity_keeps_its_body_and_stops_it_being_a_target() {
        let mut scene = Scene::new();
        let id = scene
            .spawn_entity(Some("rectangle"), None, None, None, 40.0, 7.0)
            .unwrap();

        scene.kill_entity(id).unwrap();
        assert!(!scene.is_entity(id).unwrap());
        assert_eq!(scene.entity_count(), 0);
        assert_eq!(scene.node_count(), 1);
        assert_eq!(scene.mesh(id).unwrap().vertex_count(), 4);
        assert!(matches!(
            scene.kill_entity(id),
            Err(SceneError::NotAnEntity)
        ));
    }

    #[test]
    fn a_plain_object_is_never_an_entity() {
        let mut scene = Scene::new();
        let id = scene.create("rectangle", None, None).unwrap();
        assert!(!scene.is_entity(id).unwrap());
        assert!(matches!(
            scene.kill_entity(id),
            Err(SceneError::NotAnEntity)
        ));
        assert!(matches!(
            scene.health(id),
            Err(SceneError::MissingComponent)
        ));
    }
}
