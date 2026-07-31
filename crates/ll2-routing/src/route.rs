//! A route: the corridor a driver may use between two lanelets.
//!
//! Not just the shortest path. A route also contains every lanelet the driver
//! could legally drift onto and still reach the goal — so on a three-lane road it
//! is all three lanes, not the one the optimiser happened to pick.
//!
//! Lanes within the corridor are numbered from 1000 upwards, and a new number
//! starts wherever the corridor branches or merges rather than simply continuing.
//!
//! Upstream: `lanelet2_routing/src/RouteBuilder.cpp`, `Route.cpp`

use ll2_core::geometry::lanelet as llgeom;
use ll2_core::id::Id;
use ll2_core::lanelet::Lanelet;
use ll2_core::map::{LaneletMap, Primitive};

use crate::{LaneletRelation, RelationType, RoutingGraph};

/// The first lane number a route hands out.
const FIRST_LANE_ID: Id = 1000;

pub struct Route {
    shortest_path: Vec<Lanelet>,
    /// Every lanelet in the corridor, the shortest path included.
    lanelets: Vec<Lanelet>,
    lane_ids: Vec<Id>,
    cost_id: usize,
}

impl Route {
    /// Grows the corridor outwards from a path until nothing more can be added.
    pub fn build(graph: &RoutingGraph, path: Vec<Lanelet>, goal: &Lanelet, cost_id: usize) -> Route {
        let mut lanelets = path.clone();

        // Anything reachable sideways that can still get to the goal belongs to the
        // corridor. Repeat until it stops growing.
        loop {
            let mut added = false;
            for lanelet in lanelets.clone() {
                let sideways = graph
                    .left_relations(&lanelet, cost_id)
                    .into_iter()
                    .chain(graph.right_relations(&lanelet, cost_id))
                    .filter(|relation| {
                        relation.relation == RelationType::LEFT
                            || relation.relation == RelationType::RIGHT
                    });
                for relation in sideways {
                    if lanelets
                        .iter()
                        .any(|held| held.is_same_view(&relation.lanelet))
                    {
                        continue;
                    }
                    // Only keep it if the goal is still reachable from there --
                    // otherwise it is a dead end, not part of the route.
                    if graph
                        .shortest_path(&relation.lanelet, goal, cost_id, true)
                        .is_none()
                    {
                        continue;
                    }
                    lanelets.push(relation.lanelet);
                    added = true;
                }
            }
            if !added {
                break;
            }
        }

        let lane_ids = assign_lane_ids(graph, &lanelets, cost_id);
        Route {
            shortest_path: path,
            lanelets,
            lane_ids,
            cost_id,
        }
    }

    pub fn shortest_path(&self) -> &[Lanelet] {
        &self.shortest_path
    }

    pub fn size(&self) -> usize {
        self.lanelets.len()
    }

    pub fn lanelets(&self) -> &[Lanelet] {
        &self.lanelets
    }

    pub fn contains(&self, lanelet: &Lanelet) -> bool {
        self.lanelets.iter().any(|held| held.is_same_view(lanelet))
    }

    /// The length of the *shortest path*, not of the whole corridor.
    pub fn length_2d(&self) -> f64 {
        self.shortest_path.iter().map(llgeom::length_2d).sum()
    }

    pub fn num_lanes(&self) -> usize {
        let mut seen: Vec<Id> = self.lane_ids.clone();
        seen.sort_unstable();
        seen.dedup();
        seen.len()
    }

    /// The corridor as a submap.
    pub fn lanelet_submap(&self) -> std::sync::Arc<LaneletMap> {
        let submap = LaneletMap::new_submap();
        for lanelet in &self.lanelets {
            if !lanelet.is_inverted() {
                submap.add(Primitive::Lanelet(lanelet.clone()));
            }
        }
        submap
    }

    /// The remainder of the shortest path from `lanelet` onwards.
    pub fn remaining_shortest_path(&self, lanelet: &Lanelet) -> Vec<Lanelet> {
        match self
            .shortest_path
            .iter()
            .position(|held| held.is_same_view(lanelet))
        {
            None => Vec::new(),
            Some(index) => self.shortest_path[index..].to_vec(),
        }
    }

    /// The stretch from `lanelet` onwards that continues without a branch.
    pub fn remaining_lane(&self, graph: &RoutingGraph, lanelet: &Lanelet) -> Vec<Lanelet> {
        if !self.contains(lanelet) {
            return Vec::new();
        }
        let mut lane = vec![lanelet.clone()];
        let mut current = lanelet.clone();
        loop {
            let successors: Vec<Lanelet> = graph
                .following(&current, false)
                .into_iter()
                .filter(|candidate| self.contains(candidate))
                .collect();
            if successors.len() != 1 {
                break;
            }
            let next = successors.into_iter().next().expect("just counted one");
            let predecessors = graph
                .previous(&next, false)
                .into_iter()
                .filter(|candidate| self.contains(candidate))
                .count();
            if predecessors != 1 || lane.iter().any(|held| held.is_same_view(&next)) {
                break;
            }
            lane.push(next.clone());
            current = next;
        }
        lane
    }

    /// The whole lane `lanelet` sits on, backwards as well as forwards.
    pub fn full_lane(&self, graph: &RoutingGraph, lanelet: &Lanelet) -> Vec<Lanelet> {
        if !self.contains(lanelet) {
            return Vec::new();
        }
        let mut start = lanelet.clone();
        loop {
            let predecessors: Vec<Lanelet> = graph
                .previous(&start, false)
                .into_iter()
                .filter(|candidate| self.contains(candidate))
                .collect();
            if predecessors.len() != 1 {
                break;
            }
            let previous = predecessors.into_iter().next().expect("just counted one");
            let successors = graph
                .following(&previous, false)
                .into_iter()
                .filter(|candidate| self.contains(candidate))
                .count();
            if successors != 1 || previous.is_same_view(lanelet) {
                break;
            }
            start = previous;
        }
        self.remaining_lane(graph, &start)
    }

    /// Relations to other lanelets *within* the route.
    pub fn relations(
        &self,
        graph: &RoutingGraph,
        lanelet: &Lanelet,
        relations: RelationType,
        forwards: bool,
    ) -> Vec<LaneletRelation> {
        let found = if forwards {
            graph.following_relations(lanelet, relations.contains(RelationType::LEFT))
        } else {
            graph.previous_relations(lanelet, relations.contains(RelationType::LEFT))
        };
        found
            .into_iter()
            .filter(|relation| self.contains(&relation.lanelet))
            .collect()
    }

    /// Lanelets conflicting with this one *inside* the route.
    pub fn conflicting_in_route(&self, graph: &RoutingGraph, lanelet: &Lanelet) -> Vec<Lanelet> {
        graph
            .conflicting(lanelet)
            .into_iter()
            .filter(|candidate| self.contains(candidate))
            .collect()
    }

    /// Lanelets conflicting with this one anywhere in the map.
    pub fn conflicting_in_map(&self, graph: &RoutingGraph, lanelet: &Lanelet) -> Vec<Lanelet> {
        graph.conflicting(lanelet)
    }

    pub fn all_conflicting_in_map(&self, graph: &RoutingGraph) -> Vec<Lanelet> {
        self.lanelets
            .iter()
            .flat_map(|lanelet| graph.conflicting(lanelet))
            .collect()
    }

    pub fn cost_id(&self) -> usize {
        self.cost_id
    }

    /// Problems with the route, as `checkValidity` reports them.
    pub fn check_validity(&self) -> Vec<String> {
        if self.shortest_path.is_empty() {
            return vec!["Route has no shortest path".into()];
        }
        Vec::new()
    }
}

/// Numbers the lanes of a corridor.
///
/// A new number starts wherever the corridor branches or merges — that is, where a
/// lanelet does not simply continue its predecessor.
fn assign_lane_ids(graph: &RoutingGraph, lanelets: &[Lanelet], _cost_id: usize) -> Vec<Id> {
    let mut ids = vec![0 as Id; lanelets.len()];
    let mut next = FIRST_LANE_ID;
    let position = |lanelet: &Lanelet| {
        lanelets
            .iter()
            .position(|held| held.is_same_view(lanelet))
    };

    for index in 0..lanelets.len() {
        if ids[index] != 0 {
            continue;
        }
        let lanelet = &lanelets[index];
        let predecessors: Vec<Lanelet> = graph
            .previous(lanelet, false)
            .into_iter()
            .filter(|candidate| position(candidate).is_some())
            .collect();
        // Continue the predecessor's lane only when the two are each other's sole
        // neighbour in that direction.
        let inherited = if predecessors.len() == 1 {
            let previous = &predecessors[0];
            let successors = graph
                .following(previous, false)
                .into_iter()
                .filter(|candidate| position(candidate).is_some())
                .count();
            if successors == 1 {
                position(previous).map(|at| ids[at]).filter(|&id| id != 0)
            } else {
                None
            }
        } else {
            None
        };
        ids[index] = inherited.unwrap_or_else(|| {
            let id = next;
            next += 1;
            id
        });
    }
    ids
}
