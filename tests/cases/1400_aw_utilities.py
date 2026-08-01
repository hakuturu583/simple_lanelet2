"""`utility.utilities`, minus its ROS half.

# ORACLE: aw
# TOL: abs=1e-9 rel=1e-12

The resampling family is the substance here, and it is ported literally rather than
tidied. `findNearestIndexPair` looks like a generic arc-length lookup and is not: its
front case compares against the *second* accumulated length and its back case against
the second-to-last, which changes which segment a near-endpoint target interpolates
within. Rewriting it as a bisection would agree almost everywhere and differ exactly
where it matters.

Two functions cannot be called in the reference and are repaired here: the single
lanelet form of `getLaneletLength2d`/`3d`, whose shim only ever matches the list form,
and `getConflictingLanelets`, whose binding wants a graph type Python cannot produce.
"""

from canon import by_id, emit, expect_raises, run

import autoware_lanelet2_extension_python.regulatory_elements  # noqa: F401
import autoware_lanelet2_extension_python.utility.query as query
import autoware_lanelet2_extension_python.utility.utilities as utilities
from lanelet2.core import BasicPoint2d
from lanelet2.io import Origin, load
from lanelet2.projection import UtmProjector


def coordinates(primitives):
    return [[p.x, p.y, p.z] for p in primitives]


def main():
    map_ = load(
        __import__("canon").data_path("mapping_example.osm"),
        UtmProjector(Origin(49.0, 8.4, 0.0)),
    )
    lanelets = sorted(query.roadLanelets(query.laneletLayer(map_)), key=lambda x: x.id)
    probe = lanelets[:3]

    # --- resampling ---------------------------------------------------------------
    for lanelet in probe:
        key = lanelet.id
        emit("fine_%d" % key, coordinates(utilities.generateFineCenterline(lanelet)))
        emit("fine1_%d" % key, coordinates(utilities.generateFineCenterline(lanelet, 1.0)))
        emit(
            "centre_off_%d" % key,
            coordinates(utilities.getCenterlineWithOffset(lanelet, 0.5)),
        )
        emit("left_off_%d" % key, coordinates(utilities.getLeftBoundWithOffset(lanelet, 0.5)))
        emit(
            "right_off_%d" % key,
            coordinates(utilities.getRightBoundWithOffset(lanelet, 0.5)),
        )

    # --- combining ----------------------------------------------------------------
    combined = utilities.combineLaneletsShape(probe)
    emit("combined_left", coordinates(combined.leftBound))
    emit("combined_right", coordinates(combined.rightBound))
    emit("combined_centre", coordinates(combined.centerline))

    emit("arc_0_5", coordinates(utilities.getPolygonFromArcLength(probe, 0.0, 5.0)))
    emit("arc_2_9", coordinates(utilities.getPolygonFromArcLength(probe, 2.0, 9.0)))
    # Saturated at both ends rather than raising.
    emit("arc_wide", coordinates(utilities.getPolygonFromArcLength(probe, -5.0, 1e6)))

    emit(
        "closest_segment",
        coordinates(utilities.getClosestSegment(BasicPoint2d(0.0, 0.0), probe[0].centerline)),
    )

    # --- lengths ------------------------------------------------------------------
    emit("length_list_2d", utilities.getLaneletLength2d(probe))
    emit("length_list_3d", utilities.getLaneletLength3d(probe))
    # Upstream's shim never matches the single-lanelet form.
    expect_raises("length_single_2d", lambda: utilities.getLaneletLength2d(probe[0]), msg=True)
    expect_raises("length_single_3d", lambda: utilities.getLaneletLength3d(probe[0]), msg=True)

    # --- polygon conversion -------------------------------------------------------
    # Both refuse rather than raise: too few distinct points, and no width tag.
    emit("to_polygon", utilities.lineStringToPolygon(probe[0].leftBound))
    emit("to_polygon_width", utilities.lineStringWithWidthToPolygon(probe[0].leftBound))

    # --- routing ------------------------------------------------------------------
    from lanelet2.routing import RoutingGraph
    from lanelet2.traffic_rules import Locations, Participants, create

    graph = RoutingGraph(map_, create(Locations.Germany, Participants.Vehicle))
    expect_raises(
        "conflicting", lambda: sorted(x.id for x in utilities.getConflictingLanelets(graph, probe[0]))
    )

    # --- overwriting the centerlines ----------------------------------------------
    utilities.overwriteLaneletsCenterline(map_, 1.0, True)
    emit(
        "after_overwrite",
        [coordinates(x.centerline) for x in by_id(map_.laneletLayer)][:3],
    )


run(main)
