"""`utility.query`, on a real Autoware map.

# ORACLE: aw

Each of these is one filter over one layer, but the filters are not guessable:
`getAllPartitions` accepts three type values, and the two pedestrian-marking queries
split on the linestring's *point count* rather than on any attribute. They were read
out of `lib/query.cpp` and are checked here against a map that actually exercises
them -- 1327 pedestrian polygon markings, 884 road lanelets.

Results are compared by id, because upstream walks `std::unordered_map` layers and
its order is not reproducible.
"""

from canon import emit, run

import autoware_lanelet2_extension_python.regulatory_elements  # noqa: F401
import autoware_lanelet2_extension_python.utility.query as query
from lanelet2.io import Origin, loadRobust
from lanelet2.projection import UtmProjector

MAP = "/home/masaya/workspace/torchdrivesim/nishishinjuku_autoware_map/lanelet2_map.osm"


def ids(primitives):
    return sorted(p.id for p in primitives)


def main():
    map_, _ = loadRobust(MAP, UtmProjector(Origin(35.6895, 139.6917, 0.0)))

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


run(main)
