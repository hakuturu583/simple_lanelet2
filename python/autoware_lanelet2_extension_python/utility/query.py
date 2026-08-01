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
    *_ROS_ONLY,
]
