"""`utility.query`'s parking-lot links.

# ORACLE: aw

Three of these four are **uncallable in the reference**. `getAllParkingLots` hands
back a Python list, and Boost.Python was never given a from-python converter for the
C++ vector they want, so no argument satisfies them: every call raises `ArgumentError`
against a signature list. That is an upstream defect like any other -- repaired by
default, reproduced under `LANELET2_BUG_COMPAT=1`, with the repaired results pinned as
compat-matrix differences.

`getLinkedParkingSpaces` escapes it, because its second argument is a `LaneletMap` and
nothing has to survive a round trip. It is compared against the reference directly,
and agrees.

Neither example map has a parking lot, so the geometry is built to order. What it has
to discriminate is not proximity but *orientation*: a parking space is linked to a
lanelet only when its open end faces it. The three spaces here are one facing the
road, one turned around, and one too far away.
"""

from canon import emit, expect_raises, run

import autoware_lanelet2_extension_python.regulatory_elements  # noqa: F401
import autoware_lanelet2_extension_python.utility.query as query
from lanelet2.core import (
    Lanelet,
    LineString3d,
    Point3d,
    Polygon3d,
    createMapFromLanelets,
    getId,
)


def point(x, y):
    return Point3d(getId(), x, y, 0.0)


def line(points, **attributes):
    linestring = LineString3d(getId(), points)
    for key, value in attributes.items():
        linestring.attributes[key] = value
    return linestring


def main():
    road = Lanelet(
        getId(), line([point(0, 4), point(20, 4)]), line([point(0, 0), point(20, 0)])
    )
    road.attributes["subtype"] = "road"
    map_ = createMapFromLanelets([road])

    lot = Polygon3d(
        getId(), [point(5, 2), point(18, 2), point(18, 14), point(5, 14)]
    )
    lot.attributes["type"] = "parking_lot"
    map_.add(lot)

    # Open end towards the road, away from it, and out of reach.
    facing = line([point(8, 6), point(8, 12)], type="parking_space")
    away = line([point(12, 12), point(12, 6)], type="parking_space")
    distant = line([point(15, 13), point(15, 13.5)], type="parking_space")
    for space in (facing, away, distant):
        map_.add(space)

    emit("ids", [road.id, lot.id, facing.id, away.id, distant.id])

    lanelets = query.laneletLayer(map_)
    roads = query.roadLanelets(lanelets)
    lots = query.getAllParkingLots(map_)
    emit("counts", [len(roads), len(lots), len(query.getAllParkingSpaces(map_))])

    def linked_lot():
        found = query.getLinkedParkingLot(roads[0], lots)
        return None if found is None else found.id

    def linked_lanelets(space):
        return sorted(x.id for x in query.getLinkedLanelets(space, roads, lots))

    def linked_lanelet(space):
        found = query.getLinkedLanelet(space, roads, lots)
        return None if found is None else found.id

    def linked_spaces():
        return sorted(x.id for x in query.getLinkedParkingSpaces(roads[0], map_))

    expect_raises("uncallable_lot", linked_lot)
    expect_raises("uncallable_facing", lambda: linked_lanelets(facing))
    expect_raises("uncallable_away", lambda: linked_lanelets(away))
    expect_raises("uncallable_distant", lambda: linked_lanelets(distant))
    expect_raises("uncallable_nearest_facing", lambda: linked_lanelet(facing))
    expect_raises("uncallable_nearest_away", lambda: linked_lanelet(away))
    # This one the reference can actually run.
    emit("linked_spaces", linked_spaces())


run(main)
