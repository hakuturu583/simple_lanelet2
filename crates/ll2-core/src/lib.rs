//! Core data model, geometry and algorithms for a Lanelet2-compatible library.
//!
//! This crate is pure Rust and carries no Python dependency; the PyO3 bindings live
//! in `ll2-python`. Every non-trivial algorithm here is a port of a specific piece of
//! upstream Lanelet2 C++ and carries a `file:line` reference in its doc comment.

pub mod compat;
pub mod id;

pub use id::{Id, INVAL_ID, get_id, register_id};
