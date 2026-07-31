//! Linestring geometry: lengths, interpolation, projection and arc coordinates.
//!
//! Everything here works on plain coordinate arrays; the binding layer flattens
//! whatever primitive it was handed first. Dimension is carried by the array length
//! rather than the type, so a single implementation serves both 2D and 3D.
//!
//! Upstream: `lanelet2_core/include/lanelet2_core/geometry/impl/LineString.h`,
//! `lanelet2_core/src/LineStringGeometry.cpp`

use crate::geometry::distance::{
    closest_segment, distance_2d_point_point, distance_2d_point_segment, project_onto_segment,
};

pub type Point2 = [f64; 2];
pub type Point3 = [f64; 3];

/// Arc-length coordinates relative to a linestring.
///
/// `distance` is signed, and **left of the direction of travel is positive**.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ArcCoordinates {
    pub length: f64,
    pub distance: f64,
}

pub fn distance_3d(a: Point3, b: Point3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Total length of a 2D polyline.
pub fn length_2d(line: &[Point2]) -> f64 {
    line.windows(2)
        .map(|pair| distance_2d_point_point(pair[0], pair[1]))
        .sum()
}

/// Total length of a 3D polyline.
pub fn length_3d(line: &[Point3]) -> f64 {
    line.windows(2).map(|pair| distance_3d(pair[0], pair[1])).sum()
}

/// Each segment's share of the total length.
fn length_ratios(line: &[Point2]) -> Vec<f64> {
    let total = length_2d(line);
    if total == 0.0 {
        return vec![0.0; line.len().saturating_sub(1)];
    }
    line.windows(2)
        .map(|pair| distance_2d_point_point(pair[0], pair[1]) / total)
        .collect()
}

/// The running sum of [`length_ratios`].
pub fn accumulated_length_ratios(line: &[Point2]) -> Vec<f64> {
    let mut ratios = length_ratios(line);
    for index in 1..ratios.len() {
        ratios[index] += ratios[index - 1];
    }
    ratios
}

/// The point `dist` along the line, interpolating within a segment.
///
/// A negative distance measures from the far end. Past the end, the last point is
/// returned rather than extrapolating. The `1e-8` guard returns the segment's start
/// point exactly, so a distance that lands on a vertex gives that vertex.
pub fn interpolated_point_at_distance_2d(line: &[Point2], dist: f64) -> Option<Point2> {
    if line.is_empty() {
        return None;
    }
    let (line, dist) = if dist < 0.0 {
        (line.iter().rev().copied().collect::<Vec<_>>(), -dist)
    } else {
        (line.to_vec(), dist)
    };

    let mut cumulative = 0.0;
    for pair in line.windows(2) {
        let (p1, p2) = (pair[0], pair[1]);
        let segment = distance_2d_point_point(p1, p2);
        cumulative += segment;
        if cumulative >= dist {
            let remaining = dist - (cumulative - segment);
            if remaining < 1e-8 {
                return Some(p1);
            }
            let t = remaining / segment;
            return Some([p1[0] + t * (p2[0] - p1[0]), p1[1] + t * (p2[1] - p1[1])]);
        }
    }
    line.last().copied()
}

/// The 3D counterpart of [`interpolated_point_at_distance_2d`].
pub fn interpolated_point_at_distance_3d(line: &[Point3], dist: f64) -> Option<Point3> {
    if line.is_empty() {
        return None;
    }
    let (line, dist) = if dist < 0.0 {
        (line.iter().rev().copied().collect::<Vec<_>>(), -dist)
    } else {
        (line.to_vec(), dist)
    };

    let mut cumulative = 0.0;
    for pair in line.windows(2) {
        let (p1, p2) = (pair[0], pair[1]);
        let segment = distance_3d(p1, p2);
        cumulative += segment;
        if cumulative >= dist {
            let remaining = dist - (cumulative - segment);
            if remaining < 1e-8 {
                return Some(p1);
            }
            let t = remaining / segment;
            return Some([
                p1[0] + t * (p2[0] - p1[0]),
                p1[1] + t * (p2[1] - p1[1]),
                p1[2] + t * (p2[2] - p1[2]),
            ]);
        }
    }
    line.last().copied()
}

/// The index of the existing vertex nearest to `dist` along the line.
///
/// Unlike interpolation this never invents a point: it returns whichever end of the
/// containing segment is closer.
pub fn nearest_index_at_distance_2d(line: &[Point2], dist: f64) -> Option<usize> {
    if line.is_empty() {
        return None;
    }
    let inverted = dist < 0.0;
    let dist = dist.abs();
    let walked: Vec<Point2> = if inverted {
        line.iter().rev().copied().collect()
    } else {
        line.to_vec()
    };

    let unmap = |index: usize| if inverted { line.len() - 1 - index } else { index };

    let mut cumulative = 0.0;
    for (index, pair) in walked.windows(2).enumerate() {
        let segment = distance_2d_point_point(pair[0], pair[1]);
        cumulative += segment;
        if cumulative >= dist {
            let remaining = dist - (cumulative - segment);
            return Some(unmap(if remaining > segment / 2.0 { index + 1 } else { index }));
        }
    }
    Some(unmap(walked.len() - 1))
}

/// The closest point on the line to `p`.
pub fn project_2d(line: &[Point2], p: Point2) -> Option<Point2> {
    match line.len() {
        0 => None,
        1 => Some(line[0]),
        _ => closest_segment(p, line).map(|(_, projected)| projected),
    }
}

/// The closest point on a 3D line to `p`.
pub fn project_3d(line: &[Point3], p: Point3) -> Option<Point3> {
    match line.len() {
        0 => None,
        1 => Some(line[0]),
        _ => {
            let mut best: Option<(Point3, f64)> = None;
            for pair in line.windows(2) {
                let projected = project_onto_segment_3d(p, pair[0], pair[1]);
                let d = distance_3d(p, projected);
                if best.as_ref().is_none_or(|(_, bd)| d < *bd) {
                    best = Some((projected, d));
                }
            }
            best.map(|(projected, _)| projected)
        }
    }
}

/// The 3D clamped point-to-segment projection.
pub fn project_onto_segment_3d(p: Point3, a: Point3, b: Point3) -> Point3 {
    let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let w = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
    let c1 = w[0] * v[0] + w[1] * v[1] + w[2] * v[2];
    if c1 <= 0.0 {
        return a;
    }
    let c2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if c2 <= c1 {
        return b;
    }
    let t = c1 / c2;
    [a[0] + t * v[0], a[1] + t * v[1], a[2] + t * v[2]]
}

/// The closest pair of points between two 3D polylines, one on each.
///
/// Boost has no 3D distance, so upstream computes this with the classic
/// segment-to-segment routine and takes the norm of the difference.
///
/// Upstream: `lanelet2_core/src/LineStringGeometry.cpp:60-104`
pub fn projected_point_3d(a: &[Point3], b: &[Point3]) -> Option<(Point3, Point3)> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    if a.len() == 1 || b.len() == 1 {
        let (pa, pb) = match (a.len(), b.len()) {
            (1, 1) => (a[0], b[0]),
            (1, _) => (a[0], project_3d(b, a[0])?),
            _ => (project_3d(a, b[0])?, b[0]),
        };
        return Some((pa, pb));
    }

    let mut best: Option<(Point3, Point3, f64)> = None;
    for sa in a.windows(2) {
        for sb in b.windows(2) {
            let (pa, pb) = closest_points_between_segments(sa[0], sa[1], sb[0], sb[1]);
            let d = distance_3d(pa, pb);
            if best.as_ref().is_none_or(|(_, _, bd)| d < *bd) {
                best = Some((pa, pb, d));
            }
        }
    }
    best.map(|(pa, pb, _)| (pa, pb))
}

/// The closest points on two 3D segments.
///
/// The `geomalgorithms.com/a07` routine, with the degenerate-parallel case handled
/// by falling back to a parameter of zero.
fn closest_points_between_segments(a0: Point3, a1: Point3, b0: Point3, b1: Point3) -> (Point3, Point3) {
    let sub = |p: Point3, q: Point3| [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
    let dot = |p: [f64; 3], q: [f64; 3]| p[0] * q[0] + p[1] * q[1] + p[2] * q[2];

    let u = sub(a1, a0);
    let v = sub(b1, b0);
    let w = sub(a0, b0);
    let (a, b, c, d, e) = (dot(u, u), dot(u, v), dot(v, v), dot(u, w), dot(v, w));
    let denominator = a * c - b * b;

    const EPSILON: f64 = 1e-12;
    let (mut sn, mut sd) = (denominator, denominator);
    let (mut tn, mut td) = (denominator, denominator);
    if denominator < EPSILON {
        // The segments are (nearly) parallel; pin the first parameter to zero.
        sn = 0.0;
        sd = 1.0;
        tn = e;
        td = c;
    } else {
        sn = b * e - c * d;
        tn = a * e - b * d;
        if sn < 0.0 {
            sn = 0.0;
            tn = e;
            td = c;
        } else if sn > sd {
            sn = sd;
            tn = e + b;
            td = c;
        }
    }
    if tn < 0.0 {
        tn = 0.0;
        if -d < 0.0 {
            sn = 0.0;
        } else if -d > a {
            sn = sd;
        } else {
            sn = -d;
            sd = a;
        }
    } else if tn > td {
        tn = td;
        if -d + b < 0.0 {
            sn = 0.0;
        } else if -d + b > a {
            sn = sd;
        } else {
            sn = -d + b;
            sd = a;
        }
    }
    let sc = if sd.abs() < EPSILON { 0.0 } else { sn / sd };
    let tc = if td.abs() < EPSILON { 0.0 } else { tn / td };

    (
        [a0[0] + sc * u[0], a0[1] + sc * u[1], a0[2] + sc * u[2]],
        [b0[0] + tc * v[0], b0[1] + tc * v[1], b0[2] + tc * v[2]],
    )
}

fn point_is_left_of(p1: Point2, p2: Point2, q: Point2) -> bool {
    (p2[0] - p1[0]) * (q[1] - p1[1]) - (p2[1] - p1[1]) * (q[0] - p1[0]) > 0.0
}

/// Which side of the line `p` falls on, with the reflex-vertex correction.
///
/// When the projection lands exactly on a vertex the two adjoining segments can
/// disagree about the side, so upstream consults the next one.
///
/// Upstream: `internal::isLeftOf`, `impl/LineString.h:76-92`
fn is_left_of(line: &[Point2], p: Point2, segment_index: usize, projected: Point2) -> bool {
    let (a, b) = (line[segment_index], line[segment_index + 1]);
    let is_left = point_is_left_of(a, b, p);
    if b == projected && segment_index + 2 < line.len() {
        let next = line[segment_index + 2];
        if is_left != point_is_left_of(b, next, p) && is_left == point_is_left_of(a, b, next) {
            return !is_left;
        }
    }
    is_left
}

/// Arc-length coordinates of a point relative to a linestring.
pub fn to_arc_coordinates(line: &[Point2], p: Point2) -> ArcCoordinates {
    let Some((index, projected)) = closest_segment(p, line) else {
        return ArcCoordinates {
            length: 0.0,
            distance: line
                .first()
                .map(|&only| distance_2d_point_point(p, only))
                .unwrap_or(f64::INFINITY),
        };
    };
    let raw = distance_2d_point_point(p, projected);
    let signed = if is_left_of(line, p, index, projected) { raw } else { -raw };

    let mut length = 0.0;
    for pair in line[..=index].windows(2) {
        length += distance_2d_point_point(pair[0], pair[1]);
    }
    length += distance_2d_point_point(line[index], projected);
    ArcCoordinates {
        length,
        distance: signed,
    }
}

/// Something the caller asked for that cannot be computed.
#[derive(Debug)]
pub struct InvalidInput(pub String);

/// The point at given arc coordinates.
///
/// Not an exact inverse of [`to_arc_coordinates`]: the lateral offset is applied
/// along one segment's normal, while the forward direction projects onto whichever
/// segment is nearest.
pub fn from_arc_coordinates(line: &[Point2], arc: ArcCoordinates) -> Result<Point2, InvalidInput> {
    if line.len() < 2 {
        return Err(InvalidInput(
            "Can't use arc coordinates on degenerated line string".into(),
        ));
    }
    let ratios = accumulated_length_ratios(line);
    let total = length_2d(line);

    let mut start = 0usize;
    let mut end = 0usize;
    for (index, ratio) in ratios.iter().enumerate() {
        if ratio * total > arc.length {
            start = index;
            end = index + 1;
            break;
        }
    }
    if end == 0 {
        end = line.len() - 1;
        start = end - 1;
    }

    let base = interpolated_point_at_distance_2d(line, arc.length)
        .ok_or_else(|| InvalidInput("Empty line string".into()))?;
    let dx = line[end][0] - line[start][0];
    let dy = line[end][1] - line[start][1];
    let norm = f64::hypot(dx, dy);
    if norm == 0.0 {
        return Err(InvalidInput(
            "Can't determine shift direction from two identical points on linestring".into(),
        ));
    }
    Ok([
        base[0] + (-dy / norm) * arc.distance,
        base[1] + (dx / norm) * arc.distance,
    ])
}

/// Menger curvature through three points, signed so that a left turn is positive.
///
/// Degenerate triples give infinity rather than a division by zero.
pub fn signed_curvature_2d(p1: Point2, p2: Point2, p3: Point2) -> f64 {
    let area = 0.5 * ((p2[0] - p1[0]) * (p3[1] - p1[1]) - (p2[1] - p1[1]) * (p3[0] - p1[0]));
    let product = distance_2d_point_point(p1, p2)
        * distance_2d_point_point(p1, p3)
        * distance_2d_point_point(p2, p3);
    if product < 1e-20 {
        return f64::INFINITY;
    }
    4.0 * area / product
}

pub fn curvature_2d(p1: Point2, p2: Point2, p3: Point2) -> f64 {
    signed_curvature_2d(p1, p2, p3).abs()
}

/// Twice the signed area of a ring: positive counter-clockwise.
fn shoelace_2x(ring: &[Point2]) -> f64 {
    if ring.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for index in 0..ring.len() {
        let a = ring[index];
        let b = ring[(index + 1) % ring.len()];
        sum += a[0] * b[1] - b[0] * a[1];
    }
    sum
}

/// The area of a ring.
///
/// Signed, and the sign follows the winding — Boost returns a negative area for a
/// ring wound against the registered orientation, and Lanelet2 registers clockwise.
pub fn ring_area(ring: &[Point2]) -> f64 {
    -shoelace_2x(ring) / 2.0
}

/// Whether two 2D segments meet, touching endpoints included.
pub fn segments_intersect_2d(a0: Point2, a1: Point2, b0: Point2, b1: Point2) -> bool {
    let orientation = |a: Point2, b: Point2, c: Point2| {
        let value = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if value > 0.0 {
            1i8
        } else if value < 0.0 {
            -1
        } else {
            0
        }
    };
    let on = |a: Point2, b: Point2, q: Point2| {
        q[0] >= a[0].min(b[0]) && q[0] <= a[0].max(b[0]) && q[1] >= a[1].min(b[1]) && q[1] <= a[1].max(b[1])
    };
    let (o1, o2) = (orientation(a0, a1, b0), orientation(a0, a1, b1));
    let (o3, o4) = (orientation(b0, b1, a0), orientation(b0, b1, a1));
    if o1 != o2 && o3 != o4 {
        return true;
    }
    (o1 == 0 && on(a0, a1, b0))
        || (o2 == 0 && on(a0, a1, b1))
        || (o3 == 0 && on(b0, b1, a0))
        || (o4 == 0 && on(b0, b1, a1))
}

/// Whether two polylines meet anywhere.
pub fn polylines_intersect_2d(a: &[Point2], b: &[Point2]) -> bool {
    a.windows(2)
        .any(|sa| b.windows(2).any(|sb| segments_intersect_2d(sa[0], sa[1], sb[0], sb[1])))
}

/// Where two polylines cross, ordered along the first.
///
/// One entry per crossing *segment pair*, not per distinct location: two lines that
/// meet at a shared vertex report that point once for each pair of segments
/// involved. Boost does the same, and callers compare the count.
pub fn polyline_intersections_2d(a: &[Point2], b: &[Point2]) -> Vec<Point2> {
    let mut out: Vec<Point2> = Vec::new();
    for sa in a.windows(2) {
        for sb in b.windows(2) {
            out.extend(segment_intersection_points(sa[0], sa[1], sb[0], sb[1]));
        }
    }
    out
}

/// Where two segments meet.
///
/// A crossing gives one point; a *collinear overlap* gives the two ends of the
/// overlap, in the first segment's direction. Boost reports overlaps that way, and
/// it is what makes `intersection(line, line)` on a line with itself return its own
/// vertices rather than nothing.
fn segment_intersection_points(a0: Point2, a1: Point2, b0: Point2, b1: Point2) -> Vec<Point2> {
    let d1 = [a1[0] - a0[0], a1[1] - a0[1]];
    let d2 = [b1[0] - b0[0], b1[1] - b0[1]];
    let denominator = d1[0] * d2[1] - d1[1] * d2[0];
    let along = |t: f64| [a0[0] + t * d1[0], a0[1] + t * d1[1]];

    if denominator.abs() < 1e-15 {
        // Parallel. Only a collinear pair can overlap.
        let offset = [b0[0] - a0[0], b0[1] - a0[1]];
        if (offset[0] * d1[1] - offset[1] * d1[0]).abs() > 1e-12 {
            return Vec::new();
        }
        let squared = d1[0] * d1[0] + d1[1] * d1[1];
        if squared == 0.0 {
            // A zero-length first segment is a point; it intersects when the other
            // segment passes through it.
            return if distance_2d_point_segment(a0, b0, b1) == 0.0 {
                vec![a0]
            } else {
                Vec::new()
            };
        }
        let parameter = |p: Point2| ((p[0] - a0[0]) * d1[0] + (p[1] - a0[1]) * d1[1]) / squared;
        let (t0, t1) = (parameter(b0), parameter(b1));
        let low = t0.min(t1).max(0.0);
        let high = t0.max(t1).min(1.0);
        if low > high {
            return Vec::new();
        }
        if low == high {
            return vec![along(low)];
        }
        return vec![along(low), along(high)];
    }

    let dx = b0[0] - a0[0];
    let dy = b0[1] - a0[1];
    let t = (dx * d2[1] - dy * d2[0]) / denominator;
    let u = (dx * d1[1] - dy * d1[0]) / denominator;
    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
        return vec![along(t)];
    }
    Vec::new()
}

/// The length of a polyline, sampled rather than summed.
///
/// Upstream walks the left bound in about ten steps, so this is both cheaper and
/// systematically shorter than the true length on a curve. Routing costs use it, so
/// the sampling pattern is reproduced exactly rather than improved.
///
/// Upstream: `approximatedLength2d`, `impl/Lanelet.h`
pub fn approximated_length_2d(line: &[Point2]) -> f64 {
    if line.len() < 2 {
        return 0.0;
    }
    let step = (line.len() / 10).max(1);
    let mut total = 0.0;
    let mut index = 0usize;
    while index + step < line.len() {
        let next = index + step;
        total += distance_2d_point_point(line[index], line[next]);
        if next + step >= line.len() {
            total += distance_2d_point_point(line[next], line[line.len() - 1]);
        }
        index = next;
    }
    if index == 0 {
        total += distance_2d_point_point(line[0], line[line.len() - 1]);
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: [Point2; 3] = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];

    #[test]
    fn lengths_are_summed_per_segment() {
        assert_eq!(length_2d(&L), 20.0);
        assert_eq!(length_2d(&[[0.0, 0.0]]), 0.0);
        assert_eq!(length_3d(&[[0.0, 0.0, 0.0], [0.0, 0.0, 3.0]]), 3.0);
    }

    #[test]
    fn interpolation_lands_on_vertices_exactly_and_clamps_past_the_end() {
        assert_eq!(interpolated_point_at_distance_2d(&L, 5.0), Some([5.0, 0.0]));
        assert_eq!(interpolated_point_at_distance_2d(&L, 10.0), Some([10.0, 0.0]));
        assert_eq!(interpolated_point_at_distance_2d(&L, 15.0), Some([10.0, 5.0]));
        assert_eq!(interpolated_point_at_distance_2d(&L, 99.0), Some([10.0, 10.0]));
        // A negative distance measures from the far end.
        assert_eq!(interpolated_point_at_distance_2d(&L, -5.0), Some([10.0, 5.0]));
    }

    #[test]
    fn nearest_returns_an_existing_vertex() {
        assert_eq!(nearest_index_at_distance_2d(&L, 1.0), Some(0));
        assert_eq!(nearest_index_at_distance_2d(&L, 9.0), Some(1));
        assert_eq!(nearest_index_at_distance_2d(&L, 19.0), Some(2));
    }

    #[test]
    fn arc_coordinates_are_positive_to_the_left() {
        let straight = [[0.0, 0.0], [10.0, 0.0]];
        let left = to_arc_coordinates(&straight, [4.0, 2.0]);
        assert_eq!(left.length, 4.0);
        assert_eq!(left.distance, 2.0);

        let right = to_arc_coordinates(&straight, [4.0, -2.0]);
        assert_eq!(right.distance, -2.0);
    }

    #[test]
    fn arc_coordinates_round_trip_on_a_straight_line() {
        let straight = [[0.0, 0.0], [10.0, 0.0]];
        let arc = ArcCoordinates {
            length: 4.0,
            distance: 2.0,
        };
        let point = from_arc_coordinates(&straight, arc).unwrap();
        assert!((point[0] - 4.0).abs() < 1e-12 && (point[1] - 2.0).abs() < 1e-12);
        assert!(from_arc_coordinates(&[[0.0, 0.0]], arc).is_err());
    }

    #[test]
    fn curvature_is_infinite_for_a_degenerate_triple_and_signed_by_turn() {
        // A unit circle's curvature is 1.
        let k = curvature_2d([1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]);
        assert!((k - 1.0).abs() < 1e-12, "{k}");
        assert!(signed_curvature_2d([1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]) > 0.0, "left turn");
        assert!(signed_curvature_2d([-1.0, 0.0], [0.0, 1.0], [1.0, 0.0]) < 0.0, "right turn");
        // Collinear but distinct points have zero curvature; only a *coincident*
        // pair makes the product vanish and the result infinite.
        assert_eq!(curvature_2d([0.0, 0.0], [1.0, 0.0], [2.0, 0.0]), 0.0);
        assert!(curvature_2d([0.0, 0.0], [0.0, 0.0], [2.0, 0.0]).is_infinite());
    }

    #[test]
    fn three_dimensional_projection_finds_the_closest_pair() {
        let a = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]];
        let b = [[5.0, 3.0, 4.0], [5.0, 3.0, 8.0]];
        let (pa, pb) = projected_point_3d(&a, &b).unwrap();
        assert!((pa[0] - 5.0).abs() < 1e-9, "{pa:?}");
        assert_eq!(pb, [5.0, 3.0, 4.0]);
        assert!((distance_3d(pa, pb) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn polyline_crossings_are_reported_per_segment_pair() {
        let a = [[0.0, 0.0], [10.0, 0.0]];
        let b = [[5.0, -5.0], [5.0, 5.0]];
        assert!(polylines_intersect_2d(&a, &b));
        assert_eq!(polyline_intersections_2d(&a, &b), [[5.0, 0.0]]);
        assert!(!polylines_intersect_2d(&a, &[[0.0, 1.0], [10.0, 1.0]]));

        // A collinear overlap reports both of its ends, so a line intersected with
        // itself gives back its own vertices.
        let corner = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
        assert_eq!(
            polyline_intersections_2d(&corner, &corner),
            [[0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [1.0, 0.0], [1.0, 0.0], [1.0, 1.0]]
        );
        assert_eq!(
            polyline_intersections_2d(&[[0.0, 0.0], [2.0, 0.0]], &[[1.0, 0.0], [3.0, 0.0]]),
            [[1.0, 0.0], [2.0, 0.0]]
        );

        // A zero-length segment is a point, and intersects whatever passes
        // through it.
        let degenerate = [[0.0, 0.0], [0.0, 0.0]];
        assert_eq!(polyline_intersections_2d(&degenerate, &degenerate), [[0.0, 0.0]]);
        assert!(polyline_intersections_2d(&degenerate, &[[1.0, 1.0], [2.0, 2.0]]).is_empty());
    }

    #[test]
    fn ring_area_follows_the_clockwise_convention() {
        let clockwise = [[0.0, 0.0], [0.0, 4.0], [4.0, 4.0], [4.0, 0.0]];
        assert_eq!(ring_area(&clockwise), 16.0);
        let counter_clockwise = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        assert_eq!(ring_area(&counter_clockwise), -16.0);
    }

    #[test]
    fn approximated_length_samples_rather_than_sums() {
        // With ten points or fewer the step is 1, so no sampling happens and the
        // result is the exact length.
        let corner = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        assert!((approximated_length_2d(&corner) - 20.0).abs() < 1e-9);

        // With more points the sampling shows: a densely sampled quarter circle of
        // radius 10 comes out slightly short of its true length. These are the
        // reference implementation's own numbers.
        let count = 30;
        let arc: Vec<Point2> = (0..count)
            .map(|index| {
                let angle = index as f64 * std::f64::consts::FRAC_PI_2 / (count - 1) as f64;
                [10.0 * angle.cos(), 10.0 * angle.sin()]
            })
            .collect();
        let approximate = approximated_length_2d(&arc);
        assert!((approximate - 15.691_348_766_645_977).abs() < 1e-12, "{approximate}");
        assert!((length_2d(&arc) - 15.706_043_112_158_007).abs() < 1e-12);
    }
}
