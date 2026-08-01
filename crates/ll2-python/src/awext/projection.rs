//! The projections `autoware_lanelet2_extension` adds.
//!
//! Only `TransverseMercatorProjector` so far; `MGRSProjector` needs the MGRS grid
//! alphabet and lands separately.
//!
//! Upstream: `autoware_lanelet2_extension_python/src/projection.cpp`

use std::sync::Arc;

use ll2_projection::TransverseMercatorProjector;
use ll2_projection::transverse_mercator_projector::DEFAULT_SCALE_FACTOR;
use pyo3::prelude::*;

use crate::core::basic::PyBasicPoint3d;
use crate::core::gps::{PyGpsPoint, gps_point_of};
use crate::err::argument_error;
use crate::projection::to_py_error;
use crate::io::PyOrigin;
use crate::projection::SharedProjector;
use ll2_projection::Projector;

/// `autoware_lanelet2_extension_python.projection.TransverseMercatorProjector`.
///
/// Upstream's C++ constructor takes a scale factor as well, but its Python binding
/// hardcodes the one-argument form, so 0.9996 is the only value reachable from here.
#[pyclass(
    name = "TransverseMercatorProjector",
    module = "autoware_lanelet2_extension_python.projection",
    frozen
)]
pub struct PyTransverseMercatorProjector {
    projector: SharedProjector,
}

impl PyTransverseMercatorProjector {
    pub fn shared(&self) -> SharedProjector {
        self.projector.clone()
    }
}

#[pymethods]
impl PyTransverseMercatorProjector {
    #[new]
    fn new(origin: &PyOrigin) -> Self {
        PyTransverseMercatorProjector {
            projector: SharedProjector(Arc::new(TransverseMercatorProjector::new(
                origin.inner(),
                DEFAULT_SCALE_FACTOR,
            ))),
        }
    }

    fn forward(&self, point: &Bound<'_, PyAny>) -> PyResult<PyBasicPoint3d> {
        let point = gps_point_of(point)
            .ok_or_else(|| argument_error("TransverseMercatorProjector", "forward"))?;
        let projected = self.projector.forward(point).map_err(to_py_error)?;
        Ok(PyBasicPoint3d::free(projected))
    }

    fn reverse(&self, point: &Bound<'_, PyAny>) -> PyResult<PyGpsPoint> {
        let point = point
            .cast::<PyBasicPoint3d>()
            .map_err(|_| argument_error("TransverseMercatorProjector", "reverse"))?
            .borrow();
        let geodetic = self.projector.reverse(point.values()).map_err(to_py_error)?;
        Ok(PyGpsPoint::wrap(geodetic))
    }

    /// A method, not a property, matching upstream.
    fn origin(&self) -> PyOrigin {
        PyOrigin::wrap(self.projector.origin())
    }
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyTransverseMercatorProjector>()?;
    Ok(())
}
