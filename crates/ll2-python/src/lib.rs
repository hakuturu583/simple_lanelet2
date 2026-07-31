//! PyO3 bindings for `ll2-core`, exposed to Python as the `lanelet2` package.
//!
//! Upstream Lanelet2 ships seven separate Boost.Python extension modules
//! (`lanelet2.core`, `lanelet2.geometry`, ...). We build a single extension,
//! `lanelet2._lanelet2`, containing seven genuine submodules; `python/lanelet2/
//! __init__.py` then registers them in `sys.modules` so that `import lanelet2.core`
//! and `from lanelet2.core import Point3d` behave identically.
//!
//! Each submodule's `__name__` is set to its final dotted path *before* any class is
//! registered, and every `#[pyclass]` carries an explicit `module = "lanelet2.<sub>"`,
//! so `repr(type(x))` matches upstream exactly.

use pyo3::prelude::*;
use pyo3::types::PyModule;

mod core;
mod geometry;
mod io;
mod matching;
mod projection;
mod routing;
mod traffic_rules;

/// Names of the submodules, in the order upstream's packages depend on each other.
const SUBMODULES: &[(&str, fn(&Bound<'_, PyModule>) -> PyResult<()>)] = &[
    ("core", core::register),
    ("geometry", geometry::register),
    ("projection", projection::register),
    ("io", io::register),
    ("traffic_rules", traffic_rules::register),
    ("routing", routing::register),
    ("matching", matching::register),
];

fn add_submodule(
    parent: &Bound<'_, PyModule>,
    name: &str,
    register: fn(&Bound<'_, PyModule>) -> PyResult<()>,
) -> PyResult<()> {
    let sub = PyModule::new(parent.py(), name)?;
    // Must happen before `register`, so classes defined inside see the right
    // `__name__` when Python computes their qualified name.
    sub.setattr("__name__", format!("lanelet2.{name}"))?;
    register(&sub)?;
    parent.add_submodule(&sub)?;
    Ok(())
}

#[pymodule]
fn _lanelet2(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Latch the bug-compatibility flag before anything is registered: some of the
    // switched behaviours are method signatures decided right here.
    let compat = ll2_core::compat::bug_compat();
    m.add("BUG_COMPAT", compat)?;
    m.add("__bug_compat__", compat)?;

    for &(name, register) in SUBMODULES {
        add_submodule(m, name, register)?;
    }
    Ok(())
}
