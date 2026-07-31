//! Compound linestrings and polygons.
//!
//! A compound presents several linestrings as one. Where two adjacent members meet
//! at a shared point, the duplicate is dropped — but only when it is the *same
//! point object*, not merely the same coordinates, because the underlying iterator
//! compares primitives by storage identity. Empty members are skipped entirely.
//!
//! Compounds carry no id and no attributes of their own, so upstream registers no
//! `__repr__` for them either.
//!
//! Upstream: `lanelet2_core/include/lanelet2_core/primitives/CompoundLineString.h`,
//! `lanelet2_core/include/lanelet2_core/utility/CompoundIterator.h:48-53`

use std::sync::Arc;

use crate::id::Id;
use crate::linestring::LineString;
use crate::point::Point;

/// A view over several linestrings as one continuous line.
#[derive(Clone)]
pub struct CompoundLineString {
    lines: Arc<Vec<LineString>>,
    inverted: bool,
}

impl CompoundLineString {
    pub fn new(lines: Vec<LineString>) -> Self {
        CompoundLineString {
            lines: Arc::new(lines),
            inverted: false,
        }
    }

    pub fn is_inverted(&self) -> bool {
        self.inverted
    }

    pub fn invert(&self) -> CompoundLineString {
        CompoundLineString {
            lines: self.lines.clone(),
            inverted: !self.inverted,
        }
    }

    /// The member linestrings, in view order.
    ///
    /// An inverted compound reverses the list *and* inverts each member, so that
    /// reading the members in order still traverses the same path.
    pub fn line_strings(&self) -> Vec<LineString> {
        if self.inverted {
            self.lines.iter().rev().map(LineString::invert).collect()
        } else {
            self.lines.as_ref().clone()
        }
    }

    /// The ids of the member linestrings, in view order.
    pub fn ids(&self) -> Vec<Id> {
        let ids = self.lines.iter().map(LineString::id);
        if self.inverted {
            ids.rev().collect()
        } else {
            ids.collect()
        }
    }

    /// All points, with shared seam points collapsed.
    ///
    /// Deduplication is symmetric, so collapsing first and reversing afterwards
    /// gives the same sequence as reversing first.
    pub fn points(&self) -> Vec<Point> {
        let mut out: Vec<Point> = Vec::new();
        for line in self.lines.iter() {
            for point in line.points() {
                if out.last().is_some_and(|last| last.is_same_data(&point)) {
                    continue;
                }
                out.push(point);
            }
        }
        if self.inverted {
            out.reverse();
        }
        out
    }

    /// Upstream computes this by walking the points, so it is O(n), not O(1).
    pub fn len(&self) -> usize {
        self.points().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn at(&self, index: usize) -> Option<Point> {
        self.points().into_iter().nth(index)
    }

    /// An open line has one fewer segment than it has points.
    pub fn num_segments(&self) -> usize {
        self.len().max(1) - 1
    }

    /// A polygon's closing segment counts, so a ring has as many segments as points.
    pub fn num_polygon_segments(&self) -> usize {
        self.len().max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute::AttributeMap;
    use crate::id::INVAL_ID;

    fn point(id: Id) -> Point {
        Point::new(id, id as f64, 0.0, 0.0, AttributeMap::new())
    }

    fn line(id: Id, points: Vec<Point>) -> LineString {
        LineString::new(id, points, AttributeMap::new())
    }

    fn ids_of(compound: &CompoundLineString) -> Vec<Id> {
        compound.points().iter().map(Point::id).collect()
    }

    #[test]
    fn a_shared_seam_point_appears_once() {
        let shared = point(2);
        let compound = CompoundLineString::new(vec![
            line(10, vec![point(1), shared.clone()]),
            line(11, vec![shared, point(3)]),
        ]);
        assert_eq!(ids_of(&compound), [1, 2, 3]);
        assert_eq!(compound.len(), 3);
        assert_eq!(compound.num_segments(), 2);
    }

    #[test]
    fn coincident_but_distinct_seam_points_are_both_kept() {
        // Deduplication compares storage identity, not coordinates, so two points
        // that merely happen to share an id and position both survive.
        let compound = CompoundLineString::new(vec![
            line(10, vec![point(1), point(2)]),
            line(11, vec![point(2), point(3)]),
        ]);
        assert_eq!(ids_of(&compound), [1, 2, 2, 3]);
    }

    #[test]
    fn empty_members_are_skipped() {
        let compound = CompoundLineString::new(vec![
            line(10, vec![point(1)]),
            line(11, Vec::new()),
            line(12, vec![point(2)]),
        ]);
        assert_eq!(ids_of(&compound), [1, 2]);
        assert_eq!(CompoundLineString::new(Vec::new()).len(), 0);
        assert_eq!(CompoundLineString::new(Vec::new()).num_segments(), 0);
    }

    #[test]
    fn inverting_reverses_the_points_the_members_and_the_ids() {
        let shared = point(2);
        let compound = CompoundLineString::new(vec![
            line(10, vec![point(1), shared.clone()]),
            line(11, vec![shared, point(3)]),
        ]);
        let inverted = compound.invert();

        assert_eq!(ids_of(&inverted), [3, 2, 1]);
        assert_eq!(inverted.ids(), [11, 10]);
        assert!(inverted.line_strings().iter().all(LineString::is_inverted));
        assert_eq!(compound.ids(), [10, 11], "the original view is unaffected");
    }

    #[test]
    fn a_polygons_closing_segment_counts() {
        let compound = CompoundLineString::new(vec![line(10, vec![point(1), point(2), point(3)])]);
        assert_eq!(compound.num_segments(), 2);
        assert_eq!(compound.num_polygon_segments(), 3);
        assert_eq!(
            CompoundLineString::new(vec![line(INVAL_ID, Vec::new())]).num_polygon_segments(),
            1
        );
    }
}
