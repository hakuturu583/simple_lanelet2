//! `lanelet2.projection` — placeholder; populated in a later phase.
//!
//! Ground truth: `lanelet2_python/python_api/projection.cpp`.

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub fn register(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}
