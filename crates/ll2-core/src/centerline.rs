//! Centerline computation.
//!
//! A direct port of `lanelet2_core/src/Lanelet.cpp:16-235`. This is *not* naive
//! midpoint pairing: the two bounds usually have different point counts and
//! different spacing, so the algorithm advances one side at a time, each step
//! picking the nearest point on that side whose resulting centerline segment does
//! not cross either bound. Every length, arc coordinate, lane-match and travel-time
//! cost downstream rides on the exact output, so it is worth the fidelity.
//!
//! One deliberate substitution: upstream drives the candidate loop with
//! `bgi::rtree::nearest(point, tree.size())`, whose tie-breaking between
//! equidistant candidates is unspecified. Ties are common on rectilinear maps, so
//! candidates are ordered here by `(distance, index)` instead — same sequence
//! whenever distances differ, and deterministic when they do not.

use crate::attribute::AttributeMap;
use crate::id::INVAL_ID;
use crate::linestring::LineString;
use crate::point::Point;

type Point2 = [f64; 2];
type Segment = (Point2, Point2);

fn sub(a: Point2, b: Point2) -> Point2 {
    [a[0] - b[0], a[1] - b[1]]
}

fn cross(a: Point2, b: Point2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn distance(a: Point2, b: Point2) -> f64 {
    f64::hypot(a[0] - b[0], a[1] - b[1])
}

/// Whether `q` lies strictly left of the directed line `p1 -> p2`.
///
/// Upstream: `geometry::internal::pointIsLeftOf`
fn point_is_left_of(p1: Point2, p2: Point2, q: Point2) -> bool {
    cross(sub(p2, p1), sub(q, p1)) > 0.0
}

fn orientation(a: Point2, b: Point2, c: Point2) -> i8 {
    let value = cross(sub(b, a), sub(c, a));
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

/// Whether `q` lies on segment `a-b`, given the three are already collinear.
fn on_segment(a: Point2, b: Point2, q: Point2) -> bool {
    q[0] >= a[0].min(b[0])
        && q[0] <= a[0].max(b[0])
        && q[1] >= a[1].min(b[1])
        && q[1] <= a[1].max(b[1])
}

/// Boost's `intersects(Segment, Segment)`: true when the segments share any point,
/// touching endpoints and collinear overlap included.
fn segments_intersect(s: Segment, t: Segment) -> bool {
    let (p1, p2) = s;
    let (q1, q2) = t;
    let o1 = orientation(p1, p2, q1);
    let o2 = orientation(p1, p2, q2);
    let o3 = orientation(q1, q2, p1);
    let o4 = orientation(q1, q2, p2);

    if o1 != o2 && o3 != o4 {
        return true;
    }
    (o1 == 0 && on_segment(p1, p2, q1))
        || (o2 == 0 && on_segment(p1, p2, q2))
        || (o3 == 0 && on_segment(q1, q2, p1))
        || (o4 == 0 && on_segment(q1, q2, p2))
}

fn same_point(a: Point2, b: Point2) -> bool {
    a[0] == b[0] && a[1] == b[1]
}

/// Which bound a candidate is being drawn from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
}

/// Holds the two bounds and answers the geometric questions the candidate search
/// asks of them.
///
/// Upstream: `BoundChecker`, `Lanelet.cpp:36-131`
struct BoundChecker {
    left: Vec<Point2>,
    right: Vec<Point2>,
    left_segments: Vec<Segment>,
    right_segments: Vec<Segment>,
    entry: Segment,
    exit: Segment,
}

fn segments_of(points: &[Point2]) -> Vec<Segment> {
    points.windows(2).map(|pair| (pair[0], pair[1])).collect()
}

impl BoundChecker {
    fn new(left: Vec<Point2>, right: Vec<Point2>) -> Self {
        let entry = (right[0], left[0]);
        let exit = (left[left.len() - 1], right[right.len() - 1]);
        BoundChecker {
            left_segments: segments_of(&left),
            right_segments: segments_of(&right),
            entry,
            exit,
            left,
            right,
        }
    }

    fn points(&self, side: Side) -> &[Point2] {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    fn segments(&self, side: Side) -> &[Segment] {
        match side {
            Side::Left => &self.left_segments,
            Side::Right => &self.right_segments,
        }
    }

    fn intersects(&self, seg: Segment) -> bool {
        self.intersects_bound(seg, Side::Left)
            || self.intersects_bound(seg, Side::Right)
            || self.crosses_entry(seg)
            || self.crosses_exit(seg)
    }

    /// A bound segment blocks `seg` unless it starts at the same place `seg` does —
    /// that lets the candidate segment leave the bound it is anchored to.
    fn intersects_bound(&self, seg: Segment, side: Side) -> bool {
        self.segments(side)
            .iter()
            .any(|&bound| segments_intersect(bound, seg) && !same_point(bound.0, seg.0))
    }

    /// Whether the *far* endpoint of `seg` is blocked: any bound segment crossing it
    /// that does not simply terminate at that endpoint.
    fn second_crosses_bounds(&self, seg: Segment, side: Side) -> bool {
        self.segments(side).iter().any(|&bound| {
            segments_intersect(bound, seg)
                && !same_point(bound.0, seg.1)
                && !same_point(bound.1, seg.1)
        })
    }

    fn crosses_entry(&self, seg: Segment) -> bool {
        segments_intersect(seg, self.entry)
            && point_is_left_of(self.entry.0, self.entry.1, seg.1)
    }

    fn crosses_exit(&self, seg: Segment) -> bool {
        segments_intersect(seg, self.exit) && point_is_left_of(self.exit.0, self.exit.1, seg.1)
    }

    /// Indices into `side`'s points, ordered by increasing distance from `target`.
    ///
    /// This replaces upstream's `bgi::nearest(target, tree.size())` query. Ordering
    /// by `(distance, index)` gives the same sequence whenever distances differ and
    /// removes the tie-break ambiguity when they do not.
    fn by_distance_from(&self, side: Side, target: Point2) -> Vec<usize> {
        let points = self.points(side);
        let mut order: Vec<usize> = (0..points.len()).collect();
        order.sort_by(|&a, &b| {
            distance(points[a], target)
                .total_cmp(&distance(points[b], target))
                .then(a.cmp(&b))
        });
        order
    }
}

/// Finds the nearest point on `side` at or after `from_index` whose centerline
/// segment clears both bounds.
///
/// Upstream: `findClosestNonintersectingPoint`, `Lanelet.cpp:133-177`
fn closest_nonintersecting_point(
    bounds: &BoundChecker,
    side: Side,
    from_index: usize,
    other: Point2,
    last_centerline_point: Point2,
) -> Option<(usize, f64)> {
    let mut best: Option<(usize, f64)> = None;
    let d_last_other = distance(other, last_centerline_point);
    let candidates = bounds.by_distance_from(side, other);
    let points = bounds.points(side);

    for index in candidates {
        if index < from_index {
            continue;
        }

        let candidate = points[index];
        let candidate_distance = distance(candidate, other) / 2.0;

        if let Some((_, best_distance)) = best {
            // The triangle inequality bounds how much closer any later candidate
            // could be; once that bound is exceeded, nothing better remains.
            if candidate_distance - d_last_other > best_distance {
                break;
            }
            if best_distance <= candidate_distance {
                continue;
            }
        }

        let bound_connection: Segment = (other, candidate);
        let inverse_connection: Segment = (candidate, other);
        let midpoint = [
            (other[0] + candidate[0]) / 2.0,
            (other[1] + candidate[1]) / 2.0,
        ];
        let centerline_candidate: Segment = (last_centerline_point, midpoint);

        let other_side = if side == Side::Left { Side::Right } else { Side::Left };
        if !bounds.intersects(centerline_candidate)
            && !bounds.second_crosses_bounds(bound_connection, side)
            && !bounds.second_crosses_bounds(inverse_connection, other_side)
        {
            best = Some((index, candidate_distance));
        }
    }
    best
}

fn midpoint_3d(a: [f64; 3], b: [f64; 3]) -> Point {
    Point::new(
        INVAL_ID,
        (a[0] + b[0]) / 2.0,
        (a[1] + b[1]) / 2.0,
        (a[2] + b[2]) / 2.0,
        AttributeMap::new(),
    )
}

/// Computes the centerline between two bounds.
///
/// The result and all of its points carry `InvalId`; that is how upstream later
/// distinguishes a computed centerline from one the user set explicitly.
pub fn calculate(left_bound: &LineString, right_bound: &LineString) -> LineString {
    let left_points = left_bound.points();
    let right_points = right_bound.points();

    if left_points.is_empty() || right_points.is_empty() {
        return LineString::new(INVAL_ID, Vec::new(), AttributeMap::new());
    }

    let left3: Vec<[f64; 3]> = left_points.iter().map(Point::xyz).collect();
    let right3: Vec<[f64; 3]> = right_points.iter().map(Point::xyz).collect();
    let flat = |p: &[f64; 3]| -> Point2 { [p[0], p[1]] };
    let bounds = BoundChecker::new(
        left3.iter().map(flat).collect(),
        right3.iter().map(flat).collect(),
    );

    let mut centerline = vec![midpoint_3d(left3[0], right3[0])];

    let mut left_current = 0usize;
    let mut right_current = 0usize;

    // A lanelet closed at the beginning shares the literal first point object
    // between its bounds. Note this is identity, not coordinate equality.
    if left_points[0].is_same_data(&right_points[0]) {
        left_current = 1;
    }

    loop {
        let last = centerline.last().expect("seeded with the entry midpoint");
        let last2d: Point2 = [last.x(), last.y()];

        let left_candidate = if left_current + 1 <= left3.len() {
            closest_nonintersecting_point(
                &bounds,
                Side::Left,
                left_current + 1,
                bounds.right[right_current],
                last2d,
            )
        } else {
            None
        };
        let right_candidate = if right_current + 1 <= right3.len() {
            closest_nonintersecting_point(
                &bounds,
                Side::Right,
                right_current + 1,
                bounds.left[left_current],
                last2d,
            )
        } else {
            None
        };

        match (left_candidate, right_candidate) {
            // Ties go to the left, matching upstream's `<=`.
            (Some((index, left_d)), right) if right.is_none_or(|(_, r)| left_d <= r) => {
                centerline.push(midpoint_3d(left3[index], right3[right_current]));
                left_current = index;
            }
            (left, Some((index, right_d))) if left.is_none_or(|(_, l)| l > right_d) => {
                centerline.push(midpoint_3d(left3[left_current], right3[index]));
                right_current = index;
            }
            _ => break,
        }
    }

    // The midpoint of the two end points belongs in the result in any case.
    if !(left_current == left3.len() - 1 && right_current == right3.len() - 1) {
        centerline.push(midpoint_3d(
            left3[left3.len() - 1],
            right3[right3.len() - 1],
        ));
    }

    LineString::new(INVAL_ID, centerline, AttributeMap::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(points: &[(f64, f64)]) -> LineString {
        LineString::new(
            1,
            points
                .iter()
                .map(|&(x, y)| Point::new(INVAL_ID, x, y, 0.0, AttributeMap::new()))
                .collect(),
            AttributeMap::new(),
        )
    }

    fn coords(ls: &LineString) -> Vec<(f64, f64)> {
        ls.points().iter().map(|p| (p.x(), p.y())).collect()
    }

    // Expected values below come from the reference implementation, not from
    // intuition: the algorithm advances one side at a time, so evenly spaced
    // parallel bounds yield *twice* as many centerline points as either bound.

    #[test]
    fn parallel_bounds_interleave_both_sides() {
        let left = line(&[(0.0, 1.0), (1.0, 1.0), (2.0, 1.0)]);
        let right = line(&[(0.0, -1.0), (1.0, -1.0), (2.0, -1.0)]);
        assert_eq!(
            coords(&calculate(&left, &right)),
            [(0.0, 0.0), (0.5, 0.0), (1.0, 0.0), (1.5, 0.0), (2.0, 0.0)]
        );

        let left = line(&[(0.0, 1.0), (1.0, 1.0)]);
        let right = line(&[(0.0, -1.0), (1.0, -1.0)]);
        assert_eq!(
            coords(&calculate(&left, &right)),
            [(0.0, 0.0), (0.5, 0.0), (1.0, 0.0)]
        );
    }

    #[test]
    fn an_empty_bound_gives_an_empty_centerline() {
        assert!(calculate(&line(&[]), &line(&[(0.0, 0.0)])).is_empty());
        assert!(calculate(&line(&[(0.0, 0.0)]), &line(&[])).is_empty());
    }

    #[test]
    fn the_computed_centerline_and_its_points_carry_no_id() {
        let left = line(&[(0.0, 1.0), (1.0, 1.0)]);
        let right = line(&[(0.0, -1.0), (1.0, -1.0)]);
        let centerline = calculate(&left, &right);
        assert_eq!(centerline.id(), INVAL_ID);
        assert!(centerline.points().iter().all(|p| p.id() == INVAL_ID));
    }

    #[test]
    fn unequal_point_counts_still_reach_both_end_points() {
        let left = line(&[(0.0, 1.0), (0.5, 1.0), (1.0, 1.0), (1.5, 1.0), (2.0, 1.0)]);
        let right = line(&[(0.0, -1.0), (2.0, -1.0)]);
        assert_eq!(
            coords(&calculate(&left, &right)),
            [(0.0, 0.0), (0.25, 0.0), (0.5, 0.0), (1.5, 0.0), (2.0, 0.0)]
        );
    }

    #[test]
    fn z_is_averaged_even_though_the_search_is_planar() {
        let left = LineString::new(
            1,
            vec![
                Point::new(INVAL_ID, 0.0, 1.0, 4.0, AttributeMap::new()),
                Point::new(INVAL_ID, 1.0, 1.0, 4.0, AttributeMap::new()),
            ],
            AttributeMap::new(),
        );
        let right = LineString::new(
            2,
            vec![
                Point::new(INVAL_ID, 0.0, -1.0, 0.0, AttributeMap::new()),
                Point::new(INVAL_ID, 1.0, -1.0, 0.0, AttributeMap::new()),
            ],
            AttributeMap::new(),
        );
        assert!(calculate(&left, &right).points().iter().all(|p| p.z() == 2.0));
    }
}
