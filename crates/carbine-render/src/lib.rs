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

/// What the renderer draws into: a real window or an offscreen image.
enum Target {
    /// An event loop and a device are bulky, and the offscreen case is the one
    /// every frame pays for, so the windowed half lives on the heap.
    Windowed(Box<Windowed>),
    Offscreen(Offscreen),
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
            target: Target::Offscreen(Offscreen {
                gpu,
                width: width.max(1),
                height: height.max(1),
            }),
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
            Ok(Session {
                window,
                gpu,
                surface,
                format,
                size: (size.width.max(1), size.height.max(1)),
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
        let aspect = session.size.0 as f32 / session.size.1 as f32;

        match session.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                let view = texture
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                session.gpu.draw(&view, session.format, aspect, scene)?;
                session.gpu.present(texture);
                Ok(())
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                let (width, height) = session.size;
                session
                    .gpu
                    .configure_surface(&session.surface, session.format, width, height);
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
        self.gpu
            .draw(&view, self.format(), self.aspect(), scene)
            .map(|_| ())
    }

    fn read_pixels(&mut self, scene: &Scene) -> Result<Vec<u8>, RenderError> {
        let (texture, view) = self.gpu.create_target(self.width, self.height);
        self.gpu.draw(&view, self.format(), self.aspect(), scene)?;
        self.gpu.read_pixels(&texture, self.width, self.height)
    }

    fn format(&self) -> wgpu::TextureFormat {
        wgpu::TextureFormat::Rgba8Unorm
    }

    fn aspect(&self) -> f32 {
        self.width as f32 / self.height as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use carbine_core::{Scene, Vec3, VecField};

    fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
        let offset = ((y * width + x) * 4) as usize;
        [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
    }

    #[test]
    fn an_offscreen_rectangle_rasterizes_in_its_colour() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
        let card = scene.create("rectangle", None, None).unwrap();
        scene
            .set_vec_field(card, VecField::Size, Vec3::new(0.5, 0.5, 1.0))
            .unwrap();
        scene
            .set_vec_field(card, VecField::Position, Vec3::new(0.5, 0.0, 0.0))
            .unwrap();
        scene
            .set_vec_field(card, VecField::Color, Vec3::new(1.0, 0.0, 0.0))
            .unwrap();

        let pixels = renderer.read_pixels(&scene).unwrap();

        // The camera shows two metres of height across the viewport, so a
        // half-metre square at x=0.5 covers the right of centre and nothing
        // else: the left edge stays the clear colour.
        assert_eq!(pixel(&pixels, 64, 52, 32), [255, 0, 0]);
        assert_eq!(pixel(&pixels, 64, 4, 32), [22, 21, 20]);
        assert_eq!(renderer.viewport(), (64, 64));
    }

    #[test]
    fn an_empty_scene_is_only_the_clear_colour() {
        let mut renderer = Renderer::offscreen(32, 32).unwrap();
        let scene = Scene::new();

        let pixels = renderer.read_pixels(&scene).unwrap();
        assert_eq!(pixel(&pixels, 32, 16, 16), [22, 21, 20]);
        assert_eq!(pixel(&pixels, 32, 31, 31), [22, 21, 20]);
    }

    #[test]
    fn child_geometry_moves_with_its_parent() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = Scene::new();
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

    #[test]
    fn offscreen_rendering_reports_the_adapter_it_found() {
        let renderer = Renderer::offscreen(8, 8).unwrap();
        assert!(renderer.adapter().is_some_and(|name| !name.is_empty()));
    }
}
