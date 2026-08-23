//! The geometry a 3D viewer needs and a 2D one does not.
//!
//! [`ll2_viz::Scene`] flattens everything to x and y, because an SVG page and a
//! `<canvas>` have nowhere to put the third coordinate. A Spatial3D view does, and a
//! Lanelet2 map carries it — `ele` on every node — so nothing here reads a `Scene`.
//! It works from the map's own points and keeps their elevation.
//!
//! Two things then have to be built that the 2D side gets for free:
//!
//! * A surface. A browser fills a closed path; a renderer of triangles wants
//!   triangles, so rings are ear-clipped and lanelets are stitched into ribbons.
//! * Samples along a centerline in 3D, for the driving-direction arrows.

use ll2_core::geometry::linestring::distance_3d;

/// A point in map coordinates: metres, x east, y north, z up. The same alias
/// `ll2-core`'s geometry uses, so its helpers apply unchanged.
pub type Point3 = [f64; 3];

/// Map coordinates as Rerun wants them.
///
/// The cast to `f32` is safe here in a way it would not be for raw UTM: the loader
/// subtracts the origin's own easting and northing, so a map straddles zero rather
/// than sitting at six- and seven-digit values. Within a hundred kilometres of the
/// origin `f32` still resolves a centimetre, which is finer than any Lanelet2 map is
/// surveyed.
pub fn to_f32(point: Point3) -> [f32; 3] {
    [point[0] as f32, point[1] as f32, point[2] as f32]
}

/// A point raised onto its layer and cast for Rerun — [`to_f32`] with the layer lift
/// added, which is what every position logged from a map goes through.
pub fn lifted(point: Point3, lift: f64) -> [f32; 3] {
    to_f32([point[0], point[1], point[2] + lift])
}

/// Twice the signed area of a ring, projected onto the ground plane.
///
/// Positive is counter-clockwise seen from above. Only the sign is used, so the
/// factor of two is left in.
fn signed_area(ring: &[Point3]) -> f64 {
    let mut total = 0.0;
    for (index, current) in ring.iter().enumerate() {
        let next = ring[(index + 1) % ring.len()];
        total += current[0] * next[1] - next[0] * current[1];
    }
    total
}

/// The z component of `(b - a) × (c - b)`, on the ground plane. Positive means the
/// three points turn left.
fn turn(a: Point3, b: Point3, c: Point3) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Whether `point` lies within triangle `a b c`, edges included.
fn inside_triangle(point: Point3, a: Point3, b: Point3, c: Point3) -> bool {
    turn(a, b, point) >= 0.0 && turn(b, c, point) >= 0.0 && turn(c, a, point) >= 0.0
}

/// Triangulates a closed ring by ear clipping, in the ground plane.
///
/// The returned indices refer to `ring` as it was passed in — the winding is
/// normalised internally, not by reordering the caller's vertices — so a caller can
/// keep one vertex buffer and one colour per vertex.
///
/// Ear clipping is quadratic, which is the right trade here: map polygons are
/// crosswalks, hatched markings and parking bays, a few tens of vertices each, and
/// the alternative is a dependency the size of the rest of this crate.
///
/// A ring that crosses itself has no ear to clip. That is a broken polygon rather
/// than an unsupported one, so the leftovers are fanned instead: wrong in detail,
/// but bounded, visible, and — unlike looping until an ear appears — finite.
pub fn triangulate_ring(ring: &[Point3]) -> Vec<[u32; 3]> {
    let count = ring.len();
    if count < 3 {
        return Vec::new();
    }

    // Ear clipping needs a known winding. Rings arrive either way round, so the
    // clockwise ones are walked backwards.
    let mut remaining: Vec<u32> = (0..count as u32).collect();
    if signed_area(ring) < 0.0 {
        remaining.reverse();
    }

    let mut triangles = Vec::with_capacity(count - 2);
    // Clipping an ear only changes the convexity of its two neighbours, so the next
    // search starts there rather than at the beginning of the ring.
    let mut start = 0usize;

    while remaining.len() > 3 {
        let length = remaining.len();
        let mut ear = None;
        for offset in 0..length {
            let index = (start + offset) % length;
            let previous = remaining[(index + length - 1) % length];
            let current = remaining[index];
            let next = remaining[(index + 1) % length];
            if is_ear(ring, &remaining, previous, current, next) {
                ear = Some((index, [previous, current, next]));
                break;
            }
        }
        let Some((index, triangle)) = ear else {
            break;
        };
        triangles.push(triangle);
        remaining.remove(index);
        start = index.saturating_sub(1);
    }

    if remaining.len() == 3 {
        triangles.push([remaining[0], remaining[1], remaining[2]]);
    } else {
        for index in 1..remaining.len().saturating_sub(1) {
            triangles.push([remaining[0], remaining[index], remaining[index + 1]]);
        }
    }
    triangles
}

/// Whether `current` is an ear: convex, and with no other vertex of the ring inside
/// the triangle it would cut off.
fn is_ear(ring: &[Point3], remaining: &[u32], previous: u32, current: u32, next: u32) -> bool {
    let (a, b, c) = (
        ring[previous as usize],
        ring[current as usize],
        ring[next as usize],
    );
    // Reflex vertices are not ears. Collinear ones are allowed through: a straight
    // road split into segments has plenty, and the zero-area triangle they produce
    // is harmless where refusing to clip them would strand the whole ring.
    if turn(a, b, c) < 0.0 {
        return false;
    }
    !remaining.iter().any(|&index| {
        index != previous
            && index != current
            && index != next
            && inside_triangle(ring[index as usize], a, b, c)
    })
}

/// Stitches two boundaries into a strip of quads, each split into two triangles.
///
/// Returns positions — left and right interleaved, so vertex `2i` is on the left
/// boundary and `2i + 1` opposite it — and the triangle indices over them.
///
/// A lanelet's bounds rarely have the same number of points, so both are resampled
/// to a shared parametrisation before they are paired. Ear-clipping the outline
/// would also work and would need no resampling, but a lanelet is a ribbon by
/// construction: pairing its two sides cannot produce the slivers that clipping a
/// long thin ring does, and it is linear rather than quadratic.
pub fn ribbon(left: &[Point3], right: &[Point3]) -> (Vec<Point3>, Vec<[u32; 3]>) {
    let samples = left.len().max(right.len()).max(2);
    let (left, right) = (resample(left, samples), resample(right, samples));
    if left.len() < 2 || right.len() < 2 {
        return (Vec::new(), Vec::new());
    }

    let mut positions = Vec::with_capacity(samples * 2);
    for (left_point, right_point) in left.iter().zip(&right) {
        positions.push(*left_point);
        positions.push(*right_point);
    }

    let mut triangles = Vec::with_capacity((samples - 1) * 2);
    for index in 0..samples - 1 {
        let (left_here, right_here) = (index as u32 * 2, index as u32 * 2 + 1);
        let (left_next, right_next) = (left_here + 2, right_here + 2);
        // Counter-clockwise seen from above, for a lanelet whose left bound really
        // is on its left. Rerun draws both faces by default, so this decides how the
        // surface is lit rather than whether it is visible at all.
        triangles.push([left_here, right_here, right_next]);
        triangles.push([left_here, right_next, left_next]);
    }
    (positions, triangles)
}

/// `count` points spread evenly along a polyline by arc length, ends included.
///
/// Returns nothing for a polyline with no length: two identical points describe no
/// direction, and every caller here would go on to build a degenerate shape from it.
fn resample(points: &[Point3], count: usize) -> Vec<Point3> {
    if count < 2 {
        return Vec::new();
    }
    let Some((lengths, total)) = arc_lengths(points) else {
        return Vec::new();
    };

    let mut samples = Vec::with_capacity(count);
    let step = total / (count as f64 - 1.0);
    let mut segment = 0usize;
    let mut travelled = 0.0;

    for index in 0..count {
        let target = (index as f64 * step).min(total);
        while segment + 1 < lengths.len() && travelled + lengths[segment] < target {
            travelled += lengths[segment];
            segment += 1;
        }
        let length = lengths[segment];
        let ratio = if length > 0.0 {
            ((target - travelled) / length).clamp(0.0, 1.0)
        } else {
            0.0
        };
        samples.push(lerp(points[segment], points[segment + 1], ratio));
    }
    samples
}

/// Positions and unit headings at roughly `spacing` metres along a polyline.
///
/// The 3D counterpart of the sampler behind the SVG renderer's arrowheads, and it
/// keeps that one's promise: a polyline with any length at all yields at least one
/// sample, placed at its middle. A lanelet with no arrow reads as one with no
/// direction, not as one that was too short to mark.
pub fn sample_along(points: &[Point3], spacing: f64) -> Vec<(Point3, Point3)> {
    let spacing = if spacing.is_finite() && spacing > 0.1 {
        spacing
    } else {
        25.0
    };
    let Some((lengths, total)) = arc_lengths(points) else {
        return Vec::new();
    };

    let count = (total / spacing).floor().max(1.0) as usize;
    let step = total / (count as f64 + 1.0);
    let mut samples = Vec::with_capacity(count);
    let mut target = step;
    let mut travelled = 0.0;

    for (index, length) in lengths.iter().enumerate() {
        if *length <= 0.0 {
            continue;
        }
        let (start, end) = (points[index], points[index + 1]);
        let heading = [
            (end[0] - start[0]) / length,
            (end[1] - start[1]) / length,
            (end[2] - start[2]) / length,
        ];
        while target <= travelled + length && samples.len() < count {
            let ratio = (target - travelled) / length;
            samples.push((lerp(start, end, ratio), heading));
            target += step;
        }
        travelled += length;
    }
    samples
}

/// Each segment's length and their total, or `None` for a polyline with no length.
///
/// Two identical points describe no direction, and every caller here would go on to
/// build a degenerate shape from one — so the rule lives in one place rather than at
/// the top of each sampler.
fn arc_lengths(points: &[Point3]) -> Option<(Vec<f64>, f64)> {
    if points.len() < 2 {
        return None;
    }
    let lengths: Vec<f64> = points
        .windows(2)
        .map(|pair| distance_3d(pair[0], pair[1]))
        .collect();
    let total: f64 = lengths.iter().sum();
    (total > 0.0 && total.is_finite()).then_some((lengths, total))
}

fn lerp(from: Point3, to: Point3, ratio: f64) -> Point3 {
    [
        from[0] + (to[0] - from[0]) * ratio,
        from[1] + (to[1] - from[1]) * ratio,
        from[2] + (to[2] - from[2]) * ratio,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every vertex used, no vertex used twice within a triangle, and the areas add
    /// up to the ring's own — the three things that make a triangulation one.
    fn covers(ring: &[Point3], triangles: &[[u32; 3]]) -> bool {
        if triangles.len() != ring.len() - 2 {
            return false;
        }
        let area: f64 = triangles
            .iter()
            .map(|[a, b, c]| {
                turn(ring[*a as usize], ring[*b as usize], ring[*c as usize]).abs() / 2.0
            })
            .sum();
        (area - signed_area(ring).abs() / 2.0).abs() < 1e-6
    }

    #[test]
    fn a_square_becomes_two_triangles_either_way_round() {
        let square = [
            [0.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [4.0, 4.0, 0.0],
            [0.0, 4.0, 0.0],
        ];
        let triangles = triangulate_ring(&square);
        assert!(covers(&square, &triangles));

        let mut clockwise = square;
        clockwise.reverse();
        assert!(covers(&clockwise, &triangulate_ring(&clockwise)));
    }

    /// The case a fan gets wrong: the fifth vertex is pushed into the middle, so a
    /// fan from vertex zero would cover ground that is outside the polygon.
    #[test]
    fn a_concave_ring_is_triangulated_without_covering_the_notch() {
        let arrow = [
            [0.0, 0.0, 0.0],
            [6.0, 0.0, 0.0],
            [6.0, 6.0, 0.0],
            [3.0, 2.0, 0.0],
            [0.0, 6.0, 0.0],
        ];
        let triangles = triangulate_ring(&arrow);
        assert!(covers(&arrow, &triangles));
    }

    #[test]
    fn elevation_is_carried_through_untouched() {
        let ring = [
            [0.0, 0.0, 10.0],
            [4.0, 0.0, 11.0],
            [4.0, 4.0, 12.0],
            [0.0, 4.0, 13.0],
        ];
        for [a, b, c] in triangulate_ring(&ring) {
            for index in [a, b, c] {
                assert!(ring[index as usize][2] >= 10.0);
            }
        }
    }

    #[test]
    fn a_degenerate_ring_yields_no_triangles_rather_than_panicking() {
        assert!(triangulate_ring(&[]).is_empty());
        assert!(triangulate_ring(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]).is_empty());
    }

    /// A bow tie has no ear anywhere. The clipper must still return, and must still
    /// return something drawable.
    #[test]
    fn a_self_intersecting_ring_terminates() {
        let bow_tie = [
            [0.0, 0.0, 0.0],
            [4.0, 4.0, 0.0],
            [4.0, 0.0, 0.0],
            [0.0, 4.0, 0.0],
        ];
        let triangles = triangulate_ring(&bow_tie);
        assert_eq!(triangles.len(), 2);
        for [a, b, c] in triangles {
            for index in [a, b, c] {
                assert!((index as usize) < bow_tie.len());
            }
        }
    }

    #[test]
    fn bounds_of_unequal_length_still_pair_up_into_a_ribbon() {
        let left = [[0.0, 3.0, 0.0], [10.0, 3.0, 0.0]];
        let right = [
            [0.0, 0.0, 0.0],
            [2.0, 0.0, 0.0],
            [7.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
        ];
        let (positions, triangles) = ribbon(&left, &right);
        assert_eq!(positions.len(), 8);
        assert_eq!(triangles.len(), 6);
        // Left and right interleave, and the resampling stays on each boundary.
        for pair in positions.chunks(2) {
            assert_eq!(pair[0][1], 3.0);
            assert_eq!(pair[1][1], 0.0);
        }
        // Every index addresses a vertex that exists.
        for [a, b, c] in triangles {
            for index in [a, b, c] {
                assert!((index as usize) < positions.len());
            }
        }
    }

    #[test]
    fn a_ribbon_over_a_slope_keeps_the_slope() {
        let left = [[0.0, 3.0, 0.0], [10.0, 3.0, 5.0]];
        let right = [[0.0, 0.0, 0.0], [10.0, 0.0, 5.0]];
        let (positions, _) = ribbon(&left, &right);
        assert_eq!(positions.first().unwrap()[2], 0.0);
        assert_eq!(positions.last().unwrap()[2], 5.0);
    }

    #[test]
    fn a_lanelet_with_no_length_produces_no_ribbon() {
        let degenerate = [[1.0, 1.0, 0.0], [1.0, 1.0, 0.0]];
        let (positions, triangles) = ribbon(&degenerate, &degenerate);
        assert!(positions.is_empty());
        assert!(triangles.is_empty());
    }

    #[test]
    fn resampling_hits_both_ends_and_spreads_the_middle_by_length() {
        let line = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [9.0, 0.0, 0.0]];
        let samples = resample(&line, 5);
        assert_eq!(samples.len(), 5);
        for (index, sample) in samples.iter().enumerate() {
            assert!((sample[0] - index as f64 * 2.25).abs() < 1e-9, "{sample:?}");
        }
    }

    #[test]
    fn a_short_centerline_still_gets_one_arrow_at_its_middle() {
        let samples = sample_along(&[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]], 25.0);
        assert_eq!(samples.len(), 1);
        assert!((samples[0].0[0] - 1.0).abs() < 1e-9);
        assert_eq!(samples[0].1, [1.0, 0.0, 0.0]);
    }

    #[test]
    fn arrow_headings_climb_with_the_road() {
        let samples = sample_along(&[[0.0, 0.0, 0.0], [0.0, 0.0, 4.0]], 25.0);
        assert_eq!(samples[0].1, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn sampling_a_line_with_no_length_yields_nothing() {
        assert!(sample_along(&[[1.0, 1.0, 1.0], [1.0, 1.0, 1.0]], 5.0).is_empty());
        assert!(sample_along(&[[1.0, 1.0, 1.0]], 5.0).is_empty());
    }
}
