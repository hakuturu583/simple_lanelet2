"""Map queries, minus the ones that need ROS.

Upstream imports `geometry_msgs` and `rclpy` at module top, which makes the whole
module unimportable without a ROS installation. This one imports neither: the
ROS-dependent functions are defined and raise when *called*, so everything else stays
usable.

Three of upstream's own bindings do not work either -- `trafficLights`,
`autowareTrafficLights` and `detectionAreas` return a C++ vector Boost.Python was
never taught to convert, so calling them raises `TypeError`. They are absent here for
the same practical reason: there is no working behaviour to be compatible with.
"""

import lanelet2  # noqa: F401

from lanelet2.core import ManeuverType
from lanelet2._lanelet2 import awext_query as _ext

laneletLayer = _ext.laneletLayer
subtypeLanelets = _ext.subtypeLanelets
roadLanelets = _ext.roadLanelets
crosswalkLanelets = _ext.crosswalkLanelets
walkwayLanelets = _ext.walkwayLanelets
shoulderLanelets = _ext.shoulderLanelets
curbstones = _ext.curbstones
getAllPolygonsByType = _ext.getAllPolygonsByType
getAllObstaclePolygons = _ext.getAllObstaclePolygons
getAllParkingLots = _ext.getAllParkingLots
getAllParkingSpaces = _ext.getAllParkingSpaces
getAllPartitions = _ext.getAllPartitions
getAllFences = _ext.getAllFences
getAllPedestrianPolygonMarkings = _ext.getAllPedestrianPolygonMarkings
getAllPedestrianLineMarkings = _ext.getAllPedestrianLineMarkings

def stopLinesLanelet(lanelet):
    """Every stop line a lanelet is subject to.

    Three sources, in upstream's order: a right-of-way element *only* where this
    lanelet is the one yielding, any traffic light, and the first reference line of
    any traffic sign. Written here rather than in Rust because it needs nothing but
    the typed accessors already exposed.
    """
    stoplines = []
    for row in lanelet.rightOfWay():
        if row.getManeuver(lanelet) == ManeuverType.Yield:
            line = row.stopLine
            if line is not None:
                stoplines.append(line)
    for light in lanelet.trafficLights():
        line = light.stopLine
        if line is not None:
            stoplines.append(line)
    for sign in lanelet.trafficSigns():
        lines = sign.refLines()
        if lines:
            stoplines.append(lines[0])
    return _ext.__as_const_lines__(stoplines)


def stopLinesLanelets(lanelets):
    out = []
    for lanelet in lanelets:
        out.extend(stopLinesLanelet(lanelet))
    return out


def stopSignStopLines(lanelets, stop_sign_id="stop_sign"):
    """Reference lines of traffic signs of one type, deduplicated by id.

    Upstream dedupes through a `std::set<Id>`, so a stop line shared by two lanelets
    appears once.
    """
    seen = set()
    stoplines = []
    for lanelet in lanelets:
        for sign in lanelet.trafficSigns():
            if sign.type() != stop_sign_id:
                continue
            for line in sign.refLines():
                if line.id not in seen:
                    seen.add(line.id)
                    stoplines.append(line)
    return _ext.__as_const_lines__(stoplines)


_ROS_ONLY = (
    "getClosestLanelet",
    "getClosestLaneletWithConstrains",
    "getCurrentLanelets",
    "getLaneletsWithinRange",
    "getLaneChangeableNeighbors",
    "getAllNeighbors",
)


def _ros_only(name):
    def stub(*args, **kwargs):
        raise NotImplementedError(
            "%s needs geometry_msgs and rclpy, which simple-lanelet2 deliberately "
            "does not depend on" % name
        )

    stub.__name__ = name
    return stub


for _name in _ROS_ONLY:
    globals()[_name] = _ros_only(_name)

__all__ = [
    "laneletLayer",
    "subtypeLanelets",
    "roadLanelets",
    "crosswalkLanelets",
    "walkwayLanelets",
    "shoulderLanelets",
    "curbstones",
    "getAllPolygonsByType",
    "getAllObstaclePolygons",
    "getAllParkingLots",
    "getAllParkingSpaces",
    "getAllPartitions",
    "getAllFences",
    "getAllPedestrianPolygonMarkings",
    "getAllPedestrianLineMarkings",
    "stopLinesLanelet",
    "stopLinesLanelets",
    "stopSignStopLines",
    *_ROS_ONLY,
]
