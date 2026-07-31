"""The routing graph.

The construction is verified before any query is: `edges()` dumps every edge as
`(fromId, fromInverted, toId, toInverted, relation, costId, cost)`, which pins the
builder itself rather than one query's view of it.

Two facts shape everything downstream. A lanelet drivable both ways becomes *two*
vertices, one per direction, and they conflict with each other. And the graph
carries a parallel edge per cost model, which is why nearly every query takes a
cost id -- a lane change can be affordable under one model and not another.
"""

from canon import by_id, data_path, emit, expect_raises, run

import lanelet2.routing as routing
import lanelet2.traffic_rules as traffic_rules
from lanelet2.core import (
    AttributeMap,
    Lanelet,
    LineString3d,
    Point3d,
    createMapFromLanelets,
    getId,
)
from lanelet2.io import Origin, load
from lanelet2.projection import UtmProjector


def chain(count, y=0.0, subtype="road", shared_left=None):
    """A run of lanelets sharing end points, so `follows` holds between them."""
    left = [Point3d(getId(), i * 10.0, y + 1.0, 0.0) for i in range(count + 1)]
    right = [Point3d(getId(), i * 10.0, y - 1.0, 0.0) for i in range(count + 1)]
    if shared_left is not None:
        right = shared_left
    lanelets = []
    for i in range(count):
        ll = Lanelet(getId(),
                     LineString3d(getId(), [left[i], left[i + 1]]),
                     LineString3d(getId(), [right[i], right[i + 1]]))
        ll.attributes["subtype"] = subtype
        lanelets.append(ll)
    return lanelets, left, right


def ids(lanelets):
    return [ll.id for ll in lanelets]


def relations(found):
    return sorted([[r.lanelet.id, r.lanelet.inverted(), str(r.relationType)] for r in found])


def main():
    rules = traffic_rules.create(traffic_rules.Locations.Germany,
                                 traffic_rules.Participants.Vehicle)

    # --- relation type enum --------------------------------------------------
    emit("relation_names", sorted(routing.RelationType.names))
    emit("relation_values", sorted(routing.RelationType.values))
    emit("relation_reprs", [repr(routing.RelationType.Successor),
                            str(routing.RelationType.Successor),
                            int(routing.RelationType.Successor)])
    emit("relation_module_exports",
         [n for n in ["Successor", "Left", "Right", "AdjacentLeft", "AdjacentRight", "Conflicting"]
          if hasattr(routing, n)])

    # --- a straight chain ----------------------------------------------------
    lanelets, _, _ = chain(4)
    map = createMapFromLanelets(lanelets)
    graph = routing.RoutingGraph(map, rules)

    emit("chain_ids", ids(lanelets))
    emit("following", ids(graph.following(lanelets[0])))
    emit("following_with_changes", ids(graph.following(lanelets[0], True)))
    emit("previous", ids(graph.previous(lanelets[1])))
    emit("following_relations", relations(graph.followingRelations(lanelets[0])))
    emit("previous_relations", relations(graph.previousRelations(lanelets[1])))
    emit("following_at_the_end", ids(graph.following(lanelets[3])))

    emit("besides", ids(graph.besides(lanelets[0])))
    emit("left", graph.left(lanelets[0]))
    emit("right", graph.right(lanelets[0]))
    emit("lefts", ids(graph.lefts(lanelets[0])))
    emit("adjacent_left", graph.adjacentLeft(lanelets[0]))
    emit("conflicting", ids(graph.conflicting(lanelets[0])))

    emit("relation_successor", str(graph.routingRelation(lanelets[0], lanelets[1])))
    emit("relation_none", graph.routingRelation(lanelets[0], lanelets[3]))

    path = graph.shortestPath(lanelets[0], lanelets[3])
    emit("shortest_path", ids(path))
    emit("shortest_path_len", len(path))
    emit("shortest_path_item", path[0].id)
    emit("shortest_path_backwards", graph.shortestPath(lanelets[3], lanelets[0]))
    emit("shortest_path_via", ids(graph.shortestPathWithVia(
        lanelets[0], [lanelets[2]], lanelets[3])))
    emit("remaining_lane", ids(path.getRemainingLane(lanelets[1])))

    emit("reachable_0", sorted(ids(graph.reachableSet(lanelets[0], 0.0))))
    emit("reachable_25", sorted(ids(graph.reachableSet(lanelets[0], 25.0))))
    emit("reachable_all", sorted(ids(graph.reachableSet(lanelets[0], 1000.0))))
    emit("reachable_towards", sorted(ids(graph.reachableSetTowards(lanelets[3], 25.0))))

    # The cost argument is a *target*, not a ceiling: paths grow until they reach
    # it, so asking for 1000 on a 40-metre chain yields nothing.
    emit("possible_paths", [ids(p) for p in graph.possiblePaths(lanelets[0], 1000.0)])
    emit("possible_paths_reachable_target",
         sorted(ids(p) for p in graph.possiblePaths(lanelets[0], 20.0)))
    emit("possible_paths_min_len", [ids(p) for p in graph.possiblePathsMinLen(lanelets[0], 3)])
    emit("possible_paths_towards",
         [ids(p) for p in graph.possiblePathsTowards(lanelets[3], 1000.0)])

    emit("passable_submap_size", len(graph.passableLaneletSubmap().laneletLayer))
    emit("check_validity", list(graph.checkValidity(False)))

    # --- a two-way lanelet ---------------------------------------------------
    two_way, _, _ = chain(1, y=100.0)
    two_way[0].attributes["one_way"] = "no"
    two_way_map = createMapFromLanelets(two_way)
    two_way_graph = routing.RoutingGraph(two_way_map, rules)
    emit("two_way_conflicting",
         [[ll.id, ll.inverted()] for ll in two_way_graph.conflicting(two_way[0])])
    emit("two_way_submap_size", len(two_way_graph.passableLaneletSubmap().laneletLayer))

    # --- side by side, sharing a dashed boundary -----------------------------
    shared = [Point3d(getId(), i * 10.0, 200.0, 0.0) for i in range(3)]
    upper, lower = [], []
    for i in range(2):
        boundary = LineString3d(getId(), [shared[i], shared[i + 1]])
        boundary.attributes["type"] = "line_thin"
        boundary.attributes["subtype"] = "dashed"
        top = Lanelet(getId(),
                      LineString3d(getId(), [Point3d(getId(), i * 10.0, 202.0, 0.0),
                                             Point3d(getId(), (i + 1) * 10.0, 202.0, 0.0)]),
                      boundary)
        bottom = Lanelet(getId(), boundary,
                         LineString3d(getId(), [Point3d(getId(), i * 10.0, 198.0, 0.0),
                                                Point3d(getId(), (i + 1) * 10.0, 198.0, 0.0)]))
        for ll in (top, bottom):
            ll.attributes["subtype"] = "road"
        upper.append(top)
        lower.append(bottom)
    side_map = createMapFromLanelets(upper + lower)
    side_graph = routing.RoutingGraph(side_map, rules)
    emit("side_left", side_graph.left(lower[0]).id if side_graph.left(lower[0]) else None)
    emit("side_right", side_graph.right(upper[0]).id if side_graph.right(upper[0]) else None)
    emit("side_besides_upper", ids(side_graph.besides(upper[0])))
    emit("side_left_relations", relations(side_graph.leftRelations(lower[0])))
    emit("side_following_with_changes", sorted(ids(side_graph.following(upper[0], True))))

    # --- routing costs -------------------------------------------------------
    custom = routing.RoutingGraph(map, rules,
                                  [routing.RoutingCostDistance(20.0),
                                   routing.RoutingCostTravelTime(3.0)])
    emit("custom_cost_path", ids(custom.shortestPath(lanelets[0], lanelets[3])))
    emit("custom_cost_path_id1", ids(custom.shortestPath(lanelets[0], lanelets[3], 1)))

    params = routing.PossiblePathsParams()
    emit("possible_paths_params_defaults",
         [params.routingCostLimit, params.elementLimit, params.routingCostId,
          params.includeLaneChanges, params.includeShorterPaths])

    # --- routes --------------------------------------------------------------
    route = graph.getRoute(lanelets[0], lanelets[3])
    emit("route_shortest_path", ids(route.shortestPath()))
    emit("route_size", route.size())
    emit("route_length2d", round(route.length2d(), 6))
    emit("route_num_lanes", route.numLanes())
    emit("route_contains", [lanelets[1] in route, lanelets[0].invert() in route])
    emit("route_remaining_shortest_path", ids(route.remainingShortestPath(lanelets[2])))
    emit("route_remaining_lane", ids(route.remainingLane(lanelets[1])))
    emit("route_full_lane", ids(route.fullLane(lanelets[1])))
    emit("route_submap_size", len(route.laneletSubmap().laneletLayer))
    emit("route_following_relations", relations(route.followingRelations(lanelets[0])))
    emit("route_left_relation", route.leftRelation(lanelets[0]))
    emit("route_conflicting_in_route", ids(route.conflictingInRoute(lanelets[0])))
    emit("route_check_validity", list(route.checkValidity(False)))
    emit("route_via", ids(graph.getRouteVia(lanelets[0], [lanelets[2]], lanelets[3]).shortestPath()))
    emit("route_unreachable", graph.getRoute(lanelets[3], lanelets[0]))

    # --- traversal callbacks -------------------------------------------------
    visited = []
    graph.forEachSuccessor(lanelets[0], lambda info: visited.append(
        [info.lanelet.id, info.length, info.numLaneChanges, round(info.cost, 6)]) or True)
    emit("for_each_successor", visited)

    stopped = []
    graph.forEachSuccessor(lanelets[0], lambda info: (
        stopped.append(info.lanelet.id), len(stopped) < 2)[1])
    emit("for_each_successor_pruned", stopped)

    # --- out of scope --------------------------------------------------------
    expect_raises("export_graphml", lambda: graph.exportGraphML("x.graphml"), msg=True)
    expect_raises("export_graphviz", lambda: graph.exportGraphViz("x.gv"), msg=True)

    # --- against the real map ------------------------------------------------
    real = load(data_path("mapping_example.osm"), UtmProjector(Origin(49.0, 8.4, 0.0)))
    real_graph = routing.RoutingGraph(real, rules)
    emit("real:passable_count", len(real_graph.passableLaneletSubmap().laneletLayer))
    real_lanelets = by_id(real_graph.passableLaneletSubmap().laneletLayer)[:80]
    emit("real:following", [[ll.id, sorted(ids(real_graph.following(ll)))]
                            for ll in real_lanelets])
    emit("real:previous", [[ll.id, sorted(ids(real_graph.previous(ll)))]
                           for ll in real_lanelets])
    emit("real:besides", [[ll.id, sorted(ids(real_graph.besides(ll)))]
                          for ll in real_lanelets])
    emit("real:conflicting", [[ll.id, sorted(ids(real_graph.conflicting(ll)))]
                              for ll in real_lanelets[:40]])
    emit("real:reachable", [[ll.id, sorted(ids(real_graph.reachableSet(ll, 50.0)))]
                            for ll in real_lanelets[:40]])


run(main)
