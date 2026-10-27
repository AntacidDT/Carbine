use glam::{Mat4, Vec3};

/// Carbine's vertical axis. The world is z-up: `position.z` is how high
/// something is, `size.z` is its height, and gravity pulls toward `-z`.
pub const UP: Vec3 = Vec3::Z;

/// How the camera turns meters into a picture. Perspective gives the depth
/// perception a 3D scene needs; orthographic keeps parallel lines parallel,
/// which is what a flat 2D layout wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraMode {
    Perspective,
    Orthographic,
}

/// The scene's camera. It sits at `position` and points at `target`; the
/// upright direction is always world `+z` unless it is looking straight down
/// that axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub position: Vec3,
    pub target: Vec3,
    pub mode: CameraMode,
    /// Vertical size of the visible world in meters, orthographic mode only.
    pub view_height: f32,
    /// Vertical field of view in degrees, perspective mode only.
    pub fov: f32,
    pub near: f32,
    pub far: f32,
}

/// Half the depth range in meters kept visible by the orthographic camera.
const DEPTH: f32 = 20.0;

impl Default for Camera {
    fn default() -> Self {
        Self {
            position: Vec3::new(0.0, -6.0, 4.0),
            target: Vec3::new(0.0, 0.0, 0.5),
            mode: CameraMode::Perspective,
            view_height: 2.0,
            fov: 55.0,
            near: 0.1,
            far: 200.0,
        }
    }
}

impl Camera {
    /// The camera looks down `-z` in its own space, which is what wgpu's
    /// right-handed projection matrices expect.
    pub fn view(&self) -> Mat4 {
        let forward = self.target - self.position;
        // Aiming straight down the vertical axis leaves nothing to stand up, so
        // the flat 2D framing takes over: the top of the frame is world +y.
        let up = if forward.x.hypot(forward.y) < 1e-4 {
            Vec3::Y
        } else {
            UP
        };
        glam::camera::rh::view::look_at_mat4(self.position, self.target, up)
    }

    /// Y-up, wgpu's [0, 1] depth range.
    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        let aspect = aspect.max(1e-6);
        let projection = match self.mode {
            CameraMode::Perspective => glam::camera::rh::proj::directx::perspective(
                self.fov_radians(),
                aspect,
                self.near,
                self.far,
            ),
            CameraMode::Orthographic => {
                let half_height = self.view_height.max(1e-6) / 2.0;
                let half_width = half_height * aspect;
                glam::camera::rh::proj::directx::orthographic(
                    -half_width,
                    half_width,
                    -half_height,
                    half_height,
                    -DEPTH,
                    DEPTH,
                )
            }
        };
        projection * self.view()
    }

    pub fn fov_radians(&self) -> f32 {
        self.fov.to_radians()
    }

    /// Maps viewport pixels to clip space: `(0, 0)` is the top left corner and
    /// y grows downward, the way a layout is measured. The flipped y reverses
    /// triangle winding, so the overlay pass draws both faces.
    pub fn pixel_projection(width: u32, height: u32) -> Mat4 {
        let (width, height) = (width.max(1) as f32, height.max(1) as f32);
        let centered = Mat4::from_translation(Vec3::new(-width * 0.5, -height * 0.5, 0.0));
        let to_clip = Mat4::from_scale(Vec3::new(2.0 / width, -2.0 / height, 1.0));
        to_clip * centered
    }
}

impl crate::node::Scene {
    pub fn camera(&self) -> Camera {
        self.camera
    }

    pub fn set_camera_position(&mut self, position: Vec3) {
        self.camera.position = position;
    }

    /// Points the camera at a world position, keeping world `+z` upright.
    pub fn set_camera_target(&mut self, target: Vec3) {
        self.camera.target = target;
    }

    pub fn set_camera_mode(&mut self, mode: CameraMode) {
        self.camera.mode = mode;
    }

    pub fn set_camera_view_height(&mut self, view_height: f32) {
        self.camera.view_height = view_height.max(1e-6);
    }

    /// Vertical field of view in degrees. Clamped to a usable range so a
    /// nonsensical value cannot flip the projection inside out.
    pub fn set_camera_fov(&mut self, fov: f32) {
        self.camera.fov = fov.clamp(1.0, 179.0);
    }

    pub fn set_camera_near(&mut self, near: f32) {
        self.camera.near = near.max(1e-4);
    }

    pub fn set_camera_far(&mut self, far: f32) {
        self.camera.far = far.max(self.camera.near + 1e-4);
    }

    /// The matrix the UI pass draws with: pixels, measured from the top left.
    pub fn ui_projection(&self, width: u32, height: u32) -> Mat4 {
        Camera::pixel_projection(width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Scene;

    #[test]
    fn the_default_camera_is_perspective_and_sees_the_origin() {
        let scene = Scene::new();
        let camera = scene.camera();

        assert_eq!(camera.mode, CameraMode::Perspective);
        assert_eq!(camera.target, Vec3::new(0.0, 0.0, 0.5));
        // The world is z-up, so the default camera is lifted above the ground.
        assert!(camera.position.z > 0.0);

        let center = camera
            .view_projection(16.0 / 9.0)
            .project_point3(camera.target);
        assert!(center.x.abs() < 1e-4 && center.y.abs() < 1e-4, "{center}");
        assert!(
            center.z > 0.0 && center.z < 1.0,
            "the target was not between the near and far planes: {center}"
        );
    }

    #[test]
    fn perspective_shrinks_what_is_far_and_gives_it_a_larger_depth() {
        let camera = Camera::default();
        let matrix = camera.view_projection(1.0);
        let forward = (camera.target - camera.position).normalize();

        // The same half meter offset, one meter and five meters down the view.
        let near = matrix.project_point3(camera.position + forward * 1.0 + Vec3::X * 0.5);
        let far = matrix.project_point3(camera.position + forward * 5.0 + Vec3::X * 0.5);

        assert!(
            near.x.abs() > far.x.abs(),
            "a point further from the camera should shrink: near {near}, far {far}"
        );
        assert!(
            near.z < far.z,
            "depth should grow with distance: near {near}, far {far}"
        );
    }

    #[test]
    fn an_orthographic_camera_keeps_the_flat_two_dimensional_framing() {
        let mut scene = Scene::new();
        scene.set_camera_position(Vec3::new(0.0, 0.0, 10.0));
        scene.set_camera_target(Vec3::ZERO);
        scene.set_camera_mode(CameraMode::Orthographic);
        scene.set_camera_view_height(2.0);

        let matrix = scene.camera().view_projection(1.0);
        let middle = matrix.project_point3(Vec3::ZERO);
        assert!(middle.x.abs() < 1e-5 && middle.y.abs() < 1e-5, "{middle}");

        // One meter along x or y is half the viewport when two meters are visible.
        let right = matrix.project_point3(Vec3::X);
        let up = matrix.project_point3(Vec3::Y);
        assert!(
            (right.x - 1.0).abs() < 1e-5,
            "x is not screen right: {right}"
        );
        assert!((up.y - 1.0).abs() < 1e-5, "y is not screen up: {up}");
    }

    #[test]
    fn a_top_down_view_does_not_collapse_the_upright_direction() {
        let camera = Camera {
            position: Vec3::new(0.0, 0.0, 5.0),
            target: Vec3::ZERO,
            ..Camera::default()
        };
        let matrix = camera.view_projection(1.0);

        let ahead = matrix.project_point3(Vec3::new(0.0, 1.0, 0.0));
        assert!(
            ahead.y > 0.0 && ahead.x.abs() < 1e-5,
            "looking straight down should put +y at the top of the frame: {ahead}"
        );
    }

    #[test]
    fn height_along_z_moves_an_object_toward_the_top_of_a_tilted_frame() {
        let camera = Camera::default();
        let matrix = camera.view_projection(1.0);

        let low = matrix.project_point3(Vec3::ZERO);
        let high = matrix.project_point3(Vec3::Z);
        assert!(
            high.y > low.y,
            "+z should climb the frame: low {low}, high {high}"
        );
    }

    #[test]
    fn camera_numbers_stay_inside_the_ranges_that_project_sensibly() {
        let mut scene = Scene::new();

        scene.set_camera_fov(0.0);
        assert_eq!(scene.camera().fov, 1.0);
        scene.set_camera_fov(90.0);
        assert_eq!(scene.camera().fov, 90.0);

        scene.set_camera_near(-1.0);
        assert!(scene.camera().near > 0.0);

        scene.set_camera_view_height(0.0);
        assert!(scene.camera().view_height > 0.0);
    }

    #[test]
    fn pixel_space_measures_ui_from_the_top_left() {
        let matrix = Scene::new().ui_projection(64, 32);
        let project = |x, y| matrix.project_point3(Vec3::new(x, y, 0.0));

        let top_left = project(0.0, 0.0);
        assert_eq!((top_left.x, top_left.y), (-1.0, 1.0));
        let bottom_right = project(64.0, 32.0);
        assert_eq!((bottom_right.x, bottom_right.y), (1.0, -1.0));
        let middle = project(32.0, 16.0);
        assert!(middle.x.abs() < 1e-6 && middle.y.abs() < 1e-6, "{middle}");
    }
}
