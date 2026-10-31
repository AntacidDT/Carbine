//! What the light is doing: `atmosphere.light()`.
//!
//! Every number here is the engine's own, read after a rung has been chosen, so a
//! script or an editor can show what `auto` picked on this machine without having to
//! repeat the rule it was picked by. Nothing on this handle is a knob; the one knob
//! is `atmosphere.lightQuality`.

use pyo3::prelude::*;

use crate::with_scene;

/// The light the world is being shaded by, as the engine resolved it.
#[pyclass(frozen, module = "carbine", name = "Light")]
pub struct PyLight;

impl PyLight {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[pymethods]
impl PyLight {
    /// Which rung this is: `"low"`, `"medium"`, `"high"` or `"extra"`. Never
    /// `"auto"`, because by the time anyone asks, the machine has been read.
    #[getter]
    fn quality(&self) -> PyResult<String> {
        with_scene(|scene| Ok(scene.light_quality().name().to_owned()))
    }

    /// Whether the sun pre-renders the world to read a shade out of.
    #[getter]
    fn shadows(&self) -> PyResult<bool> {
        with_scene(|scene| Ok(scene.light().shadows))
    }

    /// Texels on a side of that pre-rendered picture, or 0 when there is none.
    #[getter(shadowMap)]
    fn shadow_map(&self) -> PyResult<u32> {
        with_scene(|scene| Ok(scene.light().shadow_map))
    }

    /// How many readings a pixel takes around its place in the cast. One is a hard
    /// edge; more is a shade that fades where it ends.
    #[getter]
    fn softness(&self) -> PyResult<u32> {
        with_scene(|scene| Ok(scene.light().softness))
    }

    /// Metres around the eyes the cast reaches. Anything past it is lit and casts
    /// nothing.
    #[getter]
    fn reach(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.light().reach))
    }

    /// How much of the sun a face pointed square at it throws back as a sheen,
    /// 0..1. Zero is chalk.
    #[getter]
    fn gloss(&self) -> PyResult<f32> {
        with_scene(|scene| Ok(scene.light().gloss))
    }

    /// Whether the light the sun leaves out comes from the backdrop itself, so a
    /// north face is sky-blue at noon and starry at midnight.
    #[getter(skyFill)]
    fn sky_fill(&self) -> PyResult<bool> {
        with_scene(|scene| Ok(scene.light().sky_fill && scene.sky().enabled))
    }

    /// What kind of device the renderer is drawing on: `"discrete"`, `"integrated"`,
    /// `"cpu"`, `"virtual"` or `"other"`. Nothing until a renderer has started, which
    /// is exactly when an `auto` rung had nothing to be auto about.
    #[getter]
    fn device(&self) -> PyResult<Option<&'static str>> {
        with_scene(|scene| Ok(scene.gpu().map(|gpu| gpu.kind.name())))
    }

    /// Bytes the card owns outright, as `atmosphere.lightQuality = "auto"` sorted
    /// them out. `None` for a card that borrows the machine's memory, and for one
    /// that will not say how big it is.
    #[getter]
    fn memory(&self) -> PyResult<Option<u64>> {
        with_scene(|scene| Ok(scene.gpu().and_then(|gpu| gpu.vram)))
    }

    fn __repr__(&self) -> PyResult<String> {
        let light = with_scene(|scene| Ok(scene.light()))?;
        Ok(format!(
            "<Light {} shadows={} cast={} softness={} reach={:?} gloss={:?} skyFill={}>",
            light.quality.name(),
            light.shadows,
            light.shadow_map,
            light.softness,
            light.reach,
            light.gloss,
            light.sky_fill
        ))
    }
}
