//! Linestrings, and the polygons that share their storage.
//!
//! `invert()` is an O(1) flag flip over the *same* data, not a copy. Every accessor
//! consults the flag, so an inverted handle reads points back to front — and writes
//! through it are mapped back: appending to an inverted linestring prepends to the
//! underlying data, and assigning to index 0 overwrites the last underlying point.
//!
//! Upstream also treats a polygon as a linestring with polygon traits, so both use
//! the storage defined here; only the Python layer distinguishes them (polygons
//! expose no `invert`, and their closing segment counts).
//!
//! Upstream: `lanelet2_core/include/lanelet2_core/primitives/LineString.h:157-181,
//! 532-652, 682-714`

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use parking_lot::RwLock;

use crate::attribute::AttributeMap;
use crate::fmt::make_repr;
use crate::id::{INVAL_ID, Id};
use crate::point::Point;
use crate::refs::{Attrs, attrs};

/// The storage behind every handle to one linestring or polygon.
pub struct LineStringData {
    id: AtomicI64,
    points: RwLock<Vec<Point>>,
    attributes: Attrs,
}

/// A handle to a linestring, carrying its own orientation.
///
/// Equality compares both the storage identity *and* the inversion flag, so
/// `ls != ls.invert()` even though they share every point.
#[derive(Clone)]
pub struct LineString {
    data: Arc<LineStringData>,
    inverted: bool,
}

impl LineString {
    pub fn new(id: Id, points: Vec<Point>, attributes: AttributeMap) -> Self {
        LineString {
            data: Arc::new(LineStringData {
                id: AtomicI64::new(id),
                points: RwLock::new(points),
                attributes: attrs(attributes),
            }),
            inverted: false,
        }
    }

    /// An empty linestring with no id — upstream's default-constructed handle.
    pub fn empty() -> Self {
        LineString::new(INVAL_ID, Vec::new(), AttributeMap::new())
    }

    pub fn id(&self) -> Id {
        self.data.id.load(Ordering::Relaxed)
    }

    pub fn set_id(&self, id: Id) {
        self.data.id.store(id, Ordering::Relaxed);
    }

    pub fn attributes(&self) -> &Attrs {
        &self.data.attributes
    }

    pub fn set_attributes(&self, map: AttributeMap) {
        *self.data.attributes.write() = map;
    }

    pub fn is_inverted(&self) -> bool {
        self.inverted
    }

    /// A new handle over the same data, reading in the opposite direction.
    pub fn invert(&self) -> LineString {
        LineString {
            data: self.data.clone(),
            inverted: !self.inverted,
        }
    }

    pub fn len(&self) -> usize {
        self.data.points.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Maps a view index onto the underlying storage index.
    fn storage_index(&self, index: usize, len: usize) -> usize {
        if self.inverted {
            len - 1 - index
        } else {
            index
        }
    }

    /// The point at a view index, or `None` if out of range.
    pub fn at(&self, index: usize) -> Option<Point> {
        let points = self.data.points.read();
        let len = points.len();
        if index >= len {
            return None;
        }
        Some(points[self.storage_index(index, len)].clone())
    }

    pub fn set_at(&self, index: usize, point: Point) -> bool {
        let mut points = self.data.points.write();
        let len = points.len();
        if index >= len {
            return false;
        }
        let target = self.storage_index(index, len);
        points[target] = point;
        true
    }

    pub fn remove_at(&self, index: usize) -> bool {
        let mut points = self.data.points.write();
        let len = points.len();
        if index >= len {
            return false;
        }
        let target = self.storage_index(index, len);
        points.remove(target);
        true
    }

    /// Appends in *view* order: on an inverted handle this prepends to the
    /// underlying storage, so that the new point still reads back last.
    pub fn push(&self, point: Point) {
        let mut points = self.data.points.write();
        if self.inverted {
            points.insert(0, point);
        } else {
            points.push(point);
        }
    }

    /// All points, in view order.
    pub fn points(&self) -> Vec<Point> {
        let points = self.data.points.read();
        if self.inverted {
            points.iter().rev().cloned().collect()
        } else {
            points.clone()
        }
    }

    pub fn front(&self) -> Option<Point> {
        self.at(0)
    }

    pub fn back(&self) -> Option<Point> {
        let len = self.len();
        if len == 0 { None } else { self.at(len - 1) }
    }

    /// Whether two handles address the same storage, ignoring orientation.
    pub fn is_same_data(&self, other: &LineString) -> bool {
        Arc::ptr_eq(&self.data, &other.data)
    }

    /// Upstream's `operator==`: same storage *and* same orientation.
    pub fn is_same_view(&self, other: &LineString) -> bool {
        self.is_same_data(other) && self.inverted == other.inverted
    }

    /// A stable identity token that distinguishes the two orientations.
    pub fn identity(&self) -> usize {
        // The low bit of a pointer to a heap allocation is always clear, so
        // folding the orientation into it keeps distinct views distinct.
        (Arc::as_ptr(&self.data) as usize) | usize::from(self.inverted)
    }

    /// `str(linestring)`: `[id: 10 point ids: 1, 2]`, with `, inverted` after the
    /// id when the handle reads backwards.
    pub fn to_display_string(&self) -> String {
        let ids: Vec<String> = self.points().iter().map(|p| p.id().to_string()).collect();
        format!(
            "[id: {}{} point ids: {}]",
            self.id(),
            if self.inverted { ", inverted" } else { "" },
            ids.join(", ")
        )
    }

    /// `repr(linestring)`, given each point already rendered and the attribute
    /// argument (empty when the map is empty, which drops it entirely).
    pub fn repr(&self, class_name: &str, point_reprs: &[String], attributes_repr: &str) -> String {
        let args = vec![
            self.id().to_string(),
            format!("[{}]", point_reprs.join(", ")),
            attributes_repr.to_owned(),
        ];
        make_repr(class_name, &args)
    }
}

impl PartialEq for LineString {
    fn eq(&self, other: &Self) -> bool {
        self.is_same_view(other)
    }
}

impl Eq for LineString {}

impl std::fmt::Debug for LineString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_display_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(ids: &[Id]) -> LineString {
        let points = ids
            .iter()
            .map(|&id| Point::new(id, id as f64, 0.0, 0.0, AttributeMap::new()))
            .collect();
        LineString::new(10, points, AttributeMap::new())
    }

    fn ids(ls: &LineString) -> Vec<Id> {
        ls.points().iter().map(Point::id).collect()
    }

    #[test]
    fn inverting_reverses_the_view_without_touching_the_data() {
        let ls = line(&[1, 2, 3]);
        assert_eq!(ids(&ls), [1, 2, 3]);
        assert_eq!(ids(&ls.invert()), [3, 2, 1]);
        assert_eq!(ids(&ls), [1, 2, 3], "the original view is unaffected");
        assert_eq!(ls.invert().invert(), ls);
    }

    #[test]
    fn a_linestring_does_not_equal_its_inverse() {
        let ls = line(&[1, 2]);
        assert_ne!(ls, ls.invert());
        assert!(ls.is_same_data(&ls.invert()));
        assert_ne!(ls.identity(), ls.invert().identity());
    }

    #[test]
    fn appending_to_an_inverted_view_prepends_to_the_storage() {
        let ls = line(&[1, 2, 3]);
        let inverted = ls.invert();

        inverted.push(Point::new(9, 9.0, 0.0, 0.0, AttributeMap::new()));

        assert_eq!(ids(&ls), [9, 1, 2, 3]);
        assert_eq!(ids(&inverted), [3, 2, 1, 9], "it still reads back last");
    }

    #[test]
    fn writes_through_an_inverted_view_are_mapped_back() {
        let ls = line(&[1, 2, 3]);
        assert!(
            ls.invert()
                .set_at(0, Point::new(7, 7.0, 0.0, 0.0, AttributeMap::new()))
        );
        assert_eq!(
            ids(&ls),
            [1, 2, 7],
            "index 0 of the inverse is the last point"
        );

        let ls = line(&[1, 2, 3]);
        assert!(ls.invert().remove_at(0));
        assert_eq!(ids(&ls), [1, 2]);
    }

    #[test]
    fn text_format_marks_inversion() {
        let ls = line(&[1, 2]);
        assert_eq!(ls.to_display_string(), "[id: 10 point ids: 1, 2]");
        assert_eq!(
            ls.invert().to_display_string(),
            "[id: 10, inverted point ids: 2, 1]"
        );
        assert_eq!(
            LineString::new(11, Vec::new(), AttributeMap::new()).to_display_string(),
            "[id: 11 point ids: ]"
        );
    }

    #[test]
    fn front_and_back_follow_the_view() {
        let ls = line(&[1, 2, 3]);
        assert_eq!(ls.front().unwrap().id(), 1);
        assert_eq!(ls.back().unwrap().id(), 3);
        assert_eq!(ls.invert().front().unwrap().id(), 3);
        assert_eq!(ls.invert().back().unwrap().id(), 1);
        assert!(LineString::empty().front().is_none());
    }
}
