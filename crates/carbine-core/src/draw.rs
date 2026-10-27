use glam::{Mat4, Vec3};

use crate::mesh::{MeshData, MeshHandle};
use crate::node::{
    Children, MAX_ALPHA, NodeId, NodeMesh, NodeTexture, ObjectColor, Opacity, Scene, Transform,
};
use crate::ui::Ui;

/// One drawable object with its hierarchy-resolved transform in meters.
pub struct DrawItem<'a> {
    pub node: NodeId,
    pub handle: MeshHandle,
    pub mesh: &'a MeshData,
    pub model: Mat4,
    pub color: [f32; 3],
    /// Alpha in 0..1, ready for blending.
    pub alpha: f32,
    /// The image the renderer should skin this object with, by path.
    pub texture: Option<&'a str>,
}

impl Scene {
    /// Walks the scene from the root, composing global transforms on the way
    /// down. Nodes without geometry (the root, bodiless entities) are skipped, and
    /// a hidden node takes its children out of the frame with it.
    pub fn draw_items(&self) -> Vec<DrawItem<'_>> {
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
            let model = inherited * local_matrix(transform);

            if !hidden
                && let Some(mesh) = self.world.get::<NodeMesh>(entity)
                // UI is measured in pixels and belongs to the overlay pass, not
                // to the world.
                && self.world.get::<Ui>(entity).is_none()
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
                let texture = self
                    .world
                    .get::<NodeTexture>(entity)
                    .and_then(|texture| texture.0.as_deref());
                items.push(DrawItem {
                    node: id,
                    handle: mesh.handle,
                    mesh: &mesh.data,
                    model,
                    color: color.to_array(),
                    alpha: alpha / MAX_ALPHA,
                    texture,
                });
            }

            if let Some(children) = self.world.get::<Children>(entity) {
                stack.extend(children.0.iter().rev().map(|child| (*child, model, hidden)));
            }
        }

        items
    }
}

pub(crate) fn local_matrix(transform: Transform) -> Mat4 {
    Mat4::from_translation(transform.position)
        * Mat4::from_quat(transform.rotation)
        * Mat4::from_scale(transform.scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::VecField;
    use glam::Vec4;

    #[test]
    fn draw_items_skip_the_root_and_bodiless_entities() {
        let mut scene = Scene::new();
        let card = scene.create("rectangle", None, None).unwrap();
        let controller = scene
            .spawn_entity(None, None, None, None, 1.0, 0.0)
            .unwrap();

        let items = scene.draw_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].node, card);
        assert_ne!(items[0].node, controller);
    }

    #[test]
    fn a_hidden_node_and_everything_under_it_leave_the_frame() {
        let mut scene = Scene::new();
        let parent = scene.create("cube", None, None).unwrap();
        let child = scene.create("cube", None, Some(parent)).unwrap();
        let other = scene.create("cube", None, None).unwrap();

        assert!(scene.is_visible(parent).unwrap());
        assert_eq!(scene.draw_items().len(), 3);

        scene.set_visible(parent, false).unwrap();
        let nodes: Vec<_> = scene.draw_items().iter().map(|item| item.node).collect();
        assert_eq!(nodes, vec![other]);
        assert!(
            !nodes.contains(&child),
            "a hidden panel hides its children too"
        );

        scene.set_visible(parent, true).unwrap();
        assert_eq!(scene.draw_items().len(), 3);
    }

    #[test]
    fn child_transforms_compose_with_their_parents() {
        let mut scene = Scene::new();
        let parent = scene.create("rectangle", None, None).unwrap();
        let child = scene.create("rectangle", None, Some(parent)).unwrap();

        scene
            .set_vec_field(parent, VecField::Position, Vec3::new(1.0, 0.0, 0.0))
            .unwrap();
        scene
            .set_vec_field(child, VecField::Position, Vec3::new(0.0, 2.0, 0.0))
            .unwrap();

        let items = scene.draw_items();
        let child = items.iter().find(|item| item.node == child).unwrap();
        assert_eq!(child.model.w_axis.trunc(), Vec4::new(1.0, 2.0, 0.0, 1.0));
    }

    #[test]
    fn an_objects_size_scales_its_geometry_in_meters() {
        let mut scene = Scene::new();
        let card = scene.create("rectangle", None, None).unwrap();
        scene
            .set_vec_field(card, VecField::Size, Vec3::new(3.0, 1.0, 1.0))
            .unwrap();

        let items = scene.draw_items();
        let extent = items[0].model.transform_vector3(Vec3::X);
        assert!((extent.x - 3.0).abs() < 1e-6);
    }

    #[test]
    fn the_mesh_handle_and_colour_reach_the_renderer() {
        let mut scene = Scene::new();
        let first = scene.create("rectangle", None, None).unwrap();
        let second = scene.create("rectangle", None, None).unwrap();
        scene
            .set_vec_field(first, VecField::Color, Vec3::new(1.0, 0.0, 0.0))
            .unwrap();
        scene.set_opacity(first, 128.0).unwrap();
        scene
            .set_texture(first, Some("assets/wood.png".to_owned()))
            .unwrap();

        let items = scene.draw_items();
        assert_ne!(items[0].handle, items[1].handle);
        assert_eq!(items[0].color, [1.0, 0.0, 0.0]);
        assert_eq!(items[0].texture, Some("assets/wood.png"));
        assert_eq!(second, items[1].node);

        let alpha = 128.0 / crate::node::MAX_ALPHA;
        assert!((items[0].alpha - alpha).abs() < 1e-6, "{}", items[0].alpha);
        assert_eq!(items[1].alpha, 1.0, "an untouched node stays opaque");
        assert_eq!(items[1].texture, None);
    }
}
