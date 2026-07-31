//! `to2D` and `to3D`.
//!
//! These are type-preserving: a `Point3d` becomes a `Point2d` over the *same*
//! storage, a `LineString3d` becomes a `LineString2d`, and so on. Anything without
//! a counterpart comes back unchanged rather than raising, which is what upstream
//! does — the conversion is a chain of `extract` attempts with a fall-through.
//!
//! Upstream: `lanelet2_python/python_api/geometry.cpp:96-127`

use pyo3::prelude::*;

use crate::core::basic::{PyBasicPoint2d, PyBasicPoint3d};
use crate::core::compound::{
    PyCompoundLineString2d, PyCompoundLineString3d, PyCompoundPolygon2d, PyCompoundPolygon3d,
};
use crate::core::linestring::{
    PyConstLineString2d, PyConstLineString3d, PyConstPolygon2d, PyConstPolygon3d, PyLineString2d,
    PyLineString3d, PyPolygon2d, PyPolygon3d,
};
use crate::core::point::{PyConstPoint2d, PyConstPoint3d, PyPoint2d, PyPoint3d};

/// Tries each conversion in turn, returning the argument unchanged if none applies.
macro_rules! convert_chain {
    ($obj:expr, $py:expr, $( $from:ident => $to:ident ),+ $(,)?) => {
        $(
            if let Ok(value) = $obj.cast::<$from>() {
                let converted = $to::wrap(value.borrow().inner().clone());
                return Ok(Bound::new($py, converted)?.into_any());
            }
        )+
    };
}

pub fn to_2d<'py>(obj: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let py = obj.py();

    // Plain coordinates copy rather than share, since they have no id to keep.
    if let Ok(value) = obj.cast::<PyBasicPoint3d>() {
        let [x, y, _] = value.borrow().view().xyz();
        return Ok(Bound::new(py, PyBasicPoint2d::free([x, y, 0.0]))?.into_any());
    }

    convert_chain!(obj, py,
        PyPoint3d => PyPoint2d,
        PyConstPoint3d => PyConstPoint2d,
        PyLineString3d => PyLineString2d,
        PyConstLineString3d => PyConstLineString2d,
        PyPolygon3d => PyPolygon2d,
        PyConstPolygon3d => PyConstPolygon2d,
        PyCompoundLineString3d => PyCompoundLineString2d,
        PyCompoundPolygon3d => PyCompoundPolygon2d,
    );
    Ok(obj.clone())
}

pub fn to_3d<'py>(obj: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let py = obj.py();

    if let Ok(value) = obj.cast::<PyBasicPoint2d>() {
        let [x, y, _] = value.borrow().view().xyz();
        return Ok(Bound::new(py, PyBasicPoint3d::free([x, y, 0.0]))?.into_any());
    }

    convert_chain!(obj, py,
        PyPoint2d => PyPoint3d,
        PyConstPoint2d => PyConstPoint3d,
        PyLineString2d => PyLineString3d,
        PyConstLineString2d => PyConstLineString3d,
        PyPolygon2d => PyPolygon3d,
        PyConstPolygon2d => PyConstPolygon3d,
        PyCompoundLineString2d => PyCompoundLineString3d,
        PyCompoundPolygon2d => PyCompoundPolygon3d,
    );
    Ok(obj.clone())
}
