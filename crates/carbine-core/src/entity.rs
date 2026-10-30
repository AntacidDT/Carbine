use bevy_ecs::prelude::*;
use glam::Vec3;

use crate::mesh::MeshData;
use crate::motion::Motion;
use crate::node::{
    Children, DEFAULT_COLOR, NodeId, NodeKind, NodeMesh, NodeName, NodeTexture, ObjectColor,
    Opacity, Parent, Scene, SceneError, Transform, Visible,
};
use crate::primitives;

/// How long one hit silences the next, in seconds. A body that is hurt is not
/// hurt again until the pause runs out, so standing in a fire pit costs one bite
/// per pause rather than one per frame.
pub const DEFAULT_HIT_PAUSE: f32 = 0.5;

/// How far short of the full pause a timer may fall to still count as paid off.
const HIT_READY_SLACK: f32 = 1e-4;

/// An actor: health and damage in hit points, the numbers a game reasons about.
#[derive(Component)]
pub(crate) struct EntityData {
    pub(crate) health: f32,
    pub(crate) damage: f32,
    /// Seconds left before the next hit is allowed to bite.
    pub(crate) hurt: f32,
    /// How long a hit from anything, contact or scripted, holds the next one off.
    pub(crate) hit_pause: f32,
    /// Whether the body stays in the world, inert and still falling, once its
    /// health is gone. Off means the node is destroyed with its children.
    pub(crate) stay_after_death: bool,
}

impl EntityData {
    /// A body with health left. Zero or below is dead and stays dead — nothing in
    /// the engine heals, which is why there is no `revive` to write.
    fn living(&self) -> bool {
        self.health > 0.0
    }

    /// Whether a hit may reach this body right now: it has to be alive and the
    /// pause from the last one has to have run out.
    ///
    /// "Run out" allows for a tenth of a millisecond, because the timer is
    /// subtracted a frame at a time in the metre-sized floats the world is written
    /// in. A script that waits exactly `hitPause` deserves its hit, not a rounding
    /// error that swallows it.
    fn can_be_hit(&self) -> bool {
        self.living() && self.hurt <= HIT_READY_SLACK
    }
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
        self.branch_allows(parent_id, false)?;

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
            Visible::default(),
            EntityData {
                health,
                damage,
                hurt: 0.0,
                hit_pause: DEFAULT_HIT_PAUSE,
                stay_after_death: true,
            },
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

    /// Whether the actor still has health. A corpse answers false; a plain object
    /// answers `MissingComponent`, because it never had health to lose.
    pub fn alive(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.component::<EntityData>(id)?.living())
    }

    /// The gap one hit leaves before the next one bites, in seconds.
    pub fn hit_pause(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.component::<EntityData>(id)?.hit_pause)
    }

    pub fn set_hit_pause(&mut self, id: NodeId, seconds: f32) -> Result<(), SceneError> {
        self.component_mut::<EntityData>(id)?.hit_pause = seconds.max(0.0);
        Ok(())
    }

    pub fn stays_after_death(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.component::<EntityData>(id)?.stay_after_death)
    }

    /// Whether the body is left lying in the world when it dies, or removed with
    /// everything hanging off it.
    pub fn set_stays_after_death(&mut self, id: NodeId, stay: bool) -> Result<(), SceneError> {
        self.component_mut::<EntityData>(id)?.stay_after_death = stay;
        Ok(())
    }

    /// Applies one actor's `damage` to another and answers with the hit points it
    /// actually took. That is zero when the target is hurt or already dead, and
    /// when the attacker has nothing to give or is dead itself. This is the
    /// scripted half of `entity.hit(other)`; the other half is contact, which runs
    /// the same rule over the pairs the solver reported.
    pub fn hit_entity(&mut self, attacker: NodeId, victim: NodeId) -> Result<f32, SceneError> {
        self.entity(attacker)?;
        self.entity(victim)?;
        let weapon = self.component::<EntityData>(attacker)?;
        // A corpse has nothing to hit with. The contact pass already refuses to let
        // a dead body bite, and a script should not be able to talk it into one.
        if !weapon.living() {
            return Ok(0.0);
        }
        // `max` over a NaN leaves zero, so a damage value that is not a number
        // cannot poison the target's health.
        let dealt = weapon.damage.max(0.0);
        if dealt <= 0.0 {
            return Ok(0.0);
        }

        {
            let mut target = self.component_mut::<EntityData>(victim)?;
            if !target.can_be_hit() {
                return Ok(0.0);
            }
            target.health -= dealt;
            target.hurt = target.hit_pause;
        }
        if !self.component::<EntityData>(victim)?.living() {
            self.expire(victim)?;
        }
        Ok(dealt)
    }

    /// What the step does to every actor, whether or not anything touched it: the
    /// hurt pause runs down, so the next bite is one step nearer.
    pub(crate) fn count_down_hits(&mut self, dt: f32) {
        for id in self.node_tree() {
            if let Ok(mut data) = self.component_mut::<EntityData>(id) {
                data.hurt = (data.hurt - dt).max(0.0);
            }
        }
    }

    /// The actors that ran into each other this step trade damage, each to the
    /// other, under the same pause a scripted hit respects. A corpse neither
    /// hurts nor is hurt. Deaths are collected and applied after the round, so
    /// destroying one body cannot pull the floor out from under a pair still being
    /// read.
    pub(crate) fn hurt_by_contacts(&mut self) {
        let contacts = std::mem::take(&mut self.contacts);
        let mut bites: Vec<(NodeId, NodeId)> = Vec::new();

        for (a, b) in &contacts {
            for (attacker, victim) in [(*a, *b), (*b, *a)] {
                let Ok(weapon) = self.component::<EntityData>(attacker) else {
                    continue;
                };
                if !weapon.living() || weapon.damage.max(0.0) <= 0.0 {
                    continue;
                }
                let Ok(target) = self.component::<EntityData>(victim) else {
                    continue;
                };
                if target.can_be_hit() {
                    bites.push((attacker, victim));
                }
            }
        }

        let mut dead = Vec::new();
        for (attacker, victim) in bites {
            let Ok(weapon) = self.component::<EntityData>(attacker) else {
                continue;
            };
            let bite = weapon.damage.max(0.0);
            let Ok(mut target) = self.component_mut::<EntityData>(victim) else {
                continue;
            };
            // Two bodies can bite the same wound in one step; the pause the first
            // leaves holds the second off, exactly as it holds off the next step.
            if !target.can_be_hit() {
                continue;
            }
            target.health -= bite;
            target.hurt = target.hit_pause;
            if !target.living() {
                dead.push(victim);
            }
        }

        self.contacts = contacts;
        for victim in dead {
            let _ = self.expire(victim);
        }
    }

    /// The first time health runs out, once per body: a corpse is left inert but
    /// still pulled by gravity, or the node is destroyed outright.
    fn expire(&mut self, id: NodeId) -> Result<(), SceneError> {
        let stay = self.component::<EntityData>(id)?.stay_after_death;
        if self.player.body == Some(id) {
            // The engine hid this body to put the camera inside its eyes. A body the
            // player no longer wears is a thing lying in the world, so it gets back
            // the visibility the engine took from it.
            self.player.body = None;
            self.set_visible(id, true)?;
        }
        if stay {
            // Still falling, no longer in anyone's way, and no longer anything to
            // hit: the body is scenery the script can look at but not act with.
            self.set_collision(id, false)?;
            self.set_uses_gravity(id, true)?;
        } else {
            self.destroy(id)?;
        }
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

        let moss = crate::test_skin("entity-moss");
        scene.set_texture(id, Some(moss.clone())).unwrap();
        assert_eq!(scene.texture(id).unwrap().as_deref(), Some(moss.as_str()));
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

    fn actor(scene: &mut Scene, health: f32, damage: f32) -> NodeId {
        scene
            .spawn_entity(Some("cube"), None, None, None, health, damage)
            .unwrap()
    }

    #[test]
    fn a_new_actor_is_alive_and_holds_its_breath_for_half_a_second() {
        let mut scene = Scene::new();
        let id = actor(&mut scene, 40.0, 0.0);

        assert!(scene.alive(id).unwrap());
        assert_eq!(scene.hit_pause(id).unwrap(), DEFAULT_HIT_PAUSE);
        assert!(scene.stays_after_death(id).unwrap());
    }

    #[test]
    fn a_scripted_hit_takes_the_attackers_damage_off_the_target() {
        let mut scene = Scene::new();
        let attacker = actor(&mut scene, 40.0, 7.0);
        let victim = actor(&mut scene, 40.0, 0.0);

        assert_eq!(scene.hit_entity(attacker, victim).unwrap(), 7.0);
        assert_eq!(scene.health(victim).unwrap(), 33.0);
        assert!(scene.alive(victim).unwrap());
        // The attacker is not in the fight with itself.
        assert_eq!(scene.health(attacker).unwrap(), 40.0);
    }

    #[test]
    fn a_hit_that_does_no_harm_leaves_the_target_fresh() {
        let mut scene = Scene::new();
        let blunt = actor(&mut scene, 40.0, 0.0);
        let victim = actor(&mut scene, 40.0, 0.0);

        assert_eq!(scene.hit_entity(blunt, victim).unwrap(), 0.0);
        assert_eq!(scene.health(victim).unwrap(), 40.0);

        let sharp = actor(&mut scene, 40.0, 9.0);
        assert_eq!(scene.hit_entity(sharp, victim).unwrap(), 9.0);
    }

    #[test]
    fn a_dead_actor_has_nothing_to_hit_with() {
        let mut scene = Scene::new();
        let strong = actor(&mut scene, 40.0, 50.0);
        let weak = actor(&mut scene, 10.0, 6.0);

        scene.hit_entity(strong, weak).unwrap();
        assert!(!scene.alive(weak).unwrap());

        // Dead on its feet, so the 6 it still carries is worth nothing - the same
        // rule the contact pass already applies to a corpse.
        assert_eq!(scene.hit_entity(weak, strong).unwrap(), 0.0);
        assert_eq!(scene.health(strong).unwrap(), 40.0);
    }

    #[test]
    fn the_hurt_pause_holds_the_next_hit_off() {
        let mut scene = Scene::new();
        let attacker = actor(&mut scene, 40.0, 7.0);
        let victim = actor(&mut scene, 40.0, 0.0);
        scene.set_hit_pause(victim, 0.5).unwrap();

        assert_eq!(scene.hit_entity(attacker, victim).unwrap(), 7.0);
        assert_eq!(scene.hit_entity(attacker, victim).unwrap(), 0.0);

        // A step is capped at a tenth of a second, so the pause runs down in
        // tenths, the same way the rest of the world does.
        for _ in 0..4 {
            scene.step(0.1);
        }
        assert_eq!(scene.hit_entity(attacker, victim).unwrap(), 0.0);

        scene.step(0.1);
        assert_eq!(scene.hit_entity(attacker, victim).unwrap(), 7.0);
        assert_eq!(scene.health(victim).unwrap(), 26.0);
    }

    #[test]
    fn a_dead_body_is_not_beaten_further() {
        let mut scene = Scene::new();
        let attacker = actor(&mut scene, 40.0, 7.0);
        let victim = actor(&mut scene, 5.0, 0.0);

        assert_eq!(scene.hit_entity(attacker, victim).unwrap(), 7.0);
        assert!(!scene.alive(victim).unwrap());
        assert_eq!(scene.hit_entity(attacker, victim).unwrap(), 0.0);
        assert_eq!(scene.health(victim).unwrap(), -2.0);
    }

    #[test]
    fn death_leaves_a_corpse_that_falls_and_never_blocks() {
        let mut scene = Scene::new();
        let attacker = actor(&mut scene, 40.0, 50.0);
        let victim = actor(&mut scene, 20.0, 3.0);
        scene.set_collision(victim, true).unwrap();

        scene.hit_entity(attacker, victim).unwrap();

        assert!(!scene.alive(victim).unwrap());
        assert_eq!(scene.node_count(), 2, "the body is still in the world");
        assert!(
            !scene.has_collision(victim).unwrap(),
            "but it is no longer in anyone's way"
        );
        assert!(scene.uses_gravity(victim).unwrap(), "and it drops");
        assert_eq!(scene.health(attacker).unwrap(), 40.0);
    }

    #[test]
    fn a_body_that_does_not_stay_is_removed_whole() {
        let mut scene = Scene::new();
        let attacker = actor(&mut scene, 40.0, 50.0);
        let victim = actor(&mut scene, 20.0, 0.0);
        scene.set_stays_after_death(victim, false).unwrap();
        let hat = scene.create("cube", None, Some(victim)).unwrap();

        scene.hit_entity(attacker, victim).unwrap();

        assert_eq!(scene.node_count(), 1, "the corpse and its child are gone");
        assert!(matches!(scene.alive(hat), Err(SceneError::NodeDestroyed)));
    }

    #[test]
    fn the_player_drops_a_body_that_dies() {
        let mut scene = Scene::new();
        scene.set_player_enabled(true);
        let body = actor(&mut scene, 20.0, 0.0);
        scene.set_player_body(Some(body)).unwrap();
        let killer = actor(&mut scene, 40.0, 50.0);
        assert!(
            !scene.is_visible(body).unwrap(),
            "the eyes are inside it, so it is out of the frame"
        );

        scene.hit_entity(killer, body).unwrap();

        assert_eq!(scene.player_body(), None);
        assert_eq!(scene.node_count(), 2, "the corpse is still lying there");
        assert!(
            scene.is_visible(body).unwrap(),
            "and it comes back into the frame with the eyes gone"
        );
    }

    #[test]
    fn a_plain_object_is_neither_a_weapon_nor_a_target() {
        let mut scene = Scene::new();
        let pillar = scene.create("cube", None, None).unwrap();
        let attacker = actor(&mut scene, 40.0, 7.0);

        assert!(matches!(
            scene.hit_entity(pillar, attacker),
            Err(SceneError::MissingComponent)
        ));
        assert!(matches!(
            scene.hit_entity(attacker, pillar),
            Err(SceneError::MissingComponent)
        ));
        assert!(matches!(
            scene.alive(pillar),
            Err(SceneError::MissingComponent)
        ));
    }
}
