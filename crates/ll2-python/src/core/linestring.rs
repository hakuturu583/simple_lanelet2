//! Linestrings and polygons, in all twelve Python flavours.
//!
//! Three independent axes multiply out: dimension (2D/3D), mutability
//! (`LineString3d` vs `ConstLineString3d`) and whether the items are primitives or
//! plain coordinates (`ConstHybridLineString3d`). A polygon is the same storage with
//! polygon traits — upstream shares `LineStringData` between them — so the only
//! differences are the class name and the absence of `invert`.
//!
//! Upstream: `lanelet2_python/python_api/core.cpp:340-380, 796-996`

use ll2_core::linestring::LineString;
use ll2_core::point::Point;
use pyo3::exceptions::PyAttributeError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};

use crate::conv::{attribute_map_from_any, attributes_repr_arg, optional_attribute_map};
use crate::core::attribute::PyAttributeMap;
use crate::core::basic::{PyBasicPoint2d, PyBasicPoint3d};
use crate::core::point::{PyConstPoint2d, PyConstPoint3d, PyPoint2d, PyPoint3d, point_of};
use crate::err::{argument_error, resolve_index};

/// Identifies which registered C++ type a handle corresponds to.
///
/// Upstream registers `operator==` per concrete type, so two handles only ever
/// compare equal when they came from the same class family. A `LineString2d` and a
/// `LineString3d` over identical storage are *not* equal.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Kind {
    pub dimensions: u8,
    pub polygon: bool,
    pub hybrid: bool,
}

/// Every linestring-shaped class, so that constructors and `==` can recognise
/// each other's instances.
macro_rules! linestring_registry {
    ($($rust:ident),+ $(,)?) => {
        /// Extracts the shared linestring behind any linestring-shaped object.
        pub fn linestring_of(obj: &Bound<'_, PyAny>) -> Option<(LineString, Kind)> {
            $(
                if let Ok(value) = obj.cast::<$rust>() {
                    let value = value.borrow();
                    return Some((value.line.clone(), $rust::KIND));
                }
            )+
            None
        }
    };
}

/// Reads the `points` constructor argument: any iterable of `Point3d`.
fn points_from_any(obj: &Bound<'_, PyAny>) -> PyResult<Vec<Point>> {
    let mut out = Vec::new();
    for item in obj.try_iter()? {
        let item = item?;
        let (point, _) =
            point_of(&item).ok_or_else(|| crate::err::argument_error("LineString", "__init__"))?;
        out.push(point);
    }
    Ok(out)
}

/// Parses the constructor forms upstream registers for a linestring or polygon:
/// `(id=0, points=[], attributes=AttributeMap())`, or a single handle of the same
/// family in the other dimension, which aliases rather than copies.
fn construct(
    class: &str,
    kind: Kind,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<LineString> {
    let positional = args.len();
    let no_kwargs = kwargs.is_none_or(|d| d.is_empty());

    if positional == 0 && no_kwargs {
        return Ok(LineString::empty());
    }

    if positional == 1 && no_kwargs {
        let first = args.get_item(0)?;
        if let Some((line, other)) = linestring_of(&first) {
            // A hybrid is built from the const handle of the same dimension.
            if kind.hybrid && !other.hybrid && other.polygon == kind.polygon {
                return Ok(line);
            }
            // Linestrings additionally alias across dimensions -- `LineString3d(
            // LineString2d)`. Polygons register no such constructor.
            let cross_dimension = !kind.polygon
                && !kind.hybrid
                && !other.polygon
                && !other.hybrid
                && other.dimensions != kind.dimensions;
            if cross_dimension {
                return Ok(line);
            }
            return Err(argument_error(class, "__init__"));
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

    let id: i64 = match arg(0, "id")? {
        None => 0,
        Some(value) => value
            .extract()
            .map_err(|_| argument_error(class, "__init__"))?,
    };
    let points = match arg(1, "points")? {
        None => Vec::new(),
        Some(value) => points_from_any(&value)?,
    };
    let attributes = optional_attribute_map(arg(2, "attributes")?.as_ref())?;
    Ok(LineString::new(id, points, attributes))
}

macro_rules! linestring_class {
    (
        name: $py_name:literal,
        rust: $rust:ident,
        dimensions: $dims:tt,
        polygon: $polygon:tt,
        hybrid: $hybrid:tt,
        mutable: $mutable:tt,
        invertible: $invertible:tt,
        item: $item:ident,
    ) => {
        #[doc = concat!("`lanelet2.core.", $py_name, "`.")]
        #[pyclass(name = $py_name, module = "lanelet2.core")]
        pub struct $rust {
            line: LineString,
        }

        #[allow(dead_code)] // part of the macro's uniform shape
        impl $rust {
            pub const KIND: Kind = Kind {
                dimensions: $dims,
                polygon: $polygon,
                hybrid: $hybrid,
            };

            pub fn wrap(line: LineString) -> Self {
                $rust { line }
            }

            pub fn inner(&self) -> &LineString {
                &self.line
            }

            fn item(&self, point: &Point) -> $item {
                linestring_class!(@item $hybrid, $dims, $item, point)
            }

            fn point_reprs(&self, _py: Python<'_>) -> PyResult<Vec<String>> {
                self.line
                    .points()
                    .iter()
                    .map(|point| linestring_class!(@item_repr $hybrid, $dims, $item, _py, point))
                    .collect()
            }
        }

        #[pymethods]
        impl $rust {
            #[new]
            #[pyo3(signature = (*args, **kwargs))]
            fn new(args: &Bound<'_, PyTuple>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
                Ok($rust {
                    line: construct($py_name, Self::KIND, args, kwargs)?,
                })
            }

            #[getter]
            fn id(&self) -> i64 {
                self.line.id()
            }

            #[setter(id)]
            fn set_id(&self, value: i64) -> PyResult<()> {
                if !$mutable {
                    return Err(PyAttributeError::new_err("can't set attribute"));
                }
                self.line.set_id(value);
                Ok(())
            }

            #[getter]
            fn attributes(&self) -> PyAttributeMap {
                PyAttributeMap::proxy(self.line.attributes().clone())
            }

            #[setter(attributes)]
            fn set_attributes(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
                if !$mutable {
                    return Err(PyAttributeError::new_err("can't set attribute"));
                }
                self.line.set_attributes(attribute_map_from_any(value)?);
                Ok(())
            }

            fn __len__(&self) -> usize {
                self.line.len()
            }

            fn __getitem__(&self, index: &Bound<'_, PyAny>) -> PyResult<$item> {
                let index: isize = index
                    .extract()
                    .map_err(|_| argument_error($py_name, "__getitem__"))?;
                let index = resolve_index(index, self.line.len())?;
                let point = self.line.at(index).expect("index was just bounds-checked");
                Ok(self.item(&point))
            }

            fn __iter__(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
                let py = slf.py();
                let items: Vec<$item> =
                    slf.line.points().iter().map(|point| slf.item(point)).collect();
                Ok(PyList::new(py, items)?.try_iter()?.into())
            }

            fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
                match linestring_of(other) {
                    Some((line, kind)) => kind == Self::KIND && self.line.is_same_view(&line),
                    None => false,
                }
            }

            fn __ne__(&self, other: &Bound<'_, PyAny>) -> bool {
                !self.__eq__(other)
            }

            fn __hash__(&self) -> i64 {
                // Hash by id, matching upstream. `__eq__` compares storage
                // identity, and equal storage always shares an id, so `a == b`
                // still implies `hash(a) == hash(b)` -- the only direction
                // Python requires. Id hashing keeps set/dict iteration order
                // matching upstream for order-sensitive consumers.
                self.line.id()
            }

            fn __str__(&self) -> String {
                self.line.to_display_string()
            }

            pub fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
                let attributes = attributes_repr_arg(py, &self.line.attributes().read())?;
                Ok(self.line.repr($py_name, &self.point_reprs(py)?, &attributes))
            }

            /// A method, not a property, matching upstream. Present even on
            /// polygons, which have no `invert` to pair with it.
            fn inverted(&self) -> bool {
                self.line.is_inverted()
            }
        }

        linestring_class!(@mutators $mutable, $rust);
        linestring_class!(@inversion $invertible, $rust);
    };

    // --- item construction, by flavour ------------------------------------
    (@item true, 2, $item:ident, $point:expr) => {
        $item::free([$point.x(), $point.y(), 0.0])
    };
    (@item true, 3, $item:ident, $point:expr) => {
        $item::free($point.xyz())
    };
    (@item false, $dims:tt, $item:ident, $point:expr) => {
        $item::wrap($point.clone())
    };

    (@item_repr true, 2, $item:ident, $py:expr, $point:expr) => {
        Ok($item::free([$point.x(), $point.y(), 0.0]).__repr__())
    };
    (@item_repr true, 3, $item:ident, $py:expr, $point:expr) => {
        Ok($item::free($point.xyz()).__repr__())
    };
    (@item_repr false, $dims:tt, $item:ident, $py:expr, $point:expr) => {
        $item::wrap($point.clone()).__repr__($py)
    };

    // --- mutators, only on the writable flavours --------------------------
    (@mutators false, $rust:ident) => {};
    (@mutators true, $rust:ident) => {
        #[pymethods]
        impl $rust {
            fn __setitem__(&self, index: isize, value: &Bound<'_, PyAny>) -> PyResult<()> {
                let index = resolve_index(index, self.line.len())?;
                let (point, _) = point_of(value)
                    .ok_or_else(|| argument_error(stringify!($rust), "__setitem__"))?;
                self.line.set_at(index, point);
                Ok(())
            }

            fn __delitem__(&self, index: isize) -> PyResult<()> {
                let index = resolve_index(index, self.line.len())?;
                self.line.remove_at(index);
                Ok(())
            }

            /// Appends in view order: on an inverted handle this prepends to the
            /// underlying storage so the new point still reads back last.
            fn append(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
                let (point, _) = point_of(value)
                    .ok_or_else(|| argument_error(stringify!($rust), "append"))?;
                self.line.push(point);
                Ok(())
            }
        }
    };

    // --- inversion, absent on polygons ------------------------------------
    (@inversion false, $rust:ident) => {};
    (@inversion true, $rust:ident) => {
        #[pymethods]
        impl $rust {
            /// A new handle over the same data, reading in the opposite direction.
            fn invert(&self) -> Self {
                $rust::wrap(self.line.invert())
            }
        }
    };
}

linestring_class! {
    name: "LineString3d", rust: PyLineString3d, dimensions: 3,
    polygon: false, hybrid: false, mutable: true, invertible: true, item: PyPoint3d,
}
linestring_class! {
    name: "LineString2d", rust: PyLineString2d, dimensions: 2,
    polygon: false, hybrid: false, mutable: true, invertible: true, item: PyPoint2d,
}
linestring_class! {
    name: "ConstLineString3d", rust: PyConstLineString3d, dimensions: 3,
    polygon: false, hybrid: false, mutable: false, invertible: true, item: PyConstPoint3d,
}
linestring_class! {
    name: "ConstLineString2d", rust: PyConstLineString2d, dimensions: 2,
    polygon: false, hybrid: false, mutable: false, invertible: true, item: PyConstPoint2d,
}
linestring_class! {
    name: "ConstHybridLineString3d", rust: PyConstHybridLineString3d, dimensions: 3,
    polygon: false, hybrid: true, mutable: false, invertible: true, item: PyBasicPoint3d,
}
linestring_class! {
    name: "ConstHybridLineString2d", rust: PyConstHybridLineString2d, dimensions: 2,
    polygon: false, hybrid: true, mutable: false, invertible: true, item: PyBasicPoint2d,
}
linestring_class! {
    name: "Polygon3d", rust: PyPolygon3d, dimensions: 3,
    polygon: true, hybrid: false, mutable: true, invertible: false, item: PyPoint3d,
}
linestring_class! {
    name: "Polygon2d", rust: PyPolygon2d, dimensions: 2,
    polygon: true, hybrid: false, mutable: true, invertible: false, item: PyPoint2d,
}
linestring_class! {
    name: "ConstPolygon3d", rust: PyConstPolygon3d, dimensions: 3,
    polygon: true, hybrid: false, mutable: false, invertible: false, item: PyConstPoint3d,
}
linestring_class! {
    name: "ConstPolygon2d", rust: PyConstPolygon2d, dimensions: 2,
    polygon: true, hybrid: false, mutable: false, invertible: false, item: PyConstPoint2d,
}
linestring_class! {
    name: "ConstHybridPolygon3d", rust: PyConstHybridPolygon3d, dimensions: 3,
    polygon: true, hybrid: true, mutable: false, invertible: false, item: PyBasicPoint3d,
}
linestring_class! {
    name: "ConstHybridPolygon2d", rust: PyConstHybridPolygon2d, dimensions: 2,
    polygon: true, hybrid: true, mutable: false, invertible: false, item: PyBasicPoint2d,
}

linestring_registry![
    PyLineString3d,
    PyLineString2d,
    PyConstLineString3d,
    PyConstLineString2d,
    PyConstHybridLineString3d,
    PyConstHybridLineString2d,
    PyPolygon3d,
    PyPolygon2d,
    PyConstPolygon3d,
    PyConstPolygon2d,
    PyConstHybridPolygon3d,
    PyConstHybridPolygon2d,
];

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyLineString2d>()?;
    m.add_class::<PyLineString3d>()?;
    m.add_class::<PyConstLineString2d>()?;
    m.add_class::<PyConstLineString3d>()?;
    m.add_class::<PyConstHybridLineString2d>()?;
    m.add_class::<PyConstHybridLineString3d>()?;
    m.add_class::<PyPolygon2d>()?;
    m.add_class::<PyPolygon3d>()?;
    m.add_class::<PyConstPolygon2d>()?;
    m.add_class::<PyConstPolygon3d>()?;
    m.add_class::<PyConstHybridPolygon2d>()?;
    m.add_class::<PyConstHybridPolygon3d>()?;
    Ok(())
}
