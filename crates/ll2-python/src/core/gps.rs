//! `GPSPoint` — a geodetic position, as `lanelet2.core` exposes it.
//!
//! Upstream: `lanelet2_python/python_api/core.cpp:790-800`

use ll2_core::fmt::{make_repr, ostream_double};
use ll2_projection::GpsPoint;
use pyo3::prelude::*;

/// `lanelet2.core.GPSPoint`.
#[pyclass(name = "GPSPoint", module = "lanelet2.core")]
#[derive(Clone, Copy)]
pub struct PyGpsPoint {
    pub(crate) point: GpsPoint,
}

impl PyGpsPoint {
    pub fn wrap(point: GpsPoint) -> Self {
        PyGpsPoint { point }
    }

    pub fn inner(&self) -> GpsPoint {
        self.point
    }
}

#[pymethods]
impl PyGpsPoint {
    #[new]
    #[pyo3(signature = (lat = 0.0, lon = 0.0, ele = 0.0))]
    fn new(lat: f64, lon: f64, ele: f64) -> Self {
        PyGpsPoint {
            point: GpsPoint::new(lat, lon, ele),
        }
    }

    #[getter]
    fn lat(&self) -> f64 {
        self.point.lat
    }

    #[setter(lat)]
    fn set_lat(&mut self, value: f64) {
        self.point.lat = value;
    }

    #[getter]
    fn lon(&self) -> f64 {
        self.point.lon
    }

    #[setter(lon)]
    fn set_lon(&mut self, value: f64) {
        self.point.lon = value;
    }

    #[getter]
    fn ele(&self) -> f64 {
        self.point.ele
    }

    #[setter(ele)]
    fn set_ele(&mut self, value: f64) {
        self.point.ele = value;
    }

    /// A deprecated alias for `ele`, kept because upstream still exposes it.
    #[getter]
    fn alt(&self) -> f64 {
        self.point.ele
    }

    #[setter(alt)]
    fn set_alt(&mut self, value: f64) {
        self.point.ele = value;
    }

    fn __repr__(&self) -> String {
        make_repr(
            "GPSPoint",
            &[
                ostream_double(self.point.lat),
                ostream_double(self.point.lon),
                ostream_double(self.point.ele),
            ],
        )
    }
}

/// Reads a `GPSPoint` from Python.
pub fn gps_point_of(obj: &Bound<'_, PyAny>) -> Option<GpsPoint> {
    obj.cast::<PyGpsPoint>().ok().map(|value| value.borrow().point)
}
