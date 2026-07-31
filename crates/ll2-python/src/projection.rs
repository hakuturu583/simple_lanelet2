//! `lanelet2.projection` — converting between geodetic and local coordinates.
//!
//! Ground truth: `lanelet2_python/python_api/projection.cpp`.
//!
//! `origin()` is a *method* here, not a property, matching upstream.

use std::sync::Arc;

use ll2_projection::{
    Geocentric, GpsPoint, LocalCartesian, Origin, ProjectionError, Projector, SphericalMercator,
    Utm,
};
use pyo3::prelude::*;
use pyo3::types::PyModule;

use crate::core::basic::PyBasicPoint3d;
use crate::core::gps::{PyGpsPoint, gps_point_of};
use crate::err::{argument_error, not_instantiable, runtime};
use crate::io::PyOrigin;

fn to_py_error(error: ProjectionError) -> PyErr {
    runtime(error.message().to_owned())
}

/// Shared implementation for every projector class.
///
/// The trait object sits behind an `Arc` so the map loader can hold on to the very
/// projector the caller passed in.
#[derive(Clone)]
pub struct SharedProjector(pub Arc<dyn Projector + Send + Sync>);

impl SharedProjector {
    pub fn forward(&self, point: GpsPoint) -> Result<[f64; 3], ProjectionError> {
        self.0.forward(point)
    }

    pub fn reverse(&self, point: [f64; 3]) -> Result<GpsPoint, ProjectionError> {
        self.0.reverse(point)
    }

    pub fn origin(&self) -> Origin {
        self.0.origin()
    }
}

macro_rules! projector_class {
    ($py_name:literal, $rust:ident) => {
        #[doc = concat!("`lanelet2.projection.", $py_name, "`.")]
        #[pyclass(name = $py_name, module = "lanelet2.projection", frozen)]
        pub struct $rust {
            projector: SharedProjector,
        }

        impl $rust {
            pub fn shared(&self) -> SharedProjector {
                self.projector.clone()
            }
        }

        #[pymethods]
        impl $rust {
            fn forward(&self, point: &Bound<'_, PyAny>) -> PyResult<PyBasicPoint3d> {
                let point =
                    gps_point_of(point).ok_or_else(|| argument_error($py_name, "forward"))?;
                let projected = self.projector.forward(point).map_err(to_py_error)?;
                Ok(PyBasicPoint3d::free(projected))
            }

            fn reverse(&self, point: &Bound<'_, PyAny>) -> PyResult<PyGpsPoint> {
                let point = point
                    .cast::<PyBasicPoint3d>()
                    .map_err(|_| argument_error($py_name, "reverse"))?
                    .borrow();
                let values = point.values();
                let geodetic = self.projector.reverse(values).map_err(to_py_error)?;
                Ok(PyGpsPoint::wrap(geodetic))
            }

            /// A method, not a property, matching upstream.
            fn origin(&self) -> PyOrigin {
                PyOrigin::wrap(self.projector.origin())
            }
        }
    };
}

/// `lanelet2.projection.Projector`, the abstract base. Not constructible.
#[pyclass(name = "Projector", module = "lanelet2.projection", frozen)]
pub struct PyProjector;

#[pymethods]
impl PyProjector {
    #[new]
    fn new() -> PyResult<Self> {
        Err(not_instantiable())
    }

    // Present so the class advertises the interface, though no instance can exist.
    fn forward(&self, _point: &Bound<'_, PyAny>) -> PyResult<()> {
        Err(not_instantiable())
    }

    fn reverse(&self, _point: &Bound<'_, PyAny>) -> PyResult<()> {
        Err(not_instantiable())
    }

    fn origin(&self) -> PyResult<()> {
        Err(not_instantiable())
    }
}

projector_class!("MercatorProjector", PyMercatorProjector);
projector_class!("GeocentricProjector", PyGeocentricProjector);
projector_class!("LocalCartesianProjector", PyLocalCartesianProjector);
projector_class!("UtmProjector", PyUtmProjector);

#[pymethods]
impl PyMercatorProjector {
    #[new]
    fn new(origin: &PyOrigin) -> Self {
        PyMercatorProjector {
            projector: SharedProjector(Arc::new(SphericalMercator::new(origin.inner()))),
        }
    }
}

#[pymethods]
impl PyGeocentricProjector {
    #[new]
    fn new() -> Self {
        PyGeocentricProjector {
            projector: SharedProjector(Arc::new(Geocentric::new())),
        }
    }
}

#[pymethods]
impl PyLocalCartesianProjector {
    #[new]
    fn new(origin: &PyOrigin) -> Self {
        PyLocalCartesianProjector {
            projector: SharedProjector(Arc::new(LocalCartesian::new(origin.inner()))),
        }
    }
}

#[pymethods]
impl PyUtmProjector {
    #[new]
    #[pyo3(signature = (origin, useOffset = true, throwInPaddingArea = false))]
    #[allow(non_snake_case)]
    fn new(origin: &PyOrigin, useOffset: bool, throwInPaddingArea: bool) -> PyResult<Self> {
        let projector =
            Utm::new(origin.inner(), useOffset, throwInPaddingArea).map_err(to_py_error)?;
        Ok(PyUtmProjector {
            projector: SharedProjector(Arc::new(projector)),
        })
    }
}

/// Extracts the shared projector behind any projector object.
pub fn projector_of(obj: &Bound<'_, PyAny>) -> Option<SharedProjector> {
    macro_rules! probe {
        ($($rust:ident),+) => {
            $(
                if let Ok(value) = obj.cast::<$rust>() {
                    return Some(value.borrow().shared());
                }
            )+
        };
    }
    probe!(
        PyUtmProjector,
        PyMercatorProjector,
        PyLocalCartesianProjector,
        PyGeocentricProjector
    );
    None
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyProjector>()?;
    m.add_class::<PyMercatorProjector>()?;
    m.add_class::<PyGeocentricProjector>()?;
    m.add_class::<PyLocalCartesianProjector>()?;
    m.add_class::<PyUtmProjector>()?;
    Ok(())
}
