use glam::{Mat4, Vec3};

/// The scene's orthographic view. Distances are meters, so `view_height` is
/// the vertical size of the visible world in meters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub position: Vec3,
    pub view_height: f32,
}

/// Half the depth range in meters kept visible by the orthographic camera.
const DEPTH: f32 = 20.0;

impl Default for Camera {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            view_height: 2.0,
        }
    }
}

impl Camera {
    /// Y-up projection with wgpu's [0, 1] depth range.
    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        let half_height = self.view_height.max(1e-6) / 2.0;
        let half_width = half_height * aspect.max(1e-6);
        let projection = glam::camera::rh::proj::directx::orthographic(
            -half_width,
            half_width,
            -half_height,
            half_height,
            -DEPTH,
            DEPTH,
        );
        projection * Mat4::from_translation(-self.position)
    }
}

impl crate::node::Scene {
    pub fn camera(&self) -> Camera {
        self.camera
    }

    pub fn set_camera_position(&mut self, position: Vec3) {
        self.camera.position = position;
    }

    pub fn set_camera_view_height(&mut self, view_height: f32) {
        self.camera.view_height = view_height.max(1e-6);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Scene;

    #[test]
    fn the_default_camera_frames_a_two_meter_view() {
        let scene = Scene::new();
        let camera = scene.camera();
        assert_eq!(camera.view_height, 2.0);
        assert_eq!(camera.position, Vec3::ZERO);
    }

    #[test]
    fn view_height_is_clamped_to_a_positive_extent() {
        let mut scene = Scene::new();
        scene.set_camera_view_height(0.0);
        assert!(scene.camera().view_height > 0.0);
    }

    #[test]
    fn a_unit_quad_at_the_origin_projects_into_the_middle_of_the_view() {
        let matrix = Camera::default().view_projection(1.0);
        let corner = matrix.project_point3(Vec3::new(-0.5, -0.5, 0.0));
        assert!(
            corner.x.abs() < 1.0 && corner.y.abs() < 1.0,
            "corner left the frame: {corner}"
        );
        assert!(corner.z > 0.0 && corner.z < 1.0, "depth: {corner}");
    }

    #[test]
    fn the_camera_pans_the_world() {
        let camera = Camera {
            position: Vec3::new(1.0, 0.0, 0.0),
            ..Camera::default()
        };
        let center = camera.view_projection(1.0).project_point3(Vec3::X);
        assert!(center.x.abs() < 1e-6, "panning did not recenter: {center}");
    }
}
