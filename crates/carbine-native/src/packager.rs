//! The Packager: `carbine.packager`.
//!
//! One format per class of thing — `.glb` for objects, `.ogg` for audio, `.ktx2` for
//! images — so the engine carries one loader each instead of forty. This is the same
//! conversion Enginebay runs when a file is dropped into a project folder, reachable
//! from a script: pack a picture on the way in and hand the path it returns straight to
//! `obj.texture`.
//!
//! Everything happens in Rust. This class forwards a path and turns the answer, or the
//! refusal, into a Python value.

use std::path::PathBuf;

use pyo3::exceptions::{PyOSError, PyValueError};
use pyo3::prelude::*;

/// The import register, `carbine.packager`.
#[pyclass(frozen, module = "carbine", name = "Packager")]
pub struct PyPackager;

impl PyPackager {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[pymethods]
impl PyPackager {
    /// Converts a picture to `.ktx2` — BC7 blocks with a full mip chain, the bytes a GPU
    /// reads without a decoder — and returns the path it wrote. Left alone, the file
    /// lands next to its source under the same stem, so `assets/wood.png` answers
    /// `assets/wood.ktx2`; an `out` that names a folder packs into it, anything else is
    /// the file to write. The image the object wants is the path handed back. A
    /// `pathlib.Path` is as good as a string.
    #[pyo3(signature = (path, *, out = None))]
    fn image(&self, path: PathBuf, out: Option<PathBuf>) -> PyResult<String> {
        let written = carbine_pack::image(&path, out.as_deref()).map_err(pack_error)?;
        Ok(written.display().to_string())
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok("<Packager images to .ktx2>".to_owned())
    }
}

fn pack_error(error: carbine_pack::PackError) -> PyErr {
    match error {
        carbine_pack::PackError::Write { .. } => PyOSError::new_err(error.to_string()),
        carbine_pack::PackError::Source { .. } | carbine_pack::PackError::Encoder { .. } => {
            PyValueError::new_err(error.to_string())
        }
    }
}
