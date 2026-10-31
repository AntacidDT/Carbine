//! Copies of a node: more of the same thing, or a crowd of them sharing one
//! configuration.
//!
//! A duplicate answers *what was written here* and writes the same values into new
//! nodes. A linked duplicate answers *what this thing is made of*: the copies keep
//! their own place, turning, size and hit points, while the values that say what the
//! thing is stay a single value for the whole group, so painting one paints all
//! twenty. That is the bargain a linked duplicate in Blender strikes with the mesh it
//! shares - the data block is common, the object hanging off it is not.
//!
//! Which side of that bargain a property sits on is decided by the setters, which run
//! their write through [`Scene::spread`]. The rule the split follows is one sentence:
//! what a script writes about what a thing *is* goes to the group, and what happens
//! *to* one of its members - where it stands, how it is turned, how much of it is
//! left - stays with that member. So a corpse does not unmake its twins, and twins
//! do not inherit the wound their original was already carrying.

use bevy_ecs::prelude::*;
use glam::Vec3;

use crate::collision::Collider;
use crate::entity::EntityData;
use crate::motion::Motion;
use crate::node::{
    Children, NodeId, NodeKind, NodeMesh, NodeName, NodeTexture, ObjectColor, Opacity, Parent,
    Scene, SceneError, Transform, Visible,
};
use crate::ui::Ui;

/// Which link group a node belongs to. Only a linked copy carries it, and the
/// original picks one up the first time it is copied by link, so the first twin and
/// its parent answer to the same number from then on.
#[derive(Component)]
pub(crate) struct Link(pub(crate) u64);

impl Scene {
    /// Writes `count` more of `id`. Each copy hangs under the same parent and starts
    /// where the original stood, still, with the same size, colour and settings; it
    /// has its own place, turning and hit points from then on, and the children come
    /// along as copies of their own. Nothing is linked, so changing a copy changes
    /// nothing else.
    pub fn duplicate(&mut self, id: NodeId, count: usize) -> Result<Vec<NodeId>, SceneError> {
        self.copy_many(id, count, false)
    }

    /// Writes `count` more of `id` and ties them to it: the copies, and the node
    /// itself if it was not already in a group, share one configuration. A write
    /// about what the thing is - its colour, its skin, how it takes gravity, what it
    /// is cut as, the crowd a system flies - lands on every member; a write about
    /// where a member is or how much of it is left lands on that member alone.
    pub fn link_duplicate(&mut self, id: NodeId, count: usize) -> Result<Vec<NodeId>, SceneError> {
        self.copy_many(id, count, true)
    }

    /// The group `id` already belongs to, or a new one opened with the node at its
    /// head. A copy of a member joins the group it was copied from.
    fn group_of(&mut self, id: NodeId) -> Result<u64, SceneError> {
        if let Some(group) = self.link_of(id) {
            return Ok(group);
        }
        let entity = self.entity(id)?;
        let group = self.next_link;
        self.next_link += 1;
        self.links.insert(group, vec![id]);
        self.world.entity_mut(entity).insert(Link(group));
        Ok(group)
    }

    fn link_of(&self, id: NodeId) -> Option<u64> {
        let entity = *self.ids.get(&id)?;
        self.world.get::<Link>(entity).map(|link| link.0)
    }

    fn copy_many(
        &mut self,
        id: NodeId,
        count: usize,
        linked: bool,
    ) -> Result<Vec<NodeId>, SceneError> {
        if id == self.root() {
            return Err(SceneError::CannotCopyRoot);
        }
        self.entity(id)?;
        let group = if linked {
            Some(self.group_of(id)?)
        } else {
            None
        };
        let parent = self.parent(id)?;

        let mut copies = Vec::with_capacity(count);
        for _ in 0..count {
            copies.push(self.copy_branch(id, parent, group)?);
        }

        Ok(copies)
    }

    /// The node and the branch under it, written again. Only the node that was asked
    /// for joins a link group - its children come as plain copies, the way a linked
    /// duplicate in Blender links the object it was made from and nothing else.
    fn copy_branch(
        &mut self,
        id: NodeId,
        parent: Option<NodeId>,
        group: Option<u64>,
    ) -> Result<NodeId, SceneError> {
        let fresh = self.copy_node(id, parent, group)?;
        for child in self.children(id)? {
            self.copy_branch(child, Some(fresh), None)?;
        }
        Ok(fresh)
    }

    /// One node, read and written again. Whatever it carries is carried: a body and
    /// the mesh it is cut from, an actor's hit points, a UI element's corners, a
    /// crowd's knobs. The copy starts still, since a new body has not been pushed by
    /// anything yet.
    fn copy_node(
        &mut self,
        id: NodeId,
        parent: Option<NodeId>,
        group: Option<u64>,
    ) -> Result<NodeId, SceneError> {
        let parent_id = parent.unwrap_or(self.root());
        let source = self.entity(id)?;

        let name = self.component::<NodeName>(id)?.0.clone();
        let kind = self.component::<NodeKind>(id)?.0.clone();
        let transform = *self.component::<Transform>(id)?;
        let color = self.component::<ObjectColor>(id)?.0;
        let opacity = self.component::<Opacity>(id)?.0;
        let texture = self.component::<NodeTexture>(id)?.0.clone();
        let visible = self.component::<Visible>(id)?.0;
        let motion = {
            let source = self.component::<Motion>(id)?;
            Motion {
                anchored: source.anchored,
                gravity: source.gravity,
                gravity_pull: source.gravity_pull,
                velocity: Vec3::ZERO,
            }
        };
        // The same bytes the original is drawn from, under the same handle: the mesh
        // a primitive was built with is never rewritten, so one buffer on the GPU
        // serves a hundred copies of it.
        let body = self.world.get::<NodeMesh>(source).map(|mesh| NodeMesh {
            handle: mesh.handle,
            data: mesh.data.clone(),
        });
        let collider = self.world.get::<Collider>(source).copied();
        let ui = self.world.get::<Ui>(source).copied();
        let actor = self.world.get::<EntityData>(source).map(|data| EntityData {
            health: data.health,
            damage: data.damage,
            hurt: 0.0,
            hit_pause: data.hit_pause,
            stay_after_death: data.stay_after_death,
        });
        let flying = self.is_particle_system(id)?;

        let fresh = self.reserve();
        let mut spawned = self.world.spawn((
            NodeName(name),
            NodeKind(kind),
            transform,
            ObjectColor(color),
            Opacity(opacity),
            NodeTexture(texture),
            Visible(visible),
            motion,
            Parent(parent_id),
            Children::default(),
        ));
        if let Some(body) = body {
            spawned.insert(body);
        }
        if let Some(collider) = collider {
            spawned.insert(collider);
        }
        if let Some(ui) = ui {
            spawned.insert(ui);
        }
        if let Some(actor) = actor {
            spawned.insert(actor);
        }
        if let Some(group) = group {
            spawned.insert(Link(group));
        }
        let entity = spawned.id();
        self.insert(fresh, entity);
        self.attach(fresh, parent_id);
        if let Some(group) = group {
            self.links.entry(group).or_default().push(fresh);
        }
        if flying {
            self.copy_particles(id, fresh)?;
        }

        Ok(fresh)
    }

    /// Every live member of the node's link group, the node itself first: the member
    /// a script named is the one whose write decides what the call answers with, and
    /// the twins only follow. Members destroyed since the last ask are dropped here,
    /// which is the only housekeeping a group ever needs.
    pub(crate) fn members(&mut self, id: NodeId) -> Vec<NodeId> {
        let Some(group) = self.link_of(id) else {
            return vec![id];
        };
        let mut live = self
            .links
            .get_mut(&group)
            .map_or(Vec::new(), std::mem::take);
        live.retain(|member| self.ids.contains_key(member) && *member != id);
        live.insert(0, id);
        self.links.insert(group, live.clone());
        live
    }

    /// Runs one configuration write over the whole link group. A node that was never
    /// linked is a group of one, so every setter that shares writes the same way
    /// whether or not a twin is listening. A member that no longer carries what is
    /// being written - an actor one link has since killed - is passed over; the call
    /// answers with what happened to the node the script named.
    pub(crate) fn spread(
        &mut self,
        id: NodeId,
        write: impl Fn(&mut Self, NodeId) -> Result<(), SceneError>,
    ) -> Result<(), SceneError> {
        let members = self.members(id);
        let mut outcome = Ok(());
        for (index, member) in members.into_iter().enumerate() {
            let step = write(self, member);
            if index == 0 {
                outcome = step;
            }
        }
        outcome
    }

    /// Whether `id` is a member of a link group with more than itself in it.
    pub fn is_linked(&self, id: NodeId) -> Result<bool, SceneError> {
        let Some(group) = self.link_of(id) else {
            return Ok(false);
        };
        Ok(self
            .links
            .get(&group)
            .is_some_and(|members| members.iter().any(|member| *member != id)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{Axis, VecField};

    fn crate_(scene: &mut Scene) -> NodeId {
        scene.create("cube", None, None).unwrap()
    }

    fn place(scene: &mut Scene, id: NodeId, at: Vec3) {
        scene
            .set_vec_field(id, VecField::Position, at)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    fn field(scene: &Scene, id: NodeId, field: VecField) -> Vec3 {
        scene.vec_field(id, field).unwrap()
    }

    #[test]
    fn a_duplicate_is_a_second_one_of_the_same_thing() {
        let mut scene = Scene::new();
        let original = crate_(&mut scene);
        place(&mut scene, original, Vec3::new(2.0, 3.0, 4.0));
        scene
            .set_vec_field(original, VecField::Size, Vec3::new(2.0, 2.0, 6.0))
            .unwrap();
        scene.set_color_hex(original, "#8b93a6").unwrap();
        scene.set_opacity(original, 200.0).unwrap();
        scene.set_uses_gravity(original, true).unwrap();
        scene.set_anchored(original, true).unwrap();
        scene.set_gravity_pull(original, 1.6).unwrap();
        scene
            .set_collision_shape(original, crate::collision::ColliderShape::Capsule)
            .unwrap();

        let copies = scene.duplicate(original, 3).unwrap();
        assert_eq!(scene.node_count(), 4);
        for copy in &copies {
            assert_eq!(scene.name(*copy).unwrap(), scene.name(original).unwrap());
            assert_eq!(scene.kind(*copy).unwrap(), "cube");
            assert_eq!(
                field(&scene, *copy, VecField::Position),
                Vec3::new(2.0, 3.0, 4.0)
            );
            assert_eq!(
                field(&scene, *copy, VecField::Size),
                Vec3::new(2.0, 2.0, 6.0)
            );
            assert_eq!(scene.color_hex(*copy).unwrap(), "#8b93a6");
            assert_eq!(scene.opacity(*copy).unwrap(), 200.0);
            assert!(scene.uses_gravity(*copy).unwrap());
            assert!(scene.is_anchored(*copy).unwrap());
            assert_eq!(scene.gravity_pull(*copy).unwrap(), 1.6);
            assert!(scene.has_collision(*copy).unwrap());
            assert_eq!(
                scene.collision_shape(*copy).unwrap(),
                crate::collision::ColliderShape::Capsule
            );
        }

        // And none of them is the same node: the copies hang beside the original.
        assert_eq!(scene.children(scene.root()).unwrap().len(), 4);
    }

    #[test]
    fn one_duplicate_when_nobody_named_a_number() {
        let mut scene = Scene::new();
        let original = crate_(&mut scene);

        assert_eq!(scene.duplicate(original, 1).unwrap().len(), 1);
        assert_eq!(scene.node_count(), 2);
        assert!(scene.duplicate(original, 0).unwrap().is_empty());
        assert_eq!(scene.node_count(), 2, "asking for none makes none");
    }

    #[test]
    fn a_copy_moves_and_wears_wounds_of_its_own() {
        let mut scene = Scene::new();
        let original = scene
            .spawn_entity(Some("cube"), None, None, None, 40.0, 7.0)
            .unwrap();
        scene.set_health(original, 25.0).unwrap();

        let copy = scene.duplicate(original, 1).unwrap()[0];
        place(&mut scene, copy, Vec3::new(9.0, 0.0, 0.0));
        scene.set_health(copy, 3.0).unwrap();

        assert_eq!(field(&scene, original, VecField::Position), Vec3::ZERO);
        assert_eq!(scene.health(original).unwrap(), 25.0);
        assert_eq!(scene.health(copy).unwrap(), 3.0);
        assert_eq!(
            scene.damage(copy).unwrap(),
            7.0,
            "what it deals came with it"
        );
    }

    #[test]
    fn a_plain_copy_shares_nothing_with_the_thing_it_came_from() {
        let mut scene = Scene::new();
        let original = crate_(&mut scene);
        let copy = scene.duplicate(original, 1).unwrap()[0];

        scene.set_color_hex(copy, "#00ff00").unwrap();
        assert_eq!(scene.color_hex(original).unwrap(), "#eb7a34");
        assert!(
            !scene.is_linked(copy).unwrap(),
            "a duplicate is a copy, not a twin"
        );
    }

    #[test]
    fn a_linked_copy_takes_the_look_of_the_whole_group() {
        let mut scene = Scene::new();
        let original = crate_(&mut scene);
        let twins = scene.link_duplicate(original, 4).unwrap();

        scene.set_color_hex(original, "#1f3557").unwrap();
        assert_eq!(scene.color_hex(original).unwrap(), "#1f3557");
        for twin in &twins {
            assert_eq!(
                scene.color_hex(*twin).unwrap(),
                "#1f3557",
                "one paint, and all twenty-one are that colour"
            );
        }

        // Written from a twin, it still reaches the whole group.
        scene.set_opacity(twins[1], 40.0).unwrap();
        assert_eq!(scene.opacity(original).unwrap(), 40.0);
        for twin in &twins {
            assert_eq!(scene.opacity(*twin).unwrap(), 40.0);
        }
    }

    #[test]
    fn a_linked_copy_keeps_its_own_place_and_its_own_wounds() {
        let mut scene = Scene::new();
        let original = scene
            .spawn_entity(Some("cube"), None, None, None, 40.0, 7.0)
            .unwrap();
        let twins = scene.link_duplicate(original, 2).unwrap();

        place(&mut scene, twins[0], Vec3::new(5.0, 0.0, 0.0));
        scene
            .set_vec_field_axis(twins[0], VecField::RotationEuler, Axis::Z, 90.0)
            .unwrap();
        scene.set_health(twins[1], 0.0).unwrap();

        assert_eq!(field(&scene, original, VecField::Position), Vec3::ZERO);
        assert_eq!(
            field(&scene, twins[0], VecField::Position),
            Vec3::new(5.0, 0.0, 0.0)
        );
        assert!(field(&scene, original, VecField::RotationEuler).z.abs() < 1e-4);
        assert!(
            scene.alive(original).unwrap(),
            "one twin dying is one twin's business"
        );
        assert_eq!(scene.health(twins[1]).unwrap(), 0.0);
    }

    #[test]
    fn the_shape_a_body_is_cut_as_is_shared_but_its_wound_is_not() {
        let mut scene = Scene::new();
        let original = crate_(&mut scene);
        let twins = scene.link_duplicate(original, 2).unwrap();

        scene.set_collision_shape_name(original, "sphere").unwrap();
        for twin in &twins {
            assert!(scene.has_collision(*twin).unwrap());
            assert_eq!(
                scene.collision_shape(*twin).unwrap(),
                crate::collision::ColliderShape::Sphere
            );
        }

        scene.set_uses_gravity(original, true).unwrap();
        for twin in &twins {
            assert!(scene.uses_gravity(*twin).unwrap());
        }
    }

    #[test]
    fn a_size_is_a_place_and_belongs_to_one_body() {
        let mut scene = Scene::new();
        let original = crate_(&mut scene);
        let twin = scene.link_duplicate(original, 1).unwrap()[0];

        scene
            .set_vec_field(twin, VecField::Size, Vec3::new(3.0, 3.0, 3.0))
            .unwrap();
        assert_eq!(field(&scene, original, VecField::Size), Vec3::ONE);
        assert_eq!(
            field(&scene, twin, VecField::Size),
            Vec3::new(3.0, 3.0, 3.0)
        );
    }

    #[test]
    fn a_twin_that_was_destroyed_stops_listening() {
        let mut scene = Scene::new();
        let original = crate_(&mut scene);
        let twins = scene.link_duplicate(original, 3).unwrap();
        scene.destroy(twins[1]).unwrap();

        scene.set_color_hex(original, "#0f0f0f").unwrap();
        assert_eq!(scene.color_hex(twins[0]).unwrap(), "#0f0f0f");
        assert_eq!(scene.color_hex(twins[2]).unwrap(), "#0f0f0f");
        assert!(matches!(
            scene.color_hex(twins[1]),
            Err(SceneError::NodeDestroyed)
        ));
        assert_eq!(scene.members(original).len(), 3, "the dead one is gone");
    }

    #[test]
    fn copying_a_twin_joins_the_group_it_was_copied_from() {
        let mut scene = Scene::new();
        let original = crate_(&mut scene);
        let first = scene.link_duplicate(original, 1).unwrap()[0];
        let second = scene.link_duplicate(first, 1).unwrap()[0];

        scene.set_color_hex(second, "#abcdef").unwrap();
        assert_eq!(scene.color_hex(original).unwrap(), "#abcdef");
        assert_eq!(scene.color_hex(first).unwrap(), "#abcdef");
        assert_eq!(scene.members(original).len(), 3);
    }

    #[test]
    fn a_plain_copy_of_a_twin_leaves_the_group() {
        let mut scene = Scene::new();
        let original = crate_(&mut scene);
        scene.link_duplicate(original, 1).unwrap();
        let copy = scene.duplicate(original, 1).unwrap()[0];

        scene.set_color_hex(copy, "#000000").unwrap();
        assert_eq!(scene.color_hex(copy).unwrap(), "#000000");
        assert_ne!(
            scene.color_hex(original).unwrap(),
            "#000000",
            "a duplicate took the colour, it did not join the sharing"
        );
    }

    #[test]
    fn a_branch_comes_across_whole() {
        let mut scene = Scene::new();
        let crate_origin = crate_(&mut scene);
        let lid = scene
            .create("cube", Some("Lid".to_owned()), Some(crate_origin))
            .unwrap();
        place(&mut scene, crate_origin, Vec3::new(1.0, 0.0, 0.0));
        place(&mut scene, lid, Vec3::new(0.0, 0.0, 1.0));

        let copy = scene.duplicate(crate_origin, 1).unwrap()[0];
        let kids = scene.children(copy).unwrap();
        assert_eq!(kids.len(), 1);
        assert_eq!(scene.name(kids[0]).unwrap(), "Lid");
        assert_eq!(
            field(&scene, kids[0], VecField::Position),
            Vec3::new(0.0, 0.0, 1.0)
        );
        assert_ne!(kids[0], lid, "and it is a second lid, not the same one");
        assert_eq!(scene.node_count(), 4);
    }

    #[test]
    fn a_ui_element_copies_and_links_like_anything_else() {
        let mut scene = Scene::new();
        let panel = scene.create_ui("rectangle", None, None).unwrap();
        scene.set_corner(panel, 12.0).unwrap();
        scene.set_outline(panel, 2.0).unwrap();
        scene.set_polygon_sides(panel, 6).unwrap();

        let copy = scene.duplicate(panel, 1).unwrap()[0];
        assert!(scene.is_ui(copy).unwrap());
        assert_eq!(scene.corner(copy).unwrap(), 12.0);
        assert_eq!(scene.outline(copy).unwrap(), 2.0);
        assert_eq!(scene.polygon_sides(copy).unwrap(), 6);

        let twin = scene.link_duplicate(panel, 1).unwrap()[0];
        scene.set_corner(twin, 3.0).unwrap();
        assert_eq!(scene.corner(panel).unwrap(), 3.0);
        assert_eq!(
            scene.corner(copy).unwrap(),
            12.0,
            "the plain copy never joined in"
        );
    }

    #[test]
    fn a_crowd_copies_its_knobs_and_lays_its_own_air() {
        let mut scene = Scene::new();
        let system = scene.create_particles(None, None).unwrap();
        scene.set_particle_count(system, 30).unwrap();
        scene.set_particle_speed(system, 2.5).unwrap();
        scene.set_particles_recycle(system, false).unwrap();
        place(&mut scene, system, Vec3::new(4.0, 0.0, 6.0));

        let twin = scene.link_duplicate(system, 1).unwrap()[0];
        assert_eq!(scene.particle_count(twin).unwrap(), 30);
        assert_eq!(scene.particle_speed(twin).unwrap(), 2.5);
        assert!(!scene.particles_recycle(twin).unwrap());
        assert_eq!(
            scene.particles_in_air(twin).unwrap(),
            30,
            "its own thirty drops"
        );

        place(&mut scene, twin, Vec3::new(-4.0, 0.0, 6.0));
        scene.set_particle_size(twin, 0.75).unwrap();
        assert_eq!(
            scene.particle_size(system).unwrap(),
            0.75,
            "a knob is a knob"
        );

        // One twin parked is not both twins parked.
        scene.set_particles_active(twin, false).unwrap();
        assert!(!scene.particles_are_active(twin).unwrap());
        assert!(scene.particles_are_active(system).unwrap());
    }

    #[test]
    fn a_body_draws_from_one_mesh_for_the_whole_group() {
        let mut scene = Scene::new();
        let original = crate_(&mut scene);
        let twin = scene.link_duplicate(original, 1).unwrap()[0];

        let items = scene.draw_items();
        let handles: Vec<_> = items
            .iter()
            .filter(|item| item.node == original || item.node == twin)
            .map(|item| item.handle)
            .collect();
        assert_eq!(handles.len(), 2);
        assert_eq!(
            handles[0], handles[1],
            "linked bodies are the same geometry, so the GPU is told so once"
        );
    }

    #[test]
    fn a_dead_actor_in_a_group_takes_no_more_writes() {
        let mut scene = Scene::new();
        let original = scene
            .spawn_entity(Some("cube"), None, None, None, 40.0, 7.0)
            .unwrap();
        let twin = scene.link_duplicate(original, 1).unwrap()[0];
        scene.kill_entity(twin).unwrap();

        scene.set_hit_pause(original, 2.0).unwrap();
        assert_eq!(scene.hit_pause(original).unwrap(), 2.0);
        assert!(matches!(
            scene.hit_pause(twin),
            Err(SceneError::MissingComponent)
        ));
    }

    #[test]
    fn one_body_dying_leaves_its_twins_in_the_solver() {
        let mut scene = Scene::new();
        let killer = scene
            .spawn_entity(Some("cube"), None, None, None, 40.0, 50.0)
            .unwrap();
        let original = scene
            .spawn_entity(Some("cube"), None, None, None, 40.0, 0.0)
            .unwrap();
        scene.set_collision(original, true).unwrap();
        let twin = scene.link_duplicate(original, 1).unwrap()[0];

        scene.hit_entity(killer, twin).unwrap();

        assert!(!scene.alive(twin).unwrap());
        assert!(
            scene.has_collision(original).unwrap(),
            "what the engine does to one body is that body's own business"
        );
        assert!(
            !scene.uses_gravity(original).unwrap(),
            "and the death did not hand the twin's settings back to the group"
        );
    }

    #[test]
    fn the_root_is_not_a_thing_anyone_copies() {
        let mut scene = Scene::new();
        assert!(matches!(
            scene.duplicate(scene.root(), 1),
            Err(SceneError::CannotCopyRoot)
        ));
        assert!(matches!(
            scene.link_duplicate(scene.root(), 1),
            Err(SceneError::CannotCopyRoot)
        ));
    }

    #[test]
    fn copying_a_destroyed_node_is_refused() {
        let mut scene = Scene::new();
        let original = crate_(&mut scene);
        scene.destroy(original).unwrap();

        assert!(matches!(
            scene.duplicate(original, 1),
            Err(SceneError::NodeDestroyed)
        ));
        assert!(matches!(
            scene.link_duplicate(original, 1),
            Err(SceneError::NodeDestroyed)
        ));
    }

    #[test]
    fn a_cleared_scene_forgets_which_bodies_shared() {
        let mut scene = Scene::new();
        let original = crate_(&mut scene);
        let twin = scene.link_duplicate(original, 1).unwrap()[0];

        scene.clear();

        assert!(matches!(
            scene.color_hex(twin),
            Err(SceneError::NodeDestroyed)
        ));
        let reborn = scene.create("cube", None, None).unwrap();
        let again = scene.link_duplicate(reborn, 1).unwrap()[0];
        scene.set_color_hex(again, "#123456").unwrap();
        assert_eq!(scene.color_hex(reborn).unwrap(), "#123456");
    }

    #[test]
    fn a_twin_starts_where_the_original_stands_now() {
        let mut scene = Scene::new();
        let original = crate_(&mut scene);
        place(&mut scene, original, Vec3::new(0.0, 0.0, 9.0));

        let twin = scene.link_duplicate(original, 1).unwrap()[0];
        assert_eq!(
            field(&scene, twin, VecField::Position),
            Vec3::new(0.0, 0.0, 9.0)
        );
        assert_eq!(
            scene.parent(twin).unwrap(),
            Some(scene.root()),
            "and under the same branch the original hangs on"
        );
    }
}
