//! `LaneletSequence` — several lanelets driven as one continuous stretch.
//!
//! Its bounds and centerline are compounds of the members', so shared points at the
//! joins collapse. It carries no id and no attributes of its own.
//!
//! Upstream: `lanelet2_python/python_api/core.cpp:1074-1088`

use ll2_core::compound::CompoundLineString;
use ll2_core::lanelet::Lanelet;
use pyo3::prelude::*;
use pyo3::types::PyList;

use crate::core::compound::{
    PyCompoundLineString3d, PyCompoundPolygon2d, PyCompoundPolygon3d,
};
use crate::core::lanelet::{PyConstLanelet, lanelet_of};
use crate::err::{argument_error, resolve_index};

/// `lanelet2.core.LaneletSequence`.
#[pyclass(name = "LaneletSequence", module = "lanelet2.core")]
pub struct PyLaneletSequence {
    lanelets: Vec<Lanelet>,
    inverted: bool,
}

impl PyLaneletSequence {
    pub fn wrap(lanelets: Vec<Lanelet>) -> Self {
        PyLaneletSequence {
            lanelets,
            inverted: false,
        }
    }

    /// The members in view order; inverting reverses the list and each member.
    fn members(&self) -> Vec<Lanelet> {
        if self.inverted {
            self.lanelets.iter().rev().map(Lanelet::invert).collect()
        } else {
            self.lanelets.clone()
        }
    }

    fn compound(&self, pick: fn(&Lanelet) -> ll2_core::linestring::LineString) -> CompoundLineString {
        CompoundLineString::new(self.members().iter().map(pick).collect())
    }
}

#[pymethods]
impl PyLaneletSequence {
    #[new]
    #[pyo3(signature = (lanelets))]
    fn new(lanelets: &Bound<'_, PyAny>) -> PyResult<Self> {
        let mut members = Vec::new();
        for item in lanelets.try_iter()? {
            let item = item?;
            let (lanelet, _) = lanelet_of(&item)
                .ok_or_else(|| argument_error("LaneletSequence", "__init__"))?;
            members.push(lanelet);
        }
        Ok(PyLaneletSequence::wrap(members))
    }

    #[getter]
    fn centerline(&self) -> PyCompoundLineString3d {
        PyCompoundLineString3d::wrap(self.compound(Lanelet::centerline))
    }

    #[getter]
    #[pyo3(name = "leftBound")]
    fn left_bound(&self) -> PyCompoundLineString3d {
        PyCompoundLineString3d::wrap(self.compound(Lanelet::left_bound))
    }

    #[getter]
    #[pyo3(name = "rightBound")]
    fn right_bound(&self) -> PyCompoundLineString3d {
        PyCompoundLineString3d::wrap(self.compound(Lanelet::right_bound))
    }

    fn lanelets(&self) -> Vec<PyConstLanelet> {
        self.members().into_iter().map(PyConstLanelet::wrap).collect()
    }

    fn invert(&self) -> Self {
        PyLaneletSequence {
            lanelets: self.lanelets.clone(),
            inverted: !self.inverted,
        }
    }

    fn inverted(&self) -> bool {
        self.inverted
    }

    /// The outline of the whole stretch: every left bound, then every right bound
    /// reversed.
    #[pyo3(name = "polygon3d")]
    fn polygon3d(&self) -> PyCompoundPolygon3d {
        PyCompoundPolygon3d::wrap(self.outline())
    }

    #[pyo3(name = "polygon2d")]
    fn polygon2d(&self) -> PyCompoundPolygon2d {
        PyCompoundPolygon2d::wrap(self.outline())
    }

    fn __len__(&self) -> usize {
        self.lanelets.len()
    }

    fn __getitem__(&self, index: &Bound<'_, PyAny>) -> PyResult<PyConstLanelet> {
        let index: isize = index
            .extract()
            .map_err(|_| argument_error("LaneletSequence", "__getitem__"))?;
        let members = self.members();
        let index = resolve_index(index, members.len())?;
        Ok(PyConstLanelet::wrap(members[index].clone()))
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let items: Vec<PyConstLanelet> = slf
            .members()
            .into_iter()
            .map(PyConstLanelet::wrap)
            .collect();
        Ok(PyList::new(py, items)?.try_iter()?.into())
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let rendered: Vec<String> = self
            .members()
            .into_iter()
            .map(|lanelet| PyConstLanelet::wrap(lanelet).__repr__(py))
            .collect::<PyResult<_>>()?;
        Ok(format!("LaneletSequence([{}])", rendered.join(", ")))
    }
}

impl PyLaneletSequence {
    fn outline(&self) -> CompoundLineString {
        let members = self.members();
        let mut bounds: Vec<ll2_core::linestring::LineString> =
            members.iter().map(Lanelet::left_bound).collect();
        bounds.extend(members.iter().rev().map(|l| l.right_bound().invert()));
        CompoundLineString::new(bounds)
    }
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyLaneletSequence>()?;
    Ok(())
}
