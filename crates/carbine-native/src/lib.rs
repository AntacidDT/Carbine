mod camera;
mod entity;
mod math;
mod mesh;
mod object;
mod scene;

use std::cell::RefCell;
use std::sync::{Mutex, OnceLock};

use carbine_core::{NodeId, Scene, SceneError};
use carbine_render::Renderer;
use pyo3::create_exception;
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBytes};

use crate::camera::PyCamera;
use crate::entity::Entity;
use crate::math::extract_vec3;
use crate::object::Object;
use crate::scene::PyScene;

create_exception!(carbine, DestroyedError, PyRuntimeError);

/// The whole game lives in one scene, shared by every handle handed to Python.
static SCENE: OnceLock<Mutex<Scene>> = OnceLock::new();

thread_local! {
    /// A winit event loop and a wgpu surface cannot be moved between threads, so
    /// the renderer lives where it was created.
    static RENDERER: RefCell<Option<Renderer>> = const { RefCell::new(None) };
}

pub(crate) fn scene_handle() -> &'static Mutex<Scene> {
    SCENE.get_or_init(|| Mutex::new(Scene::new()))
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

#[pyfunction]
fn render() -> PyResult<()> {
    RENDERER.with(|cell| {
        let mut slot = cell.borrow_mut();
        let renderer = slot.as_mut().ok_or_else(needs_renderer)?;
        let scene = locked()?;
        renderer.render(&scene).map_err(render_error)
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

#[pyfunction]
fn get_scene() -> PyScene {
    PyScene::new()
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
    module.add_class::<PyCamera>()?;
    module.add_class::<math::PyVec3>()?;
    module.add_class::<math::Vec3Iterator>()?;
    module.add_class::<math::PyQuat>()?;
    module.add_class::<mesh::PyMesh>()?;
    module.add_function(wrap_pyfunction!(init, module)?)?;
    module.add_function(wrap_pyfunction!(render, module)?)?;
    module.add_function(wrap_pyfunction!(running, module)?)?;
    module.add_function(wrap_pyfunction!(viewport, module)?)?;
    module.add_function(wrap_pyfunction!(read_pixels, module)?)?;
    module.add_function(wrap_pyfunction!(close, module)?)?;
    module.add_function(wrap_pyfunction!(new, module)?)?;
    module.add_function(wrap_pyfunction!(spawn_entity, module)?)?;
    module.add_function(wrap_pyfunction!(kill_entity, module)?)?;
    module.add_function(wrap_pyfunction!(get_camera, module)?)?;
    module.add_function(wrap_pyfunction!(get_scene, module)?)?;
    module.add_function(wrap_pyfunction!(kinds, module)?)?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    module.add("DestroyedError", module.py().get_type::<DestroyedError>())?;
    Ok(())
}
