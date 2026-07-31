//! `Lanelet` and `ConstLanelet`.
//!
//! Inverting a lanelet swaps *and* reverses its bounds, and writes are mapped back
//! through that swap. The centerline is computed on first access and cached; a
//! centerline the user assigned is distinguished from a computed one purely by
//! having a non-zero id, and only computed ones are discarded when a bound changes.
//!
//! Upstream: `lanelet2_python/python_api/core.cpp:998-1082`

use ll2_core::compat;
use ll2_core::lanelet::Lanelet;
use ll2_core::linestring::LineString;
use pyo3::exceptions::PyAttributeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

use crate::conv::{attribute_map_from_any, attributes_repr_arg, optional_attribute_map};
use crate::core::attribute::PyAttributeMap;
use crate::core::linestring::{PyConstLineString3d, PyLineString3d, linestring_of};
use crate::err::argument_error;

/// Extracts the shared lanelet behind either lanelet class.
pub fn lanelet_of(obj: &Bound<'_, PyAny>) -> Option<(Lanelet, bool)> {
    if let Ok(value) = obj.cast::<PyLanelet>() {
        return Some((value.borrow().lanelet.clone(), true));
    }
    if let Ok(value) = obj.cast::<PyConstLanelet>() {
        return Some((value.borrow().lanelet.clone(), false));
    }
    None
}

fn linestring_arg(obj: &Bound<'_, PyAny>, class: &str) -> PyResult<LineString> {
    linestring_of(obj)
        .map(|(line, _)| line)
        .ok_or_else(|| argument_error(class, "__init__"))
}

/// Parses `(id, leftBound, rightBound, attributes=AttributeMap(), regelems=[])`,
/// or a single lanelet handle to share storage with.
fn construct(
    class: &str,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Lanelet> {
    let positional = args.len();
    let no_kwargs = kwargs.is_none_or(|d| d.is_empty());

    if positional == 1 && no_kwargs {
        let first = args.get_item(0)?;
        if let Some((lanelet, _)) = lanelet_of(&first) {
            return Ok(lanelet);
        }
    }

    let arg = |index: usize, name: &str| -> PyResult<Option<Bound<'_, PyAny>>> {
        if index < positional {
            return Ok(Some(args.get_item(index)?));
        }
        match kwargs {
            None => Ok(None),
            Some(dict) => dict.get_item(name),
        }
    };

    let id: i64 = arg(0, "id")?
        .ok_or_else(|| argument_error(class, "__init__"))?
        .extract()
        .map_err(|_| argument_error(class, "__init__"))?;
    let left = linestring_arg(
        &arg(1, "leftBound")?.ok_or_else(|| argument_error(class, "__init__"))?,
        class,
    )?;
    let right = linestring_arg(
        &arg(2, "rightBound")?.ok_or_else(|| argument_error(class, "__init__"))?,
        class,
    )?;
    let attributes = optional_attribute_map(arg(3, "attributes")?.as_ref())?;
    Ok(Lanelet::new(id, left, right, attributes))
}

macro_rules! lanelet_class {
    ($py_name:literal, $rust:ident, $mutable:tt, $bound:ident) => {
        #[doc = concat!("`lanelet2.core.", $py_name, "`.")]
        #[pyclass(name = $py_name, module = "lanelet2.core")]
        pub struct $rust {
            lanelet: Lanelet,
        }

        impl $rust {
            pub fn wrap(lanelet: Lanelet) -> Self {
                $rust { lanelet }
            }

            pub fn inner(&self) -> &Lanelet {
                &self.lanelet
            }
        }

        #[pymethods]
        impl $rust {
            #[new]
            #[pyo3(signature = (*args, **kwargs))]
            fn new(args: &Bound<'_, PyTuple>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
                Ok($rust {
                    lanelet: construct($py_name, args, kwargs)?,
                })
            }

            #[getter]
            fn id(&self) -> i64 {
                self.lanelet.id()
            }

            #[setter(id)]
            fn set_id(&self, value: i64) -> PyResult<()> {
                if !$mutable {
                    return Err(PyAttributeError::new_err("can't set attribute"));
                }
                self.lanelet.set_id(value);
                Ok(())
            }

            #[getter]
            fn attributes(&self) -> PyAttributeMap {
                PyAttributeMap::proxy(self.lanelet.attributes().clone())
            }

            #[setter(attributes)]
            fn set_attributes(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
                if !$mutable {
                    return Err(PyAttributeError::new_err("can't set attribute"));
                }
                self.lanelet.set_attributes(attribute_map_from_any(value)?);
                Ok(())
            }

            #[getter]
            #[pyo3(name = "leftBound")]
            fn left_bound(&self) -> $bound {
                $bound::wrap(self.lanelet.left_bound())
            }

            #[setter(leftBound)]
            fn set_left_bound(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
                if !$mutable {
                    return Err(PyAttributeError::new_err("can't set attribute"));
                }
                self.lanelet.set_left_bound(linestring_arg(value, $py_name)?);
                Ok(())
            }

            #[getter]
            #[pyo3(name = "rightBound")]
            fn right_bound(&self) -> $bound {
                $bound::wrap(self.lanelet.right_bound())
            }

            #[setter(rightBound)]
            fn set_right_bound(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
                if !$mutable {
                    return Err(PyAttributeError::new_err("can't set attribute"));
                }
                self.lanelet.set_right_bound(linestring_arg(value, $py_name)?);
                Ok(())
            }

            /// Computed on first access and cached. The computed centerline, and
            /// every point in it, carries no id.
            #[getter]
            fn centerline(&self) -> PyConstLineString3d {
                PyConstLineString3d::wrap(self.lanelet.centerline())
            }

            #[setter(centerline)]
            fn set_centerline(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
                if !$mutable {
                    return Err(PyAttributeError::new_err("can't set attribute"));
                }
                self.lanelet.set_centerline(linestring_arg(value, $py_name)?);
                Ok(())
            }

            /// Discards a computed centerline; a centerline the user assigned (that
            /// is, one with an id) is kept.
            #[pyo3(name = "resetCache")]
            fn reset_cache(&self) {
                self.lanelet.reset_cache();
            }

            fn invert(&self) -> Self {
                $rust::wrap(self.lanelet.invert())
            }

            fn inverted(&self) -> bool {
                self.lanelet.is_inverted()
            }

            fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
                match lanelet_of(other) {
                    Some((lanelet, mutable)) => {
                        mutable == $mutable && self.lanelet.is_same_view(&lanelet)
                    }
                    None => false,
                }
            }

            fn __ne__(&self, other: &Bound<'_, PyAny>) -> bool {
                !self.__eq__(other)
            }

            fn __hash__(&self) -> i64 {
                if compat::hash_by_id_only() {
                    self.lanelet.id()
                } else {
                    self.lanelet.identity() as i64
                }
            }

            fn __str__(&self) -> String {
                self.lanelet.to_display_string()
            }

            fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
                let attributes = attributes_repr_arg(py, &self.lanelet.attributes().read())?;
                let left = PyLineString3d::wrap(self.lanelet.left_bound()).__repr__(py)?;
                let right = PyLineString3d::wrap(self.lanelet.right_bound()).__repr__(py)?;
                Ok(self.lanelet.repr($py_name, &left, &right, &attributes, ""))
            }
        }
    };
}

lanelet_class!("Lanelet", PyLanelet, true, PyLineString3d);
lanelet_class!("ConstLanelet", PyConstLanelet, false, PyConstLineString3d);

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyLanelet>()?;
    m.add_class::<PyConstLanelet>()?;
    Ok(())
}
