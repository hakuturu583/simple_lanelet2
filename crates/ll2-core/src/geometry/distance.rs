//! Distances.
//!
//! Boost's `distance` between a point and a linestring is the smallest distance to
//! any of its segments; between a point and a polygon it is zero when the point is
//! covered by the ring. Lanelets and areas delegate to their outline polygons, so
//! a point inside a lanelet is at distance zero from it.
//!
//! Upstream: `lanelet2_core/include/lanelet2_core/geometry/impl/LineString.h`,
//! `.../impl/Lanelet.h`, `lanelet2_core/src/LineStringGeometry.cpp:106-120`

use crate::area::Area;
use crate::compound::CompoundLineString;
use crate::lanelet::Lanelet;
use crate::linestring::LineString;
use crate::point::Point;

type Point2 = [f64; 2];

pub fn distance_2d_point_point(a: Point2, b: Point2) -> f64 {
    f64::hypot(a[0] - b[0], a[1] - b[1])
}

/// Distance from `p` to the segment `a-b`, clamped at both ends.
pub fn distance_2d_point_segment(p: Point2, a: Point2, b: Point2) -> f64 {
    distance_2d_point_point(p, project_onto_segment(p, a, b))
}

/// The closest point on segment `a-b` to `p`.
///
/// Upstream: `LineStringGeometry.cpp:106-120`
pub fn project_onto_segment(p: Point2, a: Point2, b: Point2) -> Point2 {
    let v = [b[0] - a[0], b[1] - a[1]];
    let w = [p[0] - a[0], p[1] - a[1]];
    let c1 = w[0] * v[0] + w[1] * v[1];
    if c1 <= 0.0 {
        return a;
    }
    let c2 = v[0] * v[0] + v[1] * v[1];
    if c2 <= c1 {
        return b;
    }
    let t = c1 / c2;
    [a[0] + t * v[0], a[1] + t * v[1]]
}

fn flat(points: &[Point]) -> Vec<Point2> {
    points
        .iter()
        .map(|point| {
            let [x, y, _] = point.xyz();
            [x, y]
        })
        .collect()
}

/// Distance from a point to a polyline: the smallest distance to any segment.
pub fn distance_2d_point_polyline(p: Point2, line: &[Point2]) -> f64 {
    match line.len() {
        0 => f64::INFINITY,
        1 => distance_2d_point_point(p, line[0]),
        _ => line
            .windows(2)
            .map(|segment| distance_2d_point_segment(p, segment[0], segment[1]))
            .fold(f64::INFINITY, f64::min),
    }
}

/// Whether `p` is inside or on the boundary of the ring, which is left open (the
/// first point is not repeated at the end).
///
/// This is Boost's `covered_by`: the boundary counts as inside.
pub fn covered_by_ring(p: Point2, ring: &[Point2]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    // On the boundary counts as covered, and the crossing count is unreliable
    // exactly there, so test it first.
    for index in 0..ring.len() {
        let a = ring[index];
        let b = ring[(index + 1) % ring.len()];
        if distance_2d_point_segment(p, a, b) == 0.0 {
            return true;
        }
    }

    let mut inside = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0];
            if p[0] < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Distance from a point to a closed ring: zero when covered, otherwise the
/// distance to the nearest edge (including the closing one).
pub fn distance_2d_point_ring(p: Point2, ring: &[Point2]) -> f64 {
    if ring.is_empty() {
        return f64::INFINITY;
    }
    if covered_by_ring(p, ring) {
        return 0.0;
    }
    let mut best = f64::INFINITY;
    for index in 0..ring.len() {
        let a = ring[index];
        let b = ring[(index + 1) % ring.len()];
        best = best.min(distance_2d_point_segment(p, a, b));
    }
    best
}

pub fn distance_2d_point_to_point_primitive(p: Point2, other: &Point) -> f64 {
    let [x, y, _] = other.xyz();
    distance_2d_point_point(p, [x, y])
}

pub fn distance_2d_point_to_linestring(p: Point2, line: &LineString) -> f64 {
    distance_2d_point_polyline(p, &flat(&line.points()))
}

pub fn distance_2d_point_to_polygon(p: Point2, polygon: &LineString) -> f64 {
    distance_2d_point_ring(p, &flat(&polygon.points()))
}

pub fn distance_2d_point_to_compound_ring(p: Point2, ring: &CompoundLineString) -> f64 {
    distance_2d_point_ring(p, &flat(&ring.points()))
}

/// Distance to a lanelet's outline, zero when the point is inside it.
pub fn distance_2d_point_to_lanelet(p: Point2, lanelet: &Lanelet) -> f64 {
    distance_2d_point_to_compound_ring(p, &lanelet.polygon())
}

/// Distance to an area, zero when the point is inside the outer ring and not in a
/// hole; inside a hole, the distance to that hole's edge.
pub fn distance_2d_point_to_area(p: Point2, area: &Area) -> f64 {
    let outer = flat(&area.outer_bound_polygon().points());
    if !covered_by_ring(p, &outer) {
        return distance_2d_point_ring(p, &outer);
    }
    for hole in area.inner_bound_polygons() {
        let hole = flat(&hole.points());
        if covered_by_ring(p, &hole) {
            return distance_2d_point_ring(p, &hole);
        }
    }
    0.0
}

/// The index of the segment of `line` closest to `p`, and the projected point on it.
pub fn closest_segment(p: Point2, line: &[Point2]) -> Option<(usize, Point2)> {
    if line.len() < 2 {
        return None;
    }
    let mut best: Option<(usize, Point2, f64)> = None;
    for (index, segment) in line.windows(2).enumerate() {
        let projected = project_onto_segment(p, segment[0], segment[1]);
        let distance = distance_2d_point_point(p, projected);
        if best.as_ref().is_none_or(|(_, _, d)| distance < *d) {
            best = Some((index, projected, distance));
        }
    }
    best.map(|(index, projected, _)| (index, projected))
}

/// Distance from a polyline to a point, signed: positive when the point lies to the
/// left of the line's direction of travel.
///
/// This is what decides which of a lanelet's two boundaries is the left one when a
/// map is loaded, so the sign convention is load-bearing.
///
/// Upstream: `geometry::signedDistance`, `impl/LineString.h:128-137`
pub fn signed_distance_2d(line: &[Point2], p: Point2) -> f64 {
    let Some((index, projected)) = closest_segment(p, line) else {
        return match line.first() {
            Some(&only) => distance_2d_point_point(p, only),
            None => f64::INFINITY,
        };
    };
    let distance = distance_2d_point_point(p, projected);
    let (a, b) = (line[index], line[index + 1]);
    let cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    if cross > 0.0 { distance } else { -distance }
}

/// The point a polyline's `align` heuristic compares against: its middle vertex, or
/// the midpoint of its endpoints when it has only two.
///
/// Upstream: `geometry::align`, `impl/LineString.h:815-817`
pub fn middle_point(line: &[Point2]) -> Option<Point2> {
    match line.len() {
        0 => None,
        1 => Some(line[0]),
        2 => Some([
            (line[0][0] + line[1][0]) / 2.0,
            (line[0][1] + line[1][1]) / 2.0,
        ]),
        n => Some(line[n / 2]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_clamps_at_both_ends() {
        let (a, b) = ([0.0, 0.0], [10.0, 0.0]);
        assert_eq!(project_onto_segment([5.0, 3.0], a, b), [5.0, 0.0]);
        assert_eq!(project_onto_segment([-5.0, 3.0], a, b), a);
        assert_eq!(project_onto_segment([15.0, 3.0], a, b), b);
        // A degenerate segment projects everything onto its single point.
        assert_eq!(project_onto_segment([5.0, 3.0], a, a), a);
    }

    #[test]
    fn point_to_polyline_takes_the_nearest_segment() {
        let line = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        assert_eq!(distance_2d_point_polyline([5.0, 2.0], &line), 2.0);
        assert_eq!(distance_2d_point_polyline([12.0, 5.0], &line), 2.0);
        assert_eq!(distance_2d_point_polyline([0.0, 0.0], &line), 0.0);
        assert_eq!(distance_2d_point_polyline([1.0, 1.0], &[[1.0, 0.0]]), 1.0);
        assert!(distance_2d_point_polyline([1.0, 1.0], &[]).is_infinite());
    }

    #[test]
    fn a_rings_boundary_counts_as_inside() {
        let square = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        assert!(covered_by_ring([2.0, 2.0], &square));
        assert!(covered_by_ring([0.0, 2.0], &square), "on an edge");
        assert!(covered_by_ring([0.0, 0.0], &square), "on a corner");
        assert!(!covered_by_ring([5.0, 2.0], &square));
        assert!(!covered_by_ring([2.0, 2.0], &[[0.0, 0.0], [1.0, 1.0]]));
    }

    #[test]
    fn signed_distance_is_positive_to_the_left_of_travel() {
        let eastward = [[0.0, 0.0], [10.0, 0.0]];
        assert!(
            signed_distance_2d(&eastward, [5.0, 3.0]) > 0.0,
            "north is left"
        );
        assert!(
            signed_distance_2d(&eastward, [5.0, -3.0]) < 0.0,
            "south is right"
        );
        assert_eq!(signed_distance_2d(&eastward, [5.0, 3.0]), 3.0);
        assert_eq!(signed_distance_2d(&eastward, [5.0, -3.0]), -3.0);

        let westward = [[10.0, 0.0], [0.0, 0.0]];
        assert!(
            signed_distance_2d(&westward, [5.0, 3.0]) < 0.0,
            "reversing flips the sign"
        );
    }

    #[test]
    fn the_middle_point_is_the_midpoint_only_for_a_two_point_line() {
        assert_eq!(middle_point(&[[0.0, 0.0], [10.0, 4.0]]), Some([5.0, 2.0]));
        assert_eq!(
            middle_point(&[[0.0, 0.0], [3.0, 0.0], [10.0, 0.0]]),
            Some([3.0, 0.0])
        );
        assert_eq!(middle_point(&[]), None);
    }

    #[test]
    fn distance_to_a_ring_is_zero_inside_and_uses_the_closing_edge() {
        let square = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        assert_eq!(distance_2d_point_ring([2.0, 2.0], &square), 0.0);
        assert_eq!(distance_2d_point_ring([6.0, 2.0], &square), 2.0);
        // Only the implicit closing edge is near this point.
        assert_eq!(distance_2d_point_ring([-1.0, 2.0], &square), 1.0);
    }
}
