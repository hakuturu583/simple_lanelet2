//! `Area`, `ConstArea` and their inner-bound containers.
//!
//! An area is one outer ring plus any number of holes, each ring being a *list* of
//! linestrings chained end to start. There is no direction of travel, so no
//! `invert`, and the ring polygons are recomputed eagerly on assignment rather than
//! lazily like a lanelet's centerline.
//!
//! Upstream: `lanelet2_python/python_api/core.cpp:1122-1204`

use ll2_core::area::{Area, InnerBound};
use ll2_core::compat;
use ll2_core::id::Id;
use pyo3::exceptions::PyAttributeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};

use crate::conv::{attribute_map_from_any, attributes_repr_arg, optional_attribute_map};
use crate::core::attribute::PyAttributeMap;
use crate::core::compound::PyCompoundPolygon3d;
use crate::core::linestring::{PyConstLineString3d, PyLineString3d, linestring_of};
use crate::err::{argument_error, resolve_index, runtime};

/// Extracts the shared area behind either area class.
pub fn area_of(obj: &Bound<'_, PyAny>) -> Option<Area> {
    if let Ok(value) = obj.cast::<PyArea>() {
        return Some(value.borrow().area.clone());
    }
    if let Ok(value) = obj.cast::<PyConstArea>() {
        return Some(value.borrow().area.clone());
    }
    None
}

fn ring_from_any(obj: &Bound<'_, PyAny>, class: &str) -> PyResult<InnerBound> {
    let mut out = Vec::new();
    for item in obj.try_iter()? {
        let item = item?;
        let (line, _) = linestring_of(&item).ok_or_else(|| argument_error(class, "__init__"))?;
        out.push(line);
    }
    Ok(out)
}

fn rings_from_any(obj: &Bound<'_, PyAny>, class: &str) -> PyResult<Vec<InnerBound>> {
    if let Ok(bounds) = obj.cast::<PyInnerBounds>() {
        return Ok(bounds.borrow().area.inner_bounds());
    }
    let mut out = Vec::new();
    for item in obj.try_iter()? {
        out.push(ring_from_any(&item?, class)?);
    }
    Ok(out)
}

/// Renders an area's repr without its regulatory elements, for cycle breaking.
pub fn area_repr_without_regelems(py: Python<'_>, area: &Area) -> PyResult<String> {
    render_area(py, area, "Area", "", const_line_repr)
}

/// How a ring's linestrings are rendered: a `ConstArea` shows const bounds.
type LineRepr = fn(Python<'_>, ll2_core::linestring::LineString) -> PyResult<String>;

fn writable_line_repr(py: Python<'_>, line: ll2_core::linestring::LineString) -> PyResult<String> {
    PyLineString3d::wrap(line).__repr__(py)
}

fn const_line_repr(py: Python<'_>, line: ll2_core::linestring::LineString) -> PyResult<String> {
    PyConstLineString3d::wrap(line).__repr__(py)
}

fn render_area(
    py: Python<'_>,
    area: &Area,
    class_name: &str,
    regelems_repr: &str,
    line_repr: LineRepr,
) -> PyResult<String> {
    let outer: Vec<String> = area
        .outer_bound()
        .into_iter()
        .map(|line| line_repr(py, line))
        .collect::<PyResult<_>>()?;
    let inner: Vec<String> = area
        .inner_bounds()
        .into_iter()
        .map(|hole| -> PyResult<String> {
            let rendered: Vec<String> = hole
                .into_iter()
                .map(|line| line_repr(py, line))
                .collect::<PyResult<_>>()?;
            Ok(format!("[{}]", rendered.join(", ")))
        })
        .collect::<PyResult<_>>()?;
    let attributes = attributes_repr_arg(py, &area.attributes().read())?;
    Ok(ll2_core::fmt::make_repr(
        class_name,
        &[
            area.id().to_string(),
            format!("[{}]", outer.join(", ")),
            format!("[{}]", inner.join(", ")),
            attributes,
            regelems_repr.to_owned(),
        ],
    ))
}

macro_rules! inner_bounds_class {
    ($py_name:literal, $rust:ident, $mutable:tt) => {
        #[doc = concat!("`lanelet2.core.", $py_name, "`.")]
        ///
        /// A live view of an area's holes. Upstream binds it as a real class rather
        /// than converting to a list, because the nested vectors would otherwise
        /// come back as temporaries.
        #[pyclass(name = $py_name, module = "lanelet2.core")]
        pub struct $rust {
            area: Area,
        }

        impl $rust {
            pub fn wrap(area: Area) -> Self {
                $rust { area }
            }
        }

        #[pymethods]
        impl $rust {
            fn __len__(&self) -> usize {
                self.area.inner_bounds().len()
            }

            fn __getitem__(&self, index: &Bound<'_, PyAny>) -> PyResult<Vec<PyLineString3d>> {
                let index: isize = index
                    .extract()
                    .map_err(|_| argument_error($py_name, "__getitem__"))?;
                let bounds = self.area.inner_bounds();
                let index = resolve_index(index, bounds.len())?;
                Ok(bounds[index].iter().cloned().map(PyLineString3d::wrap).collect())
            }

            fn __iter__(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
                let py = slf.py();
                let holes: Vec<Vec<PyLineString3d>> = slf
                    .area
                    .inner_bounds()
                    .into_iter()
                    .map(|hole| hole.into_iter().map(PyLineString3d::wrap).collect())
                    .collect();
                Ok(PyList::new(py, holes)?.try_iter()?.into())
            }

            /// A plain list repr, with no class-name wrapper — as upstream does.
            fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
                let holes: Vec<String> = self
                    .area
                    .inner_bounds()
                    .into_iter()
                    .map(|hole| -> PyResult<String> {
                        let rendered: Vec<String> = hole
                            .into_iter()
                            .map(|line| PyLineString3d::wrap(line).__repr__(py))
                            .collect::<PyResult<_>>()?;
                        Ok(format!("[{}]", rendered.join(", ")))
                    })
                    .collect::<PyResult<_>>()?;
                Ok(format!("[{}]", holes.join(", ")))
            }
        }

        inner_bounds_class!(@mutators $mutable, $rust, $py_name);
    };

    (@mutators false, $rust:ident, $py_name:literal) => {};
    (@mutators true, $rust:ident, $py_name:literal) => {
        #[pymethods]
        impl $rust {
        fn __setitem__(&self, index: &Bound<'_, PyAny>, value: &Bound<'_, PyAny>) -> PyResult<()> {
            let index: isize = index
                .extract()
                .map_err(|_| argument_error($py_name, "__setitem__"))?;
            let mut bounds = self.area.inner_bounds();
            let index = resolve_index(index, bounds.len())?;
            bounds[index] = ring_from_any(value, $py_name)?;
            self.area.set_inner_bounds(bounds);
            Ok(())
        }

        fn __delitem__(&self, index: &Bound<'_, PyAny>) -> PyResult<()> {
            let index: isize = index
                .extract()
                .map_err(|_| argument_error($py_name, "__delitem__"))?;
            let mut bounds = self.area.inner_bounds();
            let index = resolve_index(index, bounds.len())?;
            bounds.remove(index);
            self.area.set_inner_bounds(bounds);
            Ok(())
        }

        fn append(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
            let mut bounds = self.area.inner_bounds();
            bounds.push(ring_from_any(value, $py_name)?);
            self.area.set_inner_bounds(bounds);
            Ok(())
        }
        }
    };
}

inner_bounds_class!("InnerBounds", PyInnerBounds, true);
inner_bounds_class!("ConstInnerBounds", PyConstInnerBounds, false);

macro_rules! area_class {
    (@line_repr true) => { writable_line_repr };
    (@line_repr false) => { const_line_repr };

    ($py_name:literal, $rust:ident, $mutable:tt, $bound:ident, $holes:ident) => {
        #[doc = concat!("`lanelet2.core.", $py_name, "`.")]
        #[pyclass(name = $py_name, module = "lanelet2.core")]
        pub struct $rust {
            area: Area,
        }

        impl $rust {
            pub fn wrap(area: Area) -> Self {
                $rust { area }
            }

            pub fn inner(&self) -> &Area {
                &self.area
            }
        }

        #[pymethods]
        impl $rust {
            #[new]
            #[pyo3(signature = (*args, **kwargs))]
            fn new(args: &Bound<'_, PyTuple>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
                let positional = args.len();
                let arg = |index: usize, name: &str| -> PyResult<Option<Bound<'_, PyAny>>> {
                    if index < positional {
                        return Ok(Some(args.get_item(index)?));
                    }
                    match kwargs {
                        None => Ok(None),
                        Some(dict) => dict.get_item(name),
                    }
                };

                if positional == 1 && kwargs.is_none_or(|d| d.is_empty()) {
                    let first = args.get_item(0)?;
                    if let Some(area) = area_of(&first) {
                        return Ok($rust { area });
                    }
                }

                let id: Id = arg(0, "id")?
                    .ok_or_else(|| argument_error($py_name, "__init__"))?
                    .extract()
                    .map_err(|_| argument_error($py_name, "__init__"))?;
                let outer = ring_from_any(
                    &arg(1, "outerBound")?.ok_or_else(|| argument_error($py_name, "__init__"))?,
                    $py_name,
                )?;
                let inner = match arg(2, "innerBounds")? {
                    None => Vec::new(),
                    Some(value) if value.is_none() => Vec::new(),
                    Some(value) => rings_from_any(&value, $py_name)?,
                };
                let attributes = optional_attribute_map(arg(3, "attributes")?.as_ref())?;
                Ok($rust {
                    area: Area::new(id, outer, inner, attributes),
                })
            }

            #[getter]
            fn id(&self) -> Id {
                self.area.id()
            }

            #[setter(id)]
            fn set_id(&self, value: Id) -> PyResult<()> {
                if !$mutable {
                    return Err(PyAttributeError::new_err("can't set attribute"));
                }
                self.area.set_id(value);
                Ok(())
            }

            #[getter]
            fn attributes(&self) -> PyAttributeMap {
                PyAttributeMap::proxy(self.area.attributes().clone())
            }

            #[setter(attributes)]
            fn set_attributes(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
                if !$mutable {
                    return Err(PyAttributeError::new_err("can't set attribute"));
                }
                self.area.set_attributes(attribute_map_from_any(value)?);
                Ok(())
            }

            #[getter]
            #[pyo3(name = "outerBound")]
            fn outer_bound(&self) -> Vec<$bound> {
                self.area.outer_bound().into_iter().map($bound::wrap).collect()
            }

            #[setter(outerBound)]
            fn set_outer_bound(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
                if !$mutable {
                    return Err(PyAttributeError::new_err("can't set attribute"));
                }
                self.area.set_outer_bound(ring_from_any(value, $py_name)?);
                Ok(())
            }

            #[getter]
            #[pyo3(name = "innerBounds")]
            fn inner_bounds(&self) -> $holes {
                $holes::wrap(self.area.clone())
            }

            #[setter(innerBounds)]
            fn set_inner_bounds(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
                if !$mutable {
                    return Err(PyAttributeError::new_err("can't set attribute"));
                }
                self.area.set_inner_bounds(rings_from_any(value, $py_name)?);
                Ok(())
            }

            #[getter]
            #[pyo3(name = "regulatoryElements")]
            fn regulatory_elements(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                crate::core::lanelet::regelems_to_py(py, &self.area.regulatory_elements())
            }

            #[pyo3(name = "outerBoundPolygon")]
            fn outer_bound_polygon(&self) -> PyCompoundPolygon3d {
                PyCompoundPolygon3d::wrap(self.area.outer_bound_polygon())
            }

            #[pyo3(name = "innerBoundPolygons")]
            fn inner_bound_polygons(&self) -> Vec<PyCompoundPolygon3d> {
                self.area
                    .inner_bound_polygons()
                    .into_iter()
                    .map(PyCompoundPolygon3d::wrap)
                    .collect()
            }

            /// Removed upstream; kept only so that calling it fails the same way.
            #[pyo3(name = "innerBoundPolygon")]
            fn inner_bound_polygon(&self) -> PyResult<()> {
                Err(runtime("innerBoundPolygon is deprecated. Use innerBoundPolygons instead!"))
            }

            fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
                match area_of(other) {
                    Some(area) => self.area.is_same_data(&area),
                    None => false,
                }
            }

            fn __ne__(&self, other: &Bound<'_, PyAny>) -> bool {
                !self.__eq__(other)
            }

            fn __hash__(&self) -> i64 {
                if compat::hash_by_id_only() {
                    self.area.id()
                } else {
                    self.area.identity() as i64
                }
            }

            fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
                let regelems = crate::core::lanelet::regelems_repr_arg(
                    py,
                    &self.area.regulatory_elements(),
                )?;
                // Upstream hardcodes the display name, so ConstArea's repr claims to
                // be an Area. Repaired by default.
                let name = if !$mutable && compat::const_area_repr_says_area() {
                    "Area"
                } else {
                    $py_name
                };
                render_area(py, &self.area, name, &regelems, area_class!(@line_repr $mutable))
            }
        }
    };
}

area_class!("Area", PyArea, true, PyLineString3d, PyInnerBounds);
area_class!("ConstArea", PyConstArea, false, PyConstLineString3d, PyConstInnerBounds);

/// The mutable area additionally lets regulatory elements be attached.
#[pymethods]
impl PyArea {
    #[pyo3(name = "addRegulatoryElement")]
    fn add_regulatory_element(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let regelem = crate::core::regelem::regelem_of(value)
            .ok_or_else(|| argument_error("Area", "addRegulatoryElement"))?;
        self.area.add_regulatory_element(regelem);
        Ok(())
    }

    #[pyo3(name = "removeRegulatoryElement")]
    fn remove_regulatory_element(&self, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        let regelem = crate::core::regelem::regelem_of(value)
            .ok_or_else(|| argument_error("Area", "removeRegulatoryElement"))?;
        Ok(self.area.remove_regulatory_element(&regelem))
    }
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyInnerBounds>()?;
    m.add_class::<PyConstInnerBounds>()?;
    m.add_class::<PyArea>()?;
    m.add_class::<PyConstArea>()?;
    Ok(())
}
