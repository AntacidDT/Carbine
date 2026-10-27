use glam::{Mat4, Vec3};

use crate::mesh::{MeshData, MeshHandle};
use crate::node::{Children, NodeId, NodeMesh, ObjectColor, Scene, Transform};

/// One drawable object with its hierarchy-resolved transform in meters.
pub struct DrawItem<'a> {
    pub node: NodeId,
    pub handle: MeshHandle,
    pub mesh: &'a MeshData,
    pub model: Mat4,
    pub color: [f32; 3],
}

impl Scene {
    /// Walks the scene from the root, composing global transforms on the way
    /// down. Nodes without geometry (the root, bodiless entities) are skipped.
    pub fn draw_items(&self) -> Vec<DrawItem<'_>> {
        let mut items = Vec::new();
        let mut stack = vec![(self.root(), Mat4::IDENTITY)];

        while let Some((id, inherited)) = stack.pop() {
            let Ok(entity) = self.entity(id) else {
                continue;
            };
            let Ok(transform) = self.transform(id) else {
                continue;
            };
            let model = inherited * local_matrix(transform);

            if let Some(mesh) = self.world.get::<NodeMesh>(entity) {
                let color = self
                    .world
                    .get::<ObjectColor>(entity)
                    .map(|color| color.0)
                    .unwrap_or(Vec3::ONE);
                items.push(DrawItem {
                    node: id,
                    handle: mesh.handle,
                    mesh: &mesh.data,
                    model,
                    color: color.to_array(),
                });
            }

            if let Some(children) = self.world.get::<Children>(entity) {
                stack.extend(children.0.iter().rev().map(|child| (*child, model)));
            }
        }

        items
    }
}

fn local_matrix(transform: Transform) -> Mat4 {
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

        let items = scene.draw_items();
        assert_ne!(items[0].handle, items[1].handle);
        assert_eq!(items[0].color, [1.0, 0.0, 0.0]);
        assert_eq!(second, items[1].node);
    }
}
