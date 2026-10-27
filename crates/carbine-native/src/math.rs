use carbine_core::{Axis, NodeId, VecField};
use glam::{EulerRot, Quat, Vec3};
use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;
use pyo3::types::PyAny;

use crate::with_scene;

#[derive(Clone, Copy)]
struct Binding {
    node: NodeId,
    field: VecField,
}

#[pyclass(module = "carbine", name = "Vec3")]
#[derive(Clone)]
pub struct PyVec3 {
    value: Vec3,
    binding: Option<Binding>,
}

impl PyVec3 {
    pub(crate) fn plain(value: Vec3) -> Self {
        Self {
            value,
            binding: None,
        }
    }

    pub(crate) fn current(&self) -> PyResult<Vec3> {
        match self.binding {
            Some(binding) => with_scene(|scene| scene.vec_field(binding.node, binding.field)),
            None => Ok(self.value),
        }
    }

    fn set_axis(&mut self, axis: Axis, value: f32) -> PyResult<()> {
        if let Some(binding) = self.binding {
            with_scene(|scene| scene.set_vec_field_axis(binding.node, binding.field, axis, value))
        } else {
            match axis {
                Axis::X => self.value.x = value,
                Axis::Y => self.value.y = value,
                Axis::Z => self.value.z = value,
            }
            Ok(())
        }
    }
}

pub(crate) fn vec3_field(node: NodeId, field: VecField) -> PyResult<PyVec3> {
    let value = with_scene(|scene| scene.vec_field(node, field))?;
    Ok(PyVec3 {
        value,
        binding: Some(Binding { node, field }),
    })
}

pub(crate) fn extract_vec3(value: &Bound<'_, PyAny>) -> PyResult<Vec3> {
    if let Ok(vector) = value.extract::<PyRef<'_, PyVec3>>() {
        return vector.current();
    }
    if let Ok((x, y, z)) = value.extract::<(f64, f64, f64)>() {
        return Ok(Vec3::new(x as f32, y as f32, z as f32));
    }
    Err(PyTypeError::new_err(
        "expected a Vec3 or an (x, y, z) tuple",
    ))
}

#[pymethods]
impl PyVec3 {
    #[new]
    #[pyo3(signature = (x=0.0, y=0.0, z=0.0))]
    fn new(x: f64, y: f64, z: f64) -> Self {
        Self::plain(Vec3::new(x as f32, y as f32, z as f32))
    }

    #[getter]
    fn x(&self) -> PyResult<f32> {
        Ok(self.current()?.x)
    }

    #[setter]
    fn set_x(&mut self, value: f64) -> PyResult<()> {
        self.set_axis(Axis::X, value as f32)
    }

    #[getter]
    fn y(&self) -> PyResult<f32> {
        Ok(self.current()?.y)
    }

    #[setter]
    fn set_y(&mut self, value: f64) -> PyResult<()> {
        self.set_axis(Axis::Y, value as f32)
    }

    #[getter]
    fn z(&self) -> PyResult<f32> {
        Ok(self.current()?.z)
    }

    #[setter]
    fn set_z(&mut self, value: f64) -> PyResult<()> {
        self.set_axis(Axis::Z, value as f32)
    }

    fn __len__(&self) -> usize {
        3
    }

    fn __iter__(&self) -> PyResult<Vec3Iterator> {
        let value = self.current()?;
        Ok(Vec3Iterator {
            values: [value.x, value.y, value.z],
            index: 0,
        })
    }

    fn __repr__(&self) -> PyResult<String> {
        let value = self.current()?;
        Ok(format!("Vec3({}, {}, {})", value.x, value.y, value.z))
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match other.extract::<PyRef<'_, PyVec3>>() {
            Ok(other) => match (self.current(), other.current()) {
                (Ok(this), Ok(other)) => this == other,
                _ => false,
            },
            Err(_) => false,
        }
    }

    fn __add__(&self, other: &Bound<'_, PyAny>) -> PyResult<PyVec3> {
        Ok(Self::plain(self.current()? + extract_vec3(other)?))
    }

    fn __sub__(&self, other: &Bound<'_, PyAny>) -> PyResult<PyVec3> {
        Ok(Self::plain(self.current()? - extract_vec3(other)?))
    }

    fn __mul__(&self, scalar: f64) -> PyResult<PyVec3> {
        Ok(Self::plain(self.current()? * scalar as f32))
    }

    fn __rmul__(&self, scalar: f64) -> PyResult<PyVec3> {
        self.__mul__(scalar)
    }

    #[pyo3(name = "toTuple")]
    fn to_tuple(&self) -> PyResult<(f32, f32, f32)> {
        let value = self.current()?;
        Ok((value.x, value.y, value.z))
    }
}

#[pyclass(module = "carbine", name = "Vec3Iterator")]
pub struct Vec3Iterator {
    values: [f32; 3],
    index: usize,
}

#[pymethods]
impl Vec3Iterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self) -> Option<f32> {
        if self.index >= self.values.len() {
            return None;
        }
        let value = self.values[self.index];
        self.index += 1;
        Some(value)
    }
}

#[pyclass(frozen, module = "carbine", name = "Quat")]
#[derive(Clone, Copy)]
pub struct PyQuat {
    value: Quat,
}

impl PyQuat {
    pub(crate) fn from_value(value: Quat) -> Self {
        Self { value }
    }

    pub(crate) fn value(&self) -> Quat {
        self.value
    }
}

#[pymethods]
impl PyQuat {
    #[new]
    #[pyo3(signature = (x=0.0, y=0.0, z=0.0, w=1.0))]
    fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        let rotation = Quat::from_xyzw(x as f32, y as f32, z as f32, w as f32);
        Self {
            value: if rotation.length_squared() > f32::EPSILON {
                rotation.normalize()
            } else {
                Quat::IDENTITY
            },
        }
    }

    #[staticmethod]
    fn identity() -> Self {
        Self {
            value: Quat::IDENTITY,
        }
    }

    #[staticmethod]
    #[pyo3(name = "fromEuler")]
    fn from_euler(x: f64, y: f64, z: f64) -> Self {
        Self {
            value: Quat::from_euler(
                EulerRot::XYZ,
                (x as f32).to_radians(),
                (y as f32).to_radians(),
                (z as f32).to_radians(),
            ),
        }
    }

    #[allow(clippy::wrong_self_convention)]
    #[pyo3(name = "toEuler")]
    fn to_euler(&self) -> (f32, f32, f32) {
        let (x, y, z) = self.value.to_euler(EulerRot::XYZ);
        (x.to_degrees(), y.to_degrees(), z.to_degrees())
    }

    #[getter]
    fn x(&self) -> f32 {
        self.value.x
    }

    #[getter]
    fn y(&self) -> f32 {
        self.value.y
    }

    #[getter]
    fn z(&self) -> f32 {
        self.value.z
    }

    #[getter]
    fn w(&self) -> f32 {
        self.value.w
    }

    fn __repr__(&self) -> String {
        format!(
            "Quat(x={}, y={}, z={}, w={})",
            self.value.x, self.value.y, self.value.z, self.value.w
        )
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, PyQuat>>()
            .map(|other| other.value == self.value)
            .unwrap_or(false)
    }
}
