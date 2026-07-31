//! `lanelet2.io` — `Origin`, and loading and writing maps.
//!
//! Ground truth: `lanelet2_python/python_api/io.cpp`.

use std::path::Path;

use ll2_core::compat;
use ll2_io::WriteParams;
use ll2_projection::{GpsPoint, Origin, Projector, SphericalMercator};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyModule, PyString, PyTuple};

use crate::core::gps::{PyGpsPoint, gps_point_of};
use crate::core::map::PyLaneletMap;
use crate::err::{argument_error, runtime};
use crate::projection::projector_of;

/// `lanelet2.io.Origin`.
///
/// The `is_default` flag is not visible from Python but decides whether a parser
/// will accept the origin: loading a georeferenced map through a default-built
/// origin would silently produce meaningless coordinates, so it raises instead.
#[pyclass(name = "Origin", module = "lanelet2.io")]
#[derive(Clone, Copy)]
pub struct PyOrigin {
    pub(crate) origin: Origin,
}

impl PyOrigin {
    pub fn wrap(origin: Origin) -> Self {
        PyOrigin { origin }
    }

    pub fn inner(&self) -> Origin {
        self.origin
    }
}

#[pymethods]
impl PyOrigin {
    /// `Origin()`, `Origin(gpsPoint)` or `Origin(lat, lon, alt)`.
    ///
    /// Upstream misnames the third keyword `lon`, so `Origin(lat=…, lon=…, alt=…)`
    /// raises there; bug-compatibility mode restores that.
    #[new]
    #[pyo3(signature = (*args, **kwargs))]
    fn new(args: &Bound<'_, PyTuple>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
        let positional = args.len();
        let no_kwargs = kwargs.is_none_or(|d| d.is_empty());

        // `Origin()` is *not* the flagged default origin: Boost.Python routes the
        // no-argument call through the three-defaulted-arguments constructor, which
        // clears the flag. Only omitting the projector argument entirely reaches a
        // default-flagged origin, and only that makes the parser refuse.
        if positional == 0 && no_kwargs {
            return Ok(PyOrigin {
                origin: Origin::new(GpsPoint::default()),
            });
        }

        if positional == 1 && no_kwargs {
            let first = args.get_item(0)?;
            if let Some(point) = gps_point_of(&first) {
                return Ok(PyOrigin {
                    origin: Origin::new(point),
                });
            }
        }

        // Upstream registers the third keyword under the same name as the second.
        // Passing `lon=` therefore fills the altitude too, and `alt=` is accepted
        // and then ignored -- so `Origin(lat=49, lon=8.4)` ends up 8.4 m up.
        let bugged = compat::origin_alt_kwarg_is_named_lon();
        let third_name = if bugged { "lon" } else { "alt" };

        let arg = |index: usize, name: &str| -> PyResult<Option<Bound<'_, PyAny>>> {
            if index < positional {
                return Ok(Some(args.get_item(index)?));
            }
            match kwargs {
                None => Ok(None),
                Some(dict) => dict.get_item(name),
            }
        };
        let number = |value: Option<Bound<'_, PyAny>>| -> PyResult<f64> {
            match value {
                None => Ok(0.0),
                Some(value) => value.extract().map_err(|_| argument_error("Origin", "__init__")),
            }
        };

        if let Some(dict) = kwargs {
            // `alt` is a recognised-but-inert keyword in bug-compat mode, exactly as
            // Boost.Python treats it; anything else is rejected in both modes.
            let accepted = ["lat", "lon", "alt"];
            for key in dict.keys() {
                let key: String = key.extract()?;
                if !accepted.contains(&key.as_str()) {
                    return Err(argument_error("Origin", "__init__"));
                }
            }
        }

        let lat = number(arg(0, "lat")?)?;
        let lon = number(arg(1, "lon")?)?;
        let alt = number(arg(2, third_name)?)?;

        Ok(PyOrigin {
            origin: Origin::new(GpsPoint::new(lat, lon, alt)),
        })
    }

    #[getter]
    fn position(&self) -> PyGpsPoint {
        PyGpsPoint::wrap(self.origin.position)
    }

    #[setter(position)]
    fn set_position(&mut self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let point = gps_point_of(value).ok_or_else(|| argument_error("Origin", "position"))?;
        self.origin.position = point;
        self.origin.is_default = false;
        Ok(())
    }
}

/// Resolves the projector argument, which may be a projector or a bare origin.
///
/// An `Origin` selects the default projector, which upstream defines as spherical
/// Mercator.
fn resolve_projector(obj: &Bound<'_, PyAny>) -> PyResult<Box<dyn Projector + Send + Sync>> {
    if let Some(shared) = projector_of(obj) {
        return Ok(Box::new(shared));
    }
    if let Ok(origin) = obj.cast::<PyOrigin>() {
        return Ok(Box::new(SphericalMercator::new(origin.borrow().inner())));
    }
    Err(argument_error("io", "load"))
}

impl Projector for crate::projection::SharedProjector {
    fn forward(&self, point: GpsPoint) -> Result<[f64; 3], ll2_projection::ProjectionError> {
        self.0.forward(point)
    }

    fn reverse(&self, point: [f64; 3]) -> Result<GpsPoint, ll2_projection::ProjectionError> {
        self.0.reverse(point)
    }

    fn origin(&self) -> Origin {
        self.0.origin()
    }
}

/// Reads the optional `params` dictionary of writer options.
fn write_params(params: Option<&Bound<'_, PyAny>>) -> PyResult<WriteParams> {
    let Some(params) = params else {
        return Ok(WriteParams::default());
    };
    if params.is_none() {
        return Ok(WriteParams::default());
    }
    let dict = params.cast::<PyDict>()?;
    let flag = |name: &str| -> PyResult<bool> {
        Ok(match dict.get_item(name)? {
            None => false,
            Some(value) => {
                let text: String = value.cast::<PyString>()?.extract()?;
                ll2_core::attribute::Attribute::new(text).as_bool().unwrap_or(false)
            }
        })
    };
    Ok(WriteParams {
        josm_upload: flag("josm_upload")?,
        josm_format_elevation: flag("josm_format_elevation")?,
    })
}

/// `lanelet2.io.load(filename, projector | origin)`.
#[pyfunction]
#[pyo3(signature = (filename, projector = None))]
fn load(filename: &str, projector: Option<&Bound<'_, PyAny>>) -> PyResult<PyLaneletMap> {
    let projector = match projector {
        Some(value) => resolve_projector(value)?,
        None => Box::new(SphericalMercator::new(Origin::default_origin())),
    };
    let map = ll2_io::load(Path::new(filename), projector.as_ref())
        .map_err(|error| runtime(error.message()))?;
    Ok(PyLaneletMap::wrap(map))
}

/// `lanelet2.io.loadRobust(filename, projector)` — the map plus any problems found.
#[pyfunction]
#[pyo3(name = "loadRobust", signature = (filename, projector))]
fn load_robust<'py>(
    py: Python<'py>,
    filename: &str,
    projector: &Bound<'_, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let projector = resolve_projector(projector)?;
    let (map, errors) = ll2_io::load_robust(Path::new(filename), projector.as_ref())
        .map_err(|error| runtime(error.message()))?;
    let result = (PyLaneletMap::wrap(map), PyList::new(py, errors)?);
    Ok(result.into_pyobject(py)?.into_any())
}

/// `lanelet2.io.write(filename, map, projector | origin, params=None)`.
#[pyfunction]
#[pyo3(signature = (filename, map, projector = None, params = None))]
fn write(
    filename: &str,
    map: &PyLaneletMap,
    projector: Option<&Bound<'_, PyAny>>,
    params: Option<&Bound<'_, PyAny>>,
) -> PyResult<()> {
    let projector = match projector {
        Some(value) => resolve_projector(value)?,
        None => Box::new(SphericalMercator::new(Origin::default_origin())),
    };
    ll2_io::write(
        Path::new(filename),
        map.inner(),
        projector.as_ref(),
        write_params(params)?,
    )
    .map_err(|error| runtime(error.message()))
}

/// `lanelet2.io.writeRobust(...)` — returns the problems instead of raising.
#[pyfunction]
#[pyo3(name = "writeRobust", signature = (filename, map, projector = None, params = None))]
fn write_robust<'py>(
    py: Python<'py>,
    filename: &str,
    map: &PyLaneletMap,
    projector: Option<&Bound<'_, PyAny>>,
    params: Option<&Bound<'_, PyAny>>,
) -> PyResult<Bound<'py, PyList>> {
    let projector = match projector {
        Some(value) => resolve_projector(value)?,
        None => Box::new(SphericalMercator::new(Origin::default_origin())),
    };
    let errors = ll2_io::write_robust(
        Path::new(filename),
        map.inner(),
        projector.as_ref(),
        write_params(params)?,
    )
    .map_err(|error| runtime(error.message()))?;
    PyList::new(py, errors)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyOrigin>()?;
    m.add_function(wrap_pyfunction!(load, m)?)?;
    m.add_function(wrap_pyfunction!(load_robust, m)?)?;
    m.add_function(wrap_pyfunction!(write, m)?)?;
    m.add_function(wrap_pyfunction!(write_robust, m)?)?;
    Ok(())
}
