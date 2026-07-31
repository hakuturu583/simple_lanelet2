//! `lanelet2.io` — `Origin`, and loading and writing maps.
//!
//! Ground truth: `lanelet2_python/python_api/io.cpp`.

use ll2_core::compat;
use ll2_projection::{GpsPoint, Origin};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyModule, PyTuple};

use crate::core::gps::{PyGpsPoint, gps_point_of};
use crate::err::argument_error;

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

        if positional == 0 && no_kwargs {
            return Ok(PyOrigin {
                origin: Origin::default_origin(),
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

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyOrigin>()?;
    Ok(())
}
