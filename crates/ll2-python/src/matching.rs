//! `lanelet2.matching` — placing a positioned object on the lanelets it may be on.
//!
//! Ground truth: `lanelet2_python/python_api/matching.cpp`.
//!
//! Only the `Const` match types are exported upstream, so those are the only ones
//! here too.

use ll2_core::fmt::ostream_double;
use ll2_matching::{
    LaneletMatch, Object2d, ObjectWithCovariance2d, Pose2d, PositionCovariance2d,
    deterministic_matches, probabilistic_matches, remove_non_rule_compliant,
};
use pyo3::prelude::*;
use pyo3::types::{PyList, PyModule};

use crate::core::basic::PyBasicPoint2d;
use crate::core::lanelet::PyConstLanelet;
use crate::core::map::PyLaneletMap;
use crate::err::{argument_error, runtime};
use crate::traffic_rules::traffic_rules_of;

/// Renders a matrix the way Eigen does: every entry right-aligned to the width of
/// the widest, rows separated by newlines.
fn eigen_matrix(rows: &[Vec<f64>]) -> String {
    let rendered: Vec<Vec<String>> = rows
        .iter()
        .map(|row| row.iter().copied().map(ostream_double).collect())
        .collect();
    let width = rendered
        .iter()
        .flatten()
        .map(String::len)
        .max()
        .unwrap_or(0);
    rendered
        .iter()
        .map(|row| {
            row.iter()
                .map(|text| format!("{text:>width$}"))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `lanelet2.matching.Pose2d`.
#[pyclass(name = "Pose2d", module = "lanelet2.matching")]
#[derive(Clone, Copy)]
pub struct PyPose2d {
    pub(crate) pose: Pose2d,
}

#[pymethods]
impl PyPose2d {
    #[new]
    #[pyo3(signature = (x = 0.0, y = 0.0, yaw = 0.0))]
    fn new(x: f64, y: f64, yaw: f64) -> Self {
        PyPose2d {
            pose: Pose2d { x, y, yaw },
        }
    }

    /// Note the double space after `y:` — upstream's format string has it.
    fn __str__(&self) -> String {
        format!(
            "x: {}\ny:  {}\nphi: {}",
            ostream_double(self.pose.x),
            ostream_double(self.pose.y),
            ostream_double(self.pose.yaw)
        )
    }
}

/// `lanelet2.matching.PositionCovariance2d`.
#[pyclass(name = "PositionCovariance2d", module = "lanelet2.matching")]
#[derive(Clone, Copy)]
pub struct PyPositionCovariance2d {
    pub(crate) covariance: PositionCovariance2d,
}

#[pymethods]
impl PyPositionCovariance2d {
    #[new]
    #[pyo3(signature = (varX = 0.0, varY = 0.0, covXY = 0.0))]
    #[allow(non_snake_case)]
    fn new(varX: f64, varY: f64, covXY: f64) -> Self {
        PyPositionCovariance2d {
            covariance: PositionCovariance2d {
                var_x: varX,
                var_y: varY,
                cov_xy: covXY,
            },
        }
    }

    fn __str__(&self) -> String {
        let c = self.covariance;
        eigen_matrix(&[vec![c.var_x, c.cov_xy], vec![c.cov_xy, c.var_y]])
    }
}

fn hull_from_any(obj: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<[f64; 2]>> {
    let Some(obj) = obj else { return Ok(Vec::new()) };
    if obj.is_none() {
        return Ok(Vec::new());
    }
    let mut hull = Vec::new();
    for item in obj.try_iter()? {
        let item = item?;
        let point = item
            .cast::<PyBasicPoint2d>()
            .map_err(|_| argument_error("matching", "absoluteHull"))?
            .borrow();
        let values = point.values();
        hull.push([values[0], values[1]]);
    }
    Ok(hull)
}

/// `lanelet2.matching.Object2d`.
#[pyclass(name = "Object2d", module = "lanelet2.matching", subclass)]
#[derive(Clone)]
pub struct PyObject2d {
    pub(crate) object: Object2d,
}

#[pymethods]
impl PyObject2d {
    #[new]
    #[pyo3(signature = (objectId = 0, pose = None, absoluteHull = None))]
    #[allow(non_snake_case)]
    fn new(
        objectId: i64,
        pose: Option<&PyPose2d>,
        absoluteHull: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        Ok(PyObject2d {
            object: Object2d {
                object_id: objectId,
                pose: pose.map(|p| p.pose).unwrap_or_default(),
                absolute_hull: hull_from_any(absoluteHull)?,
            },
        })
    }

    #[getter]
    #[pyo3(name = "objectId")]
    fn object_id(&self) -> i64 {
        self.object.object_id
    }

    #[setter(objectId)]
    fn set_object_id(&mut self, value: i64) {
        self.object.object_id = value;
    }

    #[getter]
    fn pose(&self) -> PyPose2d {
        PyPose2d {
            pose: self.object.pose,
        }
    }

    #[setter(pose)]
    fn set_pose(&mut self, value: &PyPose2d) {
        self.object.pose = value.pose;
    }

    /// In *map* coordinates, not relative to the pose.
    #[getter]
    #[pyo3(name = "absoluteHull")]
    fn absolute_hull(&self) -> Vec<PyBasicPoint2d> {
        self.object
            .absolute_hull
            .iter()
            .map(|&[x, y]| PyBasicPoint2d::free([x, y, 0.0]))
            .collect()
    }

    #[setter(absoluteHull)]
    fn set_absolute_hull(&mut self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        self.object.absolute_hull = hull_from_any(Some(value))?;
        Ok(())
    }
}

/// `lanelet2.matching.ObjectWithCovariance2d`.
#[pyclass(name = "ObjectWithCovariance2d", module = "lanelet2.matching", extends = PyObject2d)]
pub struct PyObjectWithCovariance2d {
    pub(crate) covariance: PositionCovariance2d,
    pub(crate) von_mises_kappa: f64,
}

#[pymethods]
impl PyObjectWithCovariance2d {
    #[new]
    #[pyo3(signature = (objectId = 0, pose = None, absoluteHull = None,
                        positionCovariance = None, vonMisesKappa = 0.0))]
    #[allow(non_snake_case)]
    fn new(
        objectId: i64,
        pose: Option<&PyPose2d>,
        absoluteHull: Option<&Bound<'_, PyAny>>,
        positionCovariance: Option<&PyPositionCovariance2d>,
        vonMisesKappa: f64,
    ) -> PyResult<(Self, PyObject2d)> {
        Ok((
            PyObjectWithCovariance2d {
                covariance: positionCovariance.map(|c| c.covariance).unwrap_or_default(),
                von_mises_kappa: vonMisesKappa,
            },
            PyObject2d::new(objectId, pose, absoluteHull)?,
        ))
    }

    #[getter]
    #[pyo3(name = "positionCovariance")]
    fn position_covariance(&self) -> PyPositionCovariance2d {
        PyPositionCovariance2d {
            covariance: self.covariance,
        }
    }

    #[setter(positionCovariance)]
    fn set_position_covariance(&mut self, value: &PyPositionCovariance2d) {
        self.covariance = value.covariance;
    }

    /// How concentrated the heading estimate is; larger means more certain.
    #[getter]
    #[pyo3(name = "vonMisesKappa")]
    fn von_mises_kappa(&self) -> f64 {
        self.von_mises_kappa
    }

    #[setter(vonMisesKappa)]
    fn set_von_mises_kappa(&mut self, value: f64) {
        self.von_mises_kappa = value;
    }
}

macro_rules! match_class {
    ($py_name:literal, $rust:ident, $probabilistic:tt) => {
        #[doc = concat!("`lanelet2.matching.", $py_name, "`.")]
        #[pyclass(name = $py_name, module = "lanelet2.matching")]
        pub struct $rust {
            found: LaneletMatch,
        }

        impl $rust {
            pub fn wrap(found: LaneletMatch) -> Self {
                $rust { found }
            }

            pub fn inner(&self) -> &LaneletMatch {
                &self.found
            }
        }

        #[pymethods]
        impl $rust {
            #[getter]
            fn lanelet(&self) -> PyConstLanelet {
                PyConstLanelet::wrap(self.found.lanelet.clone())
            }

            #[getter]
            fn distance(&self) -> f64 {
                self.found.distance
            }

        }

        match_class!(@probabilistic $probabilistic, $rust);
    };

    (@probabilistic false, $rust:ident) => {};
    (@probabilistic true, $rust:ident) => {
        #[pymethods]
        impl $rust {
            #[getter]
            #[pyo3(name = "mahalanobisDistSq")]
            fn mahalanobis_dist_sq(&self) -> f64 {
                self.found.mahalanobis_dist_sq
            }
        }
    };
}

match_class!("ConstLaneletMatch", PyConstLaneletMatch, false);
match_class!("ConstLaneletMatchProbabilistic", PyConstLaneletMatchProbabilistic, true);

#[pyfunction]
#[pyo3(name = "getDeterministicMatches", signature = (map, obj, maxDist))]
#[allow(non_snake_case)]
fn get_deterministic_matches<'py>(
    py: Python<'py>,
    map: &PyLaneletMap,
    obj: &PyObject2d,
    maxDist: f64,
) -> PyResult<Bound<'py, PyList>> {
    let found: Vec<PyConstLaneletMatch> = deterministic_matches(map.inner(), &obj.object, maxDist)
        .into_iter()
        .map(PyConstLaneletMatch::wrap)
        .collect();
    PyList::new(py, found)
}

#[pyfunction]
#[pyo3(name = "getProbabilisticMatches", signature = (map, obj, maxDist))]
#[allow(non_snake_case)]
fn get_probabilistic_matches<'py>(
    py: Python<'py>,
    map: &PyLaneletMap,
    obj: PyRef<'_, PyObjectWithCovariance2d>,
    maxDist: f64,
) -> PyResult<Bound<'py, PyList>> {
    let object = ObjectWithCovariance2d {
        object: obj.as_super().object.clone(),
        position_covariance: obj.covariance,
        von_mises_kappa: obj.von_mises_kappa,
    };
    let found = probabilistic_matches(map.inner(), &object, maxDist)
        .map_err(|error| runtime(error.0))?;
    PyList::new(
        py,
        found
            .into_iter()
            .map(PyConstLaneletMatchProbabilistic::wrap)
            .collect::<Vec<_>>(),
    )
}

/// Drops the matches the participant may not drive.
///
/// Both orientations of every candidate were emitted, so this is what removes the
/// wrong-way ones.
#[pyfunction]
#[pyo3(name = "removeNonRuleCompliantMatches", signature = (matches, trafficRules))]
#[allow(non_snake_case)]
fn remove_non_rule_compliant_matches<'py>(
    py: Python<'py>,
    matches: &Bound<'py, PyAny>,
    trafficRules: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyList>> {
    let rules = traffic_rules_of(trafficRules)
        .ok_or_else(|| argument_error("matching", "removeNonRuleCompliantMatches"))?;

    let mut probabilistic = Vec::new();
    let mut plain = Vec::new();
    for item in matches.try_iter()? {
        let item = item?;
        if let Ok(found) = item.cast::<PyConstLaneletMatchProbabilistic>() {
            probabilistic.push(found.borrow().inner().clone());
        } else if let Ok(found) = item.cast::<PyConstLaneletMatch>() {
            plain.push(found.borrow().inner().clone());
        } else {
            return Err(runtime(
                "Matches must be a list of ConstLaneletMatch(es) or ConstLaneletMatch(es)Probabilistic",
            ));
        }
    }
    if !probabilistic.is_empty() {
        let kept: Vec<PyConstLaneletMatchProbabilistic> =
            remove_non_rule_compliant(probabilistic, &rules)
                .into_iter()
                .map(PyConstLaneletMatchProbabilistic::wrap)
                .collect();
        return PyList::new(py, kept);
    }
    let kept: Vec<PyConstLaneletMatch> = remove_non_rule_compliant(plain, &rules)
        .into_iter()
        .map(PyConstLaneletMatch::wrap)
        .collect();
    PyList::new(py, kept)
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyPose2d>()?;
    m.add_class::<PyPositionCovariance2d>()?;
    m.add_class::<PyObject2d>()?;
    m.add_class::<PyObjectWithCovariance2d>()?;
    m.add_class::<PyConstLaneletMatch>()?;
    m.add_class::<PyConstLaneletMatchProbabilistic>()?;
    m.add_function(wrap_pyfunction!(get_deterministic_matches, m)?)?;
    m.add_function(wrap_pyfunction!(get_probabilistic_matches, m)?)?;
    m.add_function(wrap_pyfunction!(remove_non_rule_compliant_matches, m)?)?;
    Ok(())
}
