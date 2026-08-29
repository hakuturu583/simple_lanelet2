//! The polyline arithmetic every renderer built on this crate needs.
//!
//! Small, and deliberately here rather than in each renderer. Where a driving
//! direction arrow goes is a decision about what a map looks like — a lanelet with
//! no arrow reads as one with no direction rather than as one too short to mark —
//! and this crate is where those decisions live. The SVG writer, the demo's canvas
//! and the Rerun renderer all place their arrows from [`sample_along`], so a map's
//! arrows are in the same places whichever one you are looking at.
//!
//! Everything is three-dimensional and measured in three dimensions: a ramp is
//! longer than the shadow it casts, and gets the arrows its own length earns.

pub use ll2_core::geometry::linestring::Point3;

use ll2_core::geometry::linestring::distance_3d;

/// Positions and unit headings at roughly `spacing` metres along a polyline.
///
/// Always yields at least one sample — a five-metre lanelet still gets an arrow,
/// placed at its middle — and nothing at all for a polyline with no length, since
/// two identical points describe no direction and every caller would go on to build
/// a degenerate shape from one.
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
            samples.push((lerp(start, end, (target - travelled) / length), heading));
            target += step;
        }
        travelled += length;
    }
    samples
}

/// Each segment's length and their total, or `None` for a polyline with no length.
///
/// The rule about degenerate polylines, in one place rather than at the top of each
/// sampler that would otherwise divide by zero.
pub fn arc_lengths(points: &[Point3]) -> Option<(Vec<f64>, f64)> {
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

pub fn lerp(from: Point3, to: Point3, ratio: f64) -> Point3 {
    [
        from[0] + (to[0] - from[0]) * ratio,
        from[1] + (to[1] - from[1]) * ratio,
        from[2] + (to[2] - from[2]) * ratio,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_line_still_gets_exactly_one_sample_at_its_middle() {
        let samples = sample_along(&[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]], 25.0);
        assert_eq!(samples.len(), 1);
        assert!((samples[0].0[0] - 1.0).abs() < 1e-9);
        assert_eq!(samples[0].1, [1.0, 0.0, 0.0]);
    }

    /// A road that climbs is longer than its shadow, and the spacing is measured
    /// along the road — so it earns the samples its length deserves.
    #[test]
    fn sampling_measures_the_slope_rather_than_its_shadow() {
        let flat = sample_along(&[[0.0, 0.0, 0.0], [30.0, 0.0, 0.0]], 20.0);
        let steep = sample_along(&[[0.0, 0.0, 0.0], [30.0, 0.0, 40.0]], 20.0);
        assert_eq!(flat.len(), 1);
        assert_eq!(steep.len(), 2, "50 m of road, not 30");
        assert_eq!(steep[0].1, [0.6, 0.0, 0.8], "a unit heading up the slope");
    }

    #[test]
    fn headings_climb_with_the_road() {
        let samples = sample_along(&[[0.0, 0.0, 0.0], [0.0, 0.0, 4.0]], 25.0);
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].1, [0.0, 0.0, 1.0]);
    }

    #[test]
    fn a_line_with_no_length_yields_nothing_rather_than_dividing_by_zero() {
        assert!(sample_along(&[[1.0, 1.0, 1.0], [1.0, 1.0, 1.0]], 5.0).is_empty());
        assert!(sample_along(&[[1.0, 1.0, 1.0]], 5.0).is_empty());
        assert!(arc_lengths(&[[0.0; 3]]).is_none());
        assert!(arc_lengths(&[[0.0; 3], [0.0; 3]]).is_none());
    }

    #[test]
    fn lerp_hits_both_ends_and_the_middle() {
        let (from, to) = ([0.0, 0.0, 0.0], [10.0, 20.0, 30.0]);
        assert_eq!(lerp(from, to, 0.0), from);
        assert_eq!(lerp(from, to, 1.0), to);
        assert_eq!(lerp(from, to, 0.5), [5.0, 10.0, 15.0]);
    }
}
