//! Offsetting a polyline sideways, which is harder than it looks.
//!
//! Shifting every vertex along its bisector is only correct while the line turns away
//! from the offset. Where it turns towards it, neighbouring shifted segments cross and
//! the naive result loops back on itself. Upstream handles that in three steps, and
//! this is a port of them:
//!
//! 1. **Split into convex runs.** Walk the line shifting each vertex, and start a new
//!    run whenever a vertex is concave with respect to the offset direction. The runs
//!    are built so that consecutive ones are guaranteed to cross.
//! 2. **Index every segment** of every run.
//! 3. **Walk and cut.** Follow the first run; at each segment, look for the nearest
//!    self-intersection further along, jump to it, and continue from the segment it
//!    landed on. The loops between runs are skipped rather than removed afterwards.
//!
//! A sharp convex corner — more than a right angle — is bevelled instead of mitred,
//! since a mitre would shoot off to infinity as the angle closes.
//!
//! Upstream's segment lookups go through an R-tree. Here they are linear scans: the
//! answer is the same, the input is a lanelet boundary rather than a whole map, and a
//! deterministic order costs nothing.
//!
//! Upstream: `lanelet2_core/include/lanelet2_core/geometry/impl/LineString.h:169-620`

type Point = [f64; 2];
type Segment = (Point, Point);

const EPSILON: f64 = 1e-5;

fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}

fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1]]
}

fn scale(a: Point, factor: f64) -> Point {
    [a[0] * factor, a[1] * factor]
}

fn norm(a: Point) -> f64 {
    f64::hypot(a[0], a[1])
}

fn normalized(a: Point) -> Point {
    let length = norm(a);
    if length == 0.0 {
        [0.0, 0.0]
    } else {
        scale(a, 1.0 / length)
    }
}

fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

/// Rotates a quarter turn, giving the left-hand normal.
fn left_normal(a: Point) -> Point {
    [-a[1], a[0]]
}

/// The segments meeting at a vertex, as vectors. Either is zero at an end.
struct Vicinity {
    preceding: Point,
    following: Point,
}

fn vicinity(line: &[Point], index: usize) -> Vicinity {
    if index == 0 {
        return Vicinity {
            preceding: [0.0, 0.0],
            following: sub(line[index + 1], line[index]),
        };
    }
    if index + 1 == line.len() {
        return Vicinity {
            preceding: sub(line[index], line[index - 1]),
            following: [0.0, 0.0],
        };
    }
    Vicinity {
        preceding: sub(line[index], line[index - 1]),
        following: sub(line[index + 1], line[index]),
    }
}

/// Shifts a vertex square to one of its two segments rather than along the bisector.
fn shift_perpendicular(
    line: &[Point],
    index: usize,
    distance: f64,
    as_last: bool,
    pv: &Vicinity,
) -> Point {
    let perpendicular = normalized(if as_last { pv.preceding } else { pv.following });
    add(line[index], scale(left_normal(perpendicular), distance))
}

/// Shifts a vertex along the bisector of its two segments.
///
/// The offset is divided by the half-angle's sine so that the shifted vertex stays
/// the requested distance from *both* segments rather than from the corner. A
/// degenerate bisector — a doubled-back corner — collapses the shift to zero rather
/// than dividing by nearly nothing.
fn shift_lateral(line: &[Point], index: usize, offset: f64, pv: &Vicinity) -> Point {
    let (perpendicular, real_offset) = if index == 0 {
        (pv.following, offset)
    } else if index + 1 == line.len() {
        (pv.preceding, offset)
    } else {
        let bisector = add(normalized(pv.following), normalized(pv.preceding));
        let half_sine = norm(bisector) / 2.0;
        (
            bisector,
            if half_sine > EPSILON {
                offset / half_sine
            } else {
                0.0
            },
        )
    };
    add(
        line[index],
        scale(normalized(left_normal(perpendicular)), real_offset),
    )
}

/// Whether `c` lies left of the line through `a` and `b`.
fn is_left_of(a: Point, b: Point, c: Point) -> bool {
    let ab = sub(b, a);
    let ac = sub(c, a);
    ab[0] * ac[1] - ab[1] * ac[0] > 0.0
}

enum Convexity {
    Concave,
    Convex,
    ConvexSharp,
}

fn convexity(line: &[Point], index: usize, pv: &Vicinity, offset_positive: bool) -> Convexity {
    let convex = if index != 0 && index + 1 != line.len() {
        let left = is_left_of(line[index - 1], line[index], line[index + 1]);
        if offset_positive { !left } else { left }
    } else {
        true
    };
    if !convex {
        return Convexity::Concave;
    }
    if dot(pv.following, pv.preceding) < 0.0 {
        return Convexity::ConvexSharp;
    }
    Convexity::Convex
}

/// Bevels a corner sharper than a right angle, in place of an unbounded mitre.
fn shift_convex_sharp(line: &[Point], index: usize, distance: f64, pv: &Vicinity) -> Vec<Point> {
    let first = shift_perpendicular(line, index, distance, true, pv);
    let last = shift_perpendicular(line, index, distance, false, pv);
    let alpha = std::f64::consts::PI
        - (dot(pv.following, pv.preceding) / (norm(pv.preceding) * norm(pv.following))).acos();
    let overshoot = distance * (std::f64::consts::FRAC_PI_4 - alpha / 4.0).tan();
    vec![
        add(first, scale(normalized(pv.preceding), overshoot)),
        sub(last, scale(normalized(pv.following), overshoot)),
    ]
}

/// The shifted point or points for one vertex, and whether the run continues.
fn shift_point(line: &[Point], distance: f64, index: usize, pv: &Vicinity) -> (Vec<Point>, bool) {
    match convexity(line, index, pv, distance > 0.0) {
        Convexity::Concave => (
            vec![shift_perpendicular(line, index, distance, true, pv)],
            false,
        ),
        Convexity::Convex => (vec![shift_lateral(line, index, distance, pv)], true),
        Convexity::ConvexSharp => (shift_convex_sharp(line, index, distance, pv), true),
    }
}

/// Splits the offset line into runs that are convex throughout.
///
/// Consecutive runs are guaranteed to cross, which is what lets the join step below
/// walk from one to the next.
fn extract_convex(line: &[Point], distance: f64) -> Vec<Vec<Point>> {
    let mut runs = Vec::new();
    let mut current = 0usize;
    while current + 1 < line.len() {
        let pv = vicinity(line, current);
        let mut run = vec![shift_perpendicular(line, current, distance, false, &pv)];
        for i in current + 1..line.len() {
            current = i;
            let ipv = vicinity(line, i);
            let (points, still_convex) = shift_point(line, distance, i, &ipv);
            run.extend(points);
            if !still_convex {
                break;
            }
        }
        runs.push(run);
    }
    runs
}

/// Where two segments cross, if they properly do.
fn intersection(a: Segment, b: Segment) -> Option<Point> {
    let r = sub(a.1, a.0);
    let s = sub(b.1, b.0);
    let denominator = r[0] * s[1] - r[1] * s[0];
    if denominator == 0.0 {
        return None;
    }
    let qp = sub(b.0, a.0);
    let t = (qp[0] * s[1] - qp[1] * s[0]) / denominator;
    let u = (qp[0] * r[1] - qp[1] * r[0]) / denominator;
    if !(0.0..=1.0).contains(&t) || !(0.0..=1.0).contains(&u) {
        return None;
    }
    Some(add(a.0, scale(r, t)))
}

struct SelfIntersection {
    /// How far along the segment being walked the crossing lies.
    first_s: f64,
    /// The segment it crosses, and how far along that one.
    other_index: usize,
    other_s: f64,
    point: Point,
}

/// Crossings of one segment with later segments of the same flattened list.
///
/// Segments that merely share an endpoint with the one being walked are not
/// crossings; upstream excludes them by comparing endpoints, and so does this.
fn self_intersections_at(
    segments: &[Segment],
    index: usize,
    segment: Segment,
) -> Vec<SelfIntersection> {
    let mut found = Vec::new();
    for (other_index, &other) in segments.iter().enumerate() {
        if other_index <= index {
            continue;
        }
        if other.0 == segment.1 || other.1 == segment.0 || other.0 == segment.0 {
            continue;
        }
        if let Some(point) = intersection(segment, other) {
            found.push(SelfIntersection {
                first_s: norm(sub(point, segment.0)),
                other_index,
                other_s: norm(sub(point, other.0)),
                point,
            });
        }
    }
    found
}

/// The next point of the walk: the nearest crossing further along, or the segment end.
fn next_point(
    intersections: &[SelfIntersection],
    segment: Segment,
    last_s: f64,
    index: usize,
) -> (Point, usize, f64) {
    let lowest = intersections
        .iter()
        .filter(|crossing| crossing.first_s > last_s)
        .min_by(|a, b| a.first_s.total_cmp(&b.first_s));
    match lowest {
        Some(crossing) => (crossing.point, crossing.other_index, crossing.other_s),
        None => (segment.1, index + 1, 0.0),
    }
}

/// Joins the convex runs into one line, cutting the loops out at their crossings.
fn join_runs(runs: &[Vec<Point>]) -> Vec<Point> {
    // A single run needs no joining and is returned untouched. Skipping this would
    // append its last point twice, since the walk emits it and the tail push repeats
    // it -- which is exactly what happens, deliberately, when there *are* several runs.
    if runs.len() == 1 {
        return runs[0].clone();
    }
    // The runs flattened into one segment list, with a note of which run each came
    // from, so the walk can tell when it has jumped into a different run.
    let mut segments: Vec<Segment> = Vec::new();
    let mut owner: Vec<usize> = Vec::new();
    for (run_index, run) in runs.iter().enumerate() {
        for pair in run.windows(2) {
            segments.push((pair[0], pair[1]));
            owner.push(run_index);
        }
    }

    let mut out = vec![runs[0][0]];
    let mut last_s = 0.0;
    let mut index = 0usize;
    let mut run_index = 0usize;
    while run_index < runs.len() {
        let run = &runs[run_index];
        let mut jumped = false;
        for j in 0..run.len().saturating_sub(1) {
            let segment = (run[j], run[j + 1]);
            let crossings = self_intersections_at(&segments, index, segment);
            let (point, next_index, next_s) = next_point(&crossings, segment, last_s, index);
            out.push(point);
            index = next_index;
            if index >= segments.len() {
                run_index = runs.len();
                jumped = true;
                break;
            }
            last_s = next_s;
            if owner[index] != run_index {
                run_index = owner[index];
                jumped = true;
                break;
            }
        }
        if !jumped {
            break;
        }
    }
    out.push(*runs.last().unwrap().last().unwrap());
    out
}

/// Offsets a polyline sideways; positive is to the left.
///
/// Returns the line unchanged when it is too short to have a direction.
pub fn offset(line: &[Point], distance: f64) -> Vec<Point> {
    if line.len() < 2 {
        return line.to_vec();
    }
    let runs = extract_convex(line, distance);
    if runs.is_empty() {
        return line.to_vec();
    }
    join_runs(&runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point) -> bool {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9
    }

    #[test]
    fn a_straight_line_shifts_square_to_itself() {
        let line = [[0.0, 0.0], [10.0, 0.0]];
        let left = offset(&line, 1.0);
        assert_eq!(left.len(), 2);
        assert!(close(left[0], [0.0, 1.0]), "{:?}", left[0]);
        assert!(close(left[1], [10.0, 1.0]), "{:?}", left[1]);
        // Negative distance goes the other way.
        let right = offset(&line, -1.0);
        assert!(close(right[0], [0.0, -1.0]) && close(right[1], [10.0, -1.0]));
    }

    #[test]
    fn a_right_angle_is_mitred_on_its_outside() {
        // Turning left, so offsetting right puts the corner on the outside.
        let line = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        let outside = offset(&line, -1.0);
        // The mitre sits one unit out along both directions from the corner.
        assert!(
            outside.iter().any(|p| close(*p, [11.0, -1.0])),
            "expected a mitred corner, got {outside:?}"
        );
    }

    #[test]
    fn the_inside_of_a_corner_does_not_loop_back() {
        let line = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
        let inside = offset(&line, 1.0);
        // Every point must stay on the inside; a naive shift would overshoot past the
        // corner and come back, which shows up as x greater than the corner's.
        assert!(
            inside.iter().all(|p| p[0] <= 9.000001),
            "offset loops past the corner: {inside:?}"
        );
    }

    #[test]
    fn a_hairpin_is_bevelled_rather_than_mitred() {
        // Doubling back: a mitre here would run off to infinity.
        let line = [[0.0, 0.0], [10.0, 0.0], [0.0, 0.5]];
        let out = offset(&line, -1.0);
        assert!(
            out.iter().all(|p| p[0].is_finite() && p[0] < 100.0),
            "bevel escaped: {out:?}"
        );
    }

    #[test]
    fn a_degenerate_line_is_returned_unchanged() {
        assert_eq!(offset(&[], 1.0).len(), 0);
        assert_eq!(offset(&[[1.0, 2.0]], 1.0), vec![[1.0, 2.0]]);
    }
}
