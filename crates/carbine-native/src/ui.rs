use pyo3::prelude::*;

use crate::object::Object;
use crate::with_scene;

/// The screen overlay. Elements are measured in pixels from the viewport's top
/// left corner, so `position` and `size` mean something different here than they
/// do on `carbine.new()`.
#[pyclass(frozen, module = "carbine", name = "Ui")]
pub struct PyUi;

impl PyUi {
    pub(crate) fn overlay() -> Self {
        Self
    }
}

#[pymethods]
impl PyUi {
    /// Creates an element. The kind chooses a default polygon, which the
    /// `polygon` property overrides.
    #[pyo3(name = "new")]
    #[pyo3(signature = (kind = "square", *, name = None, parent = None))]
    fn create_element<'py>(
        &self,
        kind: &str,
        name: Option<String>,
        parent: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Object> {
        let parent = parent.as_ref().map(crate::node_id_of).transpose()?;
        let id = with_scene(|scene| scene.create_ui(kind, name, parent))?;
        Ok(Object::from_id(id))
    }

    fn __repr__(&self) -> &'static str {
        "<Ui overlay>"
    }
}
