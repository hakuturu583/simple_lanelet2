//! Geometry.
//!
//! Implemented directly against the upstream C++ rather than on top of a Rust
//! geometry crate: `geo`'s `covered_by` and DE-9IM predicates differ from Boost's
//! in exactly the boundary cases that matter here (touching segments, collinear
//! overlap, points exactly on an edge), and those differences would be silent.
//!
//! This module starts with what the map layers need — bounding boxes and 2D
//! distances — and grows into the full `lanelet2.geometry` surface.

pub mod bbox;
pub mod distance;

pub use bbox::{BoundingBox2d, BoundingBox3d};
pub use distance::{distance_2d_point_point, distance_2d_point_segment};
