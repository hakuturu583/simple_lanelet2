//! Plain coordinate types: `BasicPoint2d`/`BasicPoint3d`, `Vector2d` and the
//! bounding boxes.
//!
//! These carry no id and no attributes. Upstream backs them with Eigen, which shows
//! through in two places: `str()` renders a column vector with right-aligned
//! entries, and a basic point obtained from a `Point3d` is an Eigen `Map` over the
//! owner's storage rather than a copy -- so writing to it writes through.
//!
//! Upstream: `lanelet2_python/python_api/core.cpp:504-580`

use ll2_core::compat;
use ll2_core::fmt::{make_repr, ostream_double};
use ll2_core::refs::{CoordView, Coords, coords};
use pyo3::prelude::*;

use crate::err::not_instantiable;

/// Renders a coordinate vector the way Eigen's `operator<<` does: one entry per
/// line, every entry right-aligned to the width of the widest.
fn eigen_column(values: &[f64]) -> String {
    let rendered: Vec<String> = values.iter().copied().map(ostream_double).collect();
    let width = rendered.iter().map(String::len).max().unwrap_or(0);
    rendered
        .iter()
        .map(|text| format!("{text:>width$}"))
        .collect::<Vec<_>>()
        .join("\n")
}

macro_rules! basic_point {
    ($py_name:literal, $rust:ident, $dims:expr, $ctor_sig:tt,
     $($accessor:ident / $setter:ident => $index:expr),+) => {
        #[doc = concat!("`lanelet2.core.", $py_name, "`.")]
        #[pyclass(name = $py_name, module = "lanelet2.core")]
        pub struct $rust {
            view: CoordView,
        }

        impl $rust {
            pub fn from_view(view: CoordView) -> Self {
                $rust { view }
            }

            pub fn free(values: [f64; 3]) -> Self {
                $rust {
                    view: CoordView::new(coords(values[0], values[1], values[2]), true),
                }
            }

            pub fn view(&self) -> &CoordView {
                &self.view
            }

            pub fn values(&self) -> [f64; $dims] {
                let xyz = self.view.xyz();
                let mut out = [0.0; $dims];
                out.copy_from_slice(&xyz[..$dims]);
                out
            }

            fn padded(&self) -> [f64; 3] {
                let mut out = [0.0; 3];
                out[..$dims].copy_from_slice(&self.values());
                out
            }

            fn combine(&self, other: &Self, op: fn(f64, f64) -> f64) -> Self {
                let (a, b) = (self.padded(), other.padded());
                let mut out = [0.0; 3];
                for index in 0..$dims {
                    out[index] = op(a[index], b[index]);
                }
                $rust::free(out)
            }

            fn scaled(&self, factor: f64) -> Self {
                let mut out = self.padded();
                for slot in out.iter_mut().take($dims) {
                    *slot *= factor;
                }
                $rust::free(out)
            }
        }

        #[pymethods]
        impl $rust {
            #[new]
            #[pyo3(signature = $ctor_sig)]
            fn new($($accessor: f64),+) -> Self {
                let mut values = [0.0; 3];
                $(values[$index] = $accessor;)+
                $rust::free(values)
            }

            $(
                #[getter($accessor)]
                fn $accessor(&self) -> f64 {
                    self.view.get($index)
                }
            )+

            $(
                #[setter($accessor)]
                fn $setter(&self, value: f64) -> PyResult<()> {
                    if !self.view.set($index, value) {
                        return Err(pyo3::exceptions::PyAttributeError::new_err(concat!(
                            "cannot write ", stringify!($accessor), " through a const view"
                        )));
                    }
                    Ok(())
                }
            )+

            fn __add__(&self, other: &Self) -> Self {
                self.combine(other, |a, b| a + b)
            }

            fn __sub__(&self, other: &Self) -> Self {
                self.combine(other, |a, b| a - b)
            }

            fn __mul__(&self, factor: f64) -> Self {
                self.scaled(factor)
            }

            fn __rmul__(&self, factor: f64) -> Self {
                self.scaled(factor)
            }

            /// Present only so that the attribute exists as it does upstream;
            /// Python 3 never calls it.
            fn __div__(&self, divisor: f64) -> Self {
                self.scaled(1.0 / divisor)
            }

            /// Upstream registers `__div__` only, so `p / 2` raises `TypeError` on
            /// Python 3. Returning `NotImplemented` in bug-compatibility mode lets
            /// Python raise that exact error itself.
            fn __truediv__<'py>(&self, py: Python<'py>, divisor: f64) -> Bound<'py, PyAny> {
                if compat::basic_point_div_only() {
                    return py.NotImplemented().into_bound(py);
                }
                Bound::new(py, self.scaled(1.0 / divisor))
                    .expect("constructing a basic point cannot fail")
                    .into_any()
            }

            pub fn __repr__(&self) -> String {
                let args: Vec<String> =
                    self.values().iter().copied().map(ostream_double).collect();
                make_repr($py_name, &args)
            }

            fn __str__(&self) -> String {
                eigen_column(&self.values())
            }
        }
    };
}

basic_point!("BasicPoint2d", PyBasicPoint2d, 2, (x = 0.0, y = 0.0),
             x / set_x => 0, y / set_y => 1);
basic_point!("BasicPoint3d", PyBasicPoint3d, 3, (x = 0.0, y = 0.0, z = 0.0),
             x / set_x => 0, y / set_y => 1, z / set_z => 2);

/// `lanelet2.core.Vector2d`.
///
/// Upstream exports this as a distinct, non-constructible type that Eigen values
/// implicitly convert to a `BasicPoint2d` from. Nothing in the Python API returns
/// one, so it exists purely to keep the API surface identical.
#[pyclass(name = "Vector2d", module = "lanelet2.core")]
pub struct PyVector2d {
    view: CoordView,
}

#[pymethods]
impl PyVector2d {
    #[new]
    fn new() -> PyResult<Self> {
        Err(not_instantiable())
    }

    #[getter]
    fn x(&self) -> f64 {
        self.view.get(0)
    }

    #[getter]
    fn y(&self) -> f64 {
        self.view.get(1)
    }

    fn __repr__(&self) -> String {
        make_repr(
            "Vector2d",
            &[
                ostream_double(self.view.get(0)),
                ostream_double(self.view.get(1)),
            ],
        )
    }

    fn __str__(&self) -> String {
        eigen_column(&[self.view.get(0), self.view.get(1)])
    }

    // The arithmetic upstream registers. No instance can exist, so these are here
    // for the API surface alone.
    fn __add__(&self, other: &Self) -> Self {
        PyVector2d {
            view: other.view.clone(),
        }
    }

    fn __sub__(&self, other: &Self) -> Self {
        PyVector2d {
            view: other.view.clone(),
        }
    }

    fn __mul__(&self, _factor: f64) -> Self {
        PyVector2d {
            view: self.view.clone(),
        }
    }

    fn __rmul__(&self, _factor: f64) -> Self {
        PyVector2d {
            view: self.view.clone(),
        }
    }

    fn __div__(&self, _divisor: f64) -> Self {
        PyVector2d {
            view: self.view.clone(),
        }
    }
}

/// Storage shared by a bounding box and the corner views handed out by `.min`/`.max`.
struct BoxData {
    min: Coords,
    max: Coords,
}

macro_rules! bounding_box {
    ($py_name:literal, $rust:ident, $corner:ident) => {
        #[doc = concat!("`lanelet2.core.", $py_name, "`.")]
        #[pyclass(name = $py_name, module = "lanelet2.core")]
        pub struct $rust {
            data: BoxData,
        }

        impl $rust {
            pub fn from_corners(min: [f64; 3], max: [f64; 3]) -> Self {
                $rust {
                    data: BoxData {
                        min: coords(min[0], min[1], min[2]),
                        max: coords(max[0], max[1], max[2]),
                    },
                }
            }

            pub fn min_values(&self) -> [f64; 3] {
                *self.data.min.read()
            }

            pub fn max_values(&self) -> [f64; 3] {
                *self.data.max.read()
            }
        }

        #[pymethods]
        impl $rust {
            #[new]
            fn new(min: &$corner, max: &$corner) -> Self {
                let (min, max) = (min.view().xyz(), max.view().xyz());
                $rust::from_corners(min, max)
            }

            /// A live view: `bbox.min.x = 5` moves the box, as it does upstream.
            #[getter]
            fn min(&self) -> $corner {
                $corner::from_view(CoordView::new(self.data.min.clone(), true))
            }

            #[setter(min)]
            fn set_min(&self, value: &$corner) {
                *self.data.min.write() = value.view().xyz();
            }

            #[getter]
            fn max(&self) -> $corner {
                $corner::from_view(CoordView::new(self.data.max.clone(), true))
            }

            #[setter(max)]
            fn set_max(&self, value: &$corner) {
                *self.data.max.write() = value.view().xyz();
            }

            pub fn __repr__(&self) -> String {
                make_repr($py_name, &[self.min().__repr__(), self.max().__repr__()])
            }

            fn __str__(&self) -> String {
                self.__repr__()
            }
        }
    };
}

bounding_box!("BoundingBox2d", PyBoundingBox2d, PyBasicPoint2d);
bounding_box!("BoundingBox3d", PyBoundingBox3d, PyBasicPoint3d);

#[cfg(test)]
mod tests {
    use super::eigen_column;

    #[test]
    fn eigen_right_aligns_to_the_widest_entry() {
        assert_eq!(eigen_column(&[1.0, -22.5, 333.0]), "    1\n-22.5\n  333");
        assert_eq!(eigen_column(&[0.5, 0.25, 0.125]), "  0.5\n 0.25\n0.125");
        assert_eq!(eigen_column(&[1e7, 1.0, 2.0]), "1e+07\n    1\n    2");
        assert_eq!(eigen_column(&[-1.0, -2.0, -3.0]), "-1\n-2\n-3");
        assert_eq!(eigen_column(&[1.0, 2.0]), "1\n2");
    }
}
