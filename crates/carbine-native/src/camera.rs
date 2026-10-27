use carbine_core::CameraMode;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::math::{PyVec3, extract_vec3};
use crate::with_scene;

/// The scene camera. Distances are meters and the world is z-up, so `position.z`
/// is how high the camera floats and `target` is the point it aims at.
#[pyclass(frozen, module = "carbine", name = "Camera")]
pub struct PyCamera;

impl PyCamera {
    pub(crate) fn new() -> Self {
        Self
    }
}

fn mode_name(mode: CameraMode) -> &'static str {
    match mode {
        CameraMode::Perspective => "perspective",
        CameraMode::Orthographic => "orthographic",
    }
}

#[pymethods]
impl PyCamera {
    #[getter]
    fn position(&self) -> PyResult<PyVec3> {
        let position = with_scene(|scene| Ok(scene.camera().position))?;
        Ok(PyVec3::plain(position))
    }

    #[setter]
    fn set_position(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        let value = extract_vec3(&value)?;
        with_scene(|scene| {
            scene.set_camera_position(value);
            Ok(())
        })
    }

    /// The world point the camera aims at.
    #[getter]
    fn target(&self) -> PyResult<PyVec3> {
        let target = with_scene(|scene| Ok(scene.camera().target))?;
        Ok(PyVec3::plain(target))
    }

    #[setter]
    fn set_target(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        let value = extract_vec3(&value)?;
        with_scene(|scene| {
            scene.set_camera_target(value);
            Ok(())
        })
    }

    /// Aim the camera at a world point, the same way `target` does.
    #[pyo3(name = "lookAt")]
    fn look_at(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        self.set_target(value)
    }

    /// `"perspective"`, which gives depth, or `"orthographic"`, which keeps
    /// parallel lines parallel.
    #[getter]
    fn mode(&self) -> PyResult<String> {
        let mode = with_scene(|scene| Ok(scene.camera().mode))?;
        Ok(mode_name(mode).to_owned())
    }

    #[setter]
    fn set_mode(&self, value: &str) -> PyResult<()> {
        let mode = match value {
            "perspective" => CameraMode::Perspective,
            "orthographic" => CameraMode::Orthographic,
            other => {
                return Err(PyValueError::new_err(format!(
                    "\"{other}\" is not a camera mode; use \"perspective\" or \"orthographic\""
                )));
            }
        };
        with_scene(|scene| {
            scene.set_camera_mode(mode);
            Ok(())
        })
    }

    /// Vertical field of view in degrees, perspective only.
    #[getter]
    fn fov(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.camera().fov))
    }

    #[setter]
    fn set_fov(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_camera_fov(value as f32);
            Ok(())
        })
    }

    /// Meters nearer than this are cut off.
    #[getter]
    fn near(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.camera().near))
    }

    #[setter]
    fn set_near(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_camera_near(value as f32);
            Ok(())
        })
    }

    /// Meters farther than this are cut off.
    #[getter]
    fn far(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.camera().far))
    }

    #[setter]
    fn set_far(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_camera_far(value as f32);
            Ok(())
        })
    }

    /// Vertical size of the visible world in meters, orthographic only.
    #[getter(viewHeight)]
    fn view_height(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.camera().view_height))
    }

    #[setter(viewHeight)]
    fn set_view_height(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_camera_view_height(value as f32);
            Ok(())
        })
    }

    fn __repr__(&self) -> PyResult<String> {
        let camera = with_scene(|scene| Ok(scene.camera()))?;
        Ok(format!(
            "<Camera {} position=({}, {}, {}) target=({}, {}, {})>",
            mode_name(camera.mode),
            camera.position.x,
            camera.position.y,
            camera.position.z,
            camera.target.x,
            camera.target.y,
            camera.target.z,
        ))
    }
}
