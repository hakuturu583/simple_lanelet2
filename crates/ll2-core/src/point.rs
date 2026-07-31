//! Points.
//!
//! A point always stores three coordinates. `Point2d` and `Point3d` — and their
//! `Const` counterparts — are four Python types over *one* piece of data, so there
//! is a single `Point` handle here and the 2D/3D/const distinction exists only at
//! the binding boundary.
//!
//! Upstream: `lanelet2_core/include/lanelet2_core/primitives/Point.h`

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::attribute::AttributeMap;
use crate::fmt::{make_repr, ostream_double};
use crate::id::{INVAL_ID, Id};
use crate::refs::{Attrs, CoordView, Coords, attrs, coords};

/// The storage behind every handle to one point.
///
/// The id and the two shared cells are individually mutable, so no lock guards the
/// struct as a whole. That keeps `point.attributes[...] = ...` from ever having to
/// take two locks in a fixed order.
pub struct PointData {
    id: AtomicI64,
    coords: Coords,
    attributes: Attrs,
}

/// A handle to a point.
///
/// Cloning aliases: two handles over the same data see each other's writes, and
/// compare equal. Equality is identity of the underlying storage, *not* of the id —
/// two independently constructed points with the same id are not equal.
///
/// Upstream: `lanelet2_core/include/lanelet2_core/primitives/Primitive.h:85`
#[derive(Clone)]
pub struct Point {
    data: Arc<PointData>,
}

impl Point {
    pub fn new(id: Id, x: f64, y: f64, z: f64, attributes: AttributeMap) -> Self {
        Point {
            data: Arc::new(PointData {
                id: AtomicI64::new(id),
                coords: coords(x, y, z),
                attributes: attrs(attributes),
            }),
        }
    }

    /// A point at the origin with no id — upstream's default-constructed `Point3d`.
    pub fn origin() -> Self {
        Point::new(INVAL_ID, 0.0, 0.0, 0.0, AttributeMap::new())
    }

    pub fn id(&self) -> Id {
        self.data.id.load(Ordering::Relaxed)
    }

    pub fn set_id(&self, id: Id) {
        self.data.id.store(id, Ordering::Relaxed);
    }

    pub fn x(&self) -> f64 {
        self.data.coords.read()[0]
    }

    pub fn y(&self) -> f64 {
        self.data.coords.read()[1]
    }

    pub fn z(&self) -> f64 {
        self.data.coords.read()[2]
    }

    pub fn xyz(&self) -> [f64; 3] {
        *self.data.coords.read()
    }

    pub fn set_x(&self, value: f64) {
        self.data.coords.write()[0] = value;
    }

    pub fn set_y(&self, value: f64) {
        self.data.coords.write()[1] = value;
    }

    pub fn set_z(&self, value: f64) {
        self.data.coords.write()[2] = value;
    }

    /// The shared coordinate cell, for building a `basicPoint()` view.
    pub fn coords(&self) -> &Coords {
        &self.data.coords
    }

    /// A live view of the coordinates. `mutable` is false for `Const*` handles
    /// unless bug-compatibility mode reopens upstream's const hole.
    pub fn basic_point(&self, mutable: bool) -> CoordView {
        CoordView::new(self.data.coords.clone(), mutable)
    }

    /// The shared attribute map, for building a live `AttributeMap` proxy.
    pub fn attributes(&self) -> &Attrs {
        &self.data.attributes
    }

    /// Replaces the whole attribute map, as assigning to `.attributes` does.
    pub fn set_attributes(&self, map: AttributeMap) {
        *self.data.attributes.write() = map;
    }

    /// Whether two handles address the same storage. This is what `==` means.
    pub fn is_same_data(&self, other: &Point) -> bool {
        Arc::ptr_eq(&self.data, &other.data)
    }

    /// A stable identity token, for hashing consistently with [`Self::is_same_data`].
    pub fn identity(&self) -> usize {
        Arc::as_ptr(&self.data) as usize
    }

    /// `str(point)` for the 2D views: `[id: 1 x: 1.5 y: 2.5]`.
    pub fn to_string_2d(&self) -> String {
        let [x, y, _] = self.xyz();
        format!(
            "[id: {} x: {} y: {}]",
            self.id(),
            ostream_double(x),
            ostream_double(y)
        )
    }

    /// `str(point)` for the 3D views: `[id: 1 x: 1.5 y: 2.5 z: 3.5]`.
    pub fn to_string_3d(&self) -> String {
        let [x, y, z] = self.xyz();
        format!(
            "[id: {} x: {} y: {} z: {}]",
            self.id(),
            ostream_double(x),
            ostream_double(y),
            ostream_double(z)
        )
    }

    /// `repr(point)`.
    ///
    /// `attributes_repr` is the rendered attribute map, or the empty string when the
    /// map is empty — `make_repr` then drops the argument entirely, which is how
    /// upstream produces `Point3d(1000, 1, 2, 3)` rather than a trailing comma.
    pub fn repr(&self, class_name: &str, three_d: bool, attributes_repr: &str) -> String {
        let [x, y, z] = self.xyz();
        let mut args = vec![self.id().to_string(), ostream_double(x), ostream_double(y)];
        if three_d {
            args.push(ostream_double(z));
        }
        args.push(attributes_repr.to_owned());
        make_repr(class_name, &args)
    }
}

impl PartialEq for Point {
    fn eq(&self, other: &Self) -> bool {
        self.is_same_data(other)
    }
}

impl Eq for Point {}

impl std::fmt::Debug for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_string_3d())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clones_alias_and_compare_equal() {
        let p = Point::new(5, 1.0, 2.0, 3.0, AttributeMap::new());
        let alias = p.clone();

        alias.set_x(9.0);
        assert_eq!(p.x(), 9.0);
        assert_eq!(p, alias);
    }

    #[test]
    fn equality_is_identity_not_id() {
        let a = Point::new(5, 0.0, 0.0, 0.0, AttributeMap::new());
        let b = Point::new(5, 0.0, 0.0, 0.0, AttributeMap::new());
        assert_ne!(a, b, "same id but separate storage must not compare equal");
    }

    #[test]
    fn basic_point_writes_through_to_the_owner() {
        let p = Point::new(1, 1.0, 2.0, 3.0, AttributeMap::new());
        let view = p.basic_point(true);

        assert!(view.set(0, 7.0));
        assert_eq!(p.x(), 7.0);
        // Writing the 2D view must leave z alone.
        assert_eq!(p.z(), 3.0);
    }

    #[test]
    fn text_formats_match_upstream() {
        let p = Point::new(1, 1.5, 2.5, 3.5, AttributeMap::new());
        assert_eq!(p.to_string_3d(), "[id: 1 x: 1.5 y: 2.5 z: 3.5]");
        assert_eq!(p.to_string_2d(), "[id: 1 x: 1.5 y: 2.5]");
        assert_eq!(p.repr("Point3d", true, ""), "Point3d(1, 1.5, 2.5, 3.5)");
        assert_eq!(p.repr("Point2d", false, ""), "Point2d(1, 1.5, 2.5)");
        assert_eq!(
            p.repr("Point3d", true, "AttributeMap({'a': 'b'})"),
            "Point3d(1, 1.5, 2.5, 3.5, AttributeMap({'a': 'b'}))"
        );
    }

    #[test]
    fn a_default_point_sits_at_the_origin_with_no_id() {
        let p = Point::origin();
        assert_eq!(p.id(), INVAL_ID);
        assert_eq!(p.xyz(), [0.0, 0.0, 0.0]);
    }
}
