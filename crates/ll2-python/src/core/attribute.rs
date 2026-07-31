//! `AttributeMap` and the entry type its iterator yields.
//!
//! The map is a **live proxy** over the owner's storage, not a copy: upstream binds
//! `primitive.attributes` with `return_internal_reference` and its own test suite
//! relies on `primitive.attributes["k"] = "v"` changing the primitive.
//!
//! Upstream: `lanelet2_python/python_api/core.cpp:382-397, 706-712`

use ll2_core::attribute::{Attribute, AttributeMap};
use ll2_core::refs::{Attrs, attrs, empty_attrs};
use pyo3::exceptions::PyKeyError;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyTuple};

use crate::conv::{attribute_map_dict_repr, attribute_map_from_any};

/// One key/value entry, as yielded by iterating an `AttributeMap`.
///
/// Boost.Python's `map_indexing_suite` exports this helper class under exactly this
/// name, with `key()`/`data()` accessors and a `(key, value)` string form.
#[pyclass(
    name = "map_indexing_suite_AttributeMap_entry",
    module = "lanelet2.core",
    frozen
)]
pub struct PyAttributeMapEntry {
    key: String,
    data: String,
}

#[pymethods]
impl PyAttributeMapEntry {
    fn key(&self) -> &str {
        &self.key
    }

    fn data(&self) -> &str {
        &self.data
    }

    fn __str__(&self) -> String {
        format!("({}, {})", self.key, self.data)
    }

    fn __repr__(&self) -> String {
        self.__str__()
    }
}

/// A primitive's attributes: a string-to-string mapping, ordered by key.
#[pyclass(name = "AttributeMap", module = "lanelet2.core")]
pub struct PyAttributeMap {
    cell: Attrs,
}

impl PyAttributeMap {
    /// Wraps an owner's storage, so writes are visible to the owner.
    pub fn proxy(cell: Attrs) -> Self {
        PyAttributeMap { cell }
    }

    pub fn snapshot(&self) -> AttributeMap {
        self.cell.read().clone()
    }

    pub fn cell(&self) -> &Attrs {
        &self.cell
    }
}

#[pymethods]
impl PyAttributeMap {
    #[new]
    #[pyo3(signature = (attributes = None))]
    fn new(attributes: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        Ok(match attributes {
            None => PyAttributeMap {
                cell: empty_attrs(),
            },
            Some(obj) => PyAttributeMap {
                cell: attrs(attribute_map_from_any(obj)?),
            },
        })
    }

    fn __len__(&self) -> usize {
        self.cell.read().len()
    }

    fn __getitem__(&self, key: &str) -> PyResult<String> {
        self.cell
            .read()
            .get(key)
            .map(|value| value.value().to_owned())
            .ok_or_else(|| PyKeyError::new_err("Invalid key"))
    }

    fn __setitem__(&self, key: String, value: String) {
        self.cell.write().insert(key, Attribute::new(value));
    }

    fn __delitem__(&self, key: &str) -> PyResult<()> {
        self.cell
            .write()
            .remove(key)
            .map(|_| ())
            .ok_or_else(|| PyKeyError::new_err("Invalid key"))
    }

    fn __contains__(&self, key: &str) -> bool {
        self.cell.read().contains_key(key)
    }

    /// Iterating yields entry objects, not bare keys -- a `map_indexing_suite`
    /// convention that upstream inherits.
    fn __iter__(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let entries: Vec<Py<PyAttributeMapEntry>> = slf
            .cell
            .read()
            .iter()
            .map(|(key, value)| {
                Py::new(
                    py,
                    PyAttributeMapEntry {
                        key: key.clone(),
                        data: value.value().to_owned(),
                    },
                )
            })
            .collect::<PyResult<_>>()?;
        Ok(PyList::new(py, entries)?.try_iter()?.into())
    }

    fn keys<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        PyList::new(py, self.cell.read().keys())
    }

    fn values<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        PyList::new(py, self.cell.read().values().map(|value| value.value()))
    }

    fn items<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let map = self.cell.read();
        let pairs: Vec<Bound<'py, PyTuple>> = map
            .iter()
            .map(|(key, value)| PyTuple::new(py, [key.as_str(), value.value()]))
            .collect::<PyResult<_>>()?;
        PyList::new(py, pairs)
    }

    /// Compares by content, and accepts a plain `dict` on the right-hand side.
    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match attribute_map_from_any(other) {
            Ok(other) => *self.cell.read() == other,
            Err(_) => false,
        }
    }

    fn __ne__(&self, other: &Bound<'_, PyAny>) -> bool {
        !self.__eq__(other)
    }

    /// `subtype: solid type: line_thin ` -- space separated, with a trailing space.
    fn __str__(&self) -> String {
        let mut out = String::new();
        for (key, value) in self.cell.read().iter() {
            out.push_str(key);
            out.push_str(": ");
            out.push_str(value.value());
            out.push(' ');
        }
        out
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "AttributeMap({})",
            attribute_map_dict_repr(py, &self.cell.read())?
        ))
    }
}
