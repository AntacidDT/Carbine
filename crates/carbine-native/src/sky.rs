use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::math::{PyVec3, extract_vec3};
use crate::with_scene;

/// The backdrop and the one light the whole world is shaded by.
///
/// Colours are sRGB channels in 0..1, or a hex string such as `"#2f6cb3"`.
#[pyclass(frozen, module = "carbine", name = "Sky")]
pub struct PySky;

impl PySky {
    pub(crate) fn new() -> Self {
        Self
    }
}

/// Assign either the channels or a hex string to one side of the gradient.
fn assign_color(
    value: &Bound<'_, PyAny>,
    set_channels: impl FnOnce(&mut carbine_core::Scene, glam::Vec3),
    set_hex: impl FnOnce(&mut carbine_core::Scene, &str) -> Result<(), carbine_core::SceneError>,
) -> PyResult<()> {
    if let Ok(hex) = value.extract::<String>() {
        return with_scene(|scene| set_hex(scene, &hex));
    }
    let color = extract_vec3(value)?;
    with_scene(|scene| {
        set_channels(scene, color);
        Ok(())
    })
}

#[pymethods]
impl PySky {
    /// False paints the plain clear colour behind everything instead.
    #[getter]
    fn enabled(&self) -> PyResult<bool> {
        with_scene(|scene| Ok(scene.sky().enabled))
    }

    #[setter]
    fn set_enabled(&self, value: bool) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_sky_enabled(value);
            Ok(())
        })
    }

    /// Straight up.
    #[getter]
    fn zenith(&self) -> PyResult<PyVec3> {
        let color = with_scene(|scene| Ok(scene.sky().zenith))?;
        Ok(PyVec3::plain(color))
    }

    #[setter]
    fn set_zenith(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        assign_color(
            &value,
            |scene, color| scene.set_sky_zenith(color),
            |scene, hex| scene.set_sky_zenith_hex(hex),
        )
    }

    /// Where the sky meets the ground.
    #[getter]
    fn horizon(&self) -> PyResult<PyVec3> {
        let color = with_scene(|scene| Ok(scene.sky().horizon))?;
        Ok(PyVec3::plain(color))
    }

    #[setter]
    fn set_horizon(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        assign_color(
            &value,
            |scene, color| scene.set_sky_horizon(color),
            |scene, hex| scene.set_sky_horizon_hex(hex),
        )
    }

    /// Both backdrop colours at once, as hex strings.
    fn colors(&self, zenith: &str, horizon: &str) -> PyResult<()> {
        with_scene(|scene| scene.set_sky_colors_hex(zenith, horizon))
    }

    /// How much grain the gradient carries, 0..1.
    #[getter]
    fn noise(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.sky().noise))
    }

    #[setter]
    fn set_noise(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_sky_noise(value as f32);
            Ok(())
        })
    }

    /// The direction the light arrives from. Only the direction counts, so the
    /// numbers may be any length.
    #[getter]
    fn sun(&self) -> PyResult<PyVec3> {
        let direction = with_scene(|scene| Ok(scene.sky().sun))?;
        Ok(PyVec3::plain(direction))
    }

    #[setter]
    fn set_sun(&self, value: Bound<'_, PyAny>) -> PyResult<()> {
        let value = extract_vec3(&value)?;
        with_scene(|scene| {
            scene.set_sky_sun(value);
            Ok(())
        })
    }

    /// Brightness of a face that points away from the sun, 0..1.
    #[getter]
    fn ambient(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.sky().ambient))
    }

    #[setter]
    fn set_ambient(&self, value: f64) -> PyResult<()> {
        with_scene(|scene| {
            scene.set_sky_ambient(value as f32);
            Ok(())
        })
    }

    fn __repr__(&self) -> PyResult<String> {
        let sky = with_scene(|scene| Ok(scene.sky()))?;
        Ok(format!(
            "<Sky enabled={} noise={:?} ambient={:?}>",
            sky.enabled, sky.noise, sky.ambient
        ))
    }
}
