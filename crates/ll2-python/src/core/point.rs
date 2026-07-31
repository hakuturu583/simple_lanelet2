//! `Point2d`, `Point3d` and their `Const` counterparts.
//!
//! All four are views on the same storage: a point always holds three coordinates,
//! and `Point2d(p3d)` shares them rather than copying, so writing `x` through either
//! handle is visible from the other. The 2D views simply do not expose `z`.
//!
//! Upstream: `lanelet2_python/python_api/core.cpp:726-795`

use ll2_core::compat;
use ll2_core::point::Point;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

use crate::conv::{attributes_repr_arg, optional_attribute_map};
use crate::core::attribute::PyAttributeMap;
use crate::core::basic::{PyBasicPoint2d, PyBasicPoint3d};
use crate::err::{argument_error, not_instantiable};

/// Extracts the shared point behind any of the four point classes.
pub fn point_of(obj: &Bound<'_, PyAny>) -> Option<(Point, Dimension)> {
    if let Ok(p) = obj.cast::<PyPoint3d>() {
        return Some((p.borrow().point.clone(), Dimension::Three));
    }
    if let Ok(p) = obj.cast::<PyConstPoint3d>() {
        return Some((p.borrow().point.clone(), Dimension::Three));
    }
    if let Ok(p) = obj.cast::<PyPoint2d>() {
        return Some((p.borrow().point.clone(), Dimension::Two));
    }
    if let Ok(p) = obj.cast::<PyConstPoint2d>() {
        return Some((p.borrow().point.clone(), Dimension::Two));
    }
    None
}

/// Which of the two comparison families a point handle belongs to.
///
/// Upstream registers `operator==` separately for the 2D and 3D const types, and a
/// `Point2d` does not convert to a `ConstPoint3d`. So `p3d == Point2d(p3d)` is
/// `False` even though both address the same storage.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dimension {
    Two,
    Three,
}

/// Parses the four constructor forms upstream registers for a point.
///
/// PyO3 has no overloading, so the argument tuple is inspected the way
/// Boost.Python's overload chain would, in the same order:
///
/// * `()` — id 0 at the origin
/// * `(other_point)` — share the storage of a point of the *other* dimension
/// * `(id, basic_point, attributes=…)`
/// * `(id, x, y, z=0.0, attributes=…)`
fn construct(
    class: &str,
    alias_from: Dimension,
    args: &Bound<'_, PyTuple>,
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<Point> {
    let positional = args.len();
    let keyword = |name: &str| -> PyResult<Option<Bound<'_, PyAny>>> {
        match kwargs {
            None => Ok(None),
            Some(dict) => dict.get_item(name),
        }
    };

    if positional == 0 && kwargs.is_none_or(|d| d.is_empty()) {
        return Ok(Point::origin());
    }

    // `Point3d(a_2d_point)` aliases rather than copies. Only the *other*
    // dimension is accepted -- upstream registers `Point3d(Point2d)` and
    // `Point2d(Point3d)`, but no same-dimension copy constructor.
    if positional == 1 && kwargs.is_none_or(|d| d.is_empty()) {
        let first = args.get_item(0)?;
        if let Some((point, dimension)) = point_of(&first) {
            if dimension == alias_from {
                return Ok(point);
            }
            return Err(argument_error(class, "__init__"));
        }
    }

    let arg = |index: usize, name: &str| -> PyResult<Option<Bound<'_, PyAny>>> {
        if index < positional {
            return Ok(Some(args.get_item(index)?));
        }
        keyword(name)
    };

    let Some(id) = arg(0, "id")? else {
        return Err(argument_error(class, "__init__"));
    };
    let id: i64 = id.extract().map_err(|_| argument_error(class, "__init__"))?;

    let Some(second) = arg(1, "x")?.or(keyword("point")?) else {
        return Err(argument_error(class, "__init__"));
    };

    // `(id, basic_point, attributes)`.
    if let Ok(basic) = second.cast::<PyBasicPoint3d>() {
        let [x, y, z] = basic.borrow().view().xyz();
        let attributes = optional_attribute_map(arg(2, "attributes")?.as_ref())?;
        return Ok(Point::new(id, x, y, z, attributes));
    }
    if let Ok(basic) = second.cast::<PyBasicPoint2d>() {
        let [x, y, _] = basic.borrow().view().xyz();
        let attributes = optional_attribute_map(arg(2, "attributes")?.as_ref())?;
        return Ok(Point::new(id, x, y, 0.0, attributes));
    }

    // `(id, x, y, z=0.0, attributes=AttributeMap())`.
    let x: f64 = second.extract().map_err(|_| argument_error(class, "__init__"))?;
    let y: f64 = arg(2, "y")?
        .ok_or_else(|| argument_error(class, "__init__"))?
        .extract()
        .map_err(|_| argument_error(class, "__init__"))?;
    let z: f64 = match arg(3, "z")? {
        None => 0.0,
        Some(value) => value.extract().map_err(|_| argument_error(class, "__init__"))?,
    };
    let attributes = optional_attribute_map(arg(4, "attributes")?.as_ref())?;
    Ok(Point::new(id, x, y, z, attributes))
}

macro_rules! point_class {
    ($py_name:literal, $rust:ident, $dim:expr, $alias_from:expr, $three_d:expr,
     $mutable:expr, $basic:ident, { $($extra:item)* }) => {
        #[doc = concat!("`lanelet2.core.", $py_name, "`.")]
        #[pyclass(name = $py_name, module = "lanelet2.core")]
        pub struct $rust {
            point: Point,
        }

        impl $rust {
            pub fn wrap(point: Point) -> Self {
                $rust { point }
            }

            pub fn inner(&self) -> &Point {
                &self.point
            }
        }

        #[pymethods]
        impl $rust {
            #[new]
            #[pyo3(signature = (*args, **kwargs))]
            fn new(args: &Bound<'_, PyTuple>, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<Self> {
                if !$mutable {
                    return Err(not_instantiable());
                }
                Ok($rust {
                    point: construct($py_name, $alias_from, args, kwargs)?,
                })
            }

            #[getter]
            fn id(&self) -> i64 {
                self.point.id()
            }

            #[setter(id)]
            fn set_id(&self, value: i64) -> PyResult<()> {
                if !$mutable {
                    return Err(pyo3::exceptions::PyAttributeError::new_err(
                        "can't set attribute",
                    ));
                }
                self.point.set_id(value);
                Ok(())
            }

            /// A live proxy: mutating it mutates this point.
            #[getter]
            fn attributes(&self) -> PyAttributeMap {
                PyAttributeMap::proxy(self.point.attributes().clone())
            }

            #[setter(attributes)]
            fn set_attributes(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
                self.point.set_attributes(crate::conv::attribute_map_from_any(value)?);
                Ok(())
            }

            #[getter]
            fn x(&self) -> f64 {
                self.point.x()
            }

            #[setter(x)]
            fn set_x(&self, value: f64) -> PyResult<()> {
                if !$mutable {
                    return Err(pyo3::exceptions::PyAttributeError::new_err(
                        "can't set attribute",
                    ));
                }
                self.point.set_x(value);
                Ok(())
            }

            #[getter]
            fn y(&self) -> f64 {
                self.point.y()
            }

            #[setter(y)]
            fn set_y(&self, value: f64) -> PyResult<()> {
                if !$mutable {
                    return Err(pyo3::exceptions::PyAttributeError::new_err(
                        "can't set attribute",
                    ));
                }
                self.point.set_y(value);
                Ok(())
            }

            /// A live view of the coordinates.
            ///
            /// Const handles hand out a read-only view by default. Upstream returns
            /// a writable internal reference regardless of constness, which
            /// bug-compatibility mode restores.
            #[pyo3(name = "basicPoint")]
            fn basic_point(&self) -> $basic {
                let mutable = $mutable || compat::const_basic_point_is_mutable();
                $basic::from_view(self.point.basic_point(mutable))
            }

            fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
                match point_of(other) {
                    Some((point, dimension)) => {
                        dimension == $dim && self.point.is_same_data(&point)
                    }
                    None => false,
                }
            }

            fn __ne__(&self, other: &Bound<'_, PyAny>) -> bool {
                !self.__eq__(other)
            }

            /// Upstream hashes the id alone, which contradicts its identity-based
            /// `__eq__`. By default we hash identity so the two agree.
            fn __hash__(&self) -> i64 {
                if compat::hash_by_id_only() {
                    self.point.id()
                } else {
                    self.point.identity() as i64
                }
            }

            fn __str__(&self) -> String {
                if $three_d {
                    self.point.to_string_3d()
                } else {
                    self.point.to_string_2d()
                }
            }

            pub fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
                let attributes = attributes_repr_arg(py, &self.point.attributes().read())?;
                Ok(self.point.repr($py_name, $three_d, &attributes))
            }

            $($extra)*
        }
    };
}

point_class!("Point3d", PyPoint3d, Dimension::Three, Dimension::Two, true, true, PyBasicPoint3d, {
    #[getter]
    fn z(&self) -> f64 {
        self.point.z()
    }

    #[setter(z)]
    fn set_z(&self, value: f64) {
        self.point.set_z(value);
    }
});

point_class!("Point2d", PyPoint2d, Dimension::Two, Dimension::Three, false, true, PyBasicPoint2d, {});

point_class!("ConstPoint3d", PyConstPoint3d, Dimension::Three, Dimension::Two, true, false, PyBasicPoint3d, {
    #[getter]
    fn z(&self) -> f64 {
        self.point.z()
    }
});

point_class!("ConstPoint2d", PyConstPoint2d, Dimension::Two, Dimension::Three, false, false, PyBasicPoint2d, {});
