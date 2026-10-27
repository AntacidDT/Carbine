use pyo3::prelude::*;

use crate::object::Object;
use crate::with_scene;

#[pyclass(frozen, module = "carbine", name = "Scene")]
pub struct PyScene;

impl PyScene {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[pymethods]
impl PyScene {
    /// The root every object and entity hangs from.
    #[getter]
    fn root(&self) -> PyResult<Object> {
        with_scene(|scene| Ok(Object::from_id(scene.root())))
    }

    #[getter(nodeCount)]
    fn node_count(&self) -> PyResult<usize> {
        with_scene(|scene| Ok(scene.node_count()))
    }

    #[getter(entityCount)]
    fn entity_count(&self) -> PyResult<usize> {
        with_scene(|scene| Ok(scene.entity_count()))
    }

    fn __repr__(&self) -> PyResult<String> {
        with_scene(|scene| {
            Ok(format!(
                "<Scene {} objects, {} entities>",
                scene.node_count(),
                scene.entity_count()
            ))
        })
    }
}
