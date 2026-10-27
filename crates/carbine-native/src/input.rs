//! What the person at the keyboard and mouse is doing, as seen from Python.

use carbine_core::{KEYS, Key, MouseButton};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::{apply_pointer_grab, pointer_grabbed, with_scene};

/// The names every key answers to, for the error message a typo gets.
fn key_names() -> String {
    KEYS.iter()
        .copied()
        .map(Key::name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// `"w"`, `"space"`, `"escape"` — matched by the position of the key, so a
/// French keyboard's `z` is the `w` Carbine means.
pub(crate) fn key_from_str(name: &str) -> PyResult<Key> {
    Key::from_name(name).ok_or_else(|| {
        PyValueError::new_err(format!(
            "\"{name}\" is not a key Carbine knows. Use one of: {}",
            key_names()
        ))
    })
}

pub(crate) fn button_from_str(name: &str) -> PyResult<MouseButton> {
    MouseButton::from_name(name).ok_or_else(|| {
        PyValueError::new_err(format!(
            "\"{name}\" is not a mouse button. Use \"left\", \"right\" or \"middle\""
        ))
    })
}

/// The pointer, read as it was at the last frame.
///
/// `dx` and `dy` are the pixels moved since the frame before; they only mean
/// something while the pointer is grabbed. `x` and `y` are where the cursor
/// sits in the window, top left being (0, 0).
#[pyclass(frozen, module = "carbine", name = "Mouse")]
pub struct PyMouse;

impl PyMouse {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[pymethods]
impl PyMouse {
    /// Pixels moved sideways this frame; positive is right.
    #[getter]
    fn dx(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.input().mouse_delta().x))
    }

    /// Pixels moved up and down this frame; positive is down.
    #[getter]
    fn dy(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.input().mouse_delta().y))
    }

    #[getter]
    fn x(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.input().cursor().x))
    }

    #[getter]
    fn y(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.input().cursor().y))
    }

    #[getter]
    fn left(&self) -> PyResult<bool> {
        with_scene(|scene| Ok(scene.input().button(MouseButton::Left)))
    }

    #[getter]
    fn right(&self) -> PyResult<bool> {
        with_scene(|scene| Ok(scene.input().button(MouseButton::Right)))
    }

    #[getter]
    fn middle(&self) -> PyResult<bool> {
        with_scene(|scene| Ok(scene.input().button(MouseButton::Middle)))
    }

    /// Wheel notches this frame, positive being away from the user.
    #[getter]
    fn wheel(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.input().wheel()))
    }

    /// Whether the window is holding the pointer for the camera.
    #[getter]
    fn grabbed(&self) -> bool {
        pointer_grabbed()
    }

    fn __repr__(&self) -> PyResult<String> {
        let (delta, cursor) =
            with_scene(|scene| Ok((scene.input().mouse_delta(), scene.input().cursor())))?;
        Ok(format!(
            "<Mouse dx={} dy={} at=({}, {})>",
            delta.x, delta.y, cursor.x, cursor.y
        ))
    }
}

/// A key held down right now.
#[pyfunction]
pub(crate) fn key(name: &str) -> PyResult<bool> {
    let key = key_from_str(name)?;
    with_scene(|scene| Ok(scene.input().key(key)))
}

/// A key that came down during the last frame only.
#[pyfunction]
pub(crate) fn pressed(name: &str) -> PyResult<bool> {
    let key = key_from_str(name)?;
    with_scene(|scene| Ok(scene.input().pressed(key)))
}

#[pyfunction]
pub(crate) fn released(name: &str) -> PyResult<bool> {
    let key = key_from_str(name)?;
    with_scene(|scene| Ok(scene.input().released(key)))
}

#[pyfunction]
pub(crate) fn mouse() -> PyMouse {
    PyMouse::new()
}

/// The keys Carbine answers for, for a bindings screen.
#[pyfunction]
pub(crate) fn keys() -> Vec<&'static str> {
    KEYS.iter().copied().map(Key::name).collect()
}

/// Takes the pointer away from the desktop, or hands it back. While it is held
/// the cursor disappears and only relative movement is reported.
#[pyfunction]
#[pyo3(name = "grabMouse")]
pub(crate) fn grab_mouse(grab: bool) -> PyResult<bool> {
    with_scene(|scene| {
        scene.set_pointer_grab(grab);
        Ok(())
    })?;
    let wants = with_scene(|scene| Ok(scene.wants_pointer_grab()))?;
    Ok(apply_pointer_grab(wants))
}

/// Feeds the scene a keystroke that did not come from a window: a test, or a
/// replay.
#[pyfunction]
#[pyo3(name = "injectKey")]
pub(crate) fn inject_key(name: &str, down: bool) -> PyResult<()> {
    let key = key_from_str(name)?;
    with_scene(|scene| {
        scene.inject_key(key, down);
        Ok(())
    })
}

#[pyfunction]
#[pyo3(name = "injectMouse")]
pub(crate) fn inject_mouse(dx: f64, dy: f64) -> PyResult<()> {
    with_scene(|scene| {
        scene.inject_mouse_delta(glam::Vec2::new(dx as f32, dy as f32));
        Ok(())
    })
}

#[pyfunction]
#[pyo3(name = "injectButton")]
pub(crate) fn inject_button(name: &str, down: bool) -> PyResult<()> {
    let button = button_from_str(name)?;
    with_scene(|scene| {
        scene.inject_button(button, down);
        Ok(())
    })
}
