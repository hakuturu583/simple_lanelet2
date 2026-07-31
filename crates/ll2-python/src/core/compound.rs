//! `CompoundLineString2d/3d` and `CompoundPolygon2d/3d`.
//!
//! These present several linestrings as one. They carry no id and no attributes of
//! their own, which is why upstream registers no `__repr__` for them — printing one
//! gives the default `<lanelet2.core.CompoundLineString3d object at 0x...>`.
//!
//! Upstream: `lanelet2_python/python_api/core.cpp:902-972`

use ll2_core::compound::CompoundLineString;
use ll2_core::linestring::LineString;
use ll2_core::point::Point;
use pyo3::prelude::*;
use pyo3::types::PyList;

use crate::core::basic::{PyBasicPoint2d, PyBasicPoint3d};
use crate::core::linestring::{PyConstLineString2d, PyConstLineString3d, linestring_of};
use crate::core::point::{PyConstPoint2d, PyConstPoint3d};
use crate::err::{argument_error, resolve_index};

fn members_from_any(obj: &Bound<'_, PyAny>, class: &str) -> PyResult<Vec<LineString>> {
    let mut out = Vec::new();
    for item in obj.try_iter()? {
        let item = item?;
        let (line, _) = linestring_of(&item).ok_or_else(|| argument_error(class, "__init__"))?;
        out.push(line);
    }
    Ok(out)
}

macro_rules! compound_class {
    (
        name: $py_name:literal,
        rust: $rust:ident,
        polygon: $polygon:tt,
        item: $item:ident,
        member: $member:ident,
        dimensions: $dims:tt,
    ) => {
        #[doc = concat!("`lanelet2.core.", $py_name, "`.")]
        #[pyclass(name = $py_name, module = "lanelet2.core")]
        pub struct $rust {
            compound: CompoundLineString,
        }

        impl $rust {
            pub fn wrap(compound: CompoundLineString) -> Self {
                $rust { compound }
            }

            pub fn inner(&self) -> &CompoundLineString {
                &self.compound
            }

            fn item(point: &Point) -> $item {
                compound_class!(@item $dims, $item, point)
            }
        }

        #[pymethods]
        impl $rust {
            #[new]
            #[pyo3(signature = (lineStrings))]
            #[allow(non_snake_case)]
            fn new(lineStrings: &Bound<'_, PyAny>) -> PyResult<Self> {
                Ok($rust {
                    compound: CompoundLineString::new(members_from_any(lineStrings, $py_name)?),
                })
            }

            /// The member linestrings, in view order. An inverted compound reverses
            /// the list and inverts each member.
            #[pyo3(name = "lineStrings")]
            fn line_strings(&self) -> Vec<$member> {
                self.compound
                    .line_strings()
                    .into_iter()
                    .map($member::wrap)
                    .collect()
            }

            /// The ids of the member linestrings, in view order.
            fn ids(&self) -> Vec<i64> {
                self.compound.ids()
            }

            fn inverted(&self) -> bool {
                self.compound.is_inverted()
            }

            /// Upstream computes this by walking the points, so it is O(n).
            fn __len__(&self) -> usize {
                self.compound.len()
            }

            fn __getitem__(&self, index: &Bound<'_, PyAny>) -> PyResult<$item> {
                let index: isize = index
                    .extract()
                    .map_err(|_| argument_error($py_name, "__getitem__"))?;
                let points = self.compound.points();
                let index = resolve_index(index, points.len())?;
                Ok(Self::item(&points[index]))
            }

            fn __iter__(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
                let py = slf.py();
                let items: Vec<$item> =
                    slf.compound.points().iter().map(Self::item).collect();
                Ok(PyList::new(py, items)?.try_iter()?.into())
            }

            /// A polygon's closing segment counts; an open line's does not.
            #[pyo3(name = "numSegments")]
            fn num_segments(&self) -> usize {
                if $polygon {
                    self.compound.num_polygon_segments()
                } else {
                    self.compound.num_segments()
                }
            }
        }
    };

    (@item 2, $item:ident, $point:expr) => { $item::wrap($point.clone()) };
    (@item 3, $item:ident, $point:expr) => { $item::wrap($point.clone()) };
}

compound_class! {
    name: "CompoundLineString3d", rust: PyCompoundLineString3d, polygon: false,
    item: PyConstPoint3d, member: PyConstLineString3d, dimensions: 3,
}
compound_class! {
    name: "CompoundLineString2d", rust: PyCompoundLineString2d, polygon: false,
    item: PyConstPoint2d, member: PyConstLineString2d, dimensions: 2,
}
compound_class! {
    name: "CompoundPolygon3d", rust: PyCompoundPolygon3d, polygon: true,
    item: PyConstPoint3d, member: PyConstLineString3d, dimensions: 3,
}
compound_class! {
    name: "CompoundPolygon2d", rust: PyCompoundPolygon2d, polygon: true,
    item: PyConstPoint2d, member: PyConstLineString2d, dimensions: 2,
}

/// The hybrid compounds yield plain coordinates instead of primitives.
macro_rules! compound_hybrid {
    ($py_name:literal, $rust:ident, $item:ident, $dims:tt) => {
        #[doc = concat!("`lanelet2.core.", $py_name, "`.")]
        #[pyclass(name = $py_name, module = "lanelet2.core")]
        pub struct $rust {
            compound: CompoundLineString,
        }

        impl $rust {
            pub fn wrap(compound: CompoundLineString) -> Self {
                $rust { compound }
            }

            fn item(point: &Point) -> $item {
                let [x, y, _z] = point.xyz();
                compound_hybrid!(@coords $dims, $item, x, y, _z)
            }
        }

        #[pymethods]
        impl $rust {
            fn __len__(&self) -> usize {
                self.compound.len()
            }

            fn __getitem__(&self, index: &Bound<'_, PyAny>) -> PyResult<$item> {
                let index: isize = index
                    .extract()
                    .map_err(|_| argument_error($py_name, "__getitem__"))?;
                let points = self.compound.points();
                let index = resolve_index(index, points.len())?;
                Ok(Self::item(&points[index]))
            }

            fn __iter__(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
                let py = slf.py();
                let items: Vec<$item> = slf.compound.points().iter().map(Self::item).collect();
                Ok(PyList::new(py, items)?.try_iter()?.into())
            }
        }
    };

    (@coords 2, $item:ident, $x:expr, $y:expr, $z:expr) => { $item::free([$x, $y, 0.0]) };
    (@coords 3, $item:ident, $x:expr, $y:expr, $z:expr) => { $item::free([$x, $y, $z]) };
}

compound_hybrid!(
    "CompoundHybridLineString3d",
    PyCompoundHybridLineString3d,
    PyBasicPoint3d,
    3
);
compound_hybrid!(
    "CompoundHybridLineString2d",
    PyCompoundHybridLineString2d,
    PyBasicPoint2d,
    2
);
compound_hybrid!(
    "CompoundHybridPolygon3d",
    PyCompoundHybridPolygon3d,
    PyBasicPoint3d,
    3
);
compound_hybrid!(
    "CompoundHybridPolygon2d",
    PyCompoundHybridPolygon2d,
    PyBasicPoint2d,
    2
);

/// Extracts the compound behind any compound-shaped object.
pub fn compound_of(obj: &Bound<'_, PyAny>) -> Option<CompoundLineString> {
    macro_rules! probe {
        ($($rust:ident),+) => {
            $(
                if let Ok(value) = obj.cast::<$rust>() {
                    return Some(value.borrow().compound.clone());
                }
            )+
        };
    }
    probe!(
        PyCompoundLineString3d,
        PyCompoundLineString2d,
        PyCompoundPolygon3d,
        PyCompoundPolygon2d,
        PyCompoundHybridLineString3d,
        PyCompoundHybridLineString2d,
        PyCompoundHybridPolygon3d,
        PyCompoundHybridPolygon2d
    );
    None
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyCompoundLineString2d>()?;
    m.add_class::<PyCompoundLineString3d>()?;
    m.add_class::<PyCompoundPolygon2d>()?;
    m.add_class::<PyCompoundPolygon3d>()?;
    m.add_class::<PyCompoundHybridLineString2d>()?;
    m.add_class::<PyCompoundHybridLineString3d>()?;
    m.add_class::<PyCompoundHybridPolygon2d>()?;
    m.add_class::<PyCompoundHybridPolygon3d>()?;
    Ok(())
}
