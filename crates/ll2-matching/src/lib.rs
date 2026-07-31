//! Matching a positioned object to the lanelets it might be on.
//!
//! Two flavours. The deterministic one ranks candidates by plain Euclidean
//! distance. The probabilistic one ranks by squared Mahalanobis distance, combining
//! the object's position covariance with a von Mises concentration on its heading —
//! so an object pointing the wrong way down a lane scores badly even when it sits
//! exactly on the centerline.
//!
//! Both emit *both* orientations of every candidate lanelet, because which way the
//! object is travelling is exactly what is in question.
//! `remove_non_rule_compliant` is then what prunes the wrong-way ones.
//!
//! Upstream: `lanelet2_matching/src/{LaneletMatching,Utilities}.cpp`

use ll2_core::geometry::linestring::{self, Point2};
use ll2_core::lanelet::Lanelet;
use ll2_core::map::{LaneletMap, Primitive};
use ll2_traffic_rules::TrafficRules;

/// A position and heading on the ground plane.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pose2d {
    pub x: f64,
    pub y: f64,
    pub yaw: f64,
}

/// The 2x2 position covariance, held by its three distinct entries.
#[derive(Clone, Copy, Debug, Default)]
pub struct PositionCovariance2d {
    pub var_x: f64,
    pub var_y: f64,
    pub cov_xy: f64,
}

impl PositionCovariance2d {
    fn determinant(&self) -> f64 {
        self.var_x * self.var_y - self.cov_xy * self.cov_xy
    }

    fn is_zero(&self) -> bool {
        self.var_x == 0.0 && self.var_y == 0.0 && self.cov_xy == 0.0
    }

    /// `d^T * inverse * d`, without forming the inverse.
    fn quadratic_form(&self, d: [f64; 2]) -> f64 {
        let det = self.determinant();
        let inv = [
            self.var_y / det,
            -self.cov_xy / det,
            -self.cov_xy / det,
            self.var_x / det,
        ];
        d[0] * (inv[0] * d[0] + inv[1] * d[1]) + d[1] * (inv[2] * d[0] + inv[3] * d[1])
    }
}

/// An object to be matched: an id, a pose, and its footprint in map coordinates.
#[derive(Clone, Debug, Default)]
pub struct Object2d {
    pub object_id: i64,
    pub pose: Pose2d,
    /// In *map* coordinates, not relative to the pose.
    pub absolute_hull: Vec<Point2>,
}

/// An object that also knows how uncertain it is.
#[derive(Clone, Debug, Default)]
pub struct ObjectWithCovariance2d {
    pub object: Object2d,
    pub position_covariance: PositionCovariance2d,
    /// The concentration of the heading estimate; larger means more certain.
    pub von_mises_kappa: f64,
}

/// One candidate lanelet and how far the object is from it.
#[derive(Clone)]
pub struct LaneletMatch {
    pub lanelet: Lanelet,
    pub distance: f64,
    /// Only set by the probabilistic matcher.
    pub mahalanobis_dist_sq: f64,
}

/// The matching could not be performed.
#[derive(Debug)]
pub struct MatchingError(pub String);

/// Reduces an angle to `(-pi, pi]`.
///
/// The wrap point maps to `+pi`, not `-pi`, because upstream's positive
/// floating-point modulo returns the divisor rather than zero there.
fn normalize_angle(angle: f64) -> f64 {
    let two_pi = std::f64::consts::TAU;
    let shifted = angle + std::f64::consts::PI;
    let remainder = shifted % two_pi;
    let positive = if remainder > 0.0 { remainder } else { remainder + two_pi };
    positive - std::f64::consts::PI
}

fn centerline_2d(lanelet: &Lanelet) -> Vec<Point2> {
    lanelet
        .centerline()
        .points()
        .iter()
        .map(|point| {
            let [x, y, _] = point.xyz();
            [x, y]
        })
        .collect()
}

/// The squared Mahalanobis distance between an object and a lanelet.
///
/// Combines the lateral/longitudinal position error with the heading error, the
/// latter weighted by the von Mises concentration. The heading of the lane is taken
/// from a half-metre window either side of the object's projection, which smooths
/// out the vertex-to-vertex jitter of a polyline.
///
/// Upstream: `lanelet2_matching/src/Utilities.cpp:73`
pub fn mahalanobis_distance_sq(
    lanelet: &Lanelet,
    object: &ObjectWithCovariance2d,
) -> Result<f64, MatchingError> {
    if object.position_covariance.is_zero() {
        return Err(MatchingError("Covariance must not be zero".into()));
    }
    // The literal upstream uses is 10e-9, which is 1e-8 rather than 1e-9.
    if object.position_covariance.determinant().abs() < 1e-8 {
        return Err(MatchingError("Determinant must not be zero".into()));
    }

    let centerline = centerline_2d(lanelet);
    if centerline.len() < 2 {
        return Err(MatchingError("Centerline is degenerate".into()));
    }
    let position = [object.object.pose.x, object.object.pose.y];
    let arc = linestring::to_arc_coordinates(&centerline, position);

    let at = linestring::interpolated_point_at_distance_2d(&centerline, arc.length)
        .ok_or_else(|| MatchingError("Centerline is empty".into()))?;
    let before = linestring::interpolated_point_at_distance_2d(
        &centerline,
        (arc.length - 0.5).max(0.0),
    )
    .ok_or_else(|| MatchingError("Centerline is empty".into()))?;
    let after = linestring::interpolated_point_at_distance_2d(&centerline, arc.length + 0.5)
        .ok_or_else(|| MatchingError("Centerline is empty".into()))?;

    let heading = normalize_angle((after[1] - before[1]).atan2(after[0] - before[0]));
    let yaw_difference = normalize_angle(heading - normalize_angle(object.object.pose.yaw));
    let heading_term = yaw_difference * yaw_difference * object.von_mises_kappa * object.von_mises_kappa;

    let offset = [position[0] - at[0], position[1] - at[1]];
    Ok(heading_term + object.position_covariance.quadratic_form(offset))
}

/// The distance from an object to a lanelet: from its footprint if it has one,
/// otherwise from its position alone.
fn object_distance(lanelet: &Lanelet, object: &Object2d) -> f64 {
    use ll2_core::geometry::distance::distance_2d_point_to_lanelet;
    if object.absolute_hull.is_empty() {
        return distance_2d_point_to_lanelet([object.pose.x, object.pose.y], lanelet);
    }
    object
        .absolute_hull
        .iter()
        .map(|&corner| distance_2d_point_to_lanelet(corner, lanelet))
        .fold(f64::INFINITY, f64::min)
}

fn candidates(map: &LaneletMap, object: &Object2d, max_distance: f64) -> Vec<(f64, Lanelet)> {
    map.lanelets
        .all()
        .into_iter()
        .filter_map(|primitive| match primitive {
            Primitive::Lanelet(lanelet) => {
                let distance = object_distance(&lanelet, object);
                (distance <= max_distance).then_some((distance, lanelet))
            }
            _ => None,
        })
        .collect()
}

/// Candidate lanelets within `max_distance`, closest first.
///
/// Both orientations of each candidate are returned with the same distance, since
/// the direction of travel is what the caller is trying to resolve.
pub fn deterministic_matches(
    map: &LaneletMap,
    object: &Object2d,
    max_distance: f64,
) -> Vec<LaneletMatch> {
    let mut matches: Vec<LaneletMatch> = candidates(map, object, max_distance)
        .into_iter()
        .flat_map(|(distance, lanelet)| {
            let inverted = lanelet.invert();
            [
                LaneletMatch {
                    lanelet,
                    distance,
                    mahalanobis_dist_sq: 0.0,
                },
                LaneletMatch {
                    lanelet: inverted,
                    distance,
                    mahalanobis_dist_sq: 0.0,
                },
            ]
        })
        .collect();
    // Ties are broken by id and direction so the order does not depend on the
    // layer's iteration order.
    matches.sort_by(|a, b| {
        a.distance
            .total_cmp(&b.distance)
            .then(a.lanelet.id().cmp(&b.lanelet.id()))
            .then(a.lanelet.is_inverted().cmp(&b.lanelet.is_inverted()))
    });
    matches
}

/// The same candidates, ranked by squared Mahalanobis distance instead.
///
/// The candidate *set* still comes from a plain Euclidean prefilter; only the
/// ranking is probabilistic.
pub fn probabilistic_matches(
    map: &LaneletMap,
    object: &ObjectWithCovariance2d,
    max_distance: f64,
) -> Result<Vec<LaneletMatch>, MatchingError> {
    let mut matches = Vec::new();
    for (distance, lanelet) in candidates(map, &object.object, max_distance) {
        for oriented in [lanelet.clone(), lanelet.invert()] {
            let score = mahalanobis_distance_sq(&oriented, object)?;
            matches.push(LaneletMatch {
                lanelet: oriented,
                distance,
                mahalanobis_dist_sq: score,
            });
        }
    }
    matches.sort_by(|a, b| {
        a.mahalanobis_dist_sq
            .total_cmp(&b.mahalanobis_dist_sq)
            .then(a.lanelet.id().cmp(&b.lanelet.id()))
            .then(a.lanelet.is_inverted().cmp(&b.lanelet.is_inverted()))
    });
    Ok(matches)
}

/// Drops the matches the participant may not actually drive.
///
/// Since both orientations were emitted, this is what removes the wrong-way ones.
pub fn remove_non_rule_compliant(matches: Vec<LaneletMatch>, rules: &TrafficRules) -> Vec<LaneletMatch> {
    matches
        .into_iter()
        .filter(|candidate| rules.can_pass(&candidate.lanelet))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll2_core::attribute::{Attribute, AttributeMap};
    use ll2_core::linestring::LineString;
    use ll2_core::point::Point;

    fn line(points: &[(f64, f64)]) -> LineString {
        LineString::new(
            0,
            points
                .iter()
                .map(|&(x, y)| Point::new(0, x, y, 0.0, AttributeMap::new()))
                .collect(),
            AttributeMap::new(),
        )
    }

    fn road(id: i64, y: f64) -> Lanelet {
        let mut attributes = AttributeMap::new();
        attributes.insert("subtype".into(), Attribute::new("road"));
        Lanelet::new(
            id,
            line(&[(0.0, y + 1.0), (20.0, y + 1.0)]),
            line(&[(0.0, y - 1.0), (20.0, y - 1.0)]),
            attributes,
        )
    }

    fn map_with(lanelets: Vec<Lanelet>) -> std::sync::Arc<LaneletMap> {
        let map = LaneletMap::new_map();
        for lanelet in lanelets {
            map.add(Primitive::Lanelet(lanelet));
        }
        map
    }

    #[test]
    fn both_orientations_are_offered_for_every_candidate() {
        let map = map_with(vec![road(1, 0.0)]);
        let object = Object2d {
            object_id: 7,
            pose: Pose2d {
                x: 10.0,
                y: 0.0,
                yaw: 0.0,
            },
            absolute_hull: Vec::new(),
        };
        let matches = deterministic_matches(&map, &object, 1.0);
        assert_eq!(matches.len(), 2);
        assert!(matches.iter().any(|m| !m.lanelet.is_inverted()));
        assert!(matches.iter().any(|m| m.lanelet.is_inverted()));
        assert!(matches.iter().all(|m| m.distance == 0.0));
    }

    #[test]
    fn the_rule_filter_is_what_removes_the_wrong_way_match() {
        let map = map_with(vec![road(1, 0.0)]);
        let object = Object2d {
            pose: Pose2d { x: 10.0, y: 0.0, yaw: 0.0 },
            ..Object2d::default()
        };
        let rules = TrafficRules::create("de", "vehicle").unwrap();
        let kept = remove_non_rule_compliant(deterministic_matches(&map, &object, 1.0), &rules);
        assert_eq!(kept.len(), 1);
        assert!(!kept[0].lanelet.is_inverted());
    }

    #[test]
    fn distance_bounds_the_candidate_set() {
        let map = map_with(vec![road(1, 0.0), road(2, 10.0)]);
        let object = Object2d {
            pose: Pose2d { x: 10.0, y: 0.0, yaw: 0.0 },
            ..Object2d::default()
        };
        assert_eq!(deterministic_matches(&map, &object, 1.0).len(), 2, "one lanelet");
        assert_eq!(deterministic_matches(&map, &object, 20.0).len(), 4, "both");
    }

    #[test]
    fn a_wrong_way_heading_scores_worse_than_the_right_one() {
        let map = map_with(vec![road(1, 0.0)]);
        let object = ObjectWithCovariance2d {
            object: Object2d {
                pose: Pose2d { x: 10.0, y: 0.0, yaw: 0.0 },
                ..Object2d::default()
            },
            position_covariance: PositionCovariance2d {
                var_x: 1.0,
                var_y: 1.0,
                cov_xy: 0.0,
            },
            von_mises_kappa: 2.0,
        };
        let matches = probabilistic_matches(&map, &object, 1.0).unwrap();
        assert_eq!(matches.len(), 2);
        // The object points along +x, so the forward orientation must rank first.
        assert!(!matches[0].lanelet.is_inverted());
        assert!(matches[0].mahalanobis_dist_sq < matches[1].mahalanobis_dist_sq);
        assert!(matches[0].mahalanobis_dist_sq.abs() < 1e-9, "perfectly aligned");
    }

    #[test]
    fn a_degenerate_covariance_is_refused() {
        let map = map_with(vec![road(1, 0.0)]);
        let mut object = ObjectWithCovariance2d {
            object: Object2d {
                pose: Pose2d { x: 10.0, y: 0.0, yaw: 0.0 },
                ..Object2d::default()
            },
            position_covariance: PositionCovariance2d::default(),
            von_mises_kappa: 1.0,
        };
        assert!(probabilistic_matches(&map, &object, 1.0).is_err());

        // Non-zero but singular is also refused.
        object.position_covariance = PositionCovariance2d {
            var_x: 1.0,
            var_y: 1.0,
            cov_xy: 1.0,
        };
        assert!(probabilistic_matches(&map, &object, 1.0).is_err());
    }

    #[test]
    fn angles_wrap_to_a_half_open_turn() {
        assert!((normalize_angle(0.0)).abs() < 1e-12);
        assert!((normalize_angle(std::f64::consts::PI) - std::f64::consts::PI).abs() < 1e-12);
        assert!((normalize_angle(-std::f64::consts::PI) - std::f64::consts::PI).abs() < 1e-12);
        assert!((normalize_angle(std::f64::consts::TAU)).abs() < 1e-12);
    }
}
