//! Geometry.
//!
//! Implemented directly against the upstream C++ rather than on top of a Rust
//! geometry crate: `geo`'s `covered_by` and DE-9IM predicates differ from Boost's
//! in exactly the boundary cases that matter here (touching segments, collinear
//! overlap, points exactly on an edge), and those differences would be silent.

pub mod offset;
pub mod bbox;
pub mod distance;
pub mod lanelet;
pub mod linestring;

pub use bbox::{BoundingBox2d, BoundingBox3d};
pub use distance::{distance_2d_point_point, distance_2d_point_segment};
pub use linestring::ArcCoordinates;
