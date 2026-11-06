use bevy_ecs::prelude::*;
use glam::{Mat4, Vec3};

use crate::mesh::{MeshData, MeshHandle};
use crate::node::{Children, MAX_ALPHA, NodeId, NodeMesh, ObjectColor, Opacity, Scene, SceneError};
use crate::primitives;

/// A screen space overlay node. UI is laid out in pixels from the viewport's
/// top left corner and drawn over the world, so its `position` and `size` are
/// pixels rather than meters.
#[derive(Clone, Copy, Component)]
pub(crate) struct Ui {
    pub(crate) corner: f32,
    pub(crate) sides: u32,
    pub(crate) outline: f32,
}

/// Text configuration; glyph layout and font data belong to the renderer.
#[derive(Clone, Component)]
pub struct UiText {
    pub content: String,
    pub font_size: f32,
    pub word_wrap: bool,
    pub width: f32,
    pub outline: bool,
    pub outline_width: f32,
    pub outline_color: Vec3,
}

impl Default for UiText {
    fn default() -> Self {
        Self {
            content: String::new(),
            font_size: 24.0,
            word_wrap: false,
            width: 400.0,
            outline: false,
            outline_width: 1.0,
            outline_color: Vec3::ZERO,
        }
    }
}

impl Scene {
    /// Creates a UI element from a case-insensitive shape or text kind.
    pub fn create_ui(
        &mut self,
        kind: &str,
        name: Option<String>,
        parent: Option<NodeId>,
    ) -> Result<NodeId, SceneError> {
        let kind = kind.trim().to_lowercase();
        let data =
            primitives::ui_mesh(&kind).ok_or_else(|| SceneError::UnknownKind(kind.clone()))?;
        if let Some(parent) = parent {
            self.entity(parent)?;
            self.branch_allows(parent, true)?;
        }

        let id = self.spawn(&kind, name, parent, data);
        let entity = self.entity(id)?;
        self.world.entity_mut(entity).insert(Ui {
            corner: 0.0,
            sides: default_sides(&kind),
            outline: 0.0,
        });
        if kind == "text" {
            self.world.entity_mut(entity).insert(UiText::default());
        }
        // A UI element is pinned to the screen; gravity has nothing to do with it.
        self.set_anchored(id, true)?;
        Ok(id)
    }

    pub fn is_ui(&self, id: NodeId) -> Result<bool, SceneError> {
        let entity = self.entity(id)?;
        Ok(self.world.get::<Ui>(entity).is_some())
    }

    pub fn is_text(&self, id: NodeId) -> Result<bool, SceneError> {
        let entity = self.entity(id)?;
        Ok(self.world.get::<UiText>(entity).is_some())
    }

    pub fn text(&self, id: NodeId) -> Result<&UiText, SceneError> {
        let entity = self.entity(id)?;
        self.world.get::<UiText>(entity).ok_or(SceneError::NotAUi)
    }

    pub fn text_content(&self, id: NodeId) -> Result<&str, SceneError> {
        Ok(&self.text(id)?.content)
    }

    pub fn set_text_content(&mut self, id: NodeId, content: String) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.text_mut(member)?.content.clone_from(&content);
            Ok(())
        })
    }

    pub fn text_size(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.text(id)?.font_size)
    }

    pub fn set_text_size(&mut self, id: NodeId, size: f32) -> Result<(), SceneError> {
        let size = finite_clamp(size, 1.0, 400.0);
        self.spread(id, move |scene, member| {
            scene.text_mut(member)?.font_size = size;
            Ok(())
        })
    }

    pub fn text_word_wrap(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.text(id)?.word_wrap)
    }

    pub fn set_text_word_wrap(&mut self, id: NodeId, word_wrap: bool) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.text_mut(member)?.word_wrap = word_wrap;
            Ok(())
        })
    }

    pub fn text_width(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.text(id)?.width)
    }

    pub fn set_text_width(&mut self, id: NodeId, width: f32) -> Result<(), SceneError> {
        let width = finite_clamp(width, 1.0, 16384.0);
        self.spread(id, move |scene, member| {
            scene.text_mut(member)?.width = width;
            Ok(())
        })
    }

    pub fn text_outline(&self, id: NodeId) -> Result<bool, SceneError> {
        Ok(self.text(id)?.outline)
    }

    pub fn set_text_outline(&mut self, id: NodeId, outline: bool) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.text_mut(member)?.outline = outline;
            Ok(())
        })
    }

    pub fn text_outline_width(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.text(id)?.outline_width)
    }

    pub fn set_text_outline_width(&mut self, id: NodeId, width: f32) -> Result<(), SceneError> {
        let width = finite_clamp(width, 0.0, 32.0);
        self.spread(id, move |scene, member| {
            scene.text_mut(member)?.outline_width = width;
            Ok(())
        })
    }

    pub fn text_outline_color(&self, id: NodeId) -> Result<Vec3, SceneError> {
        Ok(self.text(id)?.outline_color)
    }

    pub fn set_text_outline_color(&mut self, id: NodeId, color: Vec3) -> Result<(), SceneError> {
        self.text(id)?;
        if !color.is_finite() {
            return Err(SceneError::BadColor(color.to_string()));
        }
        self.spread(id, move |scene, member| {
            scene.text_mut(member)?.outline_color = color;
            Ok(())
        })
    }

    fn text_mut(&mut self, id: NodeId) -> Result<Mut<'_, UiText>, SceneError> {
        let entity = self.entity(id)?;
        self.world
            .get_mut::<UiText>(entity)
            .ok_or(SceneError::NotAUi)
    }

    /// How round the corners are, in pixels. Kept exactly as given: a shape too
    /// round for its own size is rounded down by the renderer, not by the stored
    /// value. The shape of the element is part of what it is, so every linked copy
    /// takes the write with it.
    pub fn corner(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.ui(id)?.corner)
    }

    pub fn set_corner(&mut self, id: NodeId, corner: f32) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.ui_mut(member)?.corner = corner;
            Ok(())
        })
    }

    /// The number of polygon sides. Four fills the `size` box; any other count is
    /// a regular polygon whose points touch the box's shorter axis, and a high
    /// count reads as a circle.
    pub fn polygon_sides(&self, id: NodeId) -> Result<u32, SceneError> {
        Ok(self.ui(id)?.sides)
    }

    pub fn set_polygon_sides(&mut self, id: NodeId, sides: u32) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.ui_mut(member)?.sides = sides;
            Ok(())
        })
    }

    /// Outline thickness in pixels. Zero leaves the element filled solid.
    pub fn outline(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.ui(id)?.outline)
    }

    pub fn set_outline(&mut self, id: NodeId, thickness: f32) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| {
            scene.ui_mut(member)?.outline = thickness;
            Ok(())
        })
    }

    /// Opacity from 0 (invisible) to 255 (opaque). This is the node's
    /// `transparent` value, which a world object shares.
    pub fn alpha(&self, id: NodeId) -> Result<f32, SceneError> {
        self.opacity(id)
    }

    pub fn set_alpha(&mut self, id: NodeId, alpha: f32) -> Result<(), SceneError> {
        self.spread(id, move |scene, member| scene.fade(member, alpha))
    }

    pub(crate) fn ui(&self, id: NodeId) -> Result<&Ui, SceneError> {
        self.component::<Ui>(id).map_err(|_| SceneError::NotAUi)
    }

    pub(crate) fn ui_mut(&mut self, id: NodeId) -> Result<Mut<'_, Ui>, SceneError> {
        let entity = self.entity(id)?;
        self.world.get_mut::<Ui>(entity).ok_or(SceneError::NotAUi)
    }

    /// Walks the tree collecting UI elements in document order, the order they
    /// should be painted in. A hidden element hides everything below it too.
    pub fn ui_items(&self) -> Vec<UiItem<'_>> {
        let mut items = Vec::new();
        let mut stack = vec![(self.root(), Mat4::IDENTITY, false)];

        while let Some((id, inherited, hidden)) = stack.pop() {
            let Ok(entity) = self.entity(id) else {
                continue;
            };
            let Ok(transform) = self.transform(id) else {
                continue;
            };
            let hidden = hidden || !self.is_visible(id).unwrap_or(true);
            // A panel's size never scales its children the way it would in the
            // world: an element's position is its offset from its parent in
            // pixels, and its size is always its own.
            let layout =
                Mat4::from_translation(transform.position) * Mat4::from_quat(transform.rotation);
            let model = inherited * layout * Mat4::from_scale(transform.scale);

            if !hidden
                && let (Some(mesh), Some(ui)) = (
                    self.world.get::<NodeMesh>(entity),
                    self.world.get::<Ui>(entity),
                )
            {
                let color = self
                    .world
                    .get::<ObjectColor>(entity)
                    .map(|color| color.0)
                    .unwrap_or(Vec3::ONE);
                let alpha = self
                    .world
                    .get::<Opacity>(entity)
                    .map(|opacity| opacity.0)
                    .unwrap_or(MAX_ALPHA);
                let size = transform.scale * mesh.data.natural_extent();
                items.push(UiItem {
                    node: id,
                    handle: mesh.handle,
                    mesh: &mesh.data,
                    model,
                    color: [color.x, color.y, color.z, alpha / MAX_ALPHA],
                    size: [size.x, size.y],
                    corner: ui.corner,
                    sides: ui.sides.max(3) as f32,
                    outline: ui.outline,
                    text: self.world.get::<UiText>(entity),
                });
            }

            if let Some(children) = self.world.get::<Children>(entity) {
                let children_inherit = inherited * layout;
                stack.extend(
                    children
                        .0
                        .iter()
                        .rev()
                        .map(|child| (*child, children_inherit, hidden)),
                );
            }
        }

        items
    }
}

/// One UI element, resolved into pixels ready for the overlay pass.
pub struct UiItem<'a> {
    pub node: NodeId,
    pub handle: MeshHandle,
    pub mesh: &'a MeshData,
    /// Text uses the local origin as its top-left, with local scale applied to glyphs.
    pub model: Mat4,
    /// sRGB colour and alpha in 0..1.
    pub color: [f32; 4],
    /// Quad width and height in pixels; text layout uses its separate text width.
    pub size: [f32; 2],
    pub corner: f32,
    pub sides: f32,
    pub outline: f32,
    pub text: Option<&'a UiText>,
}

fn finite_clamp(value: f32, min: f32, max: f32) -> f32 {
    // NaN takes the minimum; infinities take their respective bounds.
    if value.is_nan() {
        min
    } else {
        value.clamp(min, max)
    }
}

fn default_sides(kind: &str) -> u32 {
    match kind {
        "triangle" => 3,
        "hexagon" => 6,
        "circle" => 64,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::VecField;
    use glam::{Quat, Vec4};

    #[test]
    fn text_creation_normalizes_kinds_and_keeps_shape_components_separate() {
        let mut scene = Scene::new();
        for kind in ["text", "Text", " TEXT ", "\tTeXt\n"] {
            let id = scene.create_ui(kind, None, None).unwrap();
            assert!(scene.is_ui(id).unwrap());
            assert!(scene.is_text(id).unwrap());
            assert_eq!(scene.kind(id).unwrap(), "text");
            assert_eq!(scene.name(id).unwrap(), "Text");
            assert_eq!(scene.parent(id).unwrap(), Some(scene.root()));
            assert!(scene.is_anchored(id).unwrap());
            assert_eq!(scene.transform(id).unwrap().scale, Vec3::ONE);
            let text = scene.text(id).unwrap();
            assert_eq!(text.content, "");
            assert_eq!(text.font_size, 24.0);
            assert!(!text.word_wrap);
            assert_eq!(text.width, 400.0);
            assert!(!text.outline);
            assert_eq!(text.outline_width, 1.0);
            assert_eq!(text.outline_color, Vec3::ZERO);
        }
        for kind in primitives::UI_KINDS.iter().filter(|kind| **kind != "text") {
            let id = scene
                .create_ui(&format!(" {} ", kind.to_uppercase()), None, None)
                .unwrap();
            assert!(!scene.is_text(id).unwrap());
            assert_eq!(scene.kind(id).unwrap(), *kind);
            assert_eq!(scene.polygon_sides(id).unwrap(), default_sides(kind));
        }
        assert!(matches!(
            scene.create_ui(" unknown ", None, None),
            Err(SceneError::UnknownKind(kind)) if kind == "unknown"
        ));
        assert!(matches!(
            scene.create("Text", None, None),
            Err(SceneError::UnknownKind(_))
        ));
    }

    #[test]
    fn text_properties_round_trip_without_changing_placement_or_shape_state() {
        let mut scene = Scene::new();
        let id = scene.create_ui("text", Some("Label".into()), None).unwrap();
        scene
            .set_vec_field(id, VecField::Position, Vec3::new(32.0, 48.0, 0.0))
            .unwrap();
        scene
            .set_vec_field(id, VecField::Scale, Vec3::new(2.0, 3.0, 1.0))
            .unwrap();
        let before = scene.transform(id).unwrap();
        scene
            .set_text_content(id, "Hello, 世界\nSecond line".into())
            .unwrap();
        scene.set_text_size(id, 36.5).unwrap();
        scene.set_text_word_wrap(id, true).unwrap();
        scene.set_text_width(id, 512.5).unwrap();
        scene.set_text_outline(id, true).unwrap();
        scene.set_text_outline_width(id, 2.5).unwrap();
        scene
            .set_text_outline_color(id, Vec3::new(0.2, 0.4, 0.6))
            .unwrap();

        assert_eq!(scene.name(id).unwrap(), "Label");
        assert_eq!(scene.text_content(id).unwrap(), "Hello, 世界\nSecond line");
        assert_eq!(scene.text_size(id).unwrap(), 36.5);
        assert!(scene.text_word_wrap(id).unwrap());
        assert_eq!(scene.text_width(id).unwrap(), 512.5);
        assert!(scene.text_outline(id).unwrap());
        assert_eq!(scene.text_outline_width(id).unwrap(), 2.5);
        assert_eq!(
            scene.text_outline_color(id).unwrap(),
            Vec3::new(0.2, 0.4, 0.6)
        );
        assert_eq!(scene.outline(id).unwrap(), 0.0);
        assert_eq!(scene.polygon_sides(id).unwrap(), 4);
        let after = scene.transform(id).unwrap();
        assert_eq!(after.position, before.position);
        assert_eq!(after.rotation, before.rotation);
        assert_eq!(after.scale, before.scale);

        scene.set_text_content(id, String::new()).unwrap();
        scene.set_text_word_wrap(id, false).unwrap();
        scene.set_text_outline(id, false).unwrap();
        assert_eq!(scene.text_content(id).unwrap(), "");
        assert!(!scene.text_word_wrap(id).unwrap());
        assert!(!scene.text_outline(id).unwrap());
    }

    #[test]
    fn text_numeric_properties_have_finite_bounds() {
        let mut scene = Scene::new();
        let id = scene.create_ui("text", None, None).unwrap();
        for (input, size, width, outline) in [
            (-100.0, 1.0, 1.0, 0.0),
            (0.0, 1.0, 1.0, 0.0),
            (1.0, 1.0, 1.0, 1.0),
            (12.5, 12.5, 12.5, 12.5),
            (32.0, 32.0, 32.0, 32.0),
            (400.0, 400.0, 400.0, 32.0),
            (16384.0, 400.0, 16384.0, 32.0),
            (100000.0, 400.0, 16384.0, 32.0),
            (f32::NAN, 1.0, 1.0, 0.0),
            (f32::NEG_INFINITY, 1.0, 1.0, 0.0),
            (f32::INFINITY, 400.0, 16384.0, 32.0),
        ] {
            scene.set_text_size(id, input).unwrap();
            scene.set_text_width(id, input).unwrap();
            scene.set_text_outline_width(id, input).unwrap();
            assert_eq!(scene.text_size(id).unwrap(), size);
            assert_eq!(scene.text_width(id).unwrap(), width);
            assert_eq!(scene.text_outline_width(id).unwrap(), outline);
        }
    }

    #[test]
    fn text_outline_rejects_nonfinite_colors_without_changing_linked_state() {
        let mut scene = Scene::new();
        let id = scene.create_ui("text", None, None).unwrap();
        scene.set_text_outline_color(id, Vec3::ONE).unwrap();
        let twin = scene.link_duplicate(id, 1).unwrap()[0];
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for color in [
                Vec3::new(invalid, 0.0, 0.0),
                Vec3::new(0.0, invalid, 0.0),
                Vec3::new(0.0, 0.0, invalid),
            ] {
                assert!(matches!(
                    scene.set_text_outline_color(twin, color),
                    Err(SceneError::BadColor(_))
                ));
                assert_eq!(scene.text_outline_color(id).unwrap(), Vec3::ONE);
                assert_eq!(scene.text_outline_color(twin).unwrap(), Vec3::ONE);
            }
        }
    }

    #[test]
    fn text_accessors_distinguish_nontext_from_destroyed_nodes() {
        let mut scene = Scene::new();
        let world = scene.create("cube", None, None).unwrap();
        let shape = scene.create_ui("square", None, None).unwrap();
        let destroyed = scene.create_ui("text", None, None).unwrap();
        scene.destroy(destroyed).unwrap();
        for id in [scene.root(), world, shape, destroyed] {
            if id == destroyed {
                assert!(matches!(scene.is_text(id), Err(SceneError::NodeDestroyed)));
            } else {
                assert!(!scene.is_text(id).unwrap());
            }
            for result in [
                scene.text(id).map(|_| ()),
                scene.text_content(id).map(|_| ()),
                scene.text_size(id).map(|_| ()),
                scene.text_word_wrap(id).map(|_| ()),
                scene.text_width(id).map(|_| ()),
                scene.text_outline(id).map(|_| ()),
                scene.text_outline_width(id).map(|_| ()),
                scene.text_outline_color(id).map(|_| ()),
                scene.set_text_content(id, "ignored".into()),
                scene.set_text_size(id, f32::NAN),
                scene.set_text_word_wrap(id, true),
                scene.set_text_width(id, f32::INFINITY),
                scene.set_text_outline(id, true),
                scene.set_text_outline_width(id, f32::NEG_INFINITY),
                scene.set_text_outline_color(id, Vec3::ONE),
                scene.set_text_outline_color(id, Vec3::splat(f32::NAN)),
            ] {
                if id == destroyed {
                    assert!(matches!(result, Err(SceneError::NodeDestroyed)));
                } else {
                    assert!(matches!(result, Err(SceneError::NotAUi)));
                }
            }
        }
    }

    #[test]
    fn text_items_keep_top_left_layout_and_local_scale_without_world_geometry() {
        let mut scene = Scene::new();
        let world = scene.create("cube", None, None).unwrap();
        let panel = scene.create_ui("rectangle", None, None).unwrap();
        let label = scene.create_ui("Text", None, Some(panel)).unwrap();
        scene.set_text_content(label, "Label".into()).unwrap();
        scene.set_text_width(label, 800.0).unwrap();
        scene.set_alpha(label, 128.0).unwrap();
        scene.set_color(label, Vec3::new(0.2, 0.4, 0.6)).unwrap();
        let position = Vec3::new(100.0, 200.0, 0.0);
        let offset = Vec3::new(10.0, 20.0, 0.0);
        let scale = Vec3::new(2.0, 3.0, 1.0);
        let parent_rotation = Quat::from_rotation_z(0.5);
        let rotation = Quat::from_rotation_z(-0.25);
        scene
            .set_vec_field(panel, VecField::Position, position)
            .unwrap();
        scene
            .set_vec_field(panel, VecField::Size, Vec3::new(640.0, 480.0, 1.0))
            .unwrap();
        scene.set_rotation(panel, parent_rotation).unwrap();
        scene
            .set_vec_field(label, VecField::Position, offset)
            .unwrap();
        scene.set_vec_field(label, VecField::Scale, scale).unwrap();
        scene.set_rotation(label, rotation).unwrap();

        let items = scene.ui_items();
        assert_eq!(items.len(), 2);
        assert!(items[0].text.is_none());
        assert_eq!(items[0].size, [640.0, 480.0]);
        let item = &items[1];
        assert_eq!(item.node, label);
        assert!(std::ptr::eq(item.text.unwrap(), scene.text(label).unwrap()));
        assert_eq!(item.text.unwrap().content, "Label");
        assert_eq!(item.text.unwrap().width, 800.0);
        assert_eq!(item.color, [0.2, 0.4, 0.6, 128.0 / MAX_ALPHA]);
        let expected = Mat4::from_translation(position)
            * Mat4::from_quat(parent_rotation)
            * Mat4::from_translation(offset)
            * Mat4::from_quat(rotation)
            * Mat4::from_scale(scale);
        assert!(item.model.abs_diff_eq(expected, 1e-5));
        assert!(
            item.model
                .transform_point3(Vec3::ZERO)
                .abs_diff_eq(position + parent_rotation * offset, 1e-5,)
        );
        assert_eq!(
            scene
                .draw_items()
                .iter()
                .map(|item| item.node)
                .collect::<Vec<_>>(),
            vec![world]
        );
        assert!(matches!(
            scene.create_ui("text", None, Some(world)),
            Err(SceneError::CrossedTree)
        ));
        assert!(matches!(
            scene.create("cube", None, Some(label)),
            Err(SceneError::CrossedTree)
        ));
    }

    #[test]
    fn hidden_text_and_ancestors_hide_their_whole_ui_branch() {
        let mut scene = Scene::new();
        let panel = scene.create_ui("rectangle", None, None).unwrap();
        let label = scene.create_ui("text", None, Some(panel)).unwrap();
        let child = scene.create_ui("text", None, Some(label)).unwrap();
        let sibling = scene.create_ui("text", None, None).unwrap();
        let nodes = |scene: &Scene| {
            scene
                .ui_items()
                .iter()
                .map(|item| item.node)
                .collect::<Vec<_>>()
        };
        assert_eq!(nodes(&scene), vec![panel, label, child, sibling]);
        scene.set_visible(label, false).unwrap();
        assert_eq!(nodes(&scene), vec![panel, sibling]);
        scene.set_visible(panel, false).unwrap();
        scene.set_visible(label, true).unwrap();
        assert_eq!(nodes(&scene), vec![sibling]);
        scene.set_visible(panel, true).unwrap();
        assert_eq!(nodes(&scene), vec![panel, label, child, sibling]);
        scene.destroy(panel).unwrap();
        assert_eq!(nodes(&scene), vec![sibling]);
        assert!(matches!(scene.text(child), Err(SceneError::NodeDestroyed)));
        assert!(scene.draw_items().is_empty());
    }

    #[test]
    fn a_ui_element_is_a_node_with_overlay_state() {
        let mut scene = Scene::new();
        let button = scene.create_ui("square", None, None).unwrap();

        assert!(scene.is_ui(button).unwrap());
        assert_eq!(scene.name(button).unwrap(), "Square");
        assert_eq!(scene.parent(button).unwrap(), Some(scene.root()));
        assert_eq!(scene.corner(button).unwrap(), 0.0);
        assert_eq!(scene.polygon_sides(button).unwrap(), 4);
        assert_eq!(scene.outline(button).unwrap(), 0.0);
        assert_eq!(scene.alpha(button).unwrap(), MAX_ALPHA);
        assert!(scene.is_anchored(button).unwrap());
    }

    #[test]
    fn the_kind_sets_the_default_polygon() {
        let mut scene = Scene::new();
        let triangle = scene.create_ui("triangle", None, None).unwrap();
        let circle = scene.create_ui("circle", None, None).unwrap();
        let card = scene.create("rectangle", None, None).unwrap();

        assert_eq!(scene.polygon_sides(triangle).unwrap(), 3);
        assert_eq!(scene.polygon_sides(circle).unwrap(), 64);
        assert!(matches!(scene.polygon_sides(card), Err(SceneError::NotAUi)));
        assert!(!scene.is_ui(card).unwrap());
    }

    #[test]
    fn a_world_object_has_shape_free_ui_state() {
        let mut scene = Scene::new();
        let card = scene.create("cube", None, None).unwrap();

        // Shape knobs belong to the overlay, but transparency belongs to any node.
        assert!(matches!(
            scene.set_corner(card, 8.0),
            Err(SceneError::NotAUi)
        ));
        assert!(matches!(
            scene.set_polygon_sides(card, 5),
            Err(SceneError::NotAUi)
        ));
        assert!(matches!(scene.corner(card), Err(SceneError::NotAUi)));

        scene.set_alpha(card, 100.0).unwrap();
        assert_eq!(scene.alpha(card).unwrap(), 100.0);
    }

    #[test]
    fn ui_properties_round_trip_without_being_rewritten() {
        let mut scene = Scene::new();
        let button = scene.create_ui("square", None, None).unwrap();

        scene.set_corner(button, 49.0).unwrap();
        scene.set_polygon_sides(button, 7).unwrap();
        scene.set_outline(button, 3.5).unwrap();
        assert_eq!(scene.corner(button).unwrap(), 49.0);
        assert_eq!(scene.polygon_sides(button).unwrap(), 7);
        assert_eq!(scene.outline(button).unwrap(), 3.5);

        // A radius bigger than the shape is legal to store; the renderer clamps.
        scene.set_corner(button, 4000.0).unwrap();
        assert_eq!(scene.corner(button).unwrap(), 4000.0);

        scene.set_corner(button, -6.0).unwrap();
        assert_eq!(scene.corner(button).unwrap(), -6.0);
    }

    #[test]
    fn alpha_is_an_eight_bit_count() {
        let mut scene = Scene::new();
        let button = scene.create_ui("square", None, None).unwrap();

        scene.set_alpha(button, 0.0).unwrap();
        assert_eq!(scene.alpha(button).unwrap(), 0.0);

        scene.set_alpha(button, 128.0).unwrap();
        assert_eq!(scene.alpha(button).unwrap(), 128.0);

        scene.set_alpha(button, 900.0).unwrap();
        assert_eq!(scene.alpha(button).unwrap(), MAX_ALPHA);

        scene.set_alpha(button, -20.0).unwrap();
        assert_eq!(scene.alpha(button).unwrap(), 0.0);
    }

    #[test]
    fn ui_items_carry_pixels_to_the_renderer() {
        let mut scene = Scene::new();
        let card = scene.create("rectangle", None, None).unwrap();
        let button = scene.create_ui("square", None, None).unwrap();
        scene
            .set_vec_field(button, VecField::Position, Vec3::new(32.0, 48.0, 0.0))
            .unwrap();
        scene
            .set_vec_field(button, VecField::Size, Vec3::new(40.0, 40.0, 1.0))
            .unwrap();
        scene.set_corner(button, 9.0).unwrap();
        scene.set_alpha(button, 64.0).unwrap();

        let items = scene.ui_items();
        assert_eq!(items.len(), 1, "world objects stay out of the UI pass");
        assert_eq!(items[0].node, button);
        assert_eq!(items[0].size, [40.0, 40.0]);
        assert_eq!(items[0].corner, 9.0);
        assert_eq!(items[0].sides, 4.0);
        assert_eq!(items[0].color[3], 64.0 / MAX_ALPHA);
        assert_eq!(
            items[0].model.transform_point3(Vec3::new(0.5, 0.5, 0.0)),
            Vec3::new(52.0, 68.0, 0.0)
        );
        assert!(!scene.is_ui(card).unwrap());
        assert_eq!(
            scene.draw_items().len(),
            1,
            "UI stays out of the world pass"
        );
    }

    #[test]
    fn a_child_element_moves_with_its_parent_and_keeps_its_shape() {
        let mut scene = Scene::new();
        let panel = scene.create_ui("square", None, None).unwrap();
        let badge = scene
            .create_ui("circle", Some("Badge".into()), Some(panel))
            .unwrap();
        scene
            .set_vec_field(panel, VecField::Position, Vec3::new(10.0, 10.0, 0.0))
            .unwrap();
        scene
            .set_vec_field(panel, VecField::Size, Vec3::new(32.0, 32.0, 1.0))
            .unwrap();
        scene
            .set_vec_field(badge, VecField::Position, Vec3::new(8.0, 8.0, 0.0))
            .unwrap();
        scene
            .set_vec_field(badge, VecField::Size, Vec3::new(16.0, 16.0, 1.0))
            .unwrap();

        let items = scene.ui_items();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].node, badge);
        assert_eq!(items[1].sides, 64.0);
        assert_eq!(
            items[1].model.w_axis.trunc(),
            Vec4::new(18.0, 18.0, 0.0, 1.0)
        );
        assert_eq!(
            items[1].size,
            [16.0, 16.0],
            "a panel's size is not its child's"
        );
    }

    #[test]
    fn destroying_a_panel_takes_its_elements_with_it() {
        let mut scene = Scene::new();
        let panel = scene.create_ui("square", None, None).unwrap();
        let badge = scene.create_ui("circle", None, Some(panel)).unwrap();

        scene.destroy(panel).unwrap();
        assert_eq!(scene.ui_items().len(), 0);
        assert!(matches!(scene.is_ui(badge), Err(SceneError::NodeDestroyed)));
    }
}
