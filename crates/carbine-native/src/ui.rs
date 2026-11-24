use pyo3::prelude::*;

use crate::object;
use crate::with_scene;

/// Screen overlay in pixels; Text uses a scalar font size rather than a size vector.
#[pyclass(frozen, module = "carbine", name = "Ui")]
pub struct PyUi;

impl PyUi {
    pub(crate) fn overlay() -> Self {
        Self
    }
}

#[pymethods]
impl PyUi {
    /// Creates a shape, Text, an Image or a Video on the overlay; kinds are
    /// case-insensitive. The handle is the node's own class: an `Object` for a shape
    /// or picture, a `Video` for a film.
    #[pyo3(name = "new")]
    #[pyo3(signature = (kind = "square", *, name = None, parent = None))]
    fn create_element<'py>(
        &self,
        py: Python<'py>,
        kind: &str,
        name: Option<String>,
        parent: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let parent = parent.as_ref().map(crate::node_id_of).transpose()?;
        let id = with_scene(|scene| scene.create_ui(kind, name, parent))?;
        object::handle_for(py, id)
    }

    fn __repr__(&self) -> &'static str {
        "<Ui overlay>"
    }
}
