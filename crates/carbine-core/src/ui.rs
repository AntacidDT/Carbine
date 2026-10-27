use bevy_ecs::prelude::*;
use glam::{Mat4, Vec3};

use crate::mesh::{MeshData, MeshHandle};
use crate::node::{Children, NodeId, NodeMesh, ObjectColor, Scene, SceneError};

/// Alpha is an eight bit count: 255 is opaque and 0 is invisible.
pub const MAX_ALPHA: f32 = 255.0;

/// A screen space overlay node. UI is laid out in pixels from the viewport's
/// top left corner and drawn over the world, so its `position` and `size` are
/// pixels rather than meters.
#[derive(Component)]
pub(crate) struct Ui {
    pub(crate) corner_radius: f32,
    pub(crate) sides: u32,
    pub(crate) outline: f32,
    pub(crate) alpha: f32,
}

impl Scene {
    /// Creates a UI element. The kind picks a default polygon; every shape is
    /// still overridable through `set_polygon_sides`.
    pub fn create_ui(
        &mut self,
        kind: &str,
        name: Option<String>,
        parent: Option<NodeId>,
    ) -> Result<NodeId, SceneError> {
        let id = self.create(kind, name, parent)?;
        let entity = self.entity(id)?;
        self.world.entity_mut(entity).insert(Ui {
            corner_radius: 0.0,
            sides: default_sides(kind),
            outline: 0.0,
            alpha: MAX_ALPHA,
        });
        // A UI element is pinned to the screen; gravity has nothing to do with it.
        self.set_anchored(id, true)?;
        Ok(id)
    }

    pub fn is_ui(&self, id: NodeId) -> Result<bool, SceneError> {
        let entity = self.entity(id)?;
        Ok(self.world.get::<Ui>(entity).is_some())
    }

    /// Corner radius in pixels. Kept exactly as given: a shape too round for its
    /// own size is rounded down by the renderer, not by the stored value.
    pub fn corner_radius(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.ui(id)?.corner_radius)
    }

    pub fn set_corner_radius(&mut self, id: NodeId, radius: f32) -> Result<(), SceneError> {
        self.ui_mut(id)?.corner_radius = radius;
        Ok(())
    }

    /// The number of polygon sides. Four fills the `size` box; any other count is
    /// a regular polygon whose points touch the box's shorter axis, and a high
    /// count reads as a circle.
    pub fn polygon_sides(&self, id: NodeId) -> Result<u32, SceneError> {
        Ok(self.ui(id)?.sides)
    }

    pub fn set_polygon_sides(&mut self, id: NodeId, sides: u32) -> Result<(), SceneError> {
        self.ui_mut(id)?.sides = sides;
        Ok(())
    }

    /// Outline thickness in pixels. Zero leaves the element filled solid.
    pub fn outline(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.ui(id)?.outline)
    }

    pub fn set_outline(&mut self, id: NodeId, thickness: f32) -> Result<(), SceneError> {
        self.ui_mut(id)?.outline = thickness;
        Ok(())
    }

    /// Opacity from 0 (invisible) to 255 (opaque).
    pub fn alpha(&self, id: NodeId) -> Result<f32, SceneError> {
        Ok(self.ui(id)?.alpha)
    }

    pub fn set_alpha(&mut self, id: NodeId, alpha: f32) -> Result<(), SceneError> {
        self.ui_mut(id)?.alpha = alpha.clamp(0.0, MAX_ALPHA);
        Ok(())
    }

    pub(crate) fn ui(&self, id: NodeId) -> Result<&Ui, SceneError> {
        self.component::<Ui>(id).map_err(|_| SceneError::NotAUi)
    }

    pub(crate) fn ui_mut(&mut self, id: NodeId) -> Result<Mut<'_, Ui>, SceneError> {
        let entity = self.entity(id)?;
        self.world.get_mut::<Ui>(entity).ok_or(SceneError::NotAUi)
    }

    /// Walks the tree collecting UI elements in document order, the order they
    /// should be painted in.
    pub fn ui_items(&self) -> Vec<UiItem<'_>> {
        let mut items = Vec::new();
        let mut stack = vec![(self.root(), Mat4::IDENTITY)];

        while let Some((id, inherited)) = stack.pop() {
            let Ok(entity) = self.entity(id) else {
                continue;
            };
            let Ok(transform) = self.transform(id) else {
                continue;
            };
            // A panel's size never scales its children the way it would in the
            // world: an element's position is its offset from its parent in
            // pixels, and its size is always its own.
            let layout =
                Mat4::from_translation(transform.position) * Mat4::from_quat(transform.rotation);
            let model = inherited * layout * Mat4::from_scale(transform.scale);

            if let (Some(mesh), Some(ui)) = (
                self.world.get::<NodeMesh>(entity),
                self.world.get::<Ui>(entity),
            ) {
                let color = self
                    .world
                    .get::<ObjectColor>(entity)
                    .map(|color| color.0)
                    .unwrap_or(Vec3::ONE);
                let size = transform.scale * mesh.data.natural_extent();
                items.push(UiItem {
                    node: id,
                    handle: mesh.handle,
                    mesh: &mesh.data,
                    model,
                    color: [color.x, color.y, color.z, ui.alpha / MAX_ALPHA],
                    size: [size.x, size.y],
                    corner_radius: ui.corner_radius,
                    sides: ui.sides.max(3) as f32,
                    outline: ui.outline,
                });
            }

            if let Some(children) = self.world.get::<Children>(entity) {
                let children_inherit = inherited * layout;
                stack.extend(
                    children
                        .0
                        .iter()
                        .rev()
                        .map(|child| (*child, children_inherit)),
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
    pub model: Mat4,
    /// sRGB colour and alpha in 0..1.
    pub color: [f32; 4],
    /// Width and height in pixels.
    pub size: [f32; 2],
    pub corner_radius: f32,
    pub sides: f32,
    pub outline: f32,
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
    use glam::Vec4;

    #[test]
    fn a_ui_element_is_a_node_with_overlay_state() {
        let mut scene = Scene::new();
        let button = scene.create_ui("square", None, None).unwrap();

        assert!(scene.is_ui(button).unwrap());
        assert_eq!(scene.name(button).unwrap(), "Square");
        assert_eq!(scene.parent(button).unwrap(), Some(scene.root()));
        assert_eq!(scene.corner_radius(button).unwrap(), 0.0);
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
    fn a_world_object_cannot_take_ui_properties() {
        let mut scene = Scene::new();
        let card = scene.create("rectangle", None, None).unwrap();

        assert!(matches!(
            scene.set_corner_radius(card, 8.0),
            Err(SceneError::NotAUi)
        ));
        assert!(matches!(
            scene.set_alpha(scene.root(), 10.0),
            Err(SceneError::NotAUi)
        ));
    }

    #[test]
    fn ui_properties_round_trip_without_being_rewritten() {
        let mut scene = Scene::new();
        let button = scene.create_ui("square", None, None).unwrap();

        scene.set_corner_radius(button, 49.0).unwrap();
        scene.set_polygon_sides(button, 7).unwrap();
        scene.set_outline(button, 3.5).unwrap();
        assert_eq!(scene.corner_radius(button).unwrap(), 49.0);
        assert_eq!(scene.polygon_sides(button).unwrap(), 7);
        assert_eq!(scene.outline(button).unwrap(), 3.5);

        // A radius bigger than the shape is legal to store; the renderer clamps.
        scene.set_corner_radius(button, 4000.0).unwrap();
        assert_eq!(scene.corner_radius(button).unwrap(), 4000.0);

        scene.set_corner_radius(button, -6.0).unwrap();
        assert_eq!(scene.corner_radius(button).unwrap(), -6.0);
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
        scene.set_corner_radius(button, 9.0).unwrap();
        scene.set_alpha(button, 64.0).unwrap();

        let items = scene.ui_items();
        assert_eq!(items.len(), 1, "world objects stay out of the UI pass");
        assert_eq!(items[0].node, button);
        assert_eq!(items[0].size, [40.0, 40.0]);
        assert_eq!(items[0].corner_radius, 9.0);
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
