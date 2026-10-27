mod camera;
mod entity;
mod input;
mod math;
mod mesh;
mod object;
mod player;
mod scene;
mod sky;
mod ui;

use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use carbine_core::{NodeId, Scene, SceneError};
use carbine_render::Renderer;
use pyo3::create_exception;
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBytes};

use crate::camera::PyCamera;
use crate::entity::Entity;
use crate::input::PyMouse;
use crate::math::extract_vec3;
use crate::object::Object;
use crate::player::PyPlayer;
use crate::scene::PyScene;
use crate::sky::PySky;
use crate::ui::PyUi;

create_exception!(carbine, DestroyedError, PyRuntimeError);

/// The whole game lives in one scene, shared by every handle handed to Python.
static SCENE: OnceLock<Mutex<Scene>> = OnceLock::new();

thread_local! {
    /// A winit event loop and a wgpu surface cannot be moved between threads, so
    /// the renderer lives where it was created.
    static RENDERER: RefCell<Option<Renderer>> = const { RefCell::new(None) };

    /// When the last frame was drawn, and how long that frame took. The second
    /// half is what `carbine.dt()` reports after the fact.
    static CLOCK: RefCell<(Option<Instant>, f32)> = const { RefCell::new((None, 0.0)) };
}

/// Seconds since the previous frame, zero for the first one.
fn tick() -> f32 {
    let now = Instant::now();
    CLOCK.with(|cell| {
        let (previous, _) = *cell.borrow();
        let delta = previous.map_or(0.0, |then| (now - then).as_secs_f32());
        *cell.borrow_mut() = (Some(now), delta);
        delta
    })
}

pub(crate) fn scene_handle() -> &'static Mutex<Scene> {
    SCENE.get_or_init(|| Mutex::new(Scene::new()))
}

/// Whether the window is currently holding the pointer for the camera.
pub(crate) fn pointer_grabbed() -> bool {
    RENDERER.with(|cell| {
        cell.borrow()
            .as_ref()
            .is_some_and(|renderer| renderer.pointer_grabbed())
    })
}

/// Hands the scene's pointer wish to the window, and reports what it managed.
pub(crate) fn apply_pointer_grab(wants: bool) -> bool {
    RENDERER.with(|cell| {
        cell.borrow_mut()
            .as_mut()
            .map(|renderer| renderer.set_pointer_grab(wants))
            .unwrap_or(false)
    })
}

fn locked() -> PyResult<std::sync::MutexGuard<'static, Scene>> {
    scene_handle()
        .lock()
        .map_err(|_| PyRuntimeError::new_err("the scene lock was poisoned"))
}

pub(crate) fn with_scene<T>(
    operation: impl FnOnce(&mut Scene) -> Result<T, SceneError>,
) -> PyResult<T> {
    let mut scene = locked()?;
    operation(&mut scene).map_err(scene_error)
}

fn scene_error(error: SceneError) -> PyErr {
    match error {
        SceneError::NodeDestroyed => DestroyedError::new_err("this node has been destroyed"),
        SceneError::MissingComponent => {
            PyRuntimeError::new_err("this node has no such engine component")
        }
        other => PyValueError::new_err(other.to_string()),
    }
}

fn render_error(error: carbine_render::RenderError) -> PyErr {
    PyRuntimeError::new_err(error.to_string())
}

fn needs_renderer() -> PyErr {
    PyRuntimeError::new_err("the renderer is not running; call carbine.init() first")
}

/// Reads the node behind an Object or an Entity handle.
fn node_id_of(value: &Bound<'_, PyAny>) -> PyResult<NodeId> {
    object::handle_id(value).ok_or_else(|| {
        PyTypeError::new_err("expected an Object or an Entity, or None for the scene root")
    })
}

#[pyfunction]
#[pyo3(signature = (width = 1280, height = 720, *, window = true, title = "Carbine"))]
fn init(width: u32, height: u32, window: bool, title: &str) -> PyResult<String> {
    let renderer = if window {
        Renderer::window(title, width, height)
    } else {
        Renderer::offscreen(width, height)
    }
    .map_err(render_error)?;

    let adapter = renderer.adapter().unwrap_or("unknown").to_owned();
    RENDERER.with(|cell| *cell.borrow_mut() = Some(renderer));
    Ok(adapter)
}

/// Draws one frame: the keyboard and mouse are collected, the world steps by
/// the time the last frame took, and the result goes to the window.
#[pyfunction]
fn render() -> PyResult<()> {
    let dt = tick();
    RENDERER.with(|cell| {
        let mut slot = cell.borrow_mut();
        let renderer = slot.as_mut().ok_or_else(needs_renderer)?;
        let frame = renderer.poll();
        let mut scene = locked()?;
        scene.update_input(&frame);
        scene.step(dt);
        let wants = scene.wants_pointer_grab();
        renderer.set_pointer_grab(wants);
        renderer.render(&scene).map_err(render_error)
    })
}

/// Seconds between the two most recent frames. The first frame reports zero.
#[pyfunction]
fn dt() -> f32 {
    CLOCK.with(|cell| cell.borrow().1)
}

/// Advances the world by a number of seconds without drawing. Use it when the
/// frame pacing is yours, or when there is no window at all.
#[pyfunction]
fn step(seconds: f64) -> PyResult<()> {
    with_scene(|scene| {
        scene.step(seconds as f32);
        Ok(())
    })
}

#[pyfunction]
fn running() -> bool {
    RENDERER.with(|cell| {
        cell.borrow()
            .as_ref()
            .is_some_and(|renderer| renderer.is_running())
    })
}

#[pyfunction]
fn viewport() -> PyResult<(u32, u32)> {
    RENDERER.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|renderer| renderer.viewport())
            .ok_or_else(needs_renderer)
    })
}

/// Renders one frame and returns it as packed RGBA bytes, top row first.
#[pyfunction]
#[pyo3(name = "readPixels")]
fn read_pixels(py: Python<'_>) -> PyResult<Bound<'_, PyBytes>> {
    RENDERER.with(|cell| {
        let mut slot = cell.borrow_mut();
        let renderer = slot.as_mut().ok_or_else(needs_renderer)?;
        let scene = locked()?;
        let pixels = renderer.read_pixels(&scene).map_err(render_error)?;
        Ok(PyBytes::new(py, &pixels))
    })
}

#[pyfunction]
fn close() {
    RENDERER.with(|cell| *cell.borrow_mut() = None);
}

/// Creates an object: authored geometry in the world.
#[pyfunction]
#[pyo3(signature = (kind, *, name=None, parent=None))]
fn new<'py>(
    kind: &str,
    name: Option<String>,
    parent: Option<Bound<'py, PyAny>>,
) -> PyResult<Object> {
    let parent = parent.as_ref().map(node_id_of).transpose()?;
    let id = with_scene(|scene| scene.create(kind, name, parent))?;
    Ok(Object::from_id(id))
}

/// Spawns an actor. Without a `kind` it has no body and draws nothing.
#[pyfunction]
#[pyo3(signature = (kind=None, *, name=None, parent=None, position=None, health=1.0, damage=0.0))]
#[pyo3(name = "spawnEntity")]
fn spawn_entity<'py>(
    kind: Option<&str>,
    name: Option<String>,
    parent: Option<Bound<'py, PyAny>>,
    position: Option<Bound<'py, PyAny>>,
    health: f64,
    damage: f64,
) -> PyResult<Entity> {
    let parent = parent.as_ref().map(node_id_of).transpose()?;
    let position = position.as_ref().map(extract_vec3).transpose()?;
    let id = with_scene(|scene| {
        scene.spawn_entity(kind, name, parent, position, health as f32, damage as f32)
    })?;
    Ok(Entity::from_id(id))
}

#[pyfunction]
#[pyo3(name = "killEntity")]
fn kill_entity(entity: &Bound<'_, PyAny>) -> PyResult<()> {
    let id = node_id_of(entity)?;
    with_scene(|scene| scene.kill_entity(id))
}

#[pyfunction]
#[pyo3(name = "camera")]
fn get_camera() -> PyCamera {
    PyCamera::new()
}

/// The backdrop and the light: `carbine.sky().zenith = "#2f6cb3"`.
#[pyfunction]
#[pyo3(name = "sky")]
fn get_sky() -> PySky {
    PySky::new()
}

#[pyfunction]
fn get_scene() -> PyScene {
    PyScene::new()
}

/// The screen overlay: `carbine.ui.new("square")`.
#[pyfunction]
fn get_ui() -> PyUi {
    PyUi::overlay()
}

/// The WASD-and-mouse controller: `carbine.player().enabled = True`.
#[pyfunction]
#[pyo3(name = "player")]
fn get_player() -> PyPlayer {
    PyPlayer::new()
}

/// The object kinds `new()` and `spawnEntity()` accept.
#[pyfunction]
fn kinds() -> Vec<&'static str> {
    carbine_core::PRIMITIVE_KINDS.to_vec()
}

#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<Object>()?;
    module.add_class::<Entity>()?;
    module.add_class::<PyScene>()?;
    module.add_class::<PyUi>()?;
    module.add_class::<PyCamera>()?;
    module.add_class::<PySky>()?;
    module.add_class::<PyPlayer>()?;
    module.add_class::<PyMouse>()?;
    module.add_class::<math::PyVec3>()?;
    module.add_class::<math::Vec3Iterator>()?;
    module.add_class::<math::PyQuat>()?;
    module.add_class::<mesh::PyMesh>()?;
    module.add_function(wrap_pyfunction!(init, module)?)?;
    module.add_function(wrap_pyfunction!(render, module)?)?;
    module.add_function(wrap_pyfunction!(dt, module)?)?;
    module.add_function(wrap_pyfunction!(step, module)?)?;
    module.add_function(wrap_pyfunction!(running, module)?)?;
    module.add_function(wrap_pyfunction!(viewport, module)?)?;
    module.add_function(wrap_pyfunction!(read_pixels, module)?)?;
    module.add_function(wrap_pyfunction!(close, module)?)?;
    module.add_function(wrap_pyfunction!(new, module)?)?;
    module.add_function(wrap_pyfunction!(spawn_entity, module)?)?;
    module.add_function(wrap_pyfunction!(kill_entity, module)?)?;
    module.add_function(wrap_pyfunction!(get_camera, module)?)?;
    module.add_function(wrap_pyfunction!(get_sky, module)?)?;
    module.add_function(wrap_pyfunction!(get_scene, module)?)?;
    module.add_function(wrap_pyfunction!(get_ui, module)?)?;
    module.add_function(wrap_pyfunction!(get_player, module)?)?;
    module.add_function(wrap_pyfunction!(kinds, module)?)?;
    module.add_function(wrap_pyfunction!(input::key, module)?)?;
    module.add_function(wrap_pyfunction!(input::pressed, module)?)?;
    module.add_function(wrap_pyfunction!(input::released, module)?)?;
    module.add_function(wrap_pyfunction!(input::keys, module)?)?;
    module.add_function(wrap_pyfunction!(input::mouse, module)?)?;
    module.add_function(wrap_pyfunction!(input::grab_mouse, module)?)?;
    module.add_function(wrap_pyfunction!(input::inject_key, module)?)?;
    module.add_function(wrap_pyfunction!(input::inject_mouse, module)?)?;
    module.add_function(wrap_pyfunction!(input::inject_button, module)?)?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    module.add("DestroyedError", module.py().get_type::<DestroyedError>())?;
    Ok(())
}
