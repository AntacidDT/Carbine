use carbine_core::MeshData;
use pyo3::prelude::*;

use crate::math::PyVec3;

#[pyclass(frozen, module = "carbine", name = "Mesh")]
#[derive(Clone)]
pub struct PyMesh {
    data: MeshData,
}

impl PyMesh {
    pub(crate) fn from_data(data: MeshData) -> Self {
        Self { data }
    }
}

#[pymethods]
impl PyMesh {
    #[getter]
    fn positions(&self) -> Vec<PyVec3> {
        self.data
            .positions
            .iter()
            .copied()
            .map(PyVec3::plain)
            .collect()
    }

    #[getter]
    fn normals(&self) -> Vec<PyVec3> {
        self.data
            .normals
            .iter()
            .copied()
            .map(PyVec3::plain)
            .collect()
    }

    #[getter]
    fn uvs(&self) -> Vec<(f32, f32)> {
        self.data.uvs.iter().map(|uv| (uv.x, uv.y)).collect()
    }

    #[getter]
    fn indices(&self) -> Vec<u32> {
        self.data.indices.clone()
    }

    #[getter(vertexCount)]
    fn vertex_count(&self) -> usize {
        self.data.vertex_count()
    }

    #[getter(triangleCount)]
    fn triangle_count(&self) -> usize {
        self.data.triangle_count()
    }

    #[getter]
    fn bounds(&self) -> Option<(PyVec3, PyVec3)> {
        self.data
            .bounds()
            .map(|(min, max)| (PyVec3::plain(min), PyVec3::plain(max)))
    }

    fn __repr__(&self) -> String {
        format!(
            "<Mesh {} vertices, {} triangles>",
            self.data.vertex_count(),
            self.data.triangle_count()
        )
    }
}
