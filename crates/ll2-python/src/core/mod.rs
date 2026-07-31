//! `lanelet2.core` — primitives, attributes, regulatory elements and map layers.
//!
//! Ground truth: `lanelet2_python/python_api/core.cpp`.

use pyo3::prelude::*;
use pyo3::types::PyModule;

pub mod area;
pub mod attribute;
pub mod basic;
pub mod compound;
pub mod convert;
pub mod gps;
pub mod lanelet;
pub mod linestring;
pub mod map;
pub mod point;
pub mod regelem;
pub mod sequence;

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
    m.add_class::<attribute::PyAttributeMap>()?;
    m.add_class::<attribute::PyAttributeMapEntry>()?;

    m.add_class::<basic::PyBasicPoint2d>()?;
    m.add_class::<basic::PyBasicPoint3d>()?;
    m.add_class::<basic::PyVector2d>()?;
    m.add_class::<basic::PyBoundingBox2d>()?;
    m.add_class::<basic::PyBoundingBox3d>()?;
    m.add_class::<gps::PyGpsPoint>()?;

    m.add_class::<point::PyPoint2d>()?;
    m.add_class::<point::PyPoint3d>()?;
    m.add_class::<point::PyConstPoint2d>()?;
    m.add_class::<point::PyConstPoint3d>()?;

    linestring::register(m)?;
    compound::register(m)?;
    lanelet::register(m)?;
    regelem::register(m)?;
    area::register(m)?;
    sequence::register(m)?;
    map::register(m)?;

    m.add_function(wrap_pyfunction!(get_id, m)?)?;
    m.add_function(wrap_pyfunction!(register_id, m)?)?;
    Ok(())
}
