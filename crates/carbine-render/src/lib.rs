//! Carbine's GPU layer: windowing, pipelines, and frame submission.
//!
//! The renderer owns the frame. `carbine-core` hands it a scene and never sees a
//! GPU type, so a game, the editor, and a headless test all drive the same API.

mod gpu;

use std::sync::Arc;
use std::time::{Duration, Instant};

use carbine_core::Scene;
pub use gpu::{Gpu, RenderError, create_instance};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::window::{Window, WindowId};

/// What the renderer draws into: a real window or an offscreen image. Both halves
/// are bulky, so each lives on the heap and the enum itself stays small.
enum Target {
    Windowed(Box<Windowed>),
    Offscreen(Box<Offscreen>),
}

pub struct Renderer {
    target: Target,
}

/// The window, surface and device, assembled together once winit has a window.
struct Session {
    window: Arc<Window>,
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    /// The world pass needs somewhere to keep depth, and a resize throws the
    /// buffer it had away.
    depth: (wgpu::Texture, wgpu::TextureView),
}

struct Windowed {
    event_loop: EventLoop<()>,
    session: Option<Session>,
    title: String,
    size: (u32, u32),
    closed: bool,
    failure: Option<RenderError>,
}

struct Offscreen {
    gpu: Gpu,
    width: u32,
    height: u32,
}

impl Renderer {
    /// Opens a window and waits until it exists, so the caller learns about a
    /// headless machine here instead of on the first `render`.
    pub fn window(title: &str, width: u32, height: u32) -> Result<Self, RenderError> {
        let event_loop =
            EventLoop::new().map_err(|error| RenderError::Window(error.to_string()))?;
        let mut windowed = Windowed {
            event_loop,
            session: None,
            title: title.to_owned(),
            size: (width.max(1), height.max(1)),
            closed: false,
            failure: None,
        };

        let deadline = Instant::now() + Duration::from_millis(2000);
        while windowed.session.is_none() && windowed.failure.is_none() {
            windowed.pump();
            if windowed.session.is_some() || windowed.failure.is_some() {
                break;
            }
            if Instant::now() >= deadline {
                return Err(RenderError::Window("the window was never created".into()));
            }
            std::thread::sleep(Duration::from_millis(2));
        }

        if let Some(error) = windowed.failure.take() {
            return Err(error);
        }

        Ok(Self {
            target: Target::Windowed(Box::new(windowed)),
        })
    }

    /// A device with no window, used by tests and by `carbine.readPixels()`.
    pub fn offscreen(width: u32, height: u32) -> Result<Self, RenderError> {
        let gpu = Gpu::new(create_instance(), None)?;
        Ok(Self {
            target: Target::Offscreen(Box::new(Offscreen {
                gpu,
                width: width.max(1),
                height: height.max(1),
            })),
        })
    }

    /// Draws one frame of `scene`.
    pub fn render(&mut self, scene: &Scene) -> Result<(), RenderError> {
        match &mut self.target {
            Target::Windowed(windowed) => windowed.render(scene),
            Target::Offscreen(offscreen) => offscreen.render(scene),
        }
    }

    /// Renders `scene` and returns its pixels as packed RGBA, top row first.
    /// Only meaningful for an offscreen renderer; a window is read through the
    /// compositor instead.
    pub fn read_pixels(&mut self, scene: &Scene) -> Result<Vec<u8>, RenderError> {
        match &mut self.target {
            Target::Offscreen(offscreen) => offscreen.read_pixels(scene),
            Target::Windowed(_) => Err(RenderError::Readback(
                "a windowed renderer cannot be read back; render offscreen".into(),
            )),
        }
    }

    /// False once the window has been closed.
    pub fn is_running(&self) -> bool {
        match &self.target {
            Target::Windowed(windowed) => !windowed.closed,
            Target::Offscreen(_) => true,
        }
    }

    /// The size the scene is drawn into, in pixels, and its aspect ratio.
    pub fn viewport(&self) -> (u32, u32) {
        match &self.target {
            Target::Windowed(windowed) => windowed.size,
            Target::Offscreen(offscreen) => (offscreen.width, offscreen.height),
        }
    }

    pub fn adapter(&self) -> Option<&str> {
        match &self.target {
            Target::Windowed(windowed) => windowed.session.as_ref().map(|s| s.gpu.adapter()),
            Target::Offscreen(offscreen) => Some(offscreen.gpu.adapter()),
        }
    }

    pub fn set_title(&mut self, title: &str) {
        if let Target::Windowed(windowed) = &mut self.target {
            windowed.title = title.to_owned();
            if let Some(session) = &windowed.session {
                session.window.set_title(title);
            }
        }
    }
}

/// Carries state across a single winit pump, since `Window` is not `Clone` and
/// the surface must outlive the window it came from.
struct Handler {
    session: Option<Session>,
    title: String,
    size: (u32, u32),
    closed: bool,
    failure: Option<RenderError>,
}

impl ApplicationHandler for Handler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.session.is_some() || self.failure.is_some() {
            return;
        }
        let built = (|| -> Result<Session, RenderError> {
            let attributes = Window::default_attributes()
                .with_title(&self.title)
                .with_inner_size(PhysicalSize::new(self.size.0, self.size.1));
            let window = Arc::new(
                event_loop
                    .create_window(attributes)
                    .map_err(|error| RenderError::Window(error.to_string()))?,
            );
            let instance = create_instance();
            let surface = instance
                .create_surface(window.clone())
                .map_err(|error| RenderError::Surface(error.to_string()))?;
            let gpu = Gpu::new(instance, Some(&surface))?;
            let format = gpu.pick_format(&surface)?;
            let size = window.inner_size();
            gpu.configure_surface(&surface, format, size.width, size.height);
            let depth = gpu.create_depth(size.width, size.height);
            Ok(Session {
                window,
                gpu,
                surface,
                format,
                size: (size.width.max(1), size.height.max(1)),
                depth,
            })
        })();

        match built {
            Ok(session) => {
                self.size = session.size;
                self.session = Some(session);
            }
            Err(error) => self.failure = Some(error),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.closed = true;
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                self.size = (size.width.max(1), size.height.max(1));
                if let Some(session) = &mut self.session {
                    session.gpu.configure_surface(
                        &session.surface,
                        session.format,
                        size.width,
                        size.height,
                    );
                    session.depth = session.gpu.create_depth(size.width, size.height);
                    session.size = self.size;
                }
            }
            _ => {}
        }
    }
}

impl Windowed {
    fn render(&mut self, scene: &Scene) -> Result<(), RenderError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }

        self.pump();

        let Some(session) = &mut self.session else {
            return Ok(());
        };

        match session.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                let view = texture
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                session
                    .gpu
                    .draw(&view, &session.depth.1, session.format, session.size, scene)?;
                session.gpu.present(texture);
                Ok(())
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                let (width, height) = session.size;
                session
                    .gpu
                    .configure_surface(&session.surface, session.format, width, height);
                session.depth = session.gpu.create_depth(width, height);
                Ok(())
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => Ok(()),
        }
    }

    fn pump(&mut self) {
        let mut handler = Handler {
            session: self.session.take(),
            title: self.title.clone(),
            size: self.size,
            closed: self.closed,
            failure: self.failure.clone(),
        };
        let status = self
            .event_loop
            .pump_app_events(Some(Duration::ZERO), &mut handler);
        let Handler {
            session,
            size,
            closed,
            failure,
            ..
        } = handler;
        self.session = session;
        self.size = size;
        self.closed = closed || matches!(status, PumpStatus::Exit(_));
        self.failure = failure;
    }
}

impl Offscreen {
    fn render(&mut self, scene: &Scene) -> Result<(), RenderError> {
        let (_texture, view) = self.gpu.create_target(self.width, self.height);
        let (_depth, depth) = self.gpu.create_depth(self.width, self.height);
        self.gpu
            .draw(
                &view,
                &depth,
                self.format(),
                (self.width, self.height),
                scene,
            )
            .map(|_| ())
    }

    fn read_pixels(&mut self, scene: &Scene) -> Result<Vec<u8>, RenderError> {
        let (texture, view) = self.gpu.create_target(self.width, self.height);
        let (_depth, depth) = self.gpu.create_depth(self.width, self.height);
        self.gpu.draw(
            &view,
            &depth,
            self.format(),
            (self.width, self.height),
            scene,
        )?;
        self.gpu.read_pixels(&texture, self.width, self.height)
    }

    fn format(&self) -> wgpu::TextureFormat {
        wgpu::TextureFormat::Rgba8Unorm
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use carbine_core::{CameraMode, NodeId, Scene, Vec3, VecField};

    fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
        let offset = ((y * width + x) * 4) as usize;
        [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
    }

    /// The frame the pixel assertions are written against: a top down
    /// orthographic camera with no backdrop and the light straight overhead, so
    /// a floor facing surface keeps exactly the colour it was given.
    fn flat(scene: &mut Scene) {
        scene.set_sky_enabled(false);
        scene.set_sky_sun(Vec3::Z);
        scene.set_camera_mode(CameraMode::Orthographic);
        scene.set_camera_position(Vec3::new(0.0, 0.0, 10.0));
        scene.set_camera_target(Vec3::ZERO);
        scene.set_camera_view_height(2.0);
    }

    fn card(scene: &mut Scene, kind: &str, size: Vec3, position: Vec3, color: [f32; 3]) -> NodeId {
        let card = scene.create(kind, None, None).unwrap();
        scene.set_vec_field(card, VecField::Size, size).unwrap();
        scene
            .set_vec_field(card, VecField::Position, position)
            .unwrap();
        scene
            .set_vec_field(card, VecField::Color, Vec3::from(color))
            .unwrap();
        card
    }

    #[test]
    fn an_offscreen_rectangle_rasterizes_in_its_colour() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
        flat(&mut scene);
        card(
            &mut scene,
            "rectangle",
            Vec3::new(0.5, 0.5, 1.0),
            Vec3::new(0.5, 0.0, 0.0),
            [1.0, 0.0, 0.0],
        );

        let pixels = renderer.read_pixels(&scene).unwrap();

        // Two metres of world span these 64 pixels, so a half metre card at
        // x=0.5 covers the right of centre and nothing else.
        assert_eq!(pixel(&pixels, 64, 52, 32), [255, 0, 0]);
        assert_eq!(pixel(&pixels, 64, 4, 32), [22, 21, 20]);
        assert_eq!(renderer.viewport(), (64, 64));
    }

    #[test]
    fn an_empty_scene_is_only_the_clear_colour() {
        let mut renderer = Renderer::offscreen(32, 32).unwrap();
        let mut scene = Scene::new();
        scene.set_sky_enabled(false);

        let pixels = renderer.read_pixels(&scene).unwrap();
        assert_eq!(pixel(&pixels, 32, 16, 16), [22, 21, 20]);
        assert_eq!(pixel(&pixels, 32, 31, 31), [22, 21, 20]);
    }

    #[test]
    fn child_geometry_moves_with_its_parent() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
        flat(&mut scene);
        let parent = scene.create("rectangle", None, None).unwrap();
        let child = scene.create("rectangle", None, Some(parent)).unwrap();
        scene
            .set_vec_field(child, VecField::Size, Vec3::new(0.5, 0.5, 1.0))
            .unwrap();
        scene
            .set_vec_field(child, VecField::Position, Vec3::new(0.5, 0.0, 0.0))
            .unwrap();
        scene
            .set_vec_field(child, VecField::Color, Vec3::new(0.0, 1.0, 0.0))
            .unwrap();

        let pixels = renderer.read_pixels(&scene).unwrap();
        assert_eq!(pixel(&pixels, 64, 52, 32), [0, 255, 0]);
    }

    /// The nearer card is drawn first, so only a real depth buffer can keep the
    /// second one from painting over it.
    #[test]
    fn depth_keeps_the_nearer_object_in_front() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
        flat(&mut scene);
        card(
            &mut scene,
            "cube",
            Vec3::ONE,
            Vec3::new(0.0, 0.0, 1.0),
            [1.0, 0.0, 0.0],
        );
        card(
            &mut scene,
            "cube",
            Vec3::ONE,
            Vec3::new(0.0, 0.0, 0.0),
            [0.0, 1.0, 0.0],
        );

        let pixels = renderer.read_pixels(&scene).unwrap();
        assert_eq!(pixel(&pixels, 64, 32, 32), [255, 0, 0], "near wins");
    }

    #[test]
    fn a_cube_is_lit_on_top_and_left_in_ambient_on_its_side() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
        scene.set_sky_enabled(false);
        scene.set_sky_sun(Vec3::Z);
        scene.set_camera_position(Vec3::new(0.0, -3.0, 3.0));
        scene.set_camera_target(Vec3::new(0.0, 0.0, 0.5));
        card(&mut scene, "cube", Vec3::ONE, Vec3::ZERO, [1.0, 0.0, 0.0]);

        let pixels = renderer.read_pixels(&scene).unwrap();
        let faces: Vec<u8> = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[0] > pixel[1] + 20 && pixel[0] > pixel[2] + 20)
            .map(|pixel| pixel[0])
            .collect();
        let brightest = faces.iter().max().copied().unwrap_or_default();
        let darkest = faces.iter().min().copied().unwrap_or_default();

        // The top face looks straight into the light; the walls do not, and fall
        // to the scene's ambient level of 30%.
        assert!(
            brightest > 250,
            "the lit face should be full colour: {brightest}"
        );
        assert!(
            (70..=82).contains(&darkest),
            "a wall away from the sun should sit at ambient: {darkest}"
        );
    }

    #[test]
    fn perspective_makes_a_farther_cube_smaller() {
        let mut near = Scene::new();
        near.set_sky_enabled(false);
        near.set_camera_position(Vec3::new(0.0, -4.0, 0.5));
        near.set_camera_target(Vec3::ZERO);
        card(&mut near, "cube", Vec3::ONE, Vec3::ZERO, [1.0, 0.0, 0.0]);

        let mut far = Scene::new();
        far.set_sky_enabled(false);
        far.set_camera_position(Vec3::new(0.0, -4.0, 0.5));
        far.set_camera_target(Vec3::ZERO);
        card(
            &mut far,
            "cube",
            Vec3::ONE,
            Vec3::new(0.0, 6.0, 0.0),
            [1.0, 0.0, 0.0],
        );

        let mut renderer = Renderer::offscreen(128, 128).unwrap();
        let mut covered = |scene: &Scene| {
            renderer
                .read_pixels(scene)
                .unwrap()
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| pixel[0] > 100)
                .count()
        };

        assert!(
            covered(&near) > covered(&far) * 2,
            "the same cube should shrink with distance"
        );
    }

    #[test]
    fn a_skin_paints_the_face_that_asked_for_it() {
        let directory =
            std::env::temp_dir().join(format!("carbine-skin-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("two-rows.png");
        let image = image::RgbaImage::from_fn(2, 2, |_x, y| {
            if y == 0 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        image.save(&path).unwrap();

        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
        flat(&mut scene);
        let tile = card(
            &mut scene,
            "plane",
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::ZERO,
            [1.0, 1.0, 1.0],
        );
        scene
            .set_texture(tile, Some(path.to_str().unwrap().to_owned()))
            .unwrap();

        let pixels = renderer.read_pixels(&scene).unwrap();
        // The top of the picture is the top of the tile on screen.
        assert_eq!(pixel(&pixels, 64, 32, 20), [255, 0, 0]);
        assert_eq!(pixel(&pixels, 64, 32, 44), [0, 0, 255]);

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn transparency_blends_a_world_object_over_what_is_behind_it() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
        flat(&mut scene);
        card(
            &mut scene,
            "plane",
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::ZERO,
            [1.0, 0.0, 0.0],
        );
        let veil = card(
            &mut scene,
            "plane",
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(0.0, 0.0, 1.0),
            [0.0, 1.0, 0.0],
        );
        scene.set_alpha(veil, 128.0).unwrap();

        let pixels = renderer.read_pixels(&scene).unwrap();
        let blended = pixel(&pixels, 64, 32, 32);
        assert!(
            (122..=132).contains(&blended[0])
                && (122..=132).contains(&blended[1])
                && blended[2] < 4,
            "half green over red should land in between: {blended:?}"
        );
    }

    #[test]
    fn the_sky_graduates_from_horizon_to_zenith() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
        scene.set_sky_noise(0.0);
        scene.set_sky_zenith(Vec3::new(0.0, 0.0, 1.0));
        scene.set_sky_horizon(Vec3::new(1.0, 1.0, 1.0));
        scene.set_camera_position(Vec3::new(0.0, -4.0, 0.0));
        scene.set_camera_target(Vec3::new(0.0, 4.0, 0.0));

        let pixels = renderer.read_pixels(&scene).unwrap();
        // Looking along the ground: the top of the frame is above the horizon,
        // the bottom is under it.
        assert!(
            pixel(&pixels, 64, 32, 2)[0] < 120,
            "the top of the frame should lean toward the zenith blue: {:?}",
            pixel(&pixels, 64, 32, 2)
        );
        assert_eq!(pixel(&pixels, 64, 32, 62), [255, 255, 255]);
    }

    #[test]
    fn switching_the_sky_off_leaves_the_clear_colour() {
        let mut renderer = Renderer::offscreen(32, 32).unwrap();
        let mut scene = Scene::new();
        scene.set_sky_enabled(false);

        let pixels = renderer.read_pixels(&scene).unwrap();
        assert_eq!(pixel(&pixels, 32, 16, 16), [22, 21, 20]);
    }

    #[test]
    fn offscreen_rendering_reports_the_adapter_it_found() {
        let renderer = Renderer::offscreen(8, 8).unwrap();
        assert!(renderer.adapter().is_some_and(|name| !name.is_empty()));
    }

    /// A 40 by 40 pixel element centred on a 64 by 64 target, in Carbine orange.
    fn button(scene: &mut Scene) -> NodeId {
        flat(scene);
        let button = scene.create_ui("square", None, None).unwrap();
        scene
            .set_vec_field(button, VecField::Position, Vec3::new(32.0, 32.0, 0.0))
            .unwrap();
        scene
            .set_vec_field(button, VecField::Size, Vec3::new(40.0, 40.0, 1.0))
            .unwrap();
        scene.set_color_hex(button, "#eb7a34").unwrap();
        button
    }

    #[test]
    fn a_square_element_fills_the_pixels_it_is_given() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
        button(&mut scene);

        let pixels = renderer.read_pixels(&scene).unwrap();
        assert_eq!(pixel(&pixels, 64, 14, 14), [235, 122, 52]);
        assert_eq!(pixel(&pixels, 64, 49, 49), [235, 122, 52]);
        assert_eq!(pixel(&pixels, 64, 9, 9), [22, 21, 20]);
    }

    #[test]
    fn a_corner_value_cuts_the_corner_and_keeps_the_middle() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
        let button = button(&mut scene);
        scene.set_corner(button, 20.0).unwrap();

        let pixels = renderer.read_pixels(&scene).unwrap();
        assert_eq!(pixel(&pixels, 64, 14, 14), [22, 21, 20], "corner");
        assert_eq!(pixel(&pixels, 64, 32, 14), [235, 122, 52], "edge");
        assert_eq!(pixel(&pixels, 64, 32, 32), [235, 122, 52], "middle");
    }

    #[test]
    fn an_outline_is_stroked_even_when_the_element_is_transparent() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
        let button = button(&mut scene);
        scene.set_outline(button, 4.0).unwrap();
        scene.set_alpha(button, 0.0).unwrap();

        let pixels = renderer.read_pixels(&scene).unwrap();
        assert_eq!(pixel(&pixels, 64, 14, 32), [235, 122, 52], "ring");
        assert_eq!(pixel(&pixels, 64, 32, 32), [22, 21, 20], "hollow middle");
        assert_eq!(pixel(&pixels, 64, 11, 32), [22, 21, 20], "outside");
    }

    #[test]
    fn transparency_blends_the_element_into_the_frame() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
        let button = button(&mut scene);
        scene.set_alpha(button, 128.0).unwrap();

        let pixels = renderer.read_pixels(&scene).unwrap();
        let blended = pixel(&pixels, 64, 14, 14);
        // Half opaque Carbine orange over Carbine black: every channel lands
        // about halfway between its own value and the frame behind it.
        assert!(
            (126..=132).contains(&blended[0])
                && (68..=75).contains(&blended[1])
                && (33..=39).contains(&blended[2]),
            "expected a blended orange, got {blended:?}"
        );
        assert_eq!(
            pixel(&pixels, 64, 9, 9),
            [22, 21, 20],
            "outside stays clear"
        );
    }

    #[test]
    fn the_polygon_count_replaces_the_box_with_a_triangle() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
        let button = button(&mut scene);
        scene.set_polygon_sides(button, 3).unwrap();

        let pixels = renderer.read_pixels(&scene).unwrap();
        assert_eq!(
            pixel(&pixels, 64, 20, 32),
            [235, 122, 52],
            "inside the point"
        );
        assert_eq!(pixel(&pixels, 64, 32, 14), [22, 21, 20], "above the flat");
        assert_eq!(pixel(&pixels, 64, 49, 49), [22, 21, 20], "the old corner");
    }

    #[test]
    fn ui_draws_over_the_world_it_shares_a_scene_with() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
        flat(&mut scene);
        card(
            &mut scene,
            "rectangle",
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::ZERO,
            [1.0, 0.0, 0.0],
        );
        let button = scene.create_ui("square", None, None).unwrap();
        scene
            .set_vec_field(button, VecField::Position, Vec3::new(32.0, 32.0, 0.0))
            .unwrap();
        scene
            .set_vec_field(button, VecField::Size, Vec3::new(16.0, 16.0, 1.0))
            .unwrap();
        scene.set_color_hex(button, "#0000ff").unwrap();

        let pixels = renderer.read_pixels(&scene).unwrap();
        assert_eq!(pixel(&pixels, 64, 32, 32), [0, 0, 255], "overlay on top");
        assert_eq!(pixel(&pixels, 64, 20, 32), [255, 0, 0], "world behind");
    }
}
