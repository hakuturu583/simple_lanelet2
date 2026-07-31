//! Conversions between Python objects and core types.
//!
//! Mirrors the implicit conversions Boost.Python registers: a plain `dict` is
//! accepted anywhere an `AttributeMap` is, any iterable is accepted where a
//! `std::vector` is expected, and `std::vector` results come back as fresh Python
//! lists rather than live views.

use ll2_core::attribute::{Attribute, AttributeMap};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyString};

use crate::core::attribute::PyAttributeMap;

/// Extracts an attribute map from either an `AttributeMap` or a plain `dict`.
///
/// Upstream's `DictToAttributeMapConverter` requires an exact `dict` (a subclass
/// will not convert) and rejects non-string keys and values.
pub fn attribute_map_from_any(obj: &Bound<'_, PyAny>) -> PyResult<AttributeMap> {
    if let Ok(map) = obj.cast::<PyAttributeMap>() {
        return Ok(map.borrow().snapshot());
    }
    let dict = obj.cast::<PyDict>()?;
    let mut out = AttributeMap::new();
    for (key, value) in dict.iter() {
        let key: String = key.cast::<PyString>()?.extract()?;
        let value: String = value.cast::<PyString>()?.extract()?;
        out.insert(key, Attribute::new(value));
    }
    Ok(out)
}

/// Same, but treats `None`/absent as an empty map.
pub fn optional_attribute_map(obj: Option<&Bound<'_, PyAny>>) -> PyResult<AttributeMap> {
    match obj {
        None => Ok(AttributeMap::new()),
        Some(obj) if obj.is_none() => Ok(AttributeMap::new()),
        Some(obj) => attribute_map_from_any(obj),
    }
}

/// Renders an attribute map the way Python renders the equivalent `dict`.
///
/// Building a real `PyDict` and asking Python for its `repr` is not laziness: it is
/// the only way to get Python's exact quoting rules (which switch to double quotes
/// when the value contains a single quote) for free.
pub fn attribute_map_dict_repr(py: Python<'_>, map: &AttributeMap) -> PyResult<String> {
    let dict = PyDict::new(py);
    // BTreeMap iterates in key order and Python dicts preserve insertion order, so
    // the rendered dict is sorted -- matching upstream's std::map-backed storage.
    for (key, value) in map {
        dict.set_item(key, value.value())?;
    }
    dict.repr()?.extract()
}

/// The attribute-map argument of a primitive's `repr`.
///
/// Empty maps render as the empty string, which `make_repr` then drops entirely --
/// that is how `Point3d(1000, 1, 2, 3)` avoids a trailing comma.
pub fn attributes_repr_arg(py: Python<'_>, map: &AttributeMap) -> PyResult<String> {
    if map.is_empty() {
        return Ok(String::new());
    }
    Ok(format!(
        "AttributeMap({})",
        attribute_map_dict_repr(py, map)?
    ))
}
