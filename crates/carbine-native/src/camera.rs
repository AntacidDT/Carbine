use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::math::{PyVec3, extract_vec3};
use crate::with_scene;

/// The scene's orthographic camera. Distances are meters, so `viewHeight` is the
/// vertical size of the visible world in meters.
#[pyclass(frozen, module = "carbine", name = "Camera")]
pub struct PyCamera;

impl PyCamera {
    pub(crate) fn new() -> Self {
        Self
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
            "<Camera position=({}, {}, {}) viewHeight={}>",
            camera.position.x, camera.position.y, camera.position.z, camera.view_height
        ))
    }
}
