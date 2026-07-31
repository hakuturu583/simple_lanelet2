//! Error translation.
//!
//! Upstream registers no exception translators at all, so every C++ exception
//! reaches Python as a plain `RuntimeError` carrying `what()`. The only structured
//! exceptions are the ones Boost.Python itself raises: `ArgumentError` when no
//! overload matches, `IndexError` on out-of-range indexing, and `KeyError` from the
//! map indexing suite.

use pyo3::exceptions::{PyIndexError, PyRuntimeError, PyTypeError};
use pyo3::prelude::*;

pyo3::create_exception!(
    _lanelet2,
    ArgumentError,
    PyTypeError,
    "No registered overload matched the given argument types."
);

/// Raised when no overload matches, mirroring Boost.Python's `ArgumentError`.
///
/// Deliberately not added to any submodule's namespace: upstream's lives in the
/// `Boost.Python` module, so exporting it from `lanelet2.core` would be a
/// gratuitous addition to the API surface.
pub fn argument_error(class: &str, method: &str) -> PyErr {
    ArgumentError::new_err(format!(
        "Python argument types in\n    {class}.{method}(...)\ndid not match C++ signature"
    ))
}

/// The generic translation of a C++ exception.
pub fn runtime(message: impl Into<String>) -> PyErr {
    PyRuntimeError::new_err(message.into())
}

/// Upstream's `no_init` classes report this when constructed from Python.
pub fn not_instantiable() -> PyErr {
    runtime("This class cannot be instantiated from Python")
}

/// Resolves a possibly-negative index against `len`, the way Boost.Python's
/// indexing suite does, or raises `IndexError: index out of range`.
pub fn resolve_index(index: isize, len: usize) -> PyResult<usize> {
    let resolved = if index < 0 { index + len as isize } else { index };
    if resolved < 0 || resolved as usize >= len {
        return Err(PyIndexError::new_err("index out of range"));
    }
    Ok(resolved as usize)
}

