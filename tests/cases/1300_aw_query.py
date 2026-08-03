"""`utility.query`, on a real Autoware map.

# ORACLE: aw

Each of these is one filter over one layer, but the filters are not guessable:
`getAllPartitions` accepts three type values, and the two pedestrian-marking queries
split on the linestring's *point count* rather than on any attribute. They were read
out of `lib/query.cpp` and are checked here against a map that actually exercises
them. The vendored example map is used rather than a real Autoware one: every query
here is exercised by it, and it is the map CI can actually get hold of.

Results are compared by id, because upstream walks `std::unordered_map` layers and
its order is not reproducible.
"""

from canon import data_path, emit, expect_raises, run

import autoware_lanelet2_extension_python.regulatory_elements  # noqa: F401
import autoware_lanelet2_extension_python.utility.query as query
from lanelet2.io import Origin, loadRobust
from lanelet2.projection import UtmProjector




def ids(primitives):
    return sorted(p.id for p in primitives)


def main():
    map_, _ = loadRobust(data_path("mapping_example.osm"), UtmProjector(Origin(49.0, 8.4, 0.0)))

    lanelets = query.laneletLayer(map_)
    emit("lanelet_layer_count", len(lanelets))
    emit("lanelet_layer_ids", ids(lanelets)[:40])

    for name in ("roadLanelets", "crosswalkLanelets", "walkwayLanelets", "shoulderLanelets"):
        found = getattr(query, name)(lanelets)
        emit("%s_count" % name, len(found))
        emit("%s_ids" % name, ids(found)[:40])

    # The named queries are `subtypeLanelets` with the string filled in.
    emit("subtype_road_count", len(query.subtypeLanelets(lanelets, "road")))
    emit("subtype_unknown_count", len(query.subtypeLanelets(lanelets, "no_such_subtype")))

    for name in (
        "curbstones",
        "getAllParkingLots",
        "getAllParkingSpaces",
        "getAllPartitions",
        "getAllFences",
        "getAllObstaclePolygons",
        "getAllPedestrianLineMarkings",
        "getAllPedestrianPolygonMarkings",
    ):
        found = getattr(query, name)(map_)
        emit("%s_count" % name, len(found))
        emit("%s_ids" % name, ids(found)[:40])
        emit("%s_type" % name, type(found[0]).__name__ if found else None)

    for kind in ("parking_lot", "obstacle", "no_such_type"):
        emit("polygons_%s" % kind, len(query.getAllPolygonsByType(map_, kind)))

    # Stop lines come from three kinds of regulatory element, and a right-of-way one
    # contributes only where *this* lanelet is the one yielding.
    stoplines = query.stopLinesLanelets(lanelets)
    emit("stoplines_count", len(stoplines))
    emit("stoplines_ids", ids(stoplines)[:40])
    emit("stoplines_type", type(stoplines[0]).__name__ if stoplines else None)

    single = query.stopLinesLanelet(lanelets[0])
    emit("stoplines_single_count", len(single))

    # Deduplicated by id upstream, through a std::set.
    signs = query.stopSignStopLines(lanelets)
    emit("stop_sign_lines", ids(signs))
    emit("stop_sign_lines_unknown", len(query.stopSignStopLines(lanelets, "no_such_sign")))

    # --- the routing-graph queries -----------------------------------------------
    from lanelet2.routing import RoutingGraph
    from lanelet2.traffic_rules import Locations, Participants, create

    graph = RoutingGraph(map_, create(Locations.Germany, Participants.Vehicle))
    probe = sorted(query.roadLanelets(lanelets), key=lambda x: x.id)[:60]

    # `left` is a permitted lane change, `adjacentLeft` merely a shared border; both
    # continue the walk, in that order.
    emit("neighbours_left", [ids(query.getAllNeighborsLeft(graph, x)) for x in probe[:20]])
    emit("neighbours_right", [ids(query.getAllNeighborsRight(graph, x)) for x in probe[:20]])
    emit("neighbours_left_total", sum(len(query.getAllNeighborsLeft(graph, x)) for x in probe))
    emit("neighbours_right_total", sum(len(query.getAllNeighborsRight(graph, x)) for x in probe))

    # Upstream cannot return a nested vector of lanelets at all -- Boost.Python was
    # never given a converter for it -- so these raise there and work here.
    def sequences(fn, *args):
        return [[x.id for x in seq] for seq in fn(*args)]

    expect_raises(
        "succeeding_broken",
        lambda: query.getSucceedingLaneletSequences(graph, probe[0], 50.0),
        msg=True,
    )
    expect_raises(
        "preceding_broken",
        lambda: query.getPrecedingLaneletSequences(graph, probe[0], 50.0),
        msg=True,
    )
    # These three do work upstream -- an earlier reading that they did not came from
    # passing the wrong argument type, not from the bindings.
    emit("traffic_lights", ids(query.trafficLights(probe)))
    emit("autoware_traffic_lights", ids(query.autowareTrafficLights(probe)))
    emit("detection_areas", ids(query.detectionAreas(probe)))
    for name in ("crosswalks", "noParkingAreas", "noStoppingAreas", "speedBumps"):
        emit(name, ids(getattr(query, name)(probe)))

    # The road slice through a lanelet: outermost left first, the lanelet itself in
    # the middle, then the right-hand neighbours.
    # Upstream's shim falls off the end of its type dispatch here and returns None
    # rather than raising, so the emitted value is `None` there and a list here.
    slices = [query.getAllNeighbors(graph, x) for x in probe[:20]]
    emit("all_neighbours", None if slices[0] is None else [ids(s) for s in slices])


run(main)
