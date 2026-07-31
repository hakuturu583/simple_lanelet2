//! `lanelet2.core` — primitives, attributes, regulatory elements and map layers.
//!
//! Ground truth: `lanelet2_python/python_api/core.cpp`.

use pyo3::prelude::*;
use pyo3::types::PyModule;

/// `lanelet2.core.getId()` — a fresh globally unique id.
#[pyfunction]
#[pyo3(name = "getId")]
fn get_id() -> i64 {
    ll2_core::get_id()
}

/// `lanelet2.core.registerId(id)` — reserve `id` so `getId()` never returns it.
#[pyfunction]
#[pyo3(name = "registerId", signature = (id))]
fn register_id(id: i64) {
    ll2_core::register_id(id);
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(get_id, m)?)?;
    m.add_function(wrap_pyfunction!(register_id, m)?)?;
    Ok(())
}
