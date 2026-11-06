//! Carbine's GPU layer: windowing, pipelines, and frame submission.
//!
//! The renderer owns the frame. `carbine-core` hands it a scene and never sees a
//! GPU type, so a game, the editor, and a headless test all drive the same API.

mod device;
mod gpu;
mod input;
mod pacing;
mod text;

use std::sync::Arc;
use std::time::{Duration, Instant};

use carbine_core::{FrameInput, Scene};
pub use gpu::{Gpu, RenderError, create_instance};
use input::InputCollector;
use pacing::Pacing;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::window::{CursorGrabMode, Window, WindowId};

/// What the renderer draws into: a real window or an offscreen image. Both halves
/// are bulky, so each lives on the heap and the enum itself stays small.
enum Target {
    Windowed(Box<Windowed>),
    Offscreen(Box<Offscreen>),
}

pub struct Renderer {
    target: Target,
    /// The frame limits, kept on the renderer rather than the window so an
    /// offscreen target answers the same questions with no screen to hold it.
    pacing: Pacing,
}

/// The window, surface and device, assembled together once winit has a window.
struct Session {
    window: Arc<Window>,
    gpu: Gpu,
    surface: wgpu::Surface<'static>,
    format: wgpu::TextureFormat,
    size: (u32, u32),
    /// The mode the surface is configured at, remembered so a resize does not
    /// have to ask the compositor what it offers again.
    present_mode: wgpu::PresentMode,
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
    /// Events gathered by a pump nobody polled yet.
    input: InputCollector,
    /// Whether the pointer is actually held, which is not always what was asked
    /// for: a compositor can refuse.
    grabbed: bool,
    /// When the screen's refresh rate was last asked for.
    monitor_seen: Option<Instant>,
}

struct Offscreen {
    gpu: Gpu,
    width: u32,
    height: u32,
}

impl Renderer {
    /// Opens a window and waits until it exists, so the caller learns about a
    /// headless machine here instead of on the first `render`.
    ///
    /// A Wayland compositor only names the screen a window stands on once that
    /// window has actually put a frame up, so `monitor_hz` reads zero until then and
    /// a synced window holds to its cap alone; from the first `render` onwards the
    /// screen is included, and holds the cap back if it turns out to be the lower.
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
            input: InputCollector::default(),
            grabbed: false,
            monitor_seen: None,
        };
        let mut pacing = Pacing::default();

        let deadline = Instant::now() + Duration::from_millis(2000);
        while windowed.session.is_none() && windowed.failure.is_none() {
            windowed.pump(pacing.monitor_sync());
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

        windowed.watch_monitor(&mut pacing, Instant::now());
        Ok(Self {
            target: Target::Windowed(Box::new(windowed)),
            pacing,
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
            pacing: Pacing::default(),
        })
    }

    /// Draws one frame of `scene`, then waits out whatever the frame limits still
    /// ask for so the next one cannot start early.
    pub fn render(&mut self, scene: &Scene) -> Result<(), RenderError> {
        match &mut self.target {
            Target::Windowed(windowed) => windowed.render(scene, &mut self.pacing),
            Target::Offscreen(offscreen) => offscreen.render(scene),
        }
    }

    /// Everything the keyboard and mouse did since the last poll, in arrival
    /// order. An offscreen renderer has no devices to ask and returns an empty
    /// frame.
    pub fn poll(&mut self) -> FrameInput {
        match &mut self.target {
            Target::Windowed(windowed) => {
                windowed.pump(self.pacing.monitor_sync());
                windowed.input.take()
            }
            Target::Offscreen(_) => FrameInput::default(),
        }
    }

    /// Holds the pointer in the window and hides it, which is how a mouse-driven
    /// camera gets relative motion out of a compositor. Returns the state the
    /// window ended up in, since a refusal is not an error worth raising.
    pub fn set_pointer_grab(&mut self, grab: bool) -> bool {
        let Target::Windowed(windowed) = &mut self.target else {
            return false;
        };
        if windowed.grabbed != grab {
            let held = windowed.apply_grab(grab);
            windowed.grabbed = held;
        }
        windowed.grabbed
    }

    pub fn pointer_grabbed(&self) -> bool {
        match &self.target {
            Target::Windowed(windowed) => windowed.grabbed,
            Target::Offscreen(_) => false,
        }
    }

    /// Frames per second the window may not pass, `None` for no cap. Returns the
    /// rate the frame actually settles at, which is the screen's if it is lower.
    pub fn set_fps_cap(&mut self, fps: Option<u32>) -> Option<u32> {
        self.pacing.set_fps_cap(fps);
        self.limit_fps()
    }

    pub fn fps_cap(&self) -> Option<u32> {
        self.pacing.fps_cap()
    }

    /// Whether the screen is in charge of the pace. Turning it on hands the swap
    /// chain to the compositor, turning it off asks for a mode that can run ahead;
    /// either way the cap still holds. Returns the settled rate.
    pub fn set_monitor_sync(&mut self, sync: bool) -> Option<u32> {
        match &mut self.target {
            Target::Windowed(windowed) => windowed.set_monitor_sync(&mut self.pacing, sync),
            Target::Offscreen(_) => self.pacing.set_monitor_sync(sync),
        }
        self.limit_fps()
    }

    pub fn monitor_sync(&self) -> bool {
        self.pacing.monitor_sync()
    }

    /// The refresh rate of the screen the window is standing on, zero when there is
    /// no screen to ask or the compositor will not say.
    pub fn monitor_hz(&self) -> u32 {
        match &self.target {
            Target::Windowed(windowed) => windowed.monitor_hz(),
            Target::Offscreen(_) => 0,
        }
    }

    /// The rate the frames settle at: the lower of the cap and the screen, `None`
    /// when nothing holds them. The screen is asked again first, because a player
    /// who has just typed a cap wants the rate that is true now, not the one the
    /// last frame happened to cache.
    pub fn limit_fps(&mut self) -> Option<u32> {
        if let Target::Windowed(windowed) = &mut self.target {
            windowed.watch_monitor(&mut self.pacing, Instant::now());
        }
        self.pacing.limit_fps()
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

    /// What kind of graphics device the renderer is drawing on and how much
    /// memory it owns, as far as this machine will say. The scene sorts a light
    /// rung out of this, so it is worth reporting once, at start-up.
    pub fn hardware(&self) -> Option<carbine_core::Gpu> {
        match &self.target {
            Target::Windowed(windowed) => windowed.session.as_ref().map(|s| s.gpu.hardware()),
            Target::Offscreen(offscreen) => Some(offscreen.gpu.hardware()),
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
    input: InputCollector,
    grabbed: bool,
    sync: bool,
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
            let present_mode = gpu.pick_present_mode(&surface, self.sync);
            gpu.configure_surface(&surface, format, size.width, size.height, present_mode);
            let depth = gpu.create_depth(size.width, size.height);
            Ok(Session {
                window,
                gpu,
                surface,
                format,
                present_mode,
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
        self.input.window_event(event.clone());
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
                        session.present_mode,
                    );
                    session.depth = session.gpu.create_depth(size.width, size.height);
                    session.size = self.size;
                }
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: winit::event::DeviceId,
        event: DeviceEvent,
    ) {
        self.input.device_event(event);
    }
}

impl Windowed {
    fn render(&mut self, scene: &Scene, pacing: &mut Pacing) -> Result<(), RenderError> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }

        self.pump(pacing.monitor_sync());
        self.remember_monitor_hz(pacing);

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
                pacing.hold();
                Ok(())
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                let (width, height) = session.size;
                session.gpu.configure_surface(
                    &session.surface,
                    session.format,
                    width,
                    height,
                    session.present_mode,
                );
                session.depth = session.gpu.create_depth(width, height);
                Ok(())
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => Ok(()),
        }
    }

    /// The refresh rate of the screen the window is standing on, zero when there is
    /// none to ask. Millihertz rounds down, which is the safe direction: 59997 is
    /// still a 60 frame pace, not a 59 one.
    fn monitor_hz(&self) -> u32 {
        let millihertz = self
            .session
            .as_ref()
            .and_then(|session| session.window.current_monitor())
            .and_then(|monitor| monitor.refresh_rate_millihertz());
        millihertz.unwrap_or_default() / 1000
    }

    /// A player unplugs at the café, so the rate is asked for again every second
    /// while the screen is in charge. A screen that has not answered yet is asked
    /// on every frame, because a window only reaches the desktop a moment after it
    /// is created and guessing 0 Hz would let the cap stand in for the screen.
    fn remember_monitor_hz(&mut self, pacing: &mut Pacing) {
        if !pacing.monitor_sync() {
            return;
        }
        let now = Instant::now();
        if self
            .monitor_seen
            .is_some_and(|then| now.duration_since(then) < Duration::from_secs(1))
        {
            return;
        }
        self.watch_monitor(pacing, now);
    }

    /// Takes the screen's rate, and the moment it was taken, so the next frame can
    /// tell whether the answer is fresh enough to keep.
    fn watch_monitor(&mut self, pacing: &mut Pacing, now: Instant) {
        let hz = self.monitor_hz();
        if hz > 0 {
            self.monitor_seen = Some(now);
        }
        pacing.set_monitor_hz(hz);
    }

    /// Hands the swap chain the mode the wish implies, so a change is felt on the
    /// frame that asked for it instead of on the next resize.
    fn set_monitor_sync(&mut self, pacing: &mut Pacing, sync: bool) {
        pacing.set_monitor_sync(sync);
        self.watch_monitor(pacing, Instant::now());

        let Some(session) = &mut self.session else {
            return;
        };
        let mode = session.gpu.pick_present_mode(&session.surface, sync);
        if mode != session.present_mode {
            let (width, height) = session.size;
            session.present_mode = mode;
            session
                .gpu
                .configure_surface(&session.surface, session.format, width, height, mode);
        }
    }

    fn pump(&mut self, sync: bool) {
        let mut handler = Handler {
            session: self.session.take(),
            title: self.title.clone(),
            size: self.size,
            closed: self.closed,
            failure: self.failure.clone(),
            input: std::mem::take(&mut self.input),
            grabbed: self.grabbed,
            sync,
        };
        let status = self
            .event_loop
            .pump_app_events(Some(Duration::ZERO), &mut handler);
        let Handler {
            session,
            size,
            closed,
            failure,
            mut input,
            grabbed,
            ..
        } = handler;
        self.session = session;
        self.size = size;
        self.grabbed = grabbed;
        self.input.absorb(input.take());
        self.closed = closed || matches!(status, PumpStatus::Exit(_));
        self.failure = failure;
    }

    /// Asks the compositor to hold the pointer. A locked cursor is what a camera
    /// wants, but not every desktop grants it, so a confined one that keeps the
    /// pointer in the window is the next best thing.
    fn apply_grab(&self, grab: bool) -> bool {
        let Some(session) = &self.session else {
            return false;
        };
        session.window.set_cursor_visible(!grab);

        if !grab {
            let _ = session.window.set_cursor_grab(CursorGrabMode::None);
            return false;
        }
        if session
            .window
            .set_cursor_grab(CursorGrabMode::Locked)
            .is_ok()
        {
            return true;
        }
        session
            .window
            .set_cursor_grab(CursorGrabMode::Confined)
            .is_ok()
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
    use carbine_core::{CameraMode, LightQuality, LightType, NodeId, Scene, Vec3, VecField};

    fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
        let offset = ((y * width + x) * 4) as usize;
        [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
    }

    /// A scene on the rung the exact colours below were measured against. What
    /// `auto` picks depends on the card the test runs on, so a fixture that reads
    /// `[255, 0, 0]` has to say which world it means: this is the bare lambert one.
    fn bare() -> Scene {
        let mut scene = Scene::new();
        scene.set_light_quality(LightQuality::Low);
        scene
    }

    /// The rung that casts, asked to lay its edges in steps. The shade numbers below
    /// were read off that look, and a woven edge reaches the same places by a
    /// different road, so a fixture that says "forty darker" names the staircase it
    /// was measured on.
    fn stepped(scene: &mut Scene) {
        scene.set_light_style(LightType::Blocky);
        scene.set_light_quality(LightQuality::Medium);
    }

    /// How a column of pixels is built: how often its brightness changes, and how
    /// many pixels in a row hold one brightness. A stepped edge changes a handful of
    /// times over long runs; a woven one changes nearly every pixel.
    fn weave(column: &[u8]) -> (usize, usize) {
        let mut changes = 0;
        let mut longest = 1;
        let mut run = 1;
        for pair in column.windows(2) {
            if pair[0] == pair[1] {
                run += 1;
            } else {
                changes += 1;
                run = 1;
            }
            longest = longest.max(run);
        }
        (changes, longest)
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
        let mut scene = bare();
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
        let mut scene = bare();
        scene.set_sky_enabled(false);

        let pixels = renderer.read_pixels(&scene).unwrap();
        assert_eq!(pixel(&pixels, 32, 16, 16), [22, 21, 20]);
        assert_eq!(pixel(&pixels, 32, 31, 31), [22, 21, 20]);
    }

    #[test]
    fn child_geometry_moves_with_its_parent() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
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
        let mut scene = bare();
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
        let mut scene = bare();
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

    /// The sun is a node, so what it throws is a colour and a strength rather than a
    /// given: switched off, the world is lit at ambient alone, and coloured, the light
    /// it leaves on a white floor is that colour.
    #[test]
    fn the_sun_is_a_node_whose_colour_and_strength_the_world_is_lit_by() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        flat(&mut scene);
        card(
            &mut scene,
            "rectangle",
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::ZERO,
            [1.0, 1.0, 1.0],
        );
        let sun = scene.sun().unwrap();

        let lit = renderer.read_pixels(&scene).unwrap();
        assert_eq!(
            pixel(&lit, 64, 32, 32),
            [255, 255, 255],
            "the floor looks straight into the sun the world was born with"
        );

        scene.set_brightness(sun, 0.0).unwrap();
        let dark = renderer.read_pixels(&scene).unwrap();
        let ambient = pixel(&dark, 64, 32, 32);
        assert!(
            (70..=82).contains(&ambient[0]) && ambient[0] == ambient[2],
            "a switched off sun leaves the scene's ambient: {ambient:?}"
        );

        scene.set_brightness(sun, 1.0).unwrap();
        scene
            .set_vec_field(sun, VecField::Color, Vec3::new(1.0, 0.0, 0.0))
            .unwrap();
        let red = renderer.read_pixels(&scene).unwrap();
        let tinted = pixel(&red, 64, 32, 32);
        assert_eq!(tinted[0], 255, "the red of a red sun arrives");
        assert!(
            (70..=82).contains(&tinted[1]),
            "and its green is only the ambient: {tinted:?}"
        );
    }

    /// A lamp on its own, with the sun switched off: the floor it reaches is bright,
    /// the floor past its reach is exactly the dark it was before the lamp existed, and
    /// a longer reach takes in what the short one could not.
    #[test]
    fn a_lamp_lights_the_floor_it_reaches_and_nothing_past_it() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        flat(&mut scene);
        scene.set_brightness(scene.sun().unwrap(), 0.0).unwrap();
        card(
            &mut scene,
            "rectangle",
            Vec3::new(2.0, 2.0, 1.0),
            Vec3::ZERO,
            [1.0, 1.0, 1.0],
        );

        let alone = renderer.read_pixels(&scene).unwrap();
        let lamp = scene.create_light(None, None).unwrap();
        scene.set_reach(lamp, 0.5).unwrap();
        scene
            .set_vec_field(lamp, VecField::Position, Vec3::new(0.0, 0.0, 0.2))
            .unwrap();

        let lit = renderer.read_pixels(&scene).unwrap();
        let middle = pixel(&lit, 64, 32, 32);
        let dark = pixel(&alone, 64, 32, 32);
        assert!(
            middle[0] as u32 > dark[0] as u32 + 40,
            "the lamp stands in the middle of the floor it reaches: {middle:?} against {dark:?}"
        );
        assert_eq!(
            pixel(&lit, 64, 4, 32),
            pixel(&alone, 64, 4, 32),
            "the far edge stands past the reach, in the same dark"
        );

        scene.set_reach(lamp, 4.0).unwrap();
        let wider = renderer.read_pixels(&scene).unwrap();
        assert!(
            pixel(&wider, 64, 4, 32)[0] > pixel(&lit, 64, 4, 32)[0],
            "a longer reach takes in what the short one could not"
        );

        // A blocky lamp takes its falloff in four steps, so the same place where a
        // straight lamp is a fifth of the way down a smooth run sits a step lower.
        scene.set_reach(lamp, 0.5).unwrap();
        scene.set_light_type_name(lamp, "blocky").unwrap();
        let stepped = renderer.read_pixels(&scene).unwrap();
        assert!(
            pixel(&stepped, 64, 32, 32)[0] < middle[0],
            "and a stepped falloff is dimmer than the straight one at the same place"
        );
    }

    #[test]
    fn a_wall_throws_a_shade_only_once_the_light_starts_casting() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        scene.set_sky_enabled(false);
        // A sun up and off to one side, so a wall standing in it shades the floor
        // beyond it instead of under itself.
        scene.set_sky_sun(Vec3::new(0.0, -1.0, 1.0).normalize());
        scene.set_camera_mode(CameraMode::Orthographic);
        scene.set_camera_position(Vec3::new(0.0, 0.0, 10.0));
        scene.set_camera_target(Vec3::ZERO);
        scene.set_camera_view_height(8.0);
        card(
            &mut scene,
            "rectangle",
            Vec3::new(10.0, 10.0, 1.0),
            Vec3::ZERO,
            [0.8, 0.8, 0.8],
        );
        card(
            &mut scene,
            "cube",
            Vec3::new(4.0, 0.4, 6.0),
            Vec3::new(0.0, -1.0, 3.0),
            [0.8, 0.8, 0.8],
        );

        // Metres of floor on the far side of the wall, and the same on the near
        // side where the sun has nothing standing in front of it. Both are the same
        // floor, the same colour and the same face, and both sit well inside or well
        // out of the shade so that the blurred edge of it is neither one.
        let mut at = |scene: &Scene, row: u32| {
            let pixels = renderer.read_pixels(scene).unwrap();
            pixel(&pixels, 64, 32, row)
        };

        let bare_shade = at(&scene, 20);
        let bare_lit = at(&scene, 60);
        assert_eq!(
            bare_shade, bare_lit,
            "a rung that casts nothing has no shade to throw"
        );

        stepped(&mut scene);
        let cast_shade = at(&scene, 20);
        let cast_lit = at(&scene, 60);

        assert!(
            cast_shade[0] + 40 < cast_lit[0],
            "the same floor beyond the wall should now be in shade: \
             {cast_shade:?} against {cast_lit:?}"
        );
        assert!(
            cast_lit[0] + 16 >= bare_lit[0],
            "the sun that still reaches the near floor should not have been lost: \
             {cast_lit:?} against {bare_lit:?}"
        );
    }

    #[test]
    fn the_sky_lends_its_colour_to_the_face_the_sun_leaves_out() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        // A backdrop that is green along the horizon and blue overhead, with no
        // grain in it, so a pixel that keeps any red at all is the cube and not the
        // air: the mix of those two colours has none.
        scene.set_sky_enabled(true);
        scene.set_sky_noise(0.0);
        scene.set_sky_zenith(Vec3::Z);
        scene.set_sky_horizon(Vec3::Y);
        scene.set_sky_sun(Vec3::X);
        scene.set_camera_mode(CameraMode::Orthographic);
        scene.set_camera_position(Vec3::new(0.0, -4.0, 0.0));
        scene.set_camera_target(Vec3::ZERO);
        scene.set_camera_view_height(2.0);
        card(&mut scene, "cube", Vec3::ONE, Vec3::ZERO, [1.0, 1.0, 1.0]);

        let low = renderer.read_pixels(&scene).unwrap();
        stepped(&mut scene);
        let cast = renderer.read_pixels(&scene).unwrap();
        let low = low.as_chunks::<4>().0;
        let cast = cast.as_chunks::<4>().0;

        // The face square to the eyes is edge on to the sun, so at `low` it is the
        // scene's flat ambient and nothing else.
        let face: Vec<usize> = low
            .iter()
            .enumerate()
            .filter(|(_, pixel)| pixel[0] > 60 && pixel[0] < 90 && pixel[1] == pixel[0])
            .map(|(place, _)| place)
            .collect();
        assert!(
            face.len() > 100,
            "the white cube should cover a patch of the frame: {}",
            face.len()
        );

        // From `medium` up that same face is lit by the sky over it, which at the
        // horizon is green: no red left, the green it was never given.
        let filled = face
            .iter()
            .filter(|place| cast[**place][0] < 20 && cast[**place][1] > 200)
            .count();
        assert_eq!(
            filled,
            face.len(),
            "every pixel of the face the sun leaves out should take the sky's own colour"
        );
    }

    #[test]
    fn the_sun_comes_back_off_a_floor_squared_at_it() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        scene.set_sky_enabled(false);
        scene.set_sky_sun(Vec3::new(0.0, -1.0, 1.0).normalize());
        scene.set_camera_position(Vec3::new(0.0, 8.0, 4.0));
        scene.set_camera_target(Vec3::ZERO);
        card(
            &mut scene,
            "rectangle",
            Vec3::new(20.0, 20.0, 1.0),
            Vec3::ZERO,
            [0.4, 0.4, 0.4],
        );

        // A flat floor lit by one sun is the same brightness everywhere it shows:
        // every pixel of it faces the sun the same way, so anything that varies from
        // the bare lambert answer is the sheen coming back off the air.
        let low = renderer.read_pixels(&scene).unwrap();
        stepped(&mut scene);
        let wet = renderer.read_pixels(&scene).unwrap();
        let low = low.as_chunks::<4>().0;
        let wet = wet.as_chunks::<4>().0;

        let floor: Vec<usize> = low
            .iter()
            .enumerate()
            .filter(|(_, pixel)| pixel[0] > 70 && pixel[0] < 90 && pixel[1] == pixel[0])
            .map(|(place, _)| place)
            .collect();
        assert!(
            floor.len() > 200,
            "the floor should fill most of the frame: {}",
            floor.len()
        );
        let same: Vec<u8> = floor.iter().map(|place| low[*place][0]).collect();
        assert!(
            same.iter().all(|red| *red == same[0]),
            "a bare lambert floor cannot vary: {:?}",
            same.iter().collect::<std::collections::HashSet<_>>()
        );

        let brightest = floor.iter().map(|place| wet[*place][0]).max().unwrap();
        assert!(
            brightest > same[0] + 10,
            "the sun should glare off the place where the floor is squared at it: \
             {brightest} against {}",
            same[0]
        );
    }

    /// The frame both looks are read at: a floor with a cube standing in a forty five
    /// degree sun, framed on the far end of the cube's shade. The shade ends about a
    /// metre and a half past the cube's own side, because the top of a two metre cube
    /// over a one metre floor is a metre and a half of height to cast with. The view
    /// height says how much of the world the frame shows, and so how many pixels the
    /// end of a shade is spread over.
    fn shade_end(view_height: f32) -> Scene {
        let mut scene = bare();
        scene.set_sky_enabled(false);
        scene.set_sky_sun(Vec3::new(0.0, -1.0, 1.0).normalize());
        scene.set_camera_mode(CameraMode::Orthographic);
        scene.set_camera_position(Vec3::new(0.0, 2.0, 10.0));
        scene.set_camera_target(Vec3::new(0.0, 2.0, 0.0));
        scene.set_camera_view_height(view_height);
        card(
            &mut scene,
            "rectangle",
            Vec3::new(40.0, 40.0, 1.0),
            Vec3::ZERO,
            [0.8, 0.8, 0.8],
        );
        card(
            &mut scene,
            "cube",
            Vec3::new(1.0, 1.0, 2.0),
            Vec3::new(0.0, 0.0, 1.0),
            [0.8, 0.8, 0.8],
        );
        scene
    }

    /// The grain a person sees when a shade's edge is drawn straight, counted where it
    /// can show: the end of the shade, read down three columns of the frame so that no
    /// one place an edge happens to fall carries the answer. A woven end is a fade, and
    /// a fade runs one way.
    #[test]
    fn a_woven_shade_end_fades_instead_of_speckling() {
        for rung in [
            LightQuality::Medium,
            LightQuality::High,
            LightQuality::Extra,
        ] {
            let mut renderer = Renderer::offscreen(64, 64).unwrap();
            let mut scene = shade_end(1.6);
            scene.set_light_quality(rung);
            scene.set_light_style(LightType::Straight);
            let pixels = renderer.read_pixels(&scene).unwrap();

            for column in [16, 32, 48] {
                let lit: Vec<u8> = (0..64)
                    .map(|row| pixel(&pixels, 64, column, row)[0])
                    .collect();
                let end = ending(&lit);
                assert!(
                    reversals(end) <= 2,
                    "{} wove {} levels of brightness back into a shade that should only \
                     be fading: {end:?}",
                    rung.name(),
                    reversals(end),
                );
                let rough = roughness(end);
                assert!(
                    rough <= 4.0,
                    "{} lays the end of a shade {} levels rough, which is a zigzag and \
                     not a fade: {end:?}",
                    rung.name(),
                    rough,
                );
            }
        }
    }

    /// The stretch of a column on which a look can be read at all: the end of the
    /// shade, from the edge of the umbra to the floor the sun reaches. Anywhere else a
    /// column is open floor or the shadow of the thing standing on it, and both are
    /// one brightness whichever way the edge is drawn, so a run measured across a whole
    /// column mostly counts the floor and not the look.
    fn ending(column: &[u8]) -> &[u8] {
        let darkest = *column.iter().min().unwrap();
        let lightest = *column.iter().max().unwrap();
        let first = column.iter().position(|value| *value == darkest).unwrap();
        let last = column.iter().rposition(|value| *value == darkest).unwrap();
        // The sun reaches the column from whichever end is the brighter one, so the
        // shade ends on that side of its own darkest run.
        if column[0] > column[column.len() - 1] {
            let from = (0..first)
                .rev()
                .find(|index| column[*index] + 1 >= lightest)
                .unwrap_or(0);
            &column[from..=first]
        } else {
            let to = (last + 1..column.len())
                .find(|index| column[*index] + 1 >= lightest)
                .unwrap_or(column.len() - 1);
            &column[last..=to]
        }
    }

    /// How much a column changes by more than the shade it lies on asks for: the
    /// average of its second differences, which a ramp falling at an even rate answers
    /// with nothing and a saw-tooth of one brightness per pixel answers in full. A
    /// stepped edge is a staircase and a grainy one is a zigzag, and both are rough;
    /// a woven edge that a person calls smooth is neither.
    fn roughness(column: &[u8]) -> f32 {
        let steps: Vec<f32> = column
            .windows(3)
            .map(|three| f32::from(three[0]) - 2.0 * f32::from(three[1]) + f32::from(three[2]))
            .collect();
        if steps.is_empty() {
            0.0
        } else {
            steps.iter().map(|step| step.abs()).sum::<f32>() / steps.len() as f32
        }
    }

    /// How much brightness a column gives back on its way into the shade. A fade
    /// answers with nothing; a weave that lays a different handful of readings in
    /// every pixel climbs again where its neighbour fell, and that swing is the grain
    /// a person sees. A column only ever gives back less than it was paid, so the
    /// smaller of its two totals is what it gave back, which way ever it runs.
    fn reversals(column: &[u8]) -> u32 {
        let (rises, falls) = column.windows(2).fold((0_u32, 0_u32), |ran, pair| {
            let (up, down) = ran;
            if pair[1] > pair[0] {
                (up + u32::from(pair[1] - pair[0]), down)
            } else {
                (up, down + u32::from(pair[0] - pair[1]))
            }
        });
        rises.min(falls)
    }

    #[test]
    fn a_straight_edge_weaves_where_a_blocky_one_lays_steps() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        // Framed on the end of the shade, close enough that one texel of the cast is
        // several pixels across: the distance a staircase has room to show itself at.
        let mut scene = shade_end(1.6);
        scene.set_light_quality(LightQuality::Medium);

        let mut down = |scene: &Scene| -> Vec<u8> {
            let pixels = renderer.read_pixels(scene).unwrap();
            (0..64).map(|row| pixel(&pixels, 64, 32, row)[0]).collect()
        };

        scene.set_light_style(LightType::Blocky);
        let blocky = down(&scene);
        scene.set_light_style(LightType::Straight);
        let straight = down(&scene);

        // The look only says how the edge is drawn; a shade is a shade either way.
        let spread = |column: &[u8]| -> i32 {
            i32::from(*column.iter().max().unwrap()) - i32::from(*column.iter().min().unwrap())
        };
        assert!(
            spread(&blocky) > 40 && spread(&straight) > 40,
            "both looks should still lay the same shade: {} against {}",
            spread(&blocky),
            spread(&straight)
        );

        // The look is only drawn on the end of a shade, so that is where it is read:
        // the umbra and the open floor are one brightness in either look.
        let (blocky_steps, blocky_plate) = weave(ending(&blocky));
        let (straight_steps, straight_plate) = weave(ending(&straight));
        assert!(
            blocky_plate >= 3,
            "a texel several pixels across should hold one brightness that long: {blocky_plate}"
        );
        assert!(
            straight_steps > blocky_steps * 2,
            "a woven edge changes brightness far oftener than a stepped one: \
             {straight_steps} against {blocky_steps}"
        );
        assert!(
            straight_plate * 2 < blocky_plate,
            "the same run breaks up once the edge is woven: {straight_plate} against \
             {blocky_plate}"
        );
    }

    #[test]
    fn the_bare_world_is_the_bare_world_whatever_the_light_type_says() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        flat(&mut scene);
        card(&mut scene, "cube", Vec3::ONE, Vec3::ZERO, [1.0, 0.0, 0.0]);

        scene.set_light_style(LightType::Straight);
        let straight = renderer.read_pixels(&scene).unwrap();
        scene.set_light_style(LightType::Blocky);
        let blocky = renderer.read_pixels(&scene).unwrap();
        assert_eq!(
            straight, blocky,
            "a rung that draws no edge has no look to draw it with"
        );
    }

    #[test]
    fn perspective_makes_a_farther_cube_smaller() {
        let mut near = bare();
        near.set_sky_enabled(false);
        near.set_camera_position(Vec3::new(0.0, -4.0, 0.5));
        near.set_camera_target(Vec3::ZERO);
        card(&mut near, "cube", Vec3::ONE, Vec3::ZERO, [1.0, 0.0, 0.0]);

        let mut far = bare();
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
        let path = directory.join("two-rows.ktx2");
        // Two whole block rows apiece, so each half of the picture is flat BC7 blocks
        // and a texel comes back within a step or two of the colour packed in.
        let mut pixels = Vec::new();
        for y in 0..8 {
            for _ in 0..8 {
                pixels.extend_from_slice(if y < 4 {
                    &[255, 0, 0, 255]
                } else {
                    &[0, 0, 255, 255]
                });
            }
        }
        std::fs::write(&path, carbine_ktx::pack(8, 8, &pixels, true).unwrap()).unwrap();

        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
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
        // The top of the picture is the top of the tile on screen. BC7 spends a level
        // or two even on a flat block, so the ask is the same colour, not the same byte.
        let top = pixel(&pixels, 64, 32, 20);
        let bottom = pixel(&pixels, 64, 32, 44);
        assert!(
            top[0] > 248 && top[1] < 8 && top[2] < 8,
            "the top half should read red: {top:?}"
        );
        assert!(
            bottom[2] > 248 && bottom[0] < 8 && bottom[1] < 8,
            "the bottom half should read blue: {bottom:?}"
        );

        std::fs::remove_dir_all(&directory).ok();
    }

    /// A skin of whole left and right halves, so a face carrying the picture once
    /// shows one edge between the two halves and a face measured in more than one
    /// tile of it shows one edge per tile.
    fn stripes(stem: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("carbine-tiling-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{stem}.ktx2"));
        let mut pixels = Vec::new();
        for _ in 0..8 {
            for x in 0..8 {
                pixels.extend_from_slice(if x < 4 {
                    &[255, 0, 0, 255]
                } else {
                    &[0, 0, 255, 255]
                });
            }
        }
        std::fs::write(&path, carbine_ktx::pack(8, 8, &pixels, true).unwrap()).unwrap();
        path
    }

    /// Which half of the striped skin a pixel came from. Shading spends red and blue
    /// alike, so the ask survives a dark corner of the face.
    fn half(pixels: &[u8], size: u32, x: u32, y: u32) -> Option<bool> {
        let [red, _green, blue] = pixel(pixels, size, x, y);
        match red.cmp(&blue) {
            std::cmp::Ordering::Greater => Some(true),
            std::cmp::Ordering::Less => Some(false),
            std::cmp::Ordering::Equal => None,
        }
    }

    /// How often the busiest straight line through the middle of the frame crosses
    /// from one half of the skin to the other, along or across. The picture only
    /// changes along one of its own axes, so this counts tiles laid down, whichever
    /// way the face happens to be turned.
    fn crossings(pixels: &[u8], size: u32) -> usize {
        let mut best = 0;
        for line in size / 4..3 * size / 4 {
            for along in 0..2 {
                let mut count = 0;
                let mut last: Option<bool> = None;
                for step in size / 8..7 * size / 8 {
                    let side = if along == 0 {
                        half(pixels, size, step, line)
                    } else {
                        half(pixels, size, line, step)
                    };
                    if let Some(side) = side {
                        count += usize::from(last.is_some_and(|previous| previous != side));
                        last = Some(side);
                    }
                }
                best = best.max(count);
            }
        }
        best
    }

    #[test]
    fn a_wide_face_gets_more_pictures_laid_on_it_instead_of_one_stretched() {
        let skin = stripes("wide");
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        flat(&mut scene);
        let floor = card(
            &mut scene,
            "plane",
            Vec3::new(2.0, 2.0, 1.0),
            Vec3::ZERO,
            [1.0, 1.0, 1.0],
        );
        scene
            .set_texture(floor, Some(skin.to_str().unwrap().to_owned()))
            .unwrap();

        // Two metres of world span the 64 pixels, so the middle of the frame is a
        // clean run down the face with its edges well outside the count.
        let tiled = crossings(&renderer.read_pixels(&scene).unwrap(), 64);

        scene.set_tiling(floor, 0.0).unwrap();
        let smeared = crossings(&renderer.read_pixels(&scene).unwrap(), 64);

        assert_eq!(
            smeared, 1,
            "one picture smeared across the face should meet its own seam once"
        );
        assert!(
            tiled >= 3,
            "a two metre face at a metre per tile should carry its picture twice over: {tiled}"
        );

        // And a tighter tile is the ask a floor tile is measured in.
        scene.set_tiling(floor, 0.5).unwrap();
        let close = crossings(&renderer.read_pixels(&scene).unwrap(), 64);
        assert!(
            close > tiled,
            "half a metre per tile should lay the picture closer than a metre: {close} \
             against {tiled}"
        );

        std::fs::remove_file(&skin).ok();
    }

    #[test]
    fn a_face_exactly_a_metre_across_wears_the_picture_it_always_wore() {
        let skin = stripes("metre");
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        flat(&mut scene);
        let tile = card(
            &mut scene,
            "plane",
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::ZERO,
            [1.0, 1.0, 1.0],
        );
        scene
            .set_texture(tile, Some(skin.to_str().unwrap().to_owned()))
            .unwrap();

        let laid = renderer.read_pixels(&scene).unwrap();
        scene.set_tiling(tile, 0.0).unwrap();
        let smeared = renderer.read_pixels(&scene).unwrap();

        // A metre of face is exactly one tile, so measuring the face out moves nothing
        // a script could see. The one ask a wrapping sampler cannot answer byte for
        // byte is the half texel blend past a picture's own edge, which takes the
        // fringe from the far side of the skin instead of the nearest colour, so the
        // ask is made well inside the four borders: a metre of it here, a texel of the
        // skin is eight pixels of this frame.
        let mut apart = Vec::new();
        for y in 24..40u32 {
            for x in 24..40u32 {
                let at = ((y * 64 + x) * 4) as usize;
                if laid[at..at + 4] != smeared[at..at + 4] {
                    apart.push([x, y]);
                }
            }
        }
        assert!(apart.is_empty(), "the two frames part at {apart:?}");

        std::fs::remove_file(&skin).ok();
    }

    #[test]
    fn a_shell_is_measured_round_it_rather_than_across_a_face() {
        let skin = stripes("shell");
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        flat(&mut scene);
        let ball = card(
            &mut scene,
            "sphere",
            Vec3::splat(2.0),
            Vec3::ZERO,
            [1.0, 1.0, 1.0],
        );
        // Tip the poles into the plane of the screen so the busiest line runs along
        // the equator instead of spiralling into a pole.
        scene
            .set_vec_field(ball, VecField::RotationEuler, Vec3::new(90.0, 0.0, 0.0))
            .unwrap();
        scene
            .set_texture(ball, Some(skin.to_str().unwrap().to_owned()))
            .unwrap();

        scene.set_tiling(ball, 0.0).unwrap();
        let smeared = crossings(&renderer.read_pixels(&scene).unwrap(), 64);
        scene.set_tiling(ball, 0.5).unwrap();
        let tiled = crossings(&renderer.read_pixels(&scene).unwrap(), 64);

        // Half a metre of tile round a two metre ball is about six pictures to the
        // turn, and the near half of the shell shows several of them.
        assert!(
            tiled >= 4 && tiled > smeared * 2,
            "a tiled shell should repeat along its arc, not smear: {smeared} against {tiled}"
        );

        std::fs::remove_file(&skin).ok();
    }

    /// The same two metres of world framed side on from wherever the lens stands, with
    /// the world's own up at the top of the frame and the sun behind it, so a wall is
    /// measured the way a person standing in front of it sees it.
    fn seen_from(scene: &mut Scene, from: Vec3) {
        scene.set_sky_enabled(false);
        scene.set_sky_sun(from.normalize());
        scene.set_camera_mode(CameraMode::Orthographic);
        scene.set_camera_position(from);
        scene.set_camera_target(Vec3::ZERO);
        scene.set_camera_view_height(2.0);
    }

    #[test]
    fn a_box_wears_its_skin_the_same_way_round_on_every_wall() {
        let skin = stripes("walls");
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        let block = card(
            &mut scene,
            "cube",
            Vec3::new(4.0, 4.0, 2.0),
            Vec3::ZERO,
            [1.0, 1.0, 1.0],
        );
        scene
            .set_texture(block, Some(skin.to_str().unwrap().to_owned()))
            .unwrap();
        // The picture laid once across each face, so the line between its two halves is
        // the mesh's own uv seam and nothing the tiling maths could move.
        scene.set_tiling(block, 0.0).unwrap();

        for from in [
            Vec3::new(0.0, -10.0, 0.0),
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::new(-10.0, 0.0, 0.0),
            Vec3::new(10.0, 0.0, 0.0),
        ] {
            seen_from(&mut scene, from);
            let pixels = renderer.read_pixels(&scene).unwrap();
            let (left, right) = (half(&pixels, 64, 20, 16), half(&pixels, 64, 44, 16));
            let above = half(&pixels, 64, 20, 48);
            let under = half(&pixels, 64, 44, 48);

            // A wall's picture runs across it and never up it, whichever of the four
            // walls the lens is on: two pixels one over the other wear the same half,
            // and two side by side wear different ones. Every wall is authored with its
            // u along the ground and its v up the height, which is what makes a pattern
            // meet itself at a corner instead of one wall's turning it.
            assert!(
                left.is_some() && right.is_some(),
                "the wall looked at from {from} is not wearing the skin at all"
            );
            assert_eq!(left, above, "the wall from {from} changes half going down");
            assert_eq!(right, under, "the wall from {from} changes half going down");
            assert_ne!(
                left, right,
                "the two halves should meet along a vertical line on the wall from {from}"
            );
        }

        std::fs::remove_file(&skin).ok();
    }

    #[test]
    fn a_tile_covers_the_same_span_of_a_wall_as_of_a_floor() {
        let skin = stripes("density");
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let path = skin.to_str().unwrap().to_owned();

        // Three faces four metres along the axis their picture runs along: a wall two
        // metres high, the same wall three times taller, and a floor. The walls are
        // looked at from the side their picture climbs away on, which is the face the
        // measurement used to take out of the wall's height instead of its length. Only
        // a metre and a half of each is on screen, through the same two metre window.
        let mut faces: Vec<(Scene, NodeId)> = Vec::new();
        for size in [Vec3::new(4.0, 0.5, 2.0), Vec3::new(4.0, 0.5, 6.0)] {
            let mut scene = bare();
            let wall = card(&mut scene, "cube", size, Vec3::ZERO, [1.0, 1.0, 1.0]);
            scene.set_texture(wall, Some(path.clone())).unwrap();
            seen_from(&mut scene, Vec3::new(0.0, 10.0, 0.0));
            faces.push((scene, wall));
        }
        let mut floor = bare();
        flat(&mut floor);
        let ground = card(
            &mut floor,
            "plane",
            Vec3::new(4.0, 4.0, 1.0),
            Vec3::ZERO,
            [1.0, 1.0, 1.0],
        );
        floor.set_texture(ground, Some(path)).unwrap();
        faces.push((floor, ground));

        // A metre per tile is a metre of wall or a metre of floor, and the face's other
        // dimensions have no say in it: measuring a wall along its thin axis instead of
        // the long one is exactly the pull that stretches a skin out of shape. The
        // striped picture turns once to a tile, so a metre and a half of window meets
        // its seams three times over at a metre apiece - and four times as fine a tile
        // meets them several times as often, wherever the face happens to be standing.
        for (metres, least, most) in [(1.0, 2, 4), (0.25, 8, 16)] {
            let mut found = Vec::new();
            for (scene, face) in faces.iter_mut() {
                scene.set_tiling(*face, metres).unwrap();
                let seams = crossings(&renderer.read_pixels(scene).unwrap(), 64);
                assert!(
                    seams >= least && seams <= most,
                    "a four metre face at {metres} m per tile should meet its seam about \
                     {} times across the window: {seams}",
                    most / 2
                );
                found.push(seams);
            }
            assert!(
                found.windows(2).all(|pair| pair[0] == pair[1]),
                "a wall, a taller wall and a floor of the same width should wear the \
                 same tile the same size: {found:?} at {metres} m per tile"
            );
        }

        // And the same three faces, left to lie one picture across the whole of them,
        // meet that seam once rather than the several times a metre of tile finds it.
        for (scene, face) in faces.iter_mut() {
            scene.set_tiling(*face, 0.0).unwrap();
            assert_eq!(
                crossings(&renderer.read_pixels(scene).unwrap(), 64),
                1,
                "one smeared picture across the face meets its own seam once"
            );
        }

        std::fs::remove_file(&skin).ok();
    }

    /// A tile as the generator would put it in a script's hands - a whole real
    /// surface, packed to KTX2 with its mip chain - so a test can be about the look of
    /// a pattern rather than about two flat colours.
    fn woven(stem: &str, kind: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("carbine-tiling-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{stem}.ktx2"));
        let bytes =
            carbine_core::generate_texture(kind, None, Some("low"), carbine_core::Seed::of(stem))
                .unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// How far, in eight-bit colour levels, the average pixel of the frame sits from
    /// the pixel `across` steps further along the skin's run - or up it, whichever of
    /// the two axes a tile is laid along. Only the band of columns (or rows) named, so
    /// a caller can keep clear of a frame's borders.
    fn apart(pixels: &[u8], size: u32, across: u32, from: u32, to: u32, up: bool) -> f32 {
        let mut total = 0.0f64;
        let mut count = 0.0f64;
        for step in 6..size - 6 {
            for line in from..to {
                let (x, y) = if up { (step, line) } else { (line, step) };
                let here = pixel(pixels, size, x, y);
                let there = if up {
                    pixel(pixels, size, x, y + across)
                } else {
                    pixel(pixels, size, x + across, y)
                };
                for channel in 0..3 {
                    total += here[channel].abs_diff(there[channel]) as f64;
                    count += 1.0;
                }
            }
        }
        (total / count) as f32
    }

    /// The surface's own grain: how far the colour moves over the few pixels a person
    /// standing in front of it can resolve. This is the yardstick for a tile-to-tile
    /// step - a grid of copies shows nothing at all a whole tile along, and one surface
    /// shows its grain there too.
    fn grain(pixels: &[u8], size: u32, from: u32, to: u32, up: bool) -> f32 {
        let mut total = 0.0f64;
        for across in 1..=8u32 {
            total += apart(pixels, size, across, from, to, up) as f64;
        }
        (total / 8.0) as f32
    }

    #[test]
    fn a_tiled_floor_reads_as_one_floor_rather_than_a_grid_of_copies() {
        let skin = woven("grassed", "grass");
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        flat(&mut scene);
        let ground = card(
            &mut scene,
            "plane",
            Vec3::new(4.0, 4.0, 1.0),
            Vec3::ZERO,
            [1.0, 1.0, 1.0],
        );
        scene
            .set_texture(ground, Some(skin.to_str().unwrap().to_owned()))
            .unwrap();

        let pixels = renderer.read_pixels(&scene).unwrap();
        // Two tiles of this skin stand in the two metre window, so thirty two pixels of
        // this frame is one whole tile: the left half of the floor and the right half
        // are the same picture laid side by side, and so is the top against the bottom.
        // Stamped over, a pixel and its tile twin wear precisely the same colour and
        // that distance reads nothing at all - which is the grid of copypasted pictures
        // a person sees before anything else. Running each copy off its lattice and
        // wearing it to its own tone spends a fair share of the surface's own grain
        // there instead, whichever of the skin's two axes it is measured along.
        for up in [false, true] {
            let grain = grain(&pixels, 64, 2, 30, up);
            let tile = apart(&pixels, 64, 32, 2, 30, up);
            assert!(
                grain > 1.0,
                "the floor is wearing a flat colour, so nothing below proves anything: {grain}"
            );
            assert!(
                tile * 3.0 > grain,
                "a whole tile along the skin should still move the picture about as much \
                 as the surface does over a few pixels, not a third of it less: {tile} \
                 against a grain of {grain}"
            );

            // And the running must not be a jump on the lattice. The line where two
            // tiles meet is one whole tile from the edge of the frame, so the step
            // across it is measured against the steps the surface makes everywhere
            // else: a picture that breaks there would be a new seam, and the ask is a
            // smoother surface, not a gridded one with the grid painted over.
            let joined = apart(&pixels, 64, 1, 31, 32, up);
            let open = (apart(&pixels, 64, 1, 2, 31, up) + apart(&pixels, 64, 1, 34, 62, up)) / 2.0;
            assert!(
                joined < open * 2.0,
                "the line where two tiles meet is harder than the surface's own detail: \
                 {joined} across it against {open} everywhere else"
            );
        }

        // A face of exactly one tile is never run in at all, so it keeps the picture the
        // file drew it with, byte for byte - see
        // `a_face_exactly_a_metre_across_wears_the_picture_it_always_wore`.
        std::fs::remove_file(&skin).ok();
    }

    #[test]
    fn transparency_blends_a_world_object_over_what_is_behind_it() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
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
        let mut scene = bare();
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

    /// The dust a fresh backdrop carries, read through a lens narrow enough that one
    /// blob of it lies across a dozen pixels: an eight degree lens shows five degrees
    /// of the dome, and the haze is laid a degree and a half wide, so a run down the
    /// frame is a handful of blobs and not a throw of the dice per pixel.
    ///
    /// A static answers a separate question in every pixel: over a run of this reach it
    /// costs about seven levels of roughness and gives back most of the brightness it
    /// walked. A haze a pixel shares with its neighbours costs about one and gives back a
    /// tenth. Being centred, it never tints the backdrop it lies on, and being drawn at
    /// all, it leaves its mark somewhere on the frame.
    #[test]
    fn the_backdrop_carries_its_dust_as_a_haze_and_not_a_static() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        scene.set_camera_mode(CameraMode::Perspective);
        scene.set_camera_fov(8.0);
        scene.set_camera_position(Vec3::new(0.0, 0.0, 0.0));
        scene.set_camera_target(Vec3::new(0.0, -1.0, 0.6));

        scene.set_sky_noise(0.0);
        let still = renderer.read_pixels(&scene).unwrap();
        scene.set_sky_noise(Scene::new().sky().noise);
        let dusty = renderer.read_pixels(&scene).unwrap();

        assert_ne!(
            still, dusty,
            "the dust the backdrop is asked to carry is not drawn"
        );

        let mean =
            |run: &[u8]| run.iter().map(|level| f32::from(*level)).sum::<f32>() / run.len() as f32;

        for column in [16, 32, 48] {
            let without: Vec<u8> = (0..64)
                .map(|row| pixel(&still, 64, column, row)[0])
                .collect();
            let with: Vec<u8> = (0..64)
                .map(|row| pixel(&dusty, 64, column, row)[0])
                .collect();

            let rough = roughness(&with);
            assert!(
                rough <= 2.0,
                "the backdrop is laid {} levels rough, which is a static and not a haze: \
                 {with:?}",
                rough,
            );
            let gave_back = reversals(&with);
            assert!(
                gave_back <= 24,
                "the backdrop gives back {gave_back} levels of its own run, which is a \
                 static and not a haze: {with:?}"
            );
            assert!(
                (mean(&with) - mean(&without)).abs() <= 2.0,
                "the dust moved the backdrop's colour, so it leans one way: {:?} against \
                 {:?}",
                mean(&with),
                mean(&without),
            );
        }
    }

    #[test]
    fn switching_the_sky_off_leaves_the_clear_colour() {
        let mut renderer = Renderer::offscreen(32, 32).unwrap();
        let mut scene = bare();
        scene.set_sky_enabled(false);

        let pixels = renderer.read_pixels(&scene).unwrap();
        assert_eq!(pixel(&pixels, 32, 16, 16), [22, 21, 20]);
    }

    #[test]
    fn offscreen_rendering_reports_the_adapter_it_found() {
        let renderer = Renderer::offscreen(8, 8).unwrap();
        assert!(renderer.adapter().is_some_and(|name| !name.is_empty()));
    }

    /// Precipitation is a particle crowd hung on the sky, so it shows as the drops
    /// the core laid around the eyes rather than a pattern smeared over the frame.
    #[test]
    fn rain_flecks_the_frame_it_falls_through() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        side_on(&mut scene);
        let rain = scene.create_precipitation("rain", None).unwrap();
        scene.set_color_hex(rain, "#ffffff").unwrap();
        // A real sky veil is spread over the whole map; a 64 pixel frame only ever
        // holds a two metre box of it, so the box is closed in on the eyes.
        veil_over_the_frame(&mut scene, rain);

        let wet = renderer.read_pixels(&scene).unwrap();
        scene.set_particles_active(rain, false).unwrap();
        let dry = renderer.read_pixels(&scene).unwrap();

        assert_eq!(&dry[0..4], &[22, 21, 20, 255]);
        let brighter = (0..64 * 64)
            .filter(|index| wet[index * 4 + 1] > dry[index * 4 + 1])
            .count();
        assert!(brighter > 400, "only {brighter} pixels caught a drop");
    }

    /// The steps the world takes move the crowd, so the same weather drawn a few
    /// steps later has come down the frame.
    #[test]
    fn the_weather_slides_as_the_world_steps() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        side_on(&mut scene);
        let snow = scene.create_precipitation("snow", None).unwrap();
        veil_over_the_frame(&mut scene, snow);

        let early = renderer.read_pixels(&scene).unwrap();
        for _ in 0..3 {
            scene.step(0.25);
        }
        // No player takes the step here, so the eyes are still where they were put
        // and only the snow has had anywhere to go.
        let late = renderer.read_pixels(&scene).unwrap();

        assert_ne!(early, late, "the snowflakes never moved");
    }

    /// The framing `flat` gives, turned to look along the ground. A veil hung on the
    /// sky stands *over* the eyes rather than through them, so a lens pointed down
    /// at the world has nothing in front of it left to show but the top of the rain.
    fn side_on(scene: &mut Scene) {
        flat(scene);
        scene.set_camera_position(Vec3::new(0.0, -4.0, 1.0));
        scene.set_camera_target(Vec3::new(0.0, 0.0, 1.0));
    }

    /// Lay a sky veil so that a test frame can actually hold drops of it: a two
    /// metre box over the eyes, with drops big enough to be a handful of pixels
    /// rather than a sub-pixel smear the sampler never resolves.
    fn veil_over_the_frame(scene: &mut Scene, veil: NodeId) {
        scene
            .set_particle_area(veil, Vec3::new(2.0, 2.0, 2.0))
            .unwrap();
        scene.set_particle_count(veil, 300).unwrap();
        scene.set_particle_size(veil, 0.1).unwrap();
        scene.restart_particles(veil).unwrap();
    }

    /// Fog is the air rather than anything falling through it, so it takes the far
    /// end of a field toward its own colour and leaves the near end alone.
    #[test]
    fn a_fog_takes_the_far_side_of_a_field_away() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        flat(&mut scene);
        // The eyes look down from ten meters, so one card is two meters away and
        // the other is the full ten.
        card(
            &mut scene,
            "rectangle",
            Vec3::new(0.4, 0.4, 1.0),
            Vec3::new(0.0, 0.0, 8.0),
            [1.0, 0.0, 0.0],
        );
        card(
            &mut scene,
            "rectangle",
            Vec3::new(0.4, 0.4, 1.0),
            Vec3::new(0.5, 0.0, 0.0),
            [1.0, 0.0, 0.0],
        );

        let clear = renderer.read_pixels(&scene).unwrap();
        assert_eq!(pixel(&clear, 64, 48, 32), [255, 0, 0], "far, no fog");

        scene.set_fog(60.0);
        scene.set_fog_color_hex("#ffffff").unwrap();
        let hazy = renderer.read_pixels(&scene).unwrap();
        let near = pixel(&hazy, 64, 32, 32);
        let far = pixel(&hazy, 64, 48, 32);

        assert_eq!(near[0], 255);
        assert_eq!(far[0], 255);
        assert!(
            far[1] as i32 - near[1] as i32 > 40,
            "the far card should grey out before the near one: {near:?} vs {far:?}"
        );
    }

    /// The crowd is drawn over the world and under the interface, because a widget
    /// is measured in pixels and no weather is allowed to wash one out.
    #[test]
    fn the_weather_falls_behind_the_interface() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        button(&mut scene);
        let rain = scene.create_precipitation("rain", None).unwrap();
        scene.set_color_hex(rain, "#ffffff").unwrap();
        // The veil is laid after the camera turns, so the crowd is written over the
        // eyes that are about to look through it.
        side_on(&mut scene);
        veil_over_the_frame(&mut scene, rain);

        let wet = renderer.read_pixels(&scene).unwrap();
        scene.set_particles_active(rain, false).unwrap();
        let dry = renderer.read_pixels(&scene).unwrap();

        let moved = (0..64 * 64)
            .filter(|index| wet[index * 4..index * 4 + 3] != dry[index * 4..index * 4 + 3])
            .count();
        assert!(
            moved > 400,
            "the rain painted only {moved} pixels for the button to sit over"
        );
        for (x, y) in [(32, 32), (14, 14), (49, 49)] {
            assert_eq!(
                pixel(&dry, 64, x, y),
                [235, 122, 52],
                "the button keeps its own colour"
            );
            assert_eq!(
                pixel(&wet, 64, x, y),
                [235, 122, 52],
                "and keeps it with the rain falling over it"
            );
        }
    }

    /// One drop, alone in the middle of the frame: a system holding a single thing,
    /// given no box to be scattered in and so standing exactly where its node does.
    fn drop(scene: &mut Scene, shape: &str, size: f32) -> NodeId {
        flat(scene);
        let crowd = scene.create_particles(None, None).unwrap();
        scene.set_particle_count(crowd, 1).unwrap();
        scene.set_particle_area(crowd, Vec3::ZERO).unwrap();
        scene.set_particle_size(crowd, size).unwrap();
        scene.set_particle_shape(crowd, shape).unwrap();
        scene.set_color_hex(crowd, "#ff0000").unwrap();
        scene.set_particles_active(crowd, false).unwrap();
        scene.set_particles_active(crowd, true).unwrap();
        crowd
    }

    /// How many pixels the frame holds that are not the clear colour.
    fn painted(pixels: &[u8]) -> usize {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|rgba| **rgba != [22, 21, 20, 255])
            .count()
    }

    /// A drop is a thing in the world, so it lands on the pixels the core says it
    /// stands at rather than being smeared over the whole frame.
    #[test]
    fn a_single_drop_is_painted_where_the_core_puts_it() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        let crowd = drop(&mut scene, "circle", 0.4);
        assert_eq!(scene.particles_in_air(crowd).unwrap(), 1);
        assert_eq!(scene.particle_items()[0].at, Vec3::ZERO);

        let pixels = renderer.read_pixels(&scene).unwrap();
        let middle = pixel(&pixels, 64, 32, 32);
        assert!(
            middle[0] > 150 && middle[1] < 80 && middle[2] < 80,
            "the middle of the crowd is {middle:?}, not the drop's own red"
        );
        assert_eq!(
            pixel(&pixels, 64, 2, 61),
            [22, 21, 20],
            "the frame's corner is air"
        );
    }

    /// The crowd is geometry, so it is tested against the depth the world wrote:
    /// a body put in front of a drop takes it out of the picture.
    #[test]
    fn a_wall_in_front_hides_the_drop_behind_it() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        drop(&mut scene, "circle", 0.4);

        let clear = renderer.read_pixels(&scene).unwrap();
        assert!(
            pixel(&clear, 64, 32, 32)[0] > 150,
            "nothing was in front of it yet"
        );

        card(
            &mut scene,
            "cube",
            Vec3::new(2.0, 2.0, 1.0),
            Vec3::new(0.0, 0.0, 1.0),
            [0.0, 1.0, 0.0],
        );
        let walled = renderer.read_pixels(&scene).unwrap();
        assert_eq!(
            pixel(&walled, 64, 32, 32),
            [0, 255, 0],
            "the drop shone through the body in front of it"
        );
    }

    /// A streak is the same width as the square it was offered and six times as
    /// long along the way it travels, which is a lot more of the frame.
    #[test]
    fn a_streak_covers_more_of_the_frame_than_a_square() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();

        let mut square_scene = bare();
        drop(&mut square_scene, "square", 0.4);
        let square = painted(&renderer.read_pixels(&square_scene).unwrap());

        let mut streak_scene = bare();
        drop(&mut streak_scene, "streak", 0.4);
        let streak = painted(&renderer.read_pixels(&streak_scene).unwrap());

        assert!(square > 20, "a square drop painted only {square} pixels");
        assert!(
            streak > square * 2,
            "a streak of the same size painted {streak}, a square painted {square}"
        );
    }

    /// A system switched off still exists and still keeps its settings; it simply
    /// has nothing in the air to draw.
    #[test]
    fn a_switched_off_system_draws_nothing_at_all() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
        let crowd = drop(&mut scene, "circle", 0.4);
        assert!(painted(&renderer.read_pixels(&scene).unwrap()) > 20);

        scene.set_particles_active(crowd, false).unwrap();
        assert_eq!(painted(&renderer.read_pixels(&scene).unwrap()), 0);
        assert_eq!(scene.particle_count(crowd).unwrap(), 1);
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
        let mut scene = bare();
        button(&mut scene);

        let pixels = renderer.read_pixels(&scene).unwrap();
        assert_eq!(pixel(&pixels, 64, 14, 14), [235, 122, 52]);
        assert_eq!(pixel(&pixels, 64, 49, 49), [235, 122, 52]);
        assert_eq!(pixel(&pixels, 64, 9, 9), [22, 21, 20]);
    }

    #[test]
    fn a_corner_value_cuts_the_corner_and_keeps_the_middle() {
        let mut renderer = Renderer::offscreen(64, 64).unwrap();
        let mut scene = bare();
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
        let mut scene = bare();
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
        let mut scene = bare();
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
        let mut scene = bare();
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
        let mut scene = bare();
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

    fn ui_pixel_mask(
        pixels: &[u8],
        width: u32,
        accepts: impl Fn([u8; 3]) -> bool,
    ) -> Vec<(u32, u32)> {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, rgba)| accepts([rgba[0], rgba[1], rgba[2]]))
            .map(|(index, _)| (index as u32 % width, index as u32 / width))
            .collect()
    }

    fn text_fill_mask(pixels: &[u8], width: u32) -> Vec<(u32, u32)> {
        ui_pixel_mask(pixels, width, |rgb| {
            rgb.into_iter().all(|channel| channel > 128)
        })
    }

    fn mask_bounds(mask: &[(u32, u32)]) -> [u32; 4] {
        assert!(!mask.is_empty(), "the glyph mask must not be empty");
        mask.iter().fold(
            [u32::MAX, u32::MAX, 0, 0],
            |[left, top, right, bottom], &(x, y)| {
                [left.min(x), top.min(y), right.max(x), bottom.max(y)]
            },
        )
    }

    fn mask_row_bands(mask: &[(u32, u32)]) -> Vec<(u32, u32)> {
        let mut bands: Vec<(u32, u32)> = Vec::new();
        for &(_, y) in mask {
            match bands.last_mut() {
                Some((_, last)) if y <= *last + 1 => *last = y,
                _ => bands.push((y, y)),
            }
        }
        bands
    }

    fn text_label(
        scene: &mut Scene,
        content: &str,
        position: Vec3,
        parent: Option<NodeId>,
    ) -> NodeId {
        let label = scene.create_ui("Text", None, parent).unwrap();
        scene.set_text_content(label, content.to_owned()).unwrap();
        scene
            .set_vec_field(label, VecField::Position, position)
            .unwrap();
        scene.set_color_hex(label, "#ffffff").unwrap();
        label
    }

    #[test]
    fn ui_text_rasterizes_glyphs_and_invalidates_changed_content() {
        let mut renderer = Renderer::offscreen(256, 96).unwrap();
        let mut scene = bare();
        scene.set_sky_enabled(false);
        let label = scene.create_ui("Text", None, None).unwrap();
        scene
            .set_vec_field(label, VecField::Position, Vec3::new(16.0, 12.0, 0.0))
            .unwrap();
        scene.set_color_hex(label, "#ffffff").unwrap();
        assert_eq!(painted(&renderer.read_pixels(&scene).unwrap()), 0);

        scene.set_text_content(label, "OOOO".into()).unwrap();
        let round_pixels = renderer.read_pixels(&scene).unwrap();
        let round = text_fill_mask(&round_pixels, 256);
        let [left, top, right, bottom] = mask_bounds(&round);
        let area = ((right - left + 1) * (bottom - top + 1)) as usize;
        assert!(round.len() > 100, "only {} glyph pixels", round.len());
        assert!(
            round.len() * 2 < area,
            "text must not fill its bounding quad"
        );
        assert!(
            left >= 16 && top >= 12,
            "text starts at its line-box origin"
        );
        assert!(
            right < 136 && bottom < 48,
            "unexpected bounds: {:?}",
            mask_bounds(&round)
        );
        assert!((12..=26).contains(&(bottom - top + 1)));
        assert_eq!(pixel(&round_pixels, 256, 8, 12), [22, 21, 20]);

        scene.set_text_content(label, "HHHH".into()).unwrap();
        let changed_pixels = renderer.read_pixels(&scene).unwrap();
        let changed = text_fill_mask(&changed_pixels, 256);
        let difference = round_pixels
            .as_chunks::<4>()
            .0
            .iter()
            .zip(changed_pixels.as_chunks::<4>().0)
            .filter(|(before, after)| before != after)
            .count();
        assert!(
            difference > 100,
            "same-length content retained stale glyphs"
        );
        assert!(changed.len() > 100);

        scene.set_text_content(label, "H".into()).unwrap();
        let short_pixels = renderer.read_pixels(&scene).unwrap();
        let short = text_fill_mask(&short_pixels, 256);
        let short_bounds = mask_bounds(&short);
        assert!(short.len() > 30 && short.len() * 2 < changed.len());
        assert!(short_bounds[2] + 30 < mask_bounds(&changed)[2]);
        let stale = changed
            .iter()
            .filter(|&&(x, y)| {
                x > short_bounds[2] + 2 && pixel(&short_pixels, 256, x, y) != [22, 21, 20]
            })
            .count();
        assert_eq!(
            stale, 0,
            "shorter content left old glyphs in the draw buffer"
        );

        scene.set_text_content(label, String::new()).unwrap();
        assert_eq!(painted(&renderer.read_pixels(&scene).unwrap()), 0);
        scene.set_text_content(label, "OOOO".into()).unwrap();
        assert!(renderer.read_pixels(&scene).unwrap() == round_pixels);

        scene.set_text_content(label, "?".into()).unwrap();
        let fallback = renderer.read_pixels(&scene).unwrap();
        assert!(text_fill_mask(&fallback, 256).len() > 20);
        scene.set_text_content(label, "界".into()).unwrap();
        assert!(renderer.read_pixels(&scene).unwrap() == fallback);
        scene.set_text_content(label, "é".into()).unwrap();
        let latin = renderer.read_pixels(&scene).unwrap();
        assert!(text_fill_mask(&latin, 256).len() > 30);
        assert!(latin != fallback, "Latin-1 must use its own glyph");
    }

    #[test]
    fn ui_text_font_size_wrap_and_newlines_change_pixel_layout() {
        let mut renderer = Renderer::offscreen(256, 192).unwrap();
        let mut scene = bare();
        scene.set_sky_enabled(false);
        let label = text_label(&mut scene, "HH", Vec3::new(12.0, 8.0, 0.0), None);
        let default_pixels = renderer.read_pixels(&scene).unwrap();
        let small = text_fill_mask(&default_pixels, 256);
        let small_bounds = mask_bounds(&small);
        assert!(small.len() > 60);
        scene.set_text_size(label, 24.0).unwrap();
        assert!(renderer.read_pixels(&scene).unwrap() == default_pixels);

        scene.set_text_size(label, 48.0).unwrap();
        let large = text_fill_mask(&renderer.read_pixels(&scene).unwrap(), 256);
        let large_bounds = mask_bounds(&large);
        assert!(large.len() > small.len() * 2 && large.len() < small.len() * 5);
        for axis in 0..2 {
            let small_span = small_bounds[axis + 2] - small_bounds[axis] + 1;
            let large_span = large_bounds[axis + 2] - large_bounds[axis] + 1;
            assert!(large_span.abs_diff(small_span * 2) <= 4);
        }

        scene.set_text_size(label, 24.0).unwrap();
        scene.set_text_content(label, "HH HH".into()).unwrap();
        let single_pixels = renderer.read_pixels(&scene).unwrap();
        let single = text_fill_mask(&single_pixels, 256);
        assert_eq!(mask_row_bands(&single).len(), 1);
        scene.set_text_width(label, 50.0).unwrap();
        assert!(
            renderer.read_pixels(&scene).unwrap() == single_pixels,
            "width alone must not wrap or clip"
        );

        scene.set_text_word_wrap(label, true).unwrap();
        let wrapped_pixels = renderer.read_pixels(&scene).unwrap();
        let wrapped = text_fill_mask(&wrapped_pixels, 256);
        let bands = mask_row_bands(&wrapped);
        assert_eq!(bands.len(), 2, "wrapped row bands: {bands:?}");
        assert!((26..=31).contains(&(bands[1].0 - bands[0].0)));
        assert!(mask_bounds(&wrapped)[2] < mask_bounds(&single)[2]);
        assert!(wrapped.len() * 5 > single.len() * 4 && wrapped.len() * 5 < single.len() * 6);

        scene.set_text_width(label, 200.0).unwrap();
        assert!(
            renderer.read_pixels(&scene).unwrap() == single_pixels,
            "changing width must invalidate cached layout"
        );
        scene.set_text_width(label, 50.0).unwrap();
        assert!(renderer.read_pixels(&scene).unwrap() == wrapped_pixels);
        scene.set_text_word_wrap(label, false).unwrap();
        assert!(renderer.read_pixels(&scene).unwrap() == single_pixels);
        scene.set_text_content(label, "HH\nHH".into()).unwrap();
        assert!(
            renderer.read_pixels(&scene).unwrap() == wrapped_pixels,
            "explicit newlines work without word wrap"
        );

        scene.set_text_content(label, "HH\n\nHH".into()).unwrap();
        let spaced = text_fill_mask(&renderer.read_pixels(&scene).unwrap(), 256);
        let bands = mask_row_bands(&spaced);
        assert_eq!(bands.len(), 2);
        assert!(
            (54..=60).contains(&(bands[1].0 - bands[0].0)),
            "empty lines retain their line height: {bands:?}"
        );
    }

    #[test]
    fn ui_text_outline_preserves_fill_and_opacity_fades_the_whole_glyph() {
        let mut renderer = Renderer::offscreen(160, 96).unwrap();
        let mut scene = bare();
        scene.set_sky_enabled(false);
        let label = text_label(&mut scene, "OH", Vec3::new(20.0, 12.0, 0.0), None);
        scene.set_text_size(label, 48.0).unwrap();
        let plain = renderer.read_pixels(&scene).unwrap();
        let fill = ui_pixel_mask(&plain, 160, |rgb| rgb.into_iter().all(|c| c > 245));
        let dark = |pixels: &[u8]| ui_pixel_mask(pixels, 160, |rgb| rgb.into_iter().all(|c| c < 8));
        assert!(fill.len() > 100);
        assert!(dark(&plain).is_empty());

        scene.set_text_outline(label, true).unwrap();
        scene.set_text_outline_width(label, 1.5).unwrap();
        scene.set_text_outline_color(label, Vec3::ZERO).unwrap();
        let thin = renderer.read_pixels(&scene).unwrap();
        let thin_dark = dark(&thin).len();
        assert!(thin_dark > 30, "outline drew only {thin_dark} dark pixels");
        assert!(painted(&thin) > painted(&plain));
        scene.set_text_outline_width(label, 3.0).unwrap();
        let thick = renderer.read_pixels(&scene).unwrap();
        assert!(
            dark(&thick).len() * 4 > thin_dark * 5,
            "a wider outline must expand the dark footprint"
        );
        assert!(painted(&thick) > painted(&thin));
        for outlined in [&thin, &thick] {
            let kept = fill
                .iter()
                .filter(|&&(x, y)| pixel(outlined, 160, x, y).into_iter().all(|c| c > 240))
                .count();
            assert!(
                kept * 10 >= fill.len() * 9,
                "the outline replaced the fill: {kept}/{}",
                fill.len()
            );
        }

        scene.set_alpha(label, 128.0).unwrap();
        let faded = renderer.read_pixels(&scene).unwrap();
        let mut worst_error = 0.0_f32;
        for (opaque, translucent) in thick
            .as_chunks::<4>()
            .0
            .iter()
            .zip(faded.as_chunks::<4>().0)
        {
            for (channel, background) in [22.0, 21.0, 20.0].into_iter().enumerate() {
                let expected =
                    background + (f32::from(opaque[channel]) - background) * (128.0 / 255.0);
                worst_error = worst_error.max((f32::from(translucent[channel]) - expected).abs());
            }
        }
        assert!(
            worst_error <= 2.0,
            "fill and outline must share opacity, worst blend error: {worst_error}"
        );
        scene.set_alpha(label, 0.0).unwrap();
        assert_eq!(
            painted(&renderer.read_pixels(&scene).unwrap()),
            0,
            "zero alpha must also remove the outline"
        );
        scene.set_alpha(label, 255.0).unwrap();
        scene.set_text_outline(label, false).unwrap();
        assert!(renderer.read_pixels(&scene).unwrap() == plain);
    }

    #[test]
    fn ui_text_obeys_hierarchy_visibility_and_local_transforms() {
        let mut renderer = Renderer::offscreen(256, 192).unwrap();
        let mut scene = bare();
        scene.set_sky_enabled(false);
        let parent = scene.create_ui("square", None, None).unwrap();
        scene.set_alpha(parent, 0.0).unwrap();
        scene
            .set_vec_field(parent, VecField::Size, Vec3::new(120.0, 80.0, 1.0))
            .unwrap();
        let label = text_label(&mut scene, "HH", Vec3::new(10.0, 8.0, 0.0), Some(parent));
        let original = text_fill_mask(&renderer.read_pixels(&scene).unwrap(), 256);
        let original_bounds = mask_bounds(&original);
        assert!(
            original.len() > 60,
            "a transparent parent must not hide its child"
        );

        scene
            .set_vec_field(parent, VecField::Position, Vec3::new(40.0, 24.0, 0.0))
            .unwrap();
        let moved_pixels = renderer.read_pixels(&scene).unwrap();
        let moved = text_fill_mask(&moved_pixels, 256);
        let moved_bounds = mask_bounds(&moved);
        for (axis, delta) in [40, 24, 40, 24].into_iter().enumerate() {
            assert!(moved_bounds[axis].abs_diff(original_bounds[axis] + delta) <= 1);
        }
        let aligned = original
            .iter()
            .filter(|&&(x, y)| pixel(&moved_pixels, 256, x + 40, y + 24)[0] > 128)
            .count();
        assert!(aligned * 100 >= original.len() * 95);
        assert!(moved.len().abs_diff(original.len()) <= original.len() / 20 + 2);
        scene
            .set_vec_field(parent, VecField::Size, Vec3::new(240.0, 160.0, 1.0))
            .unwrap();
        assert!(
            renderer.read_pixels(&scene).unwrap() == moved_pixels,
            "parent size must not scale child glyphs"
        );

        scene.set_visible(parent, false).unwrap();
        assert_eq!(painted(&renderer.read_pixels(&scene).unwrap()), 0);
        scene.set_visible(parent, true).unwrap();
        scene.set_visible(label, false).unwrap();
        assert_eq!(painted(&renderer.read_pixels(&scene).unwrap()), 0);
        scene.set_visible(label, true).unwrap();
        assert!(renderer.read_pixels(&scene).unwrap() == moved_pixels);

        scene
            .set_vec_field(label, VecField::Scale, Vec3::new(2.0, 2.0, 1.0))
            .unwrap();
        let scaled = text_fill_mask(&renderer.read_pixels(&scene).unwrap(), 256);
        let scaled_bounds = mask_bounds(&scaled);
        assert!(scaled.len() > moved.len() * 2 && scaled.len() < moved.len() * 5);
        for axis in 0..2 {
            let before = moved_bounds[axis + 2] - moved_bounds[axis] + 1;
            let after = scaled_bounds[axis + 2] - scaled_bounds[axis] + 1;
            assert!(after.abs_diff(before * 2) <= 4);
        }

        scene
            .set_vec_field(label, VecField::Scale, Vec3::ONE)
            .unwrap();
        scene
            .set_vec_field(parent, VecField::Position, Vec3::new(120.0, 24.0, 0.0))
            .unwrap();
        scene
            .set_vec_field(parent, VecField::RotationEuler, Vec3::new(0.0, 0.0, 90.0))
            .unwrap();
        let rotated_pixels = renderer.read_pixels(&scene).unwrap();
        let rotated = text_fill_mask(&rotated_pixels, 256);
        let aligned = original
            .iter()
            .filter(|&&(x, y)| pixel(&rotated_pixels, 256, 119 - y, 24 + x)[0] > 128)
            .count();
        assert!(
            aligned * 100 >= original.len() * 90,
            "parent rotation did not rotate the glyph mask"
        );
        assert!(rotated.len().abs_diff(original.len()) <= original.len() / 10 + 2);
    }

    #[test]
    fn ui_text_and_shapes_cover_each_other_in_document_order() {
        let mut renderer = Renderer::offscreen(128, 80).unwrap();
        let mut scene = bare();
        scene.set_sky_enabled(false);
        let parent = scene.create_ui("square", None, None).unwrap();
        scene.set_alpha(parent, 0.0).unwrap();
        let cover = scene.create_ui("square", None, None).unwrap();
        scene
            .set_vec_field(cover, VecField::Position, Vec3::new(64.0, 40.0, 0.0))
            .unwrap();
        scene
            .set_vec_field(cover, VecField::Size, Vec3::new(160.0, 112.0, 1.0))
            .unwrap();
        scene.set_color_hex(cover, "#0000ff").unwrap();
        text_label(&mut scene, "HH", Vec3::new(16.0, 12.0, 0.0), Some(parent));
        let front = text_label(&mut scene, "HH", Vec3::new(16.0, 12.0, 0.0), None);
        scene.set_visible(cover, false).unwrap();
        scene.set_visible(front, false).unwrap();
        let behind = text_fill_mask(&renderer.read_pixels(&scene).unwrap(), 128);
        assert!(behind.len() > 60);

        scene.set_visible(cover, true).unwrap();
        let covered = renderer.read_pixels(&scene).unwrap();
        let blue = ui_pixel_mask(&covered, 128, |rgb| rgb == [0, 0, 255]);
        assert_eq!(
            blue.len(),
            128 * 80,
            "the later shape must cover even text created after it in an earlier branch"
        );
        assert!(text_fill_mask(&covered, 128).is_empty());

        scene.set_visible(front, true).unwrap();
        let on_top = renderer.read_pixels(&scene).unwrap();
        let visible = text_fill_mask(&on_top, 128);
        assert!(visible.len() >= behind.len() * 9 / 10);
        let preserved = behind
            .iter()
            .filter(|&&(x, y)| pixel(&on_top, 128, x, y).into_iter().all(|c| c > 128))
            .count();
        assert!(
            preserved * 10 >= behind.len() * 9,
            "the later text must cover the shape"
        );
        assert_eq!(pixel(&on_top, 128, 0, 0), [0, 0, 255]);
        assert_eq!(pixel(&on_top, 128, 127, 79), [0, 0, 255]);
    }
}
