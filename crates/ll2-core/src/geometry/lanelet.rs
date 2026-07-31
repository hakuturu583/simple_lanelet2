//! Lanelet geometry: containment, lengths relative to the centerline, and the
//! topological predicates.
//!
//! `leftOf`, `rightOf` and `follows` are *identity* checks, not geometric ones.
//! Two coincident but separately-built linestrings are not "the same boundary", so
//! two lanelets that look adjacent on a plot are not adjacent to these predicates.
//! That is deliberate upstream: adjacency in Lanelet2 is expressed by sharing the
//! boundary object, not by having equal coordinates.
//!
//! Upstream: `lanelet2_core/include/lanelet2_core/geometry/impl/Lanelet.h:25-145`

use crate::geometry::bbox;
use crate::geometry::distance::{covered_by_ring, distance_2d_point_polyline};
use crate::geometry::linestring::{self, Point2, Point3};
use crate::lanelet::Lanelet;
use crate::point::Point;

fn flat_2d(points: &[Point]) -> Vec<Point2> {
    points
        .iter()
        .map(|point| {
            let [x, y, _] = point.xyz();
            [x, y]
        })
        .collect()
}

fn flat_3d(points: &[Point]) -> Vec<Point3> {
    points.iter().map(Point::xyz).collect()
}

/// The lanelet's outline as a ring of 2D coordinates.
pub fn outline_2d(lanelet: &Lanelet) -> Vec<Point2> {
    flat_2d(&lanelet.polygon().points())
}

pub fn centerline_2d(lanelet: &Lanelet) -> Vec<Point2> {
    flat_2d(&lanelet.centerline().points())
}

pub fn centerline_3d(lanelet: &Lanelet) -> Vec<Point3> {
    flat_3d(&lanelet.centerline().points())
}

/// Whether a point lies within the lanelet. The boundary counts as inside.
pub fn inside(lanelet: &Lanelet, point: Point2) -> bool {
    covered_by_ring(point, &outline_2d(lanelet))
}

pub fn length_2d(lanelet: &Lanelet) -> f64 {
    linestring::length_2d(&centerline_2d(lanelet))
}

pub fn length_3d(lanelet: &Lanelet) -> f64 {
    linestring::length_3d(&centerline_3d(lanelet))
}

/// The sampled length of the *left bound*, which is what routing costs use.
pub fn approximated_length_2d(lanelet: &Lanelet) -> f64 {
    linestring::approximated_length_2d(&flat_2d(&lanelet.left_bound().points()))
}

pub fn distance_to_centerline_2d(lanelet: &Lanelet, point: Point2) -> f64 {
    distance_2d_point_polyline(point, &centerline_2d(lanelet))
}

pub fn distance_to_centerline_3d(lanelet: &Lanelet, point: Point3) -> f64 {
    let centerline = centerline_3d(lanelet);
    match centerline.len() {
        0 => f64::INFINITY,
        1 => linestring::distance_3d(point, centerline[0]),
        _ => centerline
            .windows(2)
            .map(|segment| {
                linestring::distance_3d(
                    point,
                    linestring::project_onto_segment_3d(point, segment[0], segment[1]),
                )
            })
            .fold(f64::INFINITY, f64::min),
    }
}

/// Whether the two outlines meet at all, sharing a boundary included.
pub fn intersects_2d(lanelet: &Lanelet, other: &Lanelet) -> bool {
    if lanelet.is_same_data(other) {
        return true;
    }
    rings_intersect(&outline_2d(lanelet), &outline_2d(other))
}

/// Whether the two lanelets' *interiors* meet.
///
/// Neighbours and successors are excluded before any geometry runs: they share a
/// boundary by construction, and reporting that as an overlap would make every
/// lane in a map conflict with the one beside it.
pub fn overlaps_2d(lanelet: &Lanelet, other: &Lanelet) -> bool {
    if lanelet.is_same_data(other) {
        return true;
    }
    let shares_boundary = lanelet.right_bound() == other.left_bound()
        || lanelet.left_bound() == other.right_bound()
        || lanelet.left_bound().invert() == other.left_bound()
        || lanelet.right_bound().invert() == other.right_bound()
        || follows(lanelet, other)
        || follows(other, lanelet)
        || follows(&lanelet.invert(), other)
        || follows(&other.invert(), lanelet);
    if shares_boundary {
        return false;
    }
    if !bbox::of_lanelet_2d(lanelet).intersects(&bbox::of_lanelet_2d(other)) {
        return false;
    }
    interiors_intersect(&outline_2d(lanelet), &outline_2d(other))
}

/// Height-aware variants: the 2D test plus a check that the two centerlines pass
/// within `height_tolerance` of each other where they are closest.
pub fn intersects_3d(lanelet: &Lanelet, other: &Lanelet, height_tolerance: f64) -> bool {
    intersects_2d(lanelet, other) && heights_agree(lanelet, other, height_tolerance)
}

pub fn overlaps_3d(lanelet: &Lanelet, other: &Lanelet, height_tolerance: f64) -> bool {
    overlaps_2d(lanelet, other) && heights_agree(lanelet, other, height_tolerance)
}

fn heights_agree(lanelet: &Lanelet, other: &Lanelet, height_tolerance: f64) -> bool {
    match linestring::projected_point_3d(&centerline_3d(lanelet), &centerline_3d(other)) {
        None => false,
        Some((a, b)) => (a[2] - b[2]).abs() < height_tolerance,
    }
}

/// Where the two centerlines cross.
pub fn intersect_centerlines_2d(lanelet: &Lanelet, other: &Lanelet) -> Vec<Point2> {
    linestring::polyline_intersections_2d(&centerline_2d(lanelet), &centerline_2d(other))
}

/// Whether `left` is immediately left of `right`, by shared boundary object.
pub fn left_of(left: &Lanelet, right: &Lanelet) -> bool {
    left.right_bound() == right.left_bound()
}

pub fn right_of(right: &Lanelet, left: &Lanelet) -> bool {
    left_of(left, right)
}

/// Whether `next` continues `prev`, by shared end *points* — again identity, not
/// coordinates.
pub fn follows(prev: &Lanelet, next: &Lanelet) -> bool {
    let (pl, pr) = (prev.left_bound(), prev.right_bound());
    let (nl, nr) = (next.left_bound(), next.right_bound());
    if pl.is_empty() || pr.is_empty() || nl.is_empty() || nr.is_empty() {
        return false;
    }
    let same = |a: Option<Point>, b: Option<Point>| match (a, b) {
        (Some(a), Some(b)) => a.is_same_data(&b),
        _ => false,
    };
    same(pl.back(), nl.front()) && same(pr.back(), nr.front())
}

/// Whether two rings touch or cross at all.
fn rings_intersect(a: &[Point2], b: &[Point2]) -> bool {
    if a.len() < 2 || b.len() < 2 {
        return false;
    }
    for index in 0..a.len() {
        let sa = (a[index], a[(index + 1) % a.len()]);
        for other in 0..b.len() {
            let sb = (b[other], b[(other + 1) % b.len()]);
            if linestring::segments_intersect_2d(sa.0, sa.1, sb.0, sb.1) {
                return true;
            }
        }
    }
    // One ring may sit wholly inside the other without any edge crossing.
    covered_by_ring(a[0], b) || covered_by_ring(b[0], a)
}

/// Whether two rings share interior area, rather than merely touching.
///
/// This is Boost's `T********` DE-9IM mask. Sampling interior points is enough
/// here: an overlap of positive area always contains a vertex of one ring or a
/// midpoint of a crossing edge.
fn interiors_intersect(a: &[Point2], b: &[Point2]) -> bool {
    if a.len() < 3 || b.len() < 3 {
        return false;
    }
    let strictly_inside = |p: Point2, ring: &[Point2]| {
        covered_by_ring(p, ring) && !on_boundary(p, ring)
    };
    if a.iter().any(|&p| strictly_inside(p, b)) || b.iter().any(|&p| strictly_inside(p, a)) {
        return true;
    }
    // Two rings can overlap with every vertex outside the other; test the midpoint
    // of each pair of crossing edges.
    for index in 0..a.len() {
        let sa = (a[index], a[(index + 1) % a.len()]);
        for other in 0..b.len() {
            let sb = (b[other], b[(other + 1) % b.len()]);
            if !linestring::segments_intersect_2d(sa.0, sa.1, sb.0, sb.1) {
                continue;
            }
            let midpoints = [
                [(sa.0[0] + sa.1[0]) / 2.0, (sa.0[1] + sa.1[1]) / 2.0],
                [(sb.0[0] + sb.1[0]) / 2.0, (sb.0[1] + sb.1[1]) / 2.0],
            ];
            if strictly_inside(midpoints[0], b) || strictly_inside(midpoints[1], a) {
                return true;
            }
        }
    }
    false
}

fn on_boundary(p: Point2, ring: &[Point2]) -> bool {
    (0..ring.len()).any(|index| {
        let a = ring[index];
        let b = ring[(index + 1) % ring.len()];
        crate::geometry::distance::distance_2d_point_segment(p, a, b) < 1e-12
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute::AttributeMap;
    use crate::linestring::LineString;

    fn line(id: i64, points: &[(f64, f64)]) -> LineString {
        LineString::new(
            id,
            points
                .iter()
                .map(|&(x, y)| Point::new(0, x, y, 0.0, AttributeMap::new()))
                .collect(),
            AttributeMap::new(),
        )
    }

    fn corridor(id: i64, x0: f64, x1: f64) -> Lanelet {
        Lanelet::new(
            id,
            line(id * 10 + 1, &[(x0, 1.0), (x1, 1.0)]),
            line(id * 10 + 2, &[(x0, -1.0), (x1, -1.0)]),
            AttributeMap::new(),
        )
    }

    #[test]
    fn containment_includes_the_boundary() {
        let ll = corridor(1, 0.0, 10.0);
        assert!(inside(&ll, [5.0, 0.0]));
        assert!(inside(&ll, [5.0, 1.0]), "on the left bound");
        assert!(!inside(&ll, [5.0, 2.0]));
        assert!(!inside(&ll, [-1.0, 0.0]));
    }

    #[test]
    fn lengths_follow_the_centerline() {
        let ll = corridor(1, 0.0, 10.0);
        assert!((length_2d(&ll) - 10.0).abs() < 1e-9);
        assert!((distance_to_centerline_2d(&ll, [5.0, 3.0]) - 3.0).abs() < 1e-9);
    }

    #[test]
    fn adjacency_is_identity_not_coordinates() {
        let shared = line(100, &[(0.0, 0.0), (10.0, 0.0)]);
        let upper = Lanelet::new(1, line(101, &[(0.0, 2.0), (10.0, 2.0)]), shared.clone(), AttributeMap::new());
        let lower = Lanelet::new(2, shared, line(102, &[(0.0, -2.0), (10.0, -2.0)]), AttributeMap::new());
        assert!(left_of(&upper, &lower));
        assert!(right_of(&lower, &upper));

        // The same coordinates through a separate linestring are not adjacent.
        let clone = Lanelet::new(
            3,
            line(103, &[(0.0, 0.0), (10.0, 0.0)]),
            line(104, &[(0.0, -2.0), (10.0, -2.0)]),
            AttributeMap::new(),
        );
        assert!(!left_of(&upper, &clone));
    }

    #[test]
    fn succession_needs_shared_end_points() {
        let mid_left = Point::new(0, 10.0, 1.0, 0.0, AttributeMap::new());
        let mid_right = Point::new(0, 10.0, -1.0, 0.0, AttributeMap::new());
        let make = |id: i64, first: bool| {
            let left = if first {
                vec![Point::new(0, 0.0, 1.0, 0.0, AttributeMap::new()), mid_left.clone()]
            } else {
                vec![mid_left.clone(), Point::new(0, 20.0, 1.0, 0.0, AttributeMap::new())]
            };
            let right = if first {
                vec![Point::new(0, 0.0, -1.0, 0.0, AttributeMap::new()), mid_right.clone()]
            } else {
                vec![mid_right.clone(), Point::new(0, 20.0, -1.0, 0.0, AttributeMap::new())]
            };
            Lanelet::new(
                id,
                LineString::new(id * 10 + 1, left, AttributeMap::new()),
                LineString::new(id * 10 + 2, right, AttributeMap::new()),
                AttributeMap::new(),
            )
        };
        let first = make(1, true);
        let second = make(2, false);
        assert!(follows(&first, &second));
        assert!(!follows(&second, &first));
        assert!(!follows(&first, &corridor(9, 10.0, 20.0)), "coincident but distinct");
    }

    #[test]
    fn neighbours_and_successors_do_not_count_as_overlapping() {
        let shared = line(100, &[(0.0, 0.0), (10.0, 0.0)]);
        let upper = Lanelet::new(1, line(101, &[(0.0, 2.0), (10.0, 2.0)]), shared.clone(), AttributeMap::new());
        let lower = Lanelet::new(2, shared, line(102, &[(0.0, -2.0), (10.0, -2.0)]), AttributeMap::new());
        assert!(!overlaps_2d(&upper, &lower));
        assert!(overlaps_2d(&upper, &upper), "a lanelet overlaps itself");
    }

    #[test]
    fn genuinely_crossing_lanelets_overlap() {
        let horizontal = corridor(1, 0.0, 10.0);
        let vertical = Lanelet::new(
            2,
            line(201, &[(4.0, -5.0), (4.0, 5.0)]),
            line(202, &[(6.0, -5.0), (6.0, 5.0)]),
            AttributeMap::new(),
        );
        assert!(overlaps_2d(&horizontal, &vertical));
        assert!(!overlaps_2d(&horizontal, &corridor(3, 100.0, 110.0)));
    }
}
