"""Map queries, minus the ones that need ROS.

Upstream imports `geometry_msgs` and `rclpy` at module top, which makes the whole
module unimportable without a ROS installation. This one imports neither: the
ROS-dependent functions are defined and raise when *called*, so everything else stays
usable.

Two of upstream's own bindings do not work: `getSucceedingLaneletSequences` and
`getPrecedingLaneletSequences` return a nested C++ vector Boost.Python was never
taught to convert, so calling them raises `TypeError` about a missing to_python
converter. That is an upstream defect like any other and gets the usual treatment:
repaired by default, reproduced exactly under `LANELET2_BUG_COMPAT=1`.
"""

import lanelet2  # noqa: F401

from lanelet2.core import ManeuverType as _ManeuverType
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
        if row.getManeuver(lanelet) == _ManeuverType.Yield:
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


def getAllNeighborsLeft(graph, lanelet):
    """Every lanelet to the left, walking outward until there is none.

    `left` and `adjacentLeft` are both consulted, in that order: the first is a
    lane change the traffic rules permit, the second merely a shared border. A
    lanelet that is only adjacent still continues the walk.
    """
    lanelets = []
    current = graph.left(lanelet) or graph.adjacentLeft(lanelet)
    while current is not None:
        lanelets.append(current)
        current = graph.left(current) or graph.adjacentLeft(current)
    return lanelets


def getAllNeighborsRight(graph, lanelet):
    lanelets = []
    current = graph.right(lanelet) or graph.adjacentRight(lanelet)
    while current is not None:
        lanelets.append(current)
        current = graph.right(current) or graph.adjacentRight(current)
    return lanelets


def getAllNeighbors(graph, lanelet, search_point=None):
    """The whole road slice through a lanelet, ordered left to right.

    The left-hand neighbours come back nearest-first, so they are reversed to put
    the outermost lane first and the argument in the middle.

    Upstream also has a three-argument form taking a ROS point, which needs
    `geometry_msgs`; that one raises.

    Upstream's own shim never reaches the two-argument overload -- it dispatches on
    argument type, matches nothing, and falls off the end returning `None`. Repaired
    by default; bug-compat returns the `None` back.
    """
    if search_point is not None:
        _ros_only("getAllNeighbors")()
    if lanelet2.BUG_COMPAT:
        return None
    left = getAllNeighborsLeft(graph, lanelet)
    left.reverse()
    return left + [lanelet] + getAllNeighborsRight(graph, lanelet)


def _length3d(lanelet):
    from lanelet2.geometry import length3d

    return length3d(lanelet)


def _succeeding_recursive(graph, lanelet, length):
    following = graph.following(lanelet)
    lanelet_length = _length3d(lanelet)
    # The recursion stops on reaching a dead end *or* on the accumulated length
    # exceeding the budget -- the budget is checked against this lanelet alone, not
    # against the sequence so far, which is why it is decremented on the way down.
    if not following or lanelet_length >= length:
        return [[lanelet]]
    sequences = []
    for next_lanelet in following:
        for tail in _succeeding_recursive(graph, next_lanelet, length - lanelet_length):
            sequences.append([lanelet] + tail)
    return sequences


def getSucceedingLaneletSequences(graph, lanelet, length):
    if lanelet2.BUG_COMPAT:
        _broken_upstream("getSucceedingLaneletSequences", _SEQUENCES)()
    sequences = []
    for next_lanelet in graph.following(lanelet):
        sequences.extend(_succeeding_recursive(graph, next_lanelet, length))
    return sequences


def _contains(lanelets, lanelet):
    return any(other == lanelet for other in lanelets)


def _preceding_recursive(graph, lanelet, length, exclude):
    previous = graph.previous(lanelet)
    lanelet_length = _length3d(lanelet)
    if not previous or lanelet_length >= length:
        return [[lanelet]]
    sequences = []
    for prev in previous:
        if _contains(exclude, prev):
            continue
        for head in _preceding_recursive(graph, prev, length - lanelet_length, exclude):
            sequences.append(head + [lanelet])
    # Every predecessor excluded leaves the lanelet standing alone rather than
    # yielding nothing.
    if not sequences:
        sequences.append([lanelet])
    return sequences


def getPrecedingLaneletSequences(graph, lanelet, length, exclude_lanelets=None):
    if lanelet2.BUG_COMPAT:
        _broken_upstream("getPrecedingLaneletSequences", _SEQUENCES)()
    exclude = exclude_lanelets or []
    sequences = []
    for prev in graph.previous(lanelet):
        if _contains(exclude, prev):
            continue
        sequences.extend(_preceding_recursive(graph, prev, length, exclude))
    return sequences


_ROS_ONLY = (
    "getClosestLanelet",
    "getClosestLaneletWithConstrains",
    "getCurrentLanelets",
    "getLaneletsWithinRange",
    "getLaneChangeableNeighbors",
)


def _broken_upstream(name, kind):
    """Raises the TypeError upstream's missing converter produces.

    Boost.Python reports the C++ type it could not convert, and callers who have
    seen this error recognise it by that text, so it is reproduced verbatim rather
    than paraphrased.
    """

    def raise_it():
        raise TypeError(
            "No to_python (by-value) converter found for C++ type: %s" % kind
        )

    raise_it.__name__ = name
    return raise_it


_LANELETS = "std::vector<lanelet::ConstLanelet, std::allocator<lanelet::ConstLanelet> >"
_SEQUENCES = "std::vector<%s, std::allocator<%s > >" % (_LANELETS, _LANELETS)


def _regelems_of(lanelets, accessor):
    """The regulatory elements of one kind across a run of lanelets."""
    out = []
    for lanelet in lanelets:
        out.extend(getattr(lanelet, accessor)())
    return out


def trafficLights(lanelets):
    return _regelems_of(lanelets, "trafficLights")


def autowareTrafficLights(lanelets):
    # Upstream filters the same list down to the extension's subclass; ours can do
    # that honestly because the typed accessors are dynamic-cast shaped.
    return [
        element
        for element in _regelems_of(lanelets, "trafficLights")
        if type(element).__name__ == "AutowareTrafficLight"
    ]


def _regelems_named(lanelets, class_name):
    """Regulatory elements of one extension class across a run of lanelets.

    Selected by class name rather than by a typed accessor, because the extension's
    elements have no `lanelet.detectionAreas()` of their own -- the stock accessors
    only cover the stock kinds.
    """
    out = []
    for lanelet in lanelets:
        out.extend(
            element
            for element in lanelet.regulatoryElements
            if type(element).__name__ == class_name
        )
    return out


def detectionAreas(lanelets):
    return _regelems_named(lanelets, "DetectionArea")


def crosswalks(lanelets):
    return _regelems_named(lanelets, "Crosswalk")


def noParkingAreas(lanelets):
    return _regelems_named(lanelets, "NoParkingArea")


def noStoppingAreas(lanelets):
    return _regelems_named(lanelets, "NoStoppingArea")


def speedBumps(lanelets):
    return _regelems_named(lanelets, "SpeedBump")


_TOUCHING = 1e-12
def _linked_unavailable(name):
    """The ArgumentError upstream's missing converter produces.

    `getAllParkingLots` hands back a Python list, and Boost.Python has no from-python
    converter for `ConstPolygons3d`, so the three functions that want one cannot be
    called in the reference at all. `getLinkedParkingSpaces` escapes this because its
    second argument is a `LaneletMap`, so nothing has to survive the round trip.
    """
    raise _ext.__argument_error__(name)


def _lanelet_polygon(lanelet):
    """A lanelet's outline as a plain `Polygon2d`.

    `polygon2d()` gives a `CompoundPolygon2d`, and the distance overloads that accept
    one are not the ones needed here, so the points are copied into a polygon of their
    own. Upstream reaches past all of this into boost directly.
    """
    from lanelet2.core import Point3d, Polygon3d
    from lanelet2.geometry import to2D

    outline = lanelet.polygon3d()
    return to2D(Polygon3d(0, [Point3d(0, p.x, p.y, p.z) for p in outline]))

_PARKING_SPACE_REACH = 5.0


def _overlaps(a, b):
    """Whether two 2D shapes touch at all.

    Upstream compares a boost distance against machine epsilon rather than testing
    intersection, so shapes that merely graze each other count as linked.
    """
    from lanelet2.geometry import distance

    return distance(a, b) < _TOUCHING


def _facing_line(parking_space):
    """The probe upstream extends backwards out of a parking space.

    A space is only linked to something its *open end* points at, so the check is a
    line reaching five metres back from the space's first point, in the direction
    opposite to the space itself, and running to its last point.
    """
    from lanelet2.core import BasicPoint3d, LineString3d, Point3d

    front, back = parking_space[0], parking_space[-1]
    direction = (
        back.x - front.x,
        back.y - front.y,
        back.z - front.z,
    )
    start = BasicPoint3d(
        front.x - direction[0] * _PARKING_SPACE_REACH,
        front.y - direction[1] * _PARKING_SPACE_REACH,
        front.z - direction[2] * _PARKING_SPACE_REACH,
    )
    return LineString3d(
        0,
        [
            Point3d(0, start.x, start.y, start.z),
            Point3d(0, back.x, back.y, back.z),
        ],
    )


def _linked_by_facing(parking_space, candidates, shape_of):
    """Candidates that a parking space is both near and facing."""
    from lanelet2.geometry import distance, to2D

    space2d = to2D(parking_space)
    probe2d = to2D(_facing_line(parking_space))
    linked = []
    for candidate in candidates:
        shape = shape_of(candidate)
        if distance(space2d, shape) > _PARKING_SPACE_REACH:
            continue
        if _overlaps(probe2d, shape):
            linked.append(candidate)
    return linked


def _parking_lot_of(shape2d, all_parking_lots):
    """The first parking lot a 2D shape touches, or None.

    The private half of `getLinkedParkingLot`, so that the functions the reference
    *can* call do not go through the bug-compat gate on the ones it cannot.
    """
    from lanelet2.geometry import to2D

    for parking_lot in all_parking_lots:
        if _overlaps(shape2d, to2D(parking_lot)):
            return parking_lot
    return None


def getLinkedParkingLot(subject, all_parking_lots):
    """The parking lot a lanelet overlaps, or None.

    Upstream signals absence through a bool and an out-parameter; from Python the
    natural spelling is the polygon or None, which is what its binding returns.
    """
    if lanelet2.BUG_COMPAT:
        _linked_unavailable("getLinkedParkingLot")
    return _parking_lot_of(subject.polygon2d(), all_parking_lots)


def _lanelets_in_parking_lot(parking_lot, all_road_lanelets):
    from lanelet2.geometry import to2D

    lot2d = to2D(parking_lot)
    return [ll for ll in all_road_lanelets if _overlaps(ll.polygon2d(), lot2d)]


def getLinkedLanelets(parking_space, all_road_lanelets, all_parking_lots):
    """Road lanelets a parking space opens onto.

    Both conditions matter: the lanelet has to share the parking space's *lot*, and
    the space has to face it. Sharing a lot alone would link every space to every
    lanelet in a car park.
    """
    if lanelet2.BUG_COMPAT:
        _linked_unavailable("getLinkedLanelets")
    from lanelet2.geometry import to2D

    parking_lot = _parking_lot_of(to2D(parking_space), all_parking_lots)
    if parking_lot is None:
        return []
    candidates = _lanelets_in_parking_lot(parking_lot, all_road_lanelets)
    return _linked_by_facing(parking_space, candidates, _lanelet_polygon)


def getLinkedLanelet(parking_space, all_road_lanelets, all_parking_lots):
    """The nearest of `getLinkedLanelets`, or None."""
    if lanelet2.BUG_COMPAT:
        _linked_unavailable("getLinkedLanelet")
    from lanelet2.geometry import distance, to2D

    linked = getLinkedLanelets(parking_space, all_road_lanelets, all_parking_lots)
    if not linked:
        return None
    space2d = to2D(parking_space)
    return min(linked, key=lambda ll: distance(space2d, _lanelet_polygon(ll)))


def getLinkedParkingSpaces(lanelet, lanelet_map):
    """Parking spaces that open onto a lanelet."""
    all_parking_spaces = getAllParkingSpaces(lanelet_map)
    all_parking_lots = getAllParkingLots(lanelet_map)
    parking_lot = _parking_lot_of(lanelet.polygon2d(), all_parking_lots)
    if parking_lot is None:
        return []
    from lanelet2.geometry import to2D

    lot2d = to2D(parking_lot)
    candidates = [
        space for space in all_parking_spaces if _overlaps(to2D(space), lot2d)
    ]
    linked = []
    for space in candidates:
        if _linked_by_facing(space, [lanelet], _lanelet_polygon):
            linked.append(space)
    return _ext.__as_const_lines__(linked)


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
    "getAllNeighborsLeft",
    "getAllNeighborsRight",
    "getSucceedingLaneletSequences",
    "getPrecedingLaneletSequences",
    "getAllNeighbors",
    "trafficLights",
    "autowareTrafficLights",
    "detectionAreas",
    "crosswalks",
    "noParkingAreas",
    "noStoppingAreas",
    "speedBumps",
    "getLinkedParkingLot",
    "getLinkedLanelet",
    "getLinkedLanelets",
    "getLinkedParkingSpaces",
    *_ROS_ONLY,
]
