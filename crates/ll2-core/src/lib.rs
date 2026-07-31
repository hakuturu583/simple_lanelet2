//! Core data model, geometry and algorithms for a Lanelet2-compatible library.
//!
//! This crate is pure Rust and carries no Python dependency; the PyO3 bindings live
//! in `ll2-python`. Every non-trivial algorithm here is a port of a specific piece of
//! upstream Lanelet2 C++ and carries a `file:line` reference in its doc comment.

pub mod area;
pub mod attribute;
pub mod centerline;
pub mod compat;
pub mod compound;
pub mod fmt;
pub mod geometry;
pub mod id;
pub mod lanelet;
pub mod linestring;
pub mod map;
pub mod point;
pub mod refs;
pub mod regelem;

pub use area::Area;
pub use attribute::{Attribute, AttributeMap};
pub use id::{Id, INVAL_ID, get_id, register_id};
pub use lanelet::Lanelet;
pub use linestring::LineString;
pub use point::Point;
pub use regelem::RegulatoryElement;
