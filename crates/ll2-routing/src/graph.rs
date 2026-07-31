//! A directed multigraph with insertion-ordered edges, and the searches over it.
//!
//! Hand-rolled rather than `petgraph`, for one reason: the *order* of a vertex's
//! out-edges is part of the observable API. `following()` and `besides()` return
//! lists, and callers compare them element by element. Boost's `adjacency_list`
//! iterates out-edges in insertion order; petgraph does not, and fighting that
//! would cost more than the hundred lines here.
//!
//! Upstream: `lanelet2_routing/include/lanelet2_routing/internal/Graph.h`,
//! `.../internal/ShortestPath.h`

use std::collections::BinaryHeap;

use crate::RelationType;

pub type VertexIndex = usize;

/// One directed edge, tagged with which cost model it belongs to.
#[derive(Clone, Copy, Debug)]
pub struct Edge {
    pub from: VertexIndex,
    pub to: VertexIndex,
    pub relation: RelationType,
    pub cost_id: usize,
    pub cost: f64,
}

pub struct Graph {
    edges: Vec<Edge>,
    out: Vec<Vec<usize>>,
    incoming: Vec<Vec<usize>>,
    cost_count: usize,
}

impl Graph {
    pub fn new(vertex_count: usize, cost_count: usize) -> Self {
        Graph {
            edges: Vec::new(),
            out: vec![Vec::new(); vertex_count],
            incoming: vec![Vec::new(); vertex_count],
            cost_count,
        }
    }

    pub fn vertex_count(&self) -> usize {
        self.out.len()
    }

    pub fn cost_count(&self) -> usize {
        self.cost_count
    }

    pub fn add_edge(
        &mut self,
        from: VertexIndex,
        to: VertexIndex,
        relation: RelationType,
        cost_id: usize,
        cost: f64,
    ) {
        let index = self.edges.len();
        self.edges.push(Edge {
            from,
            to,
            relation,
            cost_id,
            cost,
        });
        self.out[from].push(index);
        self.incoming[to].push(index);
    }

    pub fn all_edges(&self) -> Vec<Edge> {
        self.edges.clone()
    }

    /// Out-edges matching a relation mask and cost model, in insertion order.
    pub fn out_edges(&self, from: VertexIndex, relations: RelationType, cost_id: usize) -> Vec<Edge> {
        self.out
            .get(from)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&index| self.edges[index])
                    .filter(|edge| edge.cost_id == cost_id && relations.contains(edge.relation))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn in_edges(&self, to: VertexIndex, relations: RelationType, cost_id: usize) -> Vec<Edge> {
        self.incoming
            .get(to)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&index| self.edges[index])
                    .filter(|edge| edge.cost_id == cost_id && relations.contains(edge.relation))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// A heap entry ordered by cost, smallest first.
#[derive(PartialEq)]
struct Candidate {
    cost: f64,
    vertex: VertexIndex,
}

impl Eq for Candidate {}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reversed, since BinaryHeap is a max-heap. Ties go to the lower index so
        // the search is deterministic.
        other
            .cost
            .total_cmp(&self.cost)
            .then(other.vertex.cmp(&self.vertex))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Graph {
    /// Dijkstra, returning the vertices along the cheapest path inclusive of both
    /// ends, or nothing if the goal cannot be reached.
    pub fn shortest_path(
        &self,
        start: VertexIndex,
        goal: VertexIndex,
        relations: RelationType,
        cost_id: usize,
    ) -> Option<Vec<VertexIndex>> {
        let mut distance = vec![f64::INFINITY; self.vertex_count()];
        let mut predecessor = vec![usize::MAX; self.vertex_count()];
        let mut heap = BinaryHeap::new();

        distance[start] = 0.0;
        heap.push(Candidate {
            cost: 0.0,
            vertex: start,
        });

        while let Some(Candidate { cost, vertex }) = heap.pop() {
            if cost > distance[vertex] {
                continue;
            }
            if vertex == goal {
                break;
            }
            for edge in self.out_edges(vertex, relations, cost_id) {
                let next = cost + edge.cost;
                if next < distance[edge.to] {
                    distance[edge.to] = next;
                    predecessor[edge.to] = vertex;
                    heap.push(Candidate {
                        cost: next,
                        vertex: edge.to,
                    });
                }
            }
        }

        if !distance[goal].is_finite() {
            return None;
        }
        let mut path = vec![goal];
        let mut current = goal;
        while current != start {
            current = predecessor[current];
            if current == usize::MAX {
                return None;
            }
            path.push(current);
        }
        path.reverse();
        Some(path)
    }

    /// Every vertex within a cost budget, the start included at cost zero.
    ///
    /// The bound is inclusive, and a vertex beyond it does not have its own
    /// successors explored.
    pub fn reachable(
        &self,
        start: VertexIndex,
        max_cost: f64,
        relations: RelationType,
        cost_id: usize,
        backwards: bool,
    ) -> Vec<VertexIndex> {
        let mut distance = vec![f64::INFINITY; self.vertex_count()];
        let mut heap = BinaryHeap::new();
        distance[start] = 0.0;
        heap.push(Candidate {
            cost: 0.0,
            vertex: start,
        });

        let mut reached = Vec::new();
        while let Some(Candidate { cost, vertex }) = heap.pop() {
            if cost > distance[vertex] {
                continue;
            }
            reached.push(vertex);
            let edges = if backwards {
                self.in_edges(vertex, relations, cost_id)
            } else {
                self.out_edges(vertex, relations, cost_id)
            };
            for edge in edges {
                let next_vertex = if backwards { edge.from } else { edge.to };
                let next = cost + edge.cost;
                if next <= max_cost && next < distance[next_vertex] {
                    distance[next_vertex] = next;
                    heap.push(Candidate {
                        cost: next,
                        vertex: next_vertex,
                    });
                }
            }
        }
        reached.sort_unstable();
        reached
    }

    /// Every path from `start` that *reaches* the given limits.
    ///
    /// The limits are targets, not ceilings: a path grows until its cost reaches
    /// `max_cost` or its length reaches `min_elements`, and only then is it a
    /// result. A path that runs out of successors before reaching either is
    /// returned only when `include_shorter` says so — so asking for paths of at
    /// least 1000 metres on a 40-metre map yields nothing at all.
    ///
    /// With both limits set, growth continues only while *both* are unmet.
    #[allow(clippy::too_many_arguments)]
    pub fn possible_paths(
        &self,
        start: VertexIndex,
        max_cost: Option<f64>,
        min_elements: Option<usize>,
        relations: RelationType,
        cost_id: usize,
        include_shorter: bool,
        backwards: bool,
    ) -> Vec<Vec<VertexIndex>> {
        let mut finished: Vec<Vec<VertexIndex>> = Vec::new();
        // Depth-first, carrying the path so cycles can be cut.
        let mut stack: Vec<(Vec<VertexIndex>, f64)> = vec![(vec![start], 0.0)];

        // Whether a path should still grow. With both limits set, growth continues
        // only while neither has been reached.
        let keep_going = |path: &[VertexIndex], cost: f64| {
            let under_cost = max_cost.is_none_or(|limit| cost < limit);
            let under_length = min_elements.is_none_or(|limit| path.len() < limit);
            under_cost && under_length
        };

        while let Some((path, cost)) = stack.pop() {
            if !keep_going(&path, cost) {
                // A limit has been reached: this path is a result, and grows no
                // further.
                finished.push(path);
                continue;
            }

            let vertex = *path.last().expect("paths are never empty");
            let edges = if backwards {
                self.in_edges(vertex, relations, cost_id)
            } else {
                self.out_edges(vertex, relations, cost_id)
            };

            let mut extended = false;
            for edge in edges {
                let next_vertex = if backwards { edge.from } else { edge.to };
                if path.contains(&next_vertex) {
                    continue;
                }
                let mut longer = path.clone();
                longer.push(next_vertex);
                stack.push((longer, cost + edge.cost));
                extended = true;
            }

            // Ran out of successors before reaching a limit.
            if !extended && include_shorter && path.len() > 1 {
                finished.push(path);
            }
        }
        finished.sort();
        finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A diamond: 0 -> 1 -> 3 and 0 -> 2 -> 3, the second route cheaper.
    fn diamond() -> Graph {
        let mut graph = Graph::new(4, 1);
        graph.add_edge(0, 1, RelationType::SUCCESSOR, 0, 5.0);
        graph.add_edge(0, 2, RelationType::SUCCESSOR, 0, 1.0);
        graph.add_edge(1, 3, RelationType::SUCCESSOR, 0, 1.0);
        graph.add_edge(2, 3, RelationType::SUCCESSOR, 0, 1.0);
        graph
    }

    #[test]
    fn out_edges_keep_insertion_order() {
        let graph = diamond();
        let edges = graph.out_edges(0, RelationType::SUCCESSOR, 0);
        assert_eq!(edges.iter().map(|e| e.to).collect::<Vec<_>>(), [1, 2]);
    }

    #[test]
    fn the_relation_mask_and_cost_id_both_filter() {
        let mut graph = Graph::new(2, 2);
        graph.add_edge(0, 1, RelationType::SUCCESSOR, 0, 1.0);
        graph.add_edge(0, 1, RelationType::CONFLICTING, 0, 1.0);
        graph.add_edge(0, 1, RelationType::SUCCESSOR, 1, 9.0);

        assert_eq!(graph.out_edges(0, RelationType::SUCCESSOR, 0).len(), 1);
        assert_eq!(graph.out_edges(0, RelationType::all(), 0).len(), 2);
        assert_eq!(graph.out_edges(0, RelationType::SUCCESSOR, 1)[0].cost, 9.0);
    }

    #[test]
    fn dijkstra_takes_the_cheaper_branch() {
        let graph = diamond();
        let path = graph.shortest_path(0, 3, RelationType::SUCCESSOR, 0).unwrap();
        assert_eq!(path, [0, 2, 3]);
        assert_eq!(graph.shortest_path(0, 0, RelationType::SUCCESSOR, 0).unwrap(), [0]);
        assert!(graph.shortest_path(3, 0, RelationType::SUCCESSOR, 0).is_none());
    }

    #[test]
    fn reachability_is_inclusive_and_includes_the_start() {
        let graph = diamond();
        assert_eq!(graph.reachable(0, 0.0, RelationType::SUCCESSOR, 0, false), [0]);
        assert_eq!(graph.reachable(0, 1.0, RelationType::SUCCESSOR, 0, false), [0, 2]);
        assert_eq!(
            graph.reachable(0, 5.0, RelationType::SUCCESSOR, 0, false),
            [0, 1, 2, 3]
        );
        // Backwards, from the far end: 0 is reachable via the cheap branch
        // (1 + 1) but the expensive one (1 + 5) is over budget.
        assert_eq!(
            graph.reachable(3, 2.0, RelationType::SUCCESSOR, 0, true),
            [0, 1, 2, 3]
        );
        assert_eq!(
            graph.reachable(3, 1.0, RelationType::SUCCESSOR, 0, true),
            [1, 2, 3]
        );
    }

    #[test]
    fn the_cost_limit_is_a_target_not_a_ceiling() {
        let graph = diamond();
        // Nothing in this graph costs 100, so nothing qualifies.
        let paths = graph.possible_paths(0, Some(100.0), None, RelationType::SUCCESSOR, 0, false, false);
        assert!(paths.is_empty(), "{paths:?}");

        // Asking for the paths that reach a cost of 2 finds both branches, each
        // stopping as soon as it gets there.
        let paths = graph.possible_paths(0, Some(2.0), None, RelationType::SUCCESSOR, 0, false, false);
        assert_eq!(paths, vec![vec![0, 1], vec![0, 2, 3]]);

        // With include_shorter, the dead ends come back too.
        let paths = graph.possible_paths(0, Some(100.0), None, RelationType::SUCCESSOR, 0, true, false);
        assert_eq!(paths, vec![vec![0, 1, 3], vec![0, 2, 3]]);
    }

    #[test]
    fn the_element_limit_is_a_target_too() {
        let graph = diamond();
        let paths = graph.possible_paths(0, None, Some(2), RelationType::SUCCESSOR, 0, false, false);
        assert_eq!(paths, vec![vec![0, 1], vec![0, 2]]);
        let paths = graph.possible_paths(0, None, Some(3), RelationType::SUCCESSOR, 0, false, false);
        assert_eq!(paths, vec![vec![0, 1, 3], vec![0, 2, 3]]);
    }

    #[test]
    fn a_cycle_does_not_trap_the_search() {
        let mut graph = Graph::new(3, 1);
        graph.add_edge(0, 1, RelationType::SUCCESSOR, 0, 1.0);
        graph.add_edge(1, 2, RelationType::SUCCESSOR, 0, 1.0);
        graph.add_edge(2, 0, RelationType::SUCCESSOR, 0, 1.0);
        let paths = graph.possible_paths(0, None, Some(3), RelationType::SUCCESSOR, 0, false, false);
        assert_eq!(paths, vec![vec![0, 1, 2]]);
    }
}
