//! Lights as things standing in the world.
//!
//! A light is a node, not a setting: it is written with `carbine.new("Light")`, hung
//! on the branch whose lamp it is, moved with the world around it and destroyed like
//! anything else. That is what an editor needs - a light someone can pick up and drag
//! - and it is what a game needs to put a light on a thing that walks.
//!
//! The sun is one of these nodes too. A fresh world holds a single light called `Sun`,
//! and everything a script writes about the backdrop's direction is a door onto it:
//! `sky.sun` still answers with where the light comes from, and turning the sun node
//! answers with the same thing from the other side.
//!
//! What a light carries is what every other object carries - a colour, a transparency,
//! a place and a turning - plus three of its own: how bright it is, how far its light
//! reaches, and how it lays its edges.

use bevy_ecs::prelude::*;
use glam::{Mat4, Quat, Vec3};

use crate::atmosphere::LightType;
use crate::draw::local_matrix;
use crate::motion::Motion;
use crate::node::{
    Children, MAX_ALPHA, NodeId, NodeKind, NodeName, NodeTexture, ObjectColor, Opacity, Parent,
    Scene, SceneError, TextureTiling, Transform, Visible,
};

/// The kind word `carbine.new()` takes for a light. It is not a shape the geometry
/// knows, so it is not in [`crate::primitives`]: a light draws nothing.
pub const KIND: &str = "light";

/// How many lamps a frame is lit by. This is the room the shader has in its register,
/// so a hundredth lamp only asks for a place in it: the ones nearest the eyes are the
/// ones drawn. See [`Scene::lamps`].
pub const MAX_LAMPS: usize = 8;

/// How far a light born without an answer reaches, in metres: the size of a room and
/// a bit of the street outside it.
const DEFAULT_REACH: f32 = 12.0;

/// What the sun is born coloured. A light that started out orange would tint the whole
/// world with a look nobody asked for, so a lamp is white until a script says otherwise.
const WHITE: Vec3 = Vec3::ONE;

/// Whether a light shines from everywhere in one direction, or out of one place.
///
/// Only the first of these casts, because only the first has one point of view to take
/// the world's picture from; a lamp gives light to whatever stands in its reach and
/// hides nothing behind anything.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LightShape {
    /// The world seen from one distance away: the sun. Its node's +z is the direction
    /// its light arrives from, so turning the node is aiming the sun.
    Sun,
    /// A lamp: a point in the world a light reaches out from.
    Point,
}

impl LightShape {
    /// The shapes a script reads from `light.shape`, in the order they are listed.
    pub const NAMES: &'static [&'static str] = &["point", "sun"];

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.trim().to_lowercase().as_str() {
            "sun" | "directional" => Self::Sun,
            "point" | "lamp" | "bulb" => Self::Point,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Sun => "sun",
            Self::Point => "point",
        }
    }
}

/// What a light node is, beyond the colour and the place it shares with everything.
#[derive(Clone, Copy, Component)]
pub(crate) struct Lamp {
    shape: LightShape,
    brightness: f32,
    reach: f32,
    /// How this light was told to lay its edges, or `None` for whatever the world is
    /// using. The same `auto` against a resolved answer that a light rung is.
    style: Option<LightType>,
}

impl Default for Lamp {
    fn default() -> Self {
        Self {
            shape: LightShape::Point,
            brightness: 1.0,
            reach: DEFAULT_REACH,
            style: None,
        }
    }
}

/// One lamp as the renderer is told to draw it: where it stands in the world, in
/// metres, and how much light it carries there.
#[derive(Clone, Copy, Debug)]
pub struct LampItem {
    pub node: NodeId,
    pub at: Vec3,
    pub color: [f32; 3],
    /// The lamp's own brightness, already taken down by how transparent it is.
    pub brightness: f32,
    pub reach: f32,
    /// Whether this lamp's falloff is laid in blocks rather than run straight.
    pub blocky: bool,
}

impl Scene {
    /// Creates a lamp. It draws nothing, hangs where it is told, and lights whatever
    /// stands within its reach.
    pub fn create_light(
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
                NodeName(name.unwrap_or_else(|| "Light".to_owned())),
                NodeKind(KIND.to_owned()),
                Transform::default(),
                ObjectColor(WHITE),
                Opacity::default(),
                NodeTexture::default(),
                TextureTiling::default(),
                Visible::default(),
                Motion::default(),
                Parent(parent_id),
                Children::default(),
                Lamp::default(),
            ))
            .id();
        self.insert(id, entity);
        self.attach(id, parent_id);

        Ok(id)
    }

    pub fn is_light(&self, id: NodeId) -> Result<bool, SceneError> {
        let entity = self.entity(id)?;
        Ok(self.world.get::<Lamp>(entity).is_some())
    }

    /// Whether a light shines from one direction or out of one place. Read, not
    /// written: a second sun would need a second picture of the world to cast out of,
    /// so the one the world is lit by stays the one the scene was born with.
    pub fn light_shape(&self, id: NodeId) -> Result<LightShape, SceneError> {
        Ok(self.lamp(id)?.shape)
    }

    /// How much light a lamp carries at its own place. One is what a lamp was born
    /// with; past that it is brighter, and at nil it gives nothing.
    pub fn brightness(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.lamp(id)?.brightness)
    }

    /// What a lamp carries is part of what it is, so a linked copy is written with it.
    pub fn set_brightness(&mut self, id: NodeId, brightness: f32) -> Result<(), SceneError> {
        let brightness = if brightness.is_finite() {
            brightness.max(0.0)
        } else {
            0.0
        };
        self.spread(id, move |scene, member| {
            scene.lamp_mut(member)?.brightness = brightness;
            Ok(())
        })
    }

    /// Metres of light the lamp throws out. Past the end of it the world is exactly as
    /// dark as it was with no lamp at all.
    pub fn reach(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.lamp(id)?.reach)
    }

    pub fn set_reach(&mut self, id: NodeId, reach: f32) -> Result<(), SceneError> {
        let reach = if reach.is_finite() {
            reach.max(0.0)
        } else {
            0.0
        };
        self.spread(id, move |scene, member| {
            scene.lamp_mut(member)?.reach = reach;
            Ok(())
        })
    }

    /// How this light lays its edges: its own answer if it has one, and whatever the
    /// world is drawn with if it does not.
    pub fn light_type(&self, id: NodeId) -> Result<LightType, SceneError> {
        Ok(self.lamp(id)?.style.unwrap_or(self.light_style()))
    }

    /// The type a script actually wrote, which may be nothing at all. An editor's
    /// checkbox reads this so it can show "follow the world" rather than the answer.
    pub fn written_light_type(&self, id: NodeId) -> Result<Option<LightType>, SceneError> {
        Ok(self.lamp(id)?.style)
    }

    pub fn set_light_type(&mut self, id: NodeId, style: LightType) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.lamp_mut(member)?.style = Some(style);
            Ok(())
        })
    }

    /// The type written the way a name is written, `"blocky"` and friends. `nil` is
    /// not a name a script has to know, so the world's own type is what a light with
    /// nothing written on it answers with.
    pub fn set_light_type_name(&mut self, id: NodeId, name: &str) -> Result<(), SceneError> {
        let style = LightType::from_name(name)
            .ok_or_else(|| SceneError::UnknownLightType(name.trim().to_owned()))?;
        self.set_light_type(id, style)
    }

    /// The lamps a frame is lit by: every point light the tree is holding, in the
    /// order the shader reads them, nearest the eyes first and no more than
    /// [`MAX_LAMPS`] of them.
    ///
    /// A lamp that is hidden, or hangs under something hidden, lights nothing: the same
    /// rule a body follows. A lamp nobody can reach is dropped rather than drawn, which
    /// is what keeps the register small in a world of a thousand of them.
    pub fn lamps(&self, eyes: Vec3) -> Vec<LampItem> {
        // A lamp that was never told a type of its own lays its edge as the world does.
        let the_world = self.light_style();
        let mut found: Vec<LampItem> = Vec::new();
        let mut stack = vec![(self.root(), Mat4::IDENTITY, false)];

        while let Some((id, inherited, hidden)) = stack.pop() {
            let Ok(entity) = self.entity(id) else {
                continue;
            };
            let Ok(transform) = self.transform(id) else {
                continue;
            };
            let hidden = hidden || !self.is_visible(id).unwrap_or(true);
            let model = inherited * local_matrix(transform);

            if !hidden
                && let Some(lamp) = self.world.get::<Lamp>(entity)
                && lamp.shape == LightShape::Point
                && lamp.brightness > 0.0
            {
                let color = self
                    .world
                    .get::<ObjectColor>(entity)
                    .map(|color| color.0)
                    .unwrap_or(WHITE);
                let alpha = self
                    .world
                    .get::<Opacity>(entity)
                    .map(|opacity| opacity.0 / MAX_ALPHA)
                    .unwrap_or(1.0);
                found.push(LampItem {
                    node: id,
                    at: model.transform_point3(Vec3::ZERO),
                    color: color.to_array(),
                    brightness: lamp.brightness * alpha,
                    reach: lamp.reach,
                    blocky: lamp.style.unwrap_or(the_world) == LightType::Blocky,
                });
            }

            if let Some(children) = self.world.get::<Children>(entity) {
                stack.extend(children.0.iter().rev().map(|child| (*child, model, hidden)));
            }
        }

        found.sort_by(|this, other| this.at.distance(eyes).total_cmp(&other.at.distance(eyes)));
        found.truncate(MAX_LAMPS);
        found
    }

    /// How many lamps the world is holding, whether or not this frame draws them -
    /// what a script that wondered whether its hundredth lamp made it asks.
    pub fn lamp_count(&self) -> usize {
        self.node_tree()
            .into_iter()
            .filter(|id| {
                self.component::<Lamp>(*id)
                    .is_ok_and(|lamp| lamp.shape == LightShape::Point && lamp.brightness > 0.0)
            })
            .count()
    }

    /// The sun's own colour, brightness and transparency folded into one tint for the
    /// light pass. A world with no sun node - one a script destroyed - is a world the
    /// sun leaves out, so it answers with nothing at all.
    pub fn sunlight(&self) -> [f32; 3] {
        let Some(sun) = self.sun else {
            return [0.0; 3];
        };
        if !self.shown_by_its_branch(sun) {
            return [0.0; 3];
        }

        let Ok(lamp) = self.lamp(sun) else {
            return [0.0; 3];
        };
        let color = self.color(sun).unwrap_or(WHITE);
        let alpha = self.opacity(sun).unwrap_or(MAX_ALPHA) / MAX_ALPHA;
        [
            color.x * lamp.brightness * alpha,
            color.y * lamp.brightness * alpha,
            color.z * lamp.brightness * alpha,
        ]
    }

    /// Whether a node and everything it hangs under is in the frame.
    fn shown_by_its_branch(&self, id: NodeId) -> bool {
        let mut current = Some(id);
        while let Some(id) = current {
            if !self.is_visible(id).unwrap_or(true) {
                return false;
            }
            current = self.parent(id).unwrap_or(None);
        }
        true
    }

    /// The sun node, which is what a script picks up to aim, dim or paint the daylight.
    pub fn sun(&self) -> Option<NodeId> {
        self.sun
    }

    /// Adds the world's one sun: a light node hung on the root and pointed wherever the
    /// backdrop is lit for. A scene is born with one and `reset` puts it back.
    ///
    /// It is a lamp like any other with one difference: it shines from one direction
    /// instead of out of one place, so it is the light the world casts its shades by
    /// and the only one of its shape the scene holds.
    pub(crate) fn make_sun(&mut self) {
        let Ok(id) = self.create_light(Some("Sun".to_owned()), None) else {
            return;
        };
        let direction = self.sky.sun;
        let _ = self
            .lamp_mut(id)
            .map(|mut lamp| lamp.shape = LightShape::Sun);
        self.sun = Some(id);
        let _ = self.point_sun(id, direction);
    }

    /// Turns a sun node so its light arrives from `direction`. The node's own +z is the
    /// way up, so an untouched rotation points straight at the sky.
    pub(crate) fn point_sun(&mut self, id: NodeId, direction: Vec3) -> Result<(), SceneError> {
        let rotation = Quat::from_rotation_arc(Vec3::Z, direction);
        self.component_mut::<Transform>(id)?.rotation = rotation;
        Ok(())
    }

    /// Called after anything has written a node's turning: when it was the sun's own,
    /// the direction the backdrop is lit for moves with it.
    pub(crate) fn sun_aimed_from(&mut self, id: NodeId) {
        if self.sun != Some(id) {
            return;
        }
        let Ok(rotation) = self.rotation(id) else {
            return;
        };
        let direction = rotation * Vec3::Z;
        if direction.length_squared() > 1e-12 {
            self.sky.sun = direction.normalize();
        }
    }

    pub(crate) fn lamp(&self, id: NodeId) -> Result<&Lamp, SceneError> {
        self.component::<Lamp>(id)
            .map_err(|_| SceneError::NotALight)
    }

    pub(crate) fn lamp_mut(&mut self, id: NodeId) -> Result<Mut<'_, Lamp>, SceneError> {
        let entity = self.entity(id)?;
        self.world
            .get_mut::<Lamp>(entity)
            .ok_or(SceneError::NotALight)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::VecField;

    #[test]
    fn a_world_is_born_with_one_sun_and_nothing_else() {
        let scene = Scene::new();
        let sun = scene.sun().expect("a fresh scene has a sun");

        assert_eq!(scene.node_count(), 0, "the sun is not authored content");
        assert_eq!(scene.name(sun).unwrap(), "Sun");
        assert_eq!(scene.kind(sun).unwrap(), KIND);
        assert!(scene.is_light(sun).unwrap());
        assert_eq!(scene.light_shape(sun).unwrap(), LightShape::Sun);
        assert_eq!(scene.children(scene.root()).unwrap(), vec![sun]);
        assert_eq!(scene.color(sun).unwrap(), WHITE);
        assert_eq!(scene.brightness(sun).unwrap(), 1.0);
    }

    #[test]
    fn the_sun_and_the_backdrop_are_the_same_direction_both_ways() {
        let mut scene = Scene::new();
        let sun = scene.sun().unwrap();

        scene.set_sky_sun(Vec3::X);
        assert_eq!(scene.sky().sun, Vec3::X);
        let turned = scene.rotation(sun).unwrap() * Vec3::Z;
        assert!((turned - Vec3::X).length() < 1e-5, "{turned}");

        // ... and the other way: turning the node aims the daylight.
        scene
            .set_vec_field(sun, VecField::RotationEuler, Vec3::new(0.0, 90.0, 0.0))
            .unwrap();
        let aimed = scene.sky().sun;
        assert!((aimed - Vec3::X).length() < 1e-5, "{aimed}");

        scene.set_rotation(sun, Quat::IDENTITY).unwrap();
        assert!((scene.sky().sun - Vec3::Z).length() < 1e-5);
    }

    #[test]
    fn a_lamp_is_written_as_a_node_and_carries_its_own_knobs() {
        let mut scene = Scene::new();
        let lamp = scene.create_light(None, None).unwrap();

        assert_eq!(scene.name(lamp).unwrap(), "Light");
        assert_eq!(scene.kind(lamp).unwrap(), KIND);
        assert_eq!(scene.light_shape(lamp).unwrap(), LightShape::Point);
        assert_eq!(scene.brightness(lamp).unwrap(), 1.0);
        assert_eq!(scene.reach(lamp).unwrap(), DEFAULT_REACH);
        assert_eq!(scene.color(lamp).unwrap(), WHITE);
        assert_eq!(scene.node_count(), 1, "a lamp is authored content");

        scene.set_brightness(lamp, 2.5).unwrap();
        scene.set_reach(lamp, 30.0).unwrap();
        assert_eq!(scene.brightness(lamp).unwrap(), 2.5);
        assert_eq!(scene.reach(lamp).unwrap(), 30.0);

        scene.set_brightness(lamp, -4.0).unwrap();
        assert_eq!(scene.brightness(lamp).unwrap(), 0.0);
        scene.set_reach(lamp, -1.0).unwrap();
        assert_eq!(scene.reach(lamp).unwrap(), 0.0);
    }

    #[test]
    fn a_lamp_hung_on_a_body_moves_with_it() {
        let mut scene = Scene::new();
        let body = scene.create("cube", None, None).unwrap();
        let lamp = scene.create_light(None, Some(body)).unwrap();

        scene
            .set_vec_field(body, VecField::Position, Vec3::new(4.0, 0.0, 1.0))
            .unwrap();
        scene
            .set_vec_field(lamp, VecField::Position, Vec3::new(0.0, 0.0, 2.0))
            .unwrap();

        let items = scene.lamps(Vec3::ZERO);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].at, Vec3::new(4.0, 0.0, 3.0));
        assert_eq!(items[0].node, lamp);
    }

    #[test]
    fn the_lamps_nearest_the_eyes_are_the_ones_drawn() {
        let mut scene = Scene::new();
        for index in 0..(MAX_LAMPS + 4) {
            let lamp = scene.create_light(None, None).unwrap();
            scene
                .set_vec_field(lamp, VecField::Position, Vec3::X * index as f32)
                .unwrap();
        }

        let items = scene.lamps(Vec3::ZERO);
        assert_eq!(items.len(), MAX_LAMPS);
        assert_eq!(items[0].at, Vec3::ZERO, "the nearest one is first");
        assert_eq!(scene.lamp_count(), MAX_LAMPS + 4);

        let items = scene.lamps(Vec3::X * 20.0);
        assert_eq!(
            items[0].at,
            Vec3::X * (MAX_LAMPS + 3) as f32,
            "the row runs from 0 upwards, so from down the row its far end is the nearest light"
        );
    }

    #[test]
    fn a_hidden_or_a_dark_lamp_lights_nothing() {
        let mut scene = Scene::new();
        let lamp = scene.create_light(None, None).unwrap();

        scene.set_brightness(lamp, 0.0).unwrap();
        assert!(scene.lamps(Vec3::ZERO).is_empty());

        scene.set_brightness(lamp, 1.0).unwrap();
        scene.set_visible(lamp, false).unwrap();
        assert!(scene.lamps(Vec3::ZERO).is_empty());

        scene.set_visible(lamp, true).unwrap();
        scene.set_opacity(lamp, 0.0).unwrap();
        assert_eq!(scene.lamps(Vec3::ZERO)[0].brightness, 0.0);
    }

    #[test]
    fn a_lamp_names_its_own_type_or_follows_the_world() {
        let mut scene = Scene::new();
        let lamp = scene.create_light(None, None).unwrap();
        let body = scene.create("cube", None, None).unwrap();

        assert_eq!(scene.written_light_type(lamp).unwrap(), None);
        assert_eq!(scene.light_type(lamp).unwrap(), LightType::Straight);

        scene.set_light_type_name(lamp, "blocky").unwrap();
        assert_eq!(scene.light_type(lamp).unwrap(), LightType::Blocky);
        assert!(scene.lamps(Vec3::ZERO)[0].blocky);

        scene.set_light_style(LightType::Blocky);
        assert_eq!(
            scene.light_type(lamp).unwrap(),
            LightType::Blocky,
            "its own answer outranks the world's"
        );

        assert!(matches!(
            scene.set_light_type_name(lamp, "wobbly"),
            Err(SceneError::UnknownLightType(_))
        ));
        assert!(matches!(
            scene.set_brightness(body, 1.0),
            Err(SceneError::NotALight)
        ));
    }

    #[test]
    fn a_lamps_light_is_the_colour_it_was_given_scaled_by_its_own_brightness() {
        let mut scene = Scene::new();
        let sun = scene.sun().unwrap();

        scene.set_color(sun, Vec3::new(1.0, 0.0, 0.0)).unwrap();
        scene.set_brightness(sun, 0.5).unwrap();
        assert_eq!(scene.sunlight(), [0.5, 0.0, 0.0]);

        scene.set_brightness(sun, 1.0).unwrap();
        scene.set_opacity(sun, MAX_ALPHA / 2.0).unwrap();
        assert_eq!(scene.sunlight(), [0.5, 0.0, 0.0]);

        scene.set_visible(sun, false).unwrap();
        assert_eq!(scene.sunlight(), [0.0; 3]);
    }

    #[test]
    fn destroying_the_sun_leaves_the_world_to_the_lamps() {
        let mut scene = Scene::new();
        let sun = scene.sun().unwrap();
        let lamp = scene.create_light(None, None).unwrap();

        scene.destroy(sun).unwrap();
        assert_eq!(scene.sun(), None);
        assert_eq!(scene.sunlight(), [0.0; 3]);
        assert_eq!(scene.lamp_count(), 1);
        assert_eq!(scene.node_count(), 1, "what is left is what was authored");

        // The backdrop still knows which way it is lit for; there is just nothing
        // standing there to shine.
        scene.set_sky_sun(Vec3::X);
        assert_eq!(scene.sky().sun, Vec3::X);
        assert!(matches!(
            scene.is_light(sun),
            Err(SceneError::NodeDestroyed)
        ));
        assert!(scene.is_light(lamp).unwrap());
    }

    #[test]
    fn clearing_the_scene_leaves_the_sun_standing() {
        let mut scene = Scene::new();
        let sun = scene.sun().unwrap();
        scene.create("cube", None, None).unwrap();
        let lamp = scene.create_light(None, None).unwrap();

        scene.clear();

        assert_eq!(scene.node_count(), 0);
        assert_eq!(scene.sun(), Some(sun));
        assert!(scene.is_light(sun).unwrap());
        assert_eq!(scene.name(lamp), Err(SceneError::NodeDestroyed));
    }
}
