//! The routing graph: which lanelets lead to which, and at what cost.
//!
//! Two things about the construction are easy to miss and shape everything else.
//!
//! A lanelet that may be driven both ways becomes **two vertices**, one per
//! direction, and the two are marked as conflicting with each other — because a
//! vehicle cannot be going both ways at once.
//!
//! And the graph carries a **parallel edge per cost model**. A lane change that is
//! long enough to be a lane change under the distance model may be too short under
//! the travel-time model, in which case the same pair of lanelets is `Left` for one
//! cost id and merely `AdjacentLeft` for another. That is why nearly every query
//! takes a cost id.
//!
//! Upstream: `lanelet2_routing/src/RoutingGraphBuilder.cpp`, `RoutingGraph.cpp`

pub mod cost;
pub mod graph;
pub mod route;

use std::collections::HashMap;
use std::sync::Arc;

use ll2_core::geometry::lanelet as llgeom;
use ll2_core::id::Id;
use ll2_core::lanelet::Lanelet;
use ll2_core::map::{LaneletMap, Primitive};
use ll2_traffic_rules::TrafficRules;

pub use cost::{RoutingCost, RoutingCostDistance, RoutingCostTravelTime, default_costs};
pub use graph::{Edge, Graph, VertexIndex};
pub use route::Route;

/// How one lanelet relates to another. A bitmask, so sets of relations can be
/// filtered in one comparison.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RelationType(pub u32);

impl RelationType {
    pub const NONE: RelationType = RelationType(0);
    pub const SUCCESSOR: RelationType = RelationType(1);
    pub const LEFT: RelationType = RelationType(2);
    pub const RIGHT: RelationType = RelationType(4);
    pub const ADJACENT_LEFT: RelationType = RelationType(8);
    pub const ADJACENT_RIGHT: RelationType = RelationType(16);
    pub const CONFLICTING: RelationType = RelationType(32);
    pub const AREA: RelationType = RelationType(64);

    pub fn all() -> RelationType {
        RelationType(127)
    }

    pub fn contains(self, other: RelationType) -> bool {
        self.0 & other.0 != 0
    }

    pub fn union(self, other: RelationType) -> RelationType {
        RelationType(self.0 | other.0)
    }

    /// The name Python sees.
    pub fn name(self) -> &'static str {
        match self {
            RelationType::SUCCESSOR => "Successor",
            RelationType::LEFT => "Left",
            RelationType::RIGHT => "Right",
            RelationType::ADJACENT_LEFT => "AdjacentLeft",
            RelationType::ADJACENT_RIGHT => "AdjacentRight",
            RelationType::CONFLICTING => "Conflicting",
            RelationType::AREA => "Area",
            _ => "None",
        }
    }
}

/// A lanelet reached from somewhere, and how.
#[derive(Clone)]
pub struct LaneletRelation {
    pub lanelet: Lanelet,
    pub relation: RelationType,
    /// What the edge cost under the queried cost model.
    pub cost: f64,
}

/// The routing graph over a map's passable lanelets.
pub struct RoutingGraph {
    graph: Graph,
    rules: Arc<TrafficRules>,
    /// One vertex per (lanelet, direction).
    lanelets: Vec<Lanelet>,
    cost_count: usize,
    passable: Arc<LaneletMap>,
}

impl RoutingGraph {
    /// Builds the graph.
    ///
    /// Only lanelets the participant may actually use become vertices; the rest of
    /// the map is invisible to routing.
    pub fn build(
        map: &LaneletMap,
        rules: Arc<TrafficRules>,
        costs: &[Box<dyn RoutingCost>],
    ) -> RoutingGraph {
        let mut lanelets: Vec<Lanelet> = Vec::new();
        let mut both_ways: Vec<Id> = Vec::new();

        for primitive in map.lanelets.all() {
            let Primitive::Lanelet(lanelet) = primitive else {
                continue;
            };
            if !rules.can_pass(&lanelet) {
                continue;
            }
            lanelets.push(lanelet.clone());
            // A lanelet drivable both ways gets a second vertex for the other
            // direction.
            if rules.can_pass(&lanelet.invert()) {
                lanelets.push(lanelet.invert());
                both_ways.push(lanelet.id());
            }
        }

        let passable = LaneletMap::new_submap();
        for lanelet in &lanelets {
            if !lanelet.is_inverted() {
                passable.add(Primitive::Lanelet(lanelet.clone()));
            }
        }

        let mut graph = Graph::new(lanelets.len(), costs.len());
        let index_of: HashMap<(usize, bool), usize> = lanelets
            .iter()
            .enumerate()
            .map(|(index, lanelet)| ((lanelet.identity(), lanelet.is_inverted()), index))
            .collect();
        let find = |lanelet: &Lanelet| -> Option<usize> {
            index_of
                .get(&(lanelet.identity(), lanelet.is_inverted()))
                .copied()
        };

        // --- successors ----------------------------------------------------
        for (from, lanelet) in lanelets.iter().enumerate() {
            for (to, candidate) in lanelets.iter().enumerate() {
                if from == to || !llgeom::follows(lanelet, candidate) {
                    continue;
                }
                if !rules.can_pass_between(lanelet, candidate) {
                    continue;
                }
                for (cost_id, model) in costs.iter().enumerate() {
                    let value = model.cost_succeeding(&rules, lanelet, candidate);
                    graph.add_edge(from, to, RelationType::SUCCESSOR, cost_id, value);
                }
            }
        }

        // --- sideways ------------------------------------------------------
        for (from, lanelet) in lanelets.iter().enumerate() {
            for (to, candidate) in lanelets.iter().enumerate() {
                if from == to {
                    continue;
                }
                // `candidate` is to the left of `lanelet` when it sits on the far
                // side of `lanelet`'s left bound.
                let to_the_left = llgeom::left_of(candidate, lanelet);
                let to_the_right = llgeom::right_of(candidate, lanelet);
                if !to_the_left && !to_the_right {
                    continue;
                }
                let changeable = rules.can_change_lane(lanelet, candidate);
                for (cost_id, model) in costs.iter().enumerate() {
                    let (relation, value) = if changeable {
                        let change = model.cost_lane_change(
                            &rules,
                            &[lanelet.clone()],
                            &[candidate.clone()],
                        );
                        if change.is_finite() {
                            // A change that is affordable is a real lane change.
                            (
                                if to_the_left {
                                    RelationType::LEFT
                                } else {
                                    RelationType::RIGHT
                                },
                                change,
                            )
                        } else {
                            // Too short to change here: adjacency only.
                            (
                                if to_the_left {
                                    RelationType::ADJACENT_LEFT
                                } else {
                                    RelationType::ADJACENT_RIGHT
                                },
                                1.0,
                            )
                        }
                    } else {
                        (
                            if to_the_left {
                                RelationType::ADJACENT_LEFT
                            } else {
                                RelationType::ADJACENT_RIGHT
                            },
                            1.0,
                        )
                    };
                    graph.add_edge(from, to, relation, cost_id, value);
                }
            }
        }

        // --- conflicts -----------------------------------------------------
        for (from, lanelet) in lanelets.iter().enumerate() {
            for (to, candidate) in lanelets.iter().enumerate() {
                if from >= to {
                    continue;
                }
                let conflict = if lanelet.is_same_data(candidate) {
                    // The two directions of a bidirectional lanelet conflict with
                    // each other by construction.
                    both_ways.contains(&lanelet.id())
                } else {
                    llgeom::overlaps_2d(lanelet, candidate)
                };
                if !conflict {
                    continue;
                }
                for cost_id in 0..costs.len() {
                    graph.add_edge(from, to, RelationType::CONFLICTING, cost_id, 1.0);
                    graph.add_edge(to, from, RelationType::CONFLICTING, cost_id, 1.0);
                }
            }
        }

        let _ = find;
        RoutingGraph {
            graph,
            rules,
            lanelets,
            cost_count: costs.len(),
            passable,
        }
    }

    pub fn rules(&self) -> &TrafficRules {
        &self.rules
    }

    pub fn cost_count(&self) -> usize {
        self.cost_count
    }

    /// The lanelets that became vertices, as a submap.
    pub fn passable_submap(&self) -> Arc<LaneletMap> {
        self.passable.clone()
    }

    fn index_of(&self, lanelet: &Lanelet) -> Option<usize> {
        self.lanelets
            .iter()
            .position(|candidate| candidate.is_same_view(lanelet))
    }

    fn lanelet_at(&self, index: usize) -> Lanelet {
        self.lanelets[index].clone()
    }

    /// Neighbours reachable by one edge of any of the given relations.
    fn neighbours(
        &self,
        lanelet: &Lanelet,
        relations: RelationType,
        cost_id: usize,
    ) -> Vec<LaneletRelation> {
        let Some(index) = self.index_of(lanelet) else {
            return Vec::new();
        };
        self.graph
            .out_edges(index, relations, cost_id)
            .into_iter()
            .map(|edge| LaneletRelation {
                lanelet: self.lanelet_at(edge.to),
                relation: edge.relation,
                cost: edge.cost,
            })
            .collect()
    }

    fn predecessors(
        &self,
        lanelet: &Lanelet,
        relations: RelationType,
        cost_id: usize,
    ) -> Vec<LaneletRelation> {
        let Some(index) = self.index_of(lanelet) else {
            return Vec::new();
        };
        self.graph
            .in_edges(index, relations, cost_id)
            .into_iter()
            .map(|edge| LaneletRelation {
                lanelet: self.lanelet_at(edge.from),
                relation: edge.relation,
                cost: edge.cost,
            })
            .collect()
    }

    fn drivable(with_lane_changes: bool) -> RelationType {
        if with_lane_changes {
            RelationType::SUCCESSOR
                .union(RelationType::LEFT)
                .union(RelationType::RIGHT)
        } else {
            RelationType::SUCCESSOR
        }
    }

    pub fn following(&self, lanelet: &Lanelet, with_lane_changes: bool) -> Vec<Lanelet> {
        self.following_relations(lanelet, with_lane_changes)
            .into_iter()
            .map(|relation| relation.lanelet)
            .collect()
    }

    pub fn following_relations(
        &self,
        lanelet: &Lanelet,
        with_lane_changes: bool,
    ) -> Vec<LaneletRelation> {
        self.neighbours(lanelet, Self::drivable(with_lane_changes), 0)
    }

    pub fn previous(&self, lanelet: &Lanelet, with_lane_changes: bool) -> Vec<Lanelet> {
        self.previous_relations(lanelet, with_lane_changes)
            .into_iter()
            .map(|relation| relation.lanelet)
            .collect()
    }

    pub fn previous_relations(
        &self,
        lanelet: &Lanelet,
        with_lane_changes: bool,
    ) -> Vec<LaneletRelation> {
        self.predecessors(lanelet, Self::drivable(with_lane_changes), 0)
    }

    fn single(&self, lanelet: &Lanelet, relation: RelationType, cost_id: usize) -> Option<Lanelet> {
        self.neighbours(lanelet, relation, cost_id)
            .into_iter()
            .next()
            .map(|found| found.lanelet)
    }

    pub fn left(&self, lanelet: &Lanelet, cost_id: usize) -> Option<Lanelet> {
        self.single(lanelet, RelationType::LEFT, cost_id)
    }

    pub fn right(&self, lanelet: &Lanelet, cost_id: usize) -> Option<Lanelet> {
        self.single(lanelet, RelationType::RIGHT, cost_id)
    }

    pub fn adjacent_left(&self, lanelet: &Lanelet, cost_id: usize) -> Option<Lanelet> {
        self.single(lanelet, RelationType::ADJACENT_LEFT, cost_id)
    }

    pub fn adjacent_right(&self, lanelet: &Lanelet, cost_id: usize) -> Option<Lanelet> {
        self.single(lanelet, RelationType::ADJACENT_RIGHT, cost_id)
    }

    /// Every lanelet reachable by repeatedly stepping in one direction.
    fn chain(
        &self,
        lanelet: &Lanelet,
        step: impl Fn(&RoutingGraph, &Lanelet, usize) -> Option<Lanelet>,
        cost_id: usize,
    ) -> Vec<Lanelet> {
        let mut out = Vec::new();
        let mut current = lanelet.clone();
        while let Some(next) = step(self, &current, cost_id) {
            if out.iter().any(|seen: &Lanelet| seen.is_same_view(&next)) {
                break;
            }
            out.push(next.clone());
            current = next;
        }
        out
    }

    pub fn lefts(&self, lanelet: &Lanelet, cost_id: usize) -> Vec<Lanelet> {
        self.chain(lanelet, RoutingGraph::left, cost_id)
    }

    pub fn rights(&self, lanelet: &Lanelet, cost_id: usize) -> Vec<Lanelet> {
        self.chain(lanelet, RoutingGraph::right, cost_id)
    }

    pub fn adjacent_lefts(&self, lanelet: &Lanelet, cost_id: usize) -> Vec<Lanelet> {
        self.chain(lanelet, RoutingGraph::adjacent_left, cost_id)
    }

    pub fn adjacent_rights(&self, lanelet: &Lanelet, cost_id: usize) -> Vec<Lanelet> {
        self.chain(lanelet, RoutingGraph::adjacent_right, cost_id)
    }

    /// Everything alongside, left to right, with the lanelet itself in the middle.
    pub fn besides(&self, lanelet: &Lanelet, cost_id: usize) -> Vec<Lanelet> {
        let mut out = self.lefts(lanelet, cost_id);
        out.reverse();
        out.push(lanelet.clone());
        out.extend(self.rights(lanelet, cost_id));
        out
    }

    pub fn left_relations(&self, lanelet: &Lanelet, cost_id: usize) -> Vec<LaneletRelation> {
        self.side_relations(lanelet, cost_id, true)
    }

    pub fn right_relations(&self, lanelet: &Lanelet, cost_id: usize) -> Vec<LaneletRelation> {
        self.side_relations(lanelet, cost_id, false)
    }

    /// Alternating runs of lane-changeable and merely-adjacent neighbours.
    fn side_relations(
        &self,
        lanelet: &Lanelet,
        cost_id: usize,
        left: bool,
    ) -> Vec<LaneletRelation> {
        let (changeable, adjacent) = if left {
            (RelationType::LEFT, RelationType::ADJACENT_LEFT)
        } else {
            (RelationType::RIGHT, RelationType::ADJACENT_RIGHT)
        };
        let mut out: Vec<LaneletRelation> = Vec::new();
        let mut current = lanelet.clone();
        loop {
            let run = self.chain(
                &current,
                move |graph, lanelet, cost_id| graph.single(lanelet, changeable, cost_id),
                cost_id,
            );
            let stepped = !run.is_empty();
            if let Some(last) = run.last() {
                current = last.clone();
            }
            out.extend(run.into_iter().map(|lanelet| LaneletRelation {
                lanelet,
                relation: changeable,
                cost: 0.0,
            }));

            let adjacents = self.chain(
                &current,
                move |graph, lanelet, cost_id| graph.single(lanelet, adjacent, cost_id),
                cost_id,
            );
            let stepped_adjacent = !adjacents.is_empty();
            if let Some(last) = adjacents.last() {
                current = last.clone();
            }
            out.extend(adjacents.into_iter().map(|lanelet| LaneletRelation {
                lanelet,
                relation: adjacent,
                cost: 1.0,
            }));

            if !stepped && !stepped_adjacent {
                break;
            }
        }
        out
    }

    pub fn conflicting(&self, lanelet: &Lanelet) -> Vec<Lanelet> {
        self.neighbours(lanelet, RelationType::CONFLICTING, 0)
            .into_iter()
            .map(|found| found.lanelet)
            .collect()
    }

    /// How `to` relates to `from`, if at all.
    pub fn routing_relation(
        &self,
        from: &Lanelet,
        to: &Lanelet,
        include_conflicting: bool,
    ) -> Option<RelationType> {
        let relations = if include_conflicting {
            RelationType::all()
        } else {
            RelationType::all()
        };
        // Upstream's "without conflicting" filter is a no-op -- it computes
        // allRelations() | ~Conflicting, which matches everything -- so conflicting
        // relations are reported either way. Repaired below.
        let found = self.neighbours(from, relations, 0);
        let matching = found
            .into_iter()
            .find(|candidate| candidate.lanelet.is_same_view(to))?;
        if !include_conflicting
            && matching.relation == RelationType::CONFLICTING
            && !ll2_core::compat::without_conflicting_is_noop()
        {
            return None;
        }
        Some(matching.relation)
    }

    /// The cheapest route between two lanelets, or nothing if none exists.
    pub fn shortest_path(
        &self,
        from: &Lanelet,
        to: &Lanelet,
        cost_id: usize,
        with_lane_changes: bool,
    ) -> Option<Vec<Lanelet>> {
        let (start, goal) = (self.index_of(from)?, self.index_of(to)?);
        let relations = Self::drivable(with_lane_changes);
        let path = self.graph.shortest_path(start, goal, relations, cost_id)?;
        Some(
            path.into_iter()
                .map(|index| self.lanelet_at(index))
                .collect(),
        )
    }

    /// A route through intermediate lanelets, one leg at a time.
    ///
    /// Legs are optimised independently, so the whole is not globally optimal —
    /// which is what upstream does too.
    pub fn shortest_path_via(
        &self,
        from: &Lanelet,
        via: &[Lanelet],
        to: &Lanelet,
        cost_id: usize,
        with_lane_changes: bool,
    ) -> Option<Vec<Lanelet>> {
        let mut waypoints = vec![from.clone()];
        waypoints.extend(via.iter().cloned());
        waypoints.push(to.clone());

        let mut path: Vec<Lanelet> = Vec::new();
        for leg in waypoints.windows(2) {
            let mut segment = self.shortest_path(&leg[0], &leg[1], cost_id, with_lane_changes)?;
            if !path.is_empty() {
                // Drop the joint, which the previous leg already contributed.
                segment.remove(0);
            }
            path.extend(segment);
        }
        Some(path)
    }

    /// Everything reachable within a cost budget, the start included.
    pub fn reachable_set(
        &self,
        from: &Lanelet,
        max_cost: f64,
        cost_id: usize,
        allow_lane_changes: bool,
    ) -> Vec<Lanelet> {
        let Some(start) = self.index_of(from) else {
            return Vec::new();
        };
        let relations = Self::drivable(allow_lane_changes);
        self.graph
            .reachable(start, max_cost, relations, cost_id, false)
            .into_iter()
            .map(|index| self.lanelet_at(index))
            .collect()
    }

    /// Everything from which this lanelet can be reached within a cost budget.
    pub fn reachable_set_towards(
        &self,
        to: &Lanelet,
        max_cost: f64,
        cost_id: usize,
        allow_lane_changes: bool,
    ) -> Vec<Lanelet> {
        let Some(goal) = self.index_of(to) else {
            return Vec::new();
        };
        let relations = Self::drivable(allow_lane_changes);
        self.graph
            .reachable(goal, max_cost, relations, cost_id, true)
            .into_iter()
            .map(|index| self.lanelet_at(index))
            .collect()
    }

    /// Every maximal path whose cost stays within the budget.
    pub fn possible_paths(
        &self,
        from: &Lanelet,
        max_cost: Option<f64>,
        min_elements: Option<usize>,
        cost_id: usize,
        allow_lane_changes: bool,
        include_shorter: bool,
    ) -> Vec<Vec<Lanelet>> {
        let Some(start) = self.index_of(from) else {
            return Vec::new();
        };
        let relations = Self::drivable(allow_lane_changes);
        self.graph
            .possible_paths(
                start,
                max_cost,
                min_elements,
                relations,
                cost_id,
                include_shorter,
                false,
            )
            .into_iter()
            .map(|path| {
                path.into_iter()
                    .map(|index| self.lanelet_at(index))
                    .collect()
            })
            .collect()
    }

    pub fn possible_paths_towards(
        &self,
        to: &Lanelet,
        max_cost: Option<f64>,
        min_elements: Option<usize>,
        cost_id: usize,
        allow_lane_changes: bool,
        include_shorter: bool,
    ) -> Vec<Vec<Lanelet>> {
        let Some(goal) = self.index_of(to) else {
            return Vec::new();
        };
        let relations = Self::drivable(allow_lane_changes);
        self.graph
            .possible_paths(
                goal,
                max_cost,
                min_elements,
                relations,
                cost_id,
                include_shorter,
                true,
            )
            .into_iter()
            .map(|path| {
                path.into_iter()
                    .rev()
                    .map(|index| self.lanelet_at(index))
                    .collect()
            })
            .collect()
    }

    /// Every edge, for inspection and for testing the builder itself.
    pub fn edges(&self) -> Vec<(Id, bool, Id, bool, RelationType, usize, f64)> {
        self.graph
            .all_edges()
            .into_iter()
            .map(|edge| {
                let from = &self.lanelets[edge.from];
                let to = &self.lanelets[edge.to];
                (
                    from.id(),
                    from.is_inverted(),
                    to.id(),
                    to.is_inverted(),
                    edge.relation,
                    edge.cost_id,
                    edge.cost,
                )
            })
            .collect()
    }

    /// Builds a route between two lanelets.
    ///
    /// The route is the shortest path plus every lanelet a driver could legally
    /// drift onto and still reach the goal — so it is a corridor, not a line.
    pub fn route(
        &self,
        from: &Lanelet,
        to: &Lanelet,
        cost_id: usize,
        with_lane_changes: bool,
    ) -> Option<Route> {
        let path = self.shortest_path(from, to, cost_id, with_lane_changes)?;
        Some(Route::build(self, path, to, cost_id))
    }

    pub fn route_via(
        &self,
        from: &Lanelet,
        via: &[Lanelet],
        to: &Lanelet,
        cost_id: usize,
        with_lane_changes: bool,
    ) -> Option<Route> {
        let path = self.shortest_path_via(from, via, to, cost_id, with_lane_changes)?;
        Some(Route::build(self, path, to, cost_id))
    }

    /// Whether a lanelet is a vertex of this graph at all.
    pub fn contains(&self, lanelet: &Lanelet) -> bool {
        self.index_of(lanelet).is_some()
    }

    /// Every vertex, in construction order.
    pub fn vertices(&self) -> &[Lanelet] {
        &self.lanelets
    }

    /// Problems with the graph's structure, as `checkValidity` reports them.
    pub fn check_validity(&self) -> Vec<String> {
        let mut errors = Vec::new();
        for lanelet in &self.lanelets {
            for (side, changeable, adjacent) in [
                ("left", RelationType::LEFT, RelationType::ADJACENT_LEFT),
                ("right", RelationType::RIGHT, RelationType::ADJACENT_RIGHT),
            ] {
                let changeables = self.neighbours(lanelet, changeable, 0);
                let adjacents = self.neighbours(lanelet, adjacent, 0);
                if !changeables.is_empty() && !adjacents.is_empty() {
                    errors.push(format!(
                        "Lanelet {} has both a {side} and an adjacent {side} neighbour",
                        lanelet.id()
                    ));
                }
                if changeables.len() > 1 {
                    errors.push(format!(
                        "More than one neighboring lanelet to {} with this relation: {:?}",
                        lanelet.id(),
                        changeables
                            .iter()
                            .map(|r| r.lanelet.id())
                            .collect::<Vec<_>>()
                    ));
                }
            }
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll2_core::attribute::{Attribute, AttributeMap};
    use ll2_core::linestring::LineString;
    use ll2_core::point::Point;

    /// Builds a straight chain of lanelets sharing their end points, so that
    /// `follows` holds between consecutive ones.
    fn chain_map(count: usize) -> (Arc<LaneletMap>, Vec<Lanelet>) {
        let map = LaneletMap::new_map();
        let mut left_points = Vec::new();
        let mut right_points = Vec::new();
        for index in 0..=count {
            let x = index as f64 * 10.0;
            left_points.push(Point::new(0, x, 1.0, 0.0, AttributeMap::new()));
            right_points.push(Point::new(0, x, -1.0, 0.0, AttributeMap::new()));
        }
        let mut lanelets = Vec::new();
        for index in 0..count {
            let mut attributes = AttributeMap::new();
            attributes.insert("subtype".into(), Attribute::new("road"));
            let lanelet = Lanelet::new(
                index as Id + 1,
                LineString::new(
                    0,
                    vec![left_points[index].clone(), left_points[index + 1].clone()],
                    AttributeMap::new(),
                ),
                LineString::new(
                    0,
                    vec![right_points[index].clone(), right_points[index + 1].clone()],
                    AttributeMap::new(),
                ),
                attributes,
            );
            map.add(Primitive::Lanelet(lanelet.clone()));
            lanelets.push(lanelet);
        }
        (map, lanelets)
    }

    fn build(map: &LaneletMap) -> RoutingGraph {
        let rules = Arc::new(TrafficRules::create("de", "vehicle").unwrap());
        RoutingGraph::build(map, rules, &default_costs())
    }

    #[test]
    fn consecutive_lanelets_become_successors() {
        let (map, lanelets) = chain_map(3);
        let graph = build(&map);
        let following = graph.following(&lanelets[0], false);
        assert_eq!(following.len(), 1);
        assert_eq!(following[0].id(), lanelets[1].id());
        assert!(graph.following(&lanelets[2], false).is_empty());
        assert_eq!(
            graph.previous(&lanelets[1], false)[0].id(),
            lanelets[0].id()
        );
    }

    #[test]
    fn the_shortest_path_runs_the_length_of_the_chain() {
        let (map, lanelets) = chain_map(4);
        let graph = build(&map);
        let path = graph
            .shortest_path(&lanelets[0], &lanelets[3], 0, false)
            .unwrap();
        assert_eq!(
            path.iter().map(|l| l.id()).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
        // Backwards there is no path at all.
        assert!(
            graph
                .shortest_path(&lanelets[3], &lanelets[0], 0, false)
                .is_none()
        );
    }

    #[test]
    fn a_one_way_road_yields_one_vertex_per_lanelet() {
        let (map, lanelets) = chain_map(2);
        let graph = build(&map);
        // Only the forward direction is passable, so the inverted view is not a
        // vertex and has no neighbours.
        assert!(graph.following(&lanelets[0].invert(), false).is_empty());
    }

    #[test]
    fn a_two_way_lanelet_gets_a_vertex_per_direction_and_they_conflict() {
        let (map, lanelets) = chain_map(1);
        lanelets[0]
            .attributes()
            .write()
            .insert("one_way".into(), Attribute::new("no"));
        let graph = build(&map);
        let conflicting = graph.conflicting(&lanelets[0]);
        assert_eq!(conflicting.len(), 1);
        assert!(conflicting[0].is_inverted());
    }

    #[test]
    fn reachability_respects_the_budget() {
        let (map, lanelets) = chain_map(4);
        let graph = build(&map);
        // Each step costs the mean of two ten-metre lanelets.
        assert_eq!(graph.reachable_set(&lanelets[0], 0.0, 0, false).len(), 1);
        assert_eq!(graph.reachable_set(&lanelets[0], 25.0, 0, false).len(), 3);
        assert_eq!(graph.reachable_set(&lanelets[0], 1000.0, 0, false).len(), 4);
    }

    #[test]
    fn possible_paths_reach_the_end_of_the_chain() {
        let (map, lanelets) = chain_map(3);
        let graph = build(&map);
        // The cost limit is a target: nothing here costs 1000, so nothing
        // qualifies unless shorter paths are asked for.
        assert!(
            graph
                .possible_paths(&lanelets[0], Some(1000.0), None, 0, false, false)
                .is_empty()
        );
        let paths = graph.possible_paths(&lanelets[0], Some(1000.0), None, 0, false, true);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].len(), 3);
    }

    #[test]
    fn a_route_via_a_waypoint_passes_through_it() {
        let (map, lanelets) = chain_map(4);
        let graph = build(&map);
        let path = graph
            .shortest_path_via(&lanelets[0], &[lanelets[2].clone()], &lanelets[3], 0, false)
            .unwrap();
        assert_eq!(
            path.iter().map(|l| l.id()).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
    }

    #[test]
    fn every_cost_model_gets_its_own_edge() {
        let (map, _) = chain_map(2);
        let graph = build(&map);
        let successors: Vec<_> = graph
            .edges()
            .into_iter()
            .filter(|edge| edge.4 == RelationType::SUCCESSOR)
            .collect();
        assert_eq!(successors.len(), 2, "one edge per cost model");
        assert_eq!(successors[0].5, 0);
        assert_eq!(successors[1].5, 1);
    }
}
