//! `lanelet2.routing` — placeholder; populated in a later phase.
//!
//! Ground truth: `lanelet2_python/python_api/routing.cpp`.

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // RelationType is installed by the Python shim, because Boost's enum derives
    // from int and a PyO3 enum cannot.
    m.add("_needs_RelationType", true)?;
    Ok(())
}
