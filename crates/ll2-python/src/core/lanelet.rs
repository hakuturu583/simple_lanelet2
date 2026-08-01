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
use crate::core::compound::{PyCompoundPolygon2d, PyCompoundPolygon3d};
use crate::core::linestring::{PyConstLineString3d, PyLineString3d, linestring_of};
use crate::core::regelem::{PyRegulatoryElement, regelem_of};
use crate::err::argument_error;
use ll2_core::regelem::{RegElemKind, RegulatoryElement};
use pyo3::types::PyList;

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

/// Wraps regulatory elements in the concrete Python class each one reports.
///
/// PyO3 needs the subclass initializer, so the mapping from kind to class is
/// spelled out rather than inferred.
pub fn regelems_to_py(py: Python<'_>, regelems: &[RegulatoryElement]) -> PyResult<Py<PyAny>> {
    use crate::core::regelem::{
        PyAllWayStop, PyRightOfWay, PySpeedLimit, PyTrafficLight, PyTrafficSign,
    };

    let list = PyList::empty(py);
    for regelem in regelems {
        let base = PyRegulatoryElement::wrap(regelem.clone());
        let object: Py<PyAny> = match regelem.kind() {
            RegElemKind::TrafficLight => Py::new(py, (PyTrafficLight, base))?.into_any(),
            RegElemKind::RightOfWay => Py::new(py, (PyRightOfWay, base))?.into_any(),
            RegElemKind::AllWayStop => Py::new(py, (PyAllWayStop, base))?.into_any(),
            RegElemKind::TrafficSign => Py::new(py, (PyTrafficSign, base))?.into_any(),
            RegElemKind::SpeedLimit => Py::new(
                py,
                PyClassInitializer::from(base)
                    .add_subclass(PyTrafficSign)
                    .add_subclass(PySpeedLimit),
            )?
            .into_any(),
            // `Generic`, and any kind a loaded extension registered but has no
            // Python class for, surface as the plain base class.
            _ => Py::new(py, base)?.into_any(),
        };
        list.append(object)?;
    }
    Ok(list.into_any().unbind())
}

/// The regulatory-element argument of a repr: a bracketed list, or empty when
/// there are none so that `make_repr` drops it.
pub fn regelems_repr_arg(py: Python<'_>, regelems: &[RegulatoryElement]) -> PyResult<String> {
    if regelems.is_empty() {
        return Ok(String::new());
    }
    let rendered: Vec<String> = regelems
        .iter()
        .map(|regelem| PyRegulatoryElement::wrap(regelem.clone()).__repr__(py))
        .collect::<PyResult<_>>()?;
    Ok(format!("[{}]", rendered.join(", ")))
}

/// Filters the attached elements down to one kind, for `trafficLights()` and friends.
///
/// Upstream these are `regulatoryElementsAs<T>()`, i.e. `dynamic_pointer_cast`, so a
/// derived kind answers to its base's accessor — `trafficSigns()` includes a
/// `SpeedLimit`. Comparing kinds for equality would silently drop those.
fn regelems_of_kind(py: Python<'_>, lanelet: &Lanelet, kind: RegElemKind) -> PyResult<Py<PyAny>> {
    let matching: Vec<RegulatoryElement> = lanelet
        .regulatory_elements()
        .into_iter()
        .filter(|regelem| regelem.kind().is_a(kind))
        .collect();
    regelems_to_py(py, &matching)
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
    let lanelet = Lanelet::new(id, left, right, attributes);
    if let Some(regelems) = arg(4, "regelems")? {
        if !regelems.is_none() {
            for item in regelems.try_iter()? {
                let item = item?;
                let regelem = regelem_of(&item).ok_or_else(|| argument_error(class, "__init__"))?;
                lanelet.add_regulatory_element(regelem);
            }
        }
    }
    Ok(lanelet)
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
            fn new(
                args: &Bound<'_, PyTuple>,
                kwargs: Option<&Bound<'_, PyDict>>,
            ) -> PyResult<Self> {
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
                self.lanelet
                    .set_left_bound(linestring_arg(value, $py_name)?);
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
                self.lanelet
                    .set_right_bound(linestring_arg(value, $py_name)?);
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
                self.lanelet
                    .set_centerline(linestring_arg(value, $py_name)?);
                Ok(())
            }

            /// Discards a computed centerline; a centerline the user assigned (that
            /// is, one with an id) is kept.
            #[pyo3(name = "resetCache")]
            fn reset_cache(&self) {
                self.lanelet.reset_cache();
            }

            #[getter]
            #[pyo3(name = "regulatoryElements")]
            fn regulatory_elements(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                regelems_to_py(py, &self.lanelet.regulatory_elements())
            }

            #[pyo3(name = "trafficLights")]
            fn traffic_lights(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                regelems_of_kind(py, &self.lanelet, RegElemKind::TrafficLight)
            }

            #[pyo3(name = "trafficSigns")]
            fn traffic_signs(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                regelems_of_kind(py, &self.lanelet, RegElemKind::TrafficSign)
            }

            #[pyo3(name = "speedLimits")]
            fn speed_limits(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                regelems_of_kind(py, &self.lanelet, RegElemKind::SpeedLimit)
            }

            #[pyo3(name = "rightOfWay")]
            fn right_of_way(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                regelems_of_kind(py, &self.lanelet, RegElemKind::RightOfWay)
            }

            #[pyo3(name = "allWayStop")]
            fn all_way_stop(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                regelems_of_kind(py, &self.lanelet, RegElemKind::AllWayStop)
            }

            /// The outline: the left bound followed by the reversed right bound.
            #[pyo3(name = "polygon3d")]
            fn polygon3d(&self) -> PyCompoundPolygon3d {
                PyCompoundPolygon3d::wrap(self.lanelet.polygon())
            }

            #[pyo3(name = "polygon2d")]
            fn polygon2d(&self) -> PyCompoundPolygon2d {
                PyCompoundPolygon2d::wrap(self.lanelet.polygon())
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

            pub fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
                let attributes = attributes_repr_arg(py, &self.lanelet.attributes().read())?;
                let left = $bound::wrap(self.lanelet.left_bound()).__repr__(py)?;
                let right = $bound::wrap(self.lanelet.right_bound()).__repr__(py)?;
                let regelems = regelems_repr_arg(py, &self.lanelet.regulatory_elements())?;
                Ok(self
                    .lanelet
                    .repr($py_name, &left, &right, &attributes, &regelems))
            }
        }
    };
}

lanelet_class!("Lanelet", PyLanelet, true, PyLineString3d);
lanelet_class!("ConstLanelet", PyConstLanelet, false, PyConstLineString3d);

#[pymethods]
impl PyLanelet {
    #[pyo3(name = "addRegulatoryElement")]
    fn add_regulatory_element(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let regelem =
            regelem_of(value).ok_or_else(|| argument_error("Lanelet", "addRegulatoryElement"))?;
        self.lanelet.add_regulatory_element(regelem);
        Ok(())
    }

    #[pyo3(name = "removeRegulatoryElement")]
    fn remove_regulatory_element(&self, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        let regelem = regelem_of(value)
            .ok_or_else(|| argument_error("Lanelet", "removeRegulatoryElement"))?;
        Ok(self.lanelet.remove_regulatory_element(&regelem))
    }
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyLanelet>()?;
    m.add_class::<PyConstLanelet>()?;
    Ok(())
}
