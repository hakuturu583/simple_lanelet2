//! What an edge costs to traverse.
//!
//! Two models ship: distance and travel time. A graph is built with *several* of
//! them at once and carries a parallel edge per model, which is why nearly every
//! query takes a `routing_cost_id`.
//!
//! Upstream: `lanelet2_routing/src/RoutingCost.cpp`

use ll2_core::geometry::lanelet as llgeom;
use ll2_core::lanelet::Lanelet;
use ll2_traffic_rules::TrafficRules;

/// A cost model.
pub trait RoutingCost: Send + Sync {
    /// The cost of driving from one lanelet into the next.
    fn cost_succeeding(&self, rules: &TrafficRules, from: &Lanelet, to: &Lanelet) -> f64;

    /// The cost of changing lane across a run of lanelets.
    ///
    /// Returning infinity means "not routable", which downgrades the edge from a
    /// lane change to mere adjacency.
    fn cost_lane_change(&self, rules: &TrafficRules, from: &[Lanelet], to: &[Lanelet]) -> f64;

    fn name(&self) -> &'static str;
}

/// Cost measured in metres.
pub struct RoutingCostDistance {
    lane_change_cost: f64,
    min_lane_change_length: f64,
}

impl RoutingCostDistance {
    pub fn new(lane_change_cost: f64, min_lane_change_length: f64) -> Self {
        RoutingCostDistance {
            lane_change_cost,
            min_lane_change_length,
        }
    }
}

impl RoutingCost for RoutingCostDistance {
    fn cost_succeeding(&self, _rules: &TrafficRules, from: &Lanelet, to: &Lanelet) -> f64 {
        (llgeom::approximated_length_2d(from) + llgeom::approximated_length_2d(to)) / 2.0
    }

    fn cost_lane_change(&self, _rules: &TrafficRules, from: &[Lanelet], _to: &[Lanelet]) -> f64 {
        if self.min_lane_change_length <= 0.0 {
            return self.lane_change_cost;
        }
        let available: f64 = from.iter().map(llgeom::approximated_length_2d).sum();
        if available >= self.min_lane_change_length {
            self.lane_change_cost
        } else {
            f64::INFINITY
        }
    }

    fn name(&self) -> &'static str {
        "RoutingCostDistance"
    }
}

/// Cost measured in seconds.
pub struct RoutingCostTravelTime {
    lane_change_cost: f64,
    min_lane_change_time: f64,
}

impl RoutingCostTravelTime {
    pub fn new(lane_change_cost: f64, min_lane_change_time: f64) -> Self {
        RoutingCostTravelTime {
            lane_change_cost,
            min_lane_change_time,
        }
    }

    /// Travel time along a lanelet.
    ///
    /// A speed limit of zero is the "unknown" sentinel, and upstream substitutes
    /// the largest representable speed rather than dividing by zero — so an
    /// unknown limit costs essentially no time at all.
    fn travel_time(&self, rules: &TrafficRules, lanelet: &Lanelet) -> f64 {
        let limit = rules.speed_limit(lanelet).speed;
        let speed = if limit == 0.0 { f64::MAX } else { limit };
        llgeom::approximated_length_2d(lanelet) / speed
    }
}

impl RoutingCost for RoutingCostTravelTime {
    fn cost_succeeding(&self, rules: &TrafficRules, from: &Lanelet, to: &Lanelet) -> f64 {
        (self.travel_time(rules, from) + self.travel_time(rules, to)) / 2.0
    }

    fn cost_lane_change(&self, rules: &TrafficRules, from: &[Lanelet], _to: &[Lanelet]) -> f64 {
        if self.min_lane_change_time <= 0.0 {
            return self.lane_change_cost;
        }
        let available: f64 = from.iter().map(|l| self.travel_time(rules, l)).sum();
        if available >= self.min_lane_change_time {
            self.lane_change_cost
        } else {
            f64::INFINITY
        }
    }

    fn name(&self) -> &'static str {
        "RoutingCostTravelTime"
    }
}

/// The models a graph uses when the caller does not choose: distance first, then
/// travel time. So cost id 0 is distance and cost id 1 is travel time.
pub fn default_costs() -> Vec<Box<dyn RoutingCost>> {
    vec![
        Box::new(RoutingCostDistance::new(10.0, 0.0)),
        Box::new(RoutingCostTravelTime::new(5.0, 0.0)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll2_core::attribute::{Attribute, AttributeMap};
    use ll2_core::linestring::LineString;
    use ll2_core::point::Point;

    fn lanelet(length: f64, subtype: &str) -> Lanelet {
        let line = |y: f64| {
            LineString::new(
                0,
                vec![
                    Point::new(0, 0.0, y, 0.0, AttributeMap::new()),
                    Point::new(0, length, y, 0.0, AttributeMap::new()),
                ],
                AttributeMap::new(),
            )
        };
        let mut attributes = AttributeMap::new();
        attributes.insert("subtype".into(), Attribute::new(subtype));
        Lanelet::new(1, line(1.0), line(-1.0), attributes)
    }

    fn rules() -> TrafficRules {
        TrafficRules::create("de", "vehicle").unwrap()
    }

    #[test]
    fn distance_cost_is_the_mean_of_the_two_lengths() {
        let cost = RoutingCostDistance::new(10.0, 0.0);
        let value = cost.cost_succeeding(&rules(), &lanelet(10.0, "road"), &lanelet(20.0, "road"));
        assert!((value - 15.0).abs() < 1e-9, "{value}");
        assert_eq!(cost.cost_lane_change(&rules(), &[lanelet(1.0, "road")], &[]), 10.0);
    }

    #[test]
    fn a_lane_change_that_is_too_short_costs_infinity() {
        let cost = RoutingCostDistance::new(10.0, 50.0);
        assert!(cost.cost_lane_change(&rules(), &[lanelet(10.0, "road")], &[]).is_infinite());
        assert_eq!(cost.cost_lane_change(&rules(), &[lanelet(60.0, "road")], &[]), 10.0);
    }

    #[test]
    fn travel_time_uses_the_speed_limit() {
        let cost = RoutingCostTravelTime::new(5.0, 0.0);
        // 100 m of urban road at 50 km/h is 7.2 s; the mean of two such is the same.
        let value = cost.cost_succeeding(&rules(), &lanelet(100.0, "road"), &lanelet(100.0, "road"));
        assert!((value - 7.2).abs() < 1e-9, "{value}");
    }

    #[test]
    fn an_unknown_speed_limit_costs_no_time() {
        let cost = RoutingCostTravelTime::new(5.0, 0.0);
        // `parking` has no limit, so the zero sentinel becomes infinite speed.
        let value = cost.cost_succeeding(&rules(), &lanelet(100.0, "parking"), &lanelet(100.0, "parking"));
        assert!(value < 1e-300, "{value}");
    }

    #[test]
    fn the_default_models_are_distance_then_travel_time() {
        let costs = default_costs();
        assert_eq!(costs.len(), 2);
        assert_eq!(costs[0].name(), "RoutingCostDistance");
        assert_eq!(costs[1].name(), "RoutingCostTravelTime");
    }
}
