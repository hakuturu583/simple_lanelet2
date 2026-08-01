"""Geometry helpers, minus the ones that need ROS.

Upstream imports `geometry_msgs` and `rclpy` at module top, so its version of this
module cannot be imported at all without a ROS installation. This one imports neither:
the ROS-dependent functions are defined and raise when *called*, leaving the rest
usable.

The resampling family — `generateFineCenterline` and the three offset variants — all
share one shape: resample both bounds to the same number of points, then combine them.
It is ported literally rather than tidied, because `findNearestIndexPair` is not the
generic arc-length interpolator it looks like: its front and back cases compare
against the *second* and *second-to-last* accumulated lengths, which changes which
segment an endpoint interpolates within.
"""

import math

import lanelet2  # noqa: F401
from lanelet2.core import Lanelet, LineString3d, Point3d, Polygon3d, getId
from lanelet2.geometry import distance, length, to2D

from lanelet2._lanelet2 import awext_query as _query

DEFAULT_RESOLUTION = 5.0


def _accumulated_lengths(linestring):
    """Running total of segment lengths, starting at zero."""
    lengths = [0.0]
    for i in range(1, len(linestring)):
        lengths.append(lengths[-1] + distance(linestring[i - 1], linestring[i]))
    return lengths


def _nearest_index_pair(accumulated, target):
    """The segment a target arc length falls in, upstream's way.

    Not a plain search: the first branch compares against `accumulated[1]` and the
    second against `accumulated[-2]`, so a target exactly on an interior vertex near
    either end is attributed to the end segment rather than to the one a bisection
    would pick. Copied rather than rewritten for that reason.
    """
    n = len(accumulated)
    if target < accumulated[1]:
        return 0, 1
    if target > accumulated[n - 2]:
        return n - 2, n - 1
    for i in range(1, n):
        if accumulated[i - 1] <= target <= accumulated[i]:
            return i - 1, i
    raise RuntimeError("No nearest point found.")


def _resample_points(linestring, num_segments):
    """`num_segments + 1` points spaced evenly by arc length along a linestring."""
    line_length = length(linestring)
    accumulated = _accumulated_lengths(linestring)
    if len(accumulated) < 2:
        return []

    points = []
    for i in range(num_segments + 1):
        target = (float(i) / num_segments) * line_length
        back, front = _nearest_index_pair(accumulated, target)
        back_point = linestring[back]
        front_point = linestring[front]
        segment = accumulated[front] - accumulated[back]
        ratio = (target - accumulated[back]) / segment
        points.append(
            (
                back_point.x + (front_point.x - back_point.x) * ratio,
                back_point.y + (front_point.y - back_point.y) * ratio,
                back_point.z + (front_point.z - back_point.z) * ratio,
            )
        )
    return points


def _segment_count(lanelet, resolution):
    """How many segments to cut both bounds into.

    The *longer* bound decides, so the two are resampled to the same count and their
    points pair up index by index.
    """
    longer = max(length(lanelet.leftBound), length(lanelet.rightBound))
    return max(int(math.ceil(longer / resolution)), 1)


def _resampled_bounds(lanelet, resolution):
    segments = _segment_count(lanelet, resolution)
    return (
        _resample_points(lanelet.leftBound, segments),
        _resample_points(lanelet.rightBound, segments),
        segments,
    )


def _normalised(a, b):
    """The unit vector from `b` to `a`."""
    dx, dy, dz = a[0] - b[0], a[1] - b[1], a[2] - b[2]
    norm = math.sqrt(dx * dx + dy * dy + dz * dz)
    if norm == 0.0:
        return 0.0, 0.0, 0.0
    return dx / norm, dy / norm, dz / norm


def _linestring(points):
    return LineString3d(getId(), [Point3d(getId(), *p) for p in points])


def generateFineCenterline(lanelet_obj, resolution=DEFAULT_RESOLUTION):
    """A centerline resampled to roughly `resolution`-metre spacing."""
    left, right, segments = _resampled_bounds(lanelet_obj, resolution)
    return _linestring(
        [
            (
                (right[i][0] + left[i][0]) / 2.0,
                (right[i][1] + left[i][1]) / 2.0,
                (right[i][2] + left[i][2]) / 2.0,
            )
            for i in range(segments + 1)
        ]
    )


def getCenterlineWithOffset(lanelet_obj, offset, resolution=DEFAULT_RESOLUTION):
    """The centerline shifted sideways, positive being towards the left bound."""
    left, right, segments = _resampled_bounds(lanelet_obj, resolution)
    points = []
    for i in range(segments + 1):
        centre = (
            (right[i][0] + left[i][0]) / 2.0,
            (right[i][1] + left[i][1]) / 2.0,
            (right[i][2] + left[i][2]) / 2.0,
        )
        direction = _normalised(left[i], right[i])
        points.append(tuple(centre[k] + direction[k] * offset for k in range(3)))
    return _query.__as_const_lines__([_linestring(points)])[0]


def getRightBoundWithOffset(lanelet_obj, offset, resolution=DEFAULT_RESOLUTION):
    """The right bound shifted, positive being *away* from the left bound."""
    left, right, segments = _resampled_bounds(lanelet_obj, resolution)
    points = []
    for i in range(segments + 1):
        direction = _normalised(right[i], left[i])
        points.append(tuple(right[i][k] + direction[k] * offset for k in range(3)))
    return _query.__as_const_lines__([_linestring(points)])[0]


def getLeftBoundWithOffset(lanelet_obj, offset, resolution=DEFAULT_RESOLUTION):
    """The left bound shifted, positive being *away* from the right bound."""
    left, right, segments = _resampled_bounds(lanelet_obj, resolution)
    points = []
    for i in range(segments + 1):
        direction = _normalised(left[i], right[i])
        points.append(tuple(left[i][k] + direction[k] * offset for k in range(3)))
    return _query.__as_const_lines__([_linestring(points)])[0]


def getClosestSegment(search_point, linestring):
    """The one segment of a linestring nearest a 2D point.

    Ties go to the earlier segment, since the comparison is strict.
    """
    if len(linestring) < 2:
        return LineString3d(getId(), [])
    closest = None
    best = float("inf")
    for i in range(1, len(linestring)):
        previous, current = linestring[i - 1], linestring[i]
        segment = LineString3d(
            getId(),
            [
                Point3d(getId(), previous.x, previous.y, previous.z),
                Point3d(getId(), current.x, current.y, current.z),
            ],
        )
        gap = distance(to2D(segment), search_point)
        if gap < best:
            closest, best = segment, gap
    return _query.__as_const_lines__([closest])[0]


def _unique_points(points, existing):
    """Appends points that are not already within a centimetre of one present.

    Upstream's threshold is 0.01 m and the comparison is against *every* point
    collected so far, not just the previous one, so a lanelet that doubles back does
    not contribute the same point twice.
    """
    for point in points:
        duplicate = any(
            distance(Point3d(0, point.x, point.y, point.z), other) <= 0.01
            for other in existing
        )
        if not duplicate:
            existing.append(Point3d(getId(), point.x, point.y, point.z))
    return existing


def combineLaneletsShape(lanelets):
    """One lanelet spanning a run of them, with duplicate join points dropped."""
    lefts, rights, centres = [], [], []
    for lanelet_obj in lanelets:
        _unique_points(lanelet_obj.leftBound, lefts)
        _unique_points(lanelet_obj.rightBound, rights)
        _unique_points(lanelet_obj.centerline, centres)
    combined = Lanelet(getId(), LineString3d(getId(), lefts), LineString3d(getId(), rights))
    combined.centerline = LineString3d(getId(), centres)
    return combined


def _linestring_from_arc_length(linestring, s1, s2):
    """The stretch of a linestring between two arc lengths.

    Ported literally: the start and end scans are separate loops, each accumulating
    from zero, and each stops at the *first* segment that overshoots. An `s1` past the
    end therefore leaves `start_index` at `len(linestring)`, which the interpolation
    guards against rather than clamping beforehand.

    Every point here is built with the invalid id, as upstream does. That is not
    cosmetic: the polygon these bounds end up in drops points that repeat at the
    joins, and which ones count as repeats depends on their ids.
    """
    points = []
    if len(linestring) == 0:
        return LineString3d(getId(), points)

    accumulated = 0.0
    start_index = len(linestring)
    for i in range(len(linestring) - 1):
        step = distance(linestring[i], linestring[i + 1])
        if accumulated + step > s1:
            start_index = i
            break
        accumulated += step
    if start_index < len(linestring) - 1:
        p1, p2 = linestring[start_index], linestring[start_index + 1]
        direction = _normalised((p2.x, p2.y, p2.z), (p1.x, p1.y, p1.z))
        residue = s1 - accumulated
        points.append(
            Point3d(0, *(v + d * residue for v, d in zip((p1.x, p1.y, p1.z), direction)))
        )

    accumulated = 0.0
    end_index = len(linestring)
    for i in range(len(linestring) - 1):
        step = distance(linestring[i], linestring[i + 1])
        if accumulated + step > s2:
            end_index = i
            break
        accumulated += step

    for i in range(start_index + 1, min(end_index, len(linestring))):
        p = linestring[i]
        points.append(Point3d(0, p.x, p.y, p.z))

    if end_index < len(linestring) - 1:
        p1, p2 = linestring[end_index], linestring[end_index + 1]
        direction = _normalised((p2.x, p2.y, p2.z), (p1.x, p1.y, p1.z))
        residue = s2 - accumulated
        points.append(
            Point3d(0, *(v + d * residue for v, d in zip((p1.x, p1.y, p1.z), direction)))
        )
    return LineString3d(0, points)


def getPolygonFromArcLength(lanelets, s1, s2):
    """The outline of the stretch of a lanelet run between two arc lengths.

    The arc lengths are measured along the *combined* lanelet in 2D, then converted
    to a ratio and applied to each bound's own 3D length -- so the two bounds are cut
    proportionally rather than at the same distance.
    """
    from lanelet2.geometry import length2d

    combined = combineLaneletsShape(lanelets)
    total = length2d(combined)
    if total == 0.0:
        return Lanelet(0, LineString3d(0, []), LineString3d(0, [])).polygon3d()

    ratio1 = max(0.0, min(s1, total)) / total
    ratio2 = max(0.0, min(s2, total)) / total
    # Summed the same way the scan below sums, not through `length`. The two agree
    # mathematically and not bit for bit, and with `s2` saturated to exactly the total
    # the scan's strict `>` lands on either side of the last segment depending on
    # which sum was used -- the difference being one interpolated end point.
    left_length = _accumulated_lengths(combined.leftBound)[-1]
    right_length = _accumulated_lengths(combined.rightBound)[-1]
    left = _linestring_from_arc_length(
        combined.leftBound, ratio1 * left_length, ratio2 * left_length
    )
    right = _linestring_from_arc_length(
        combined.rightBound, ratio1 * right_length, ratio2 * right_length
    )
    return Lanelet(0, left, right).polygon3d()


def overwriteLaneletsCenterline(lanelet_map, resolution=DEFAULT_RESOLUTION, force_overwrite=False):
    """Replaces every computed centerline with a finely resampled one.

    A centerline the map author supplied is left alone unless `force_overwrite`; that
    is what `hasCustomCenterline` distinguishes.
    """
    for lanelet_obj in lanelet_map.laneletLayer:
        if force_overwrite or not lanelet_obj.hasCustomCenterline():
            lanelet_obj.centerline = generateFineCenterline(lanelet_obj, resolution)


def getConflictingLanelets(graph, lanelet_obj):
    """Lanelets whose area a lanelet overlaps, dropping the conflicting *areas*.

    Upstream's binding wants a graph type Python cannot produce, so this cannot be
    called there at all. Repaired by default; bug-compat keeps the ArgumentError.
    """
    if lanelet2.BUG_COMPAT:
        raise _query.__argument_error__("getConflictingLanelets")
    return [x for x in graph.conflicting(lanelet_obj) if isinstance(x, type(lanelet_obj))]


_NOT_A_LANELET = "argument number does not match or 1st argument is not Lanelet or [Lanelet] type"


def _lanelet_length(lanelet_obj, measure):
    """Length of a lanelet, or of a run of them.

    Upstream's shim accepts only the list form -- its type dispatch never matches a
    bare lanelet and it raises. The C++ has both. Repaired by default, reproduced
    under bug-compat.
    """
    if isinstance(lanelet_obj, (list, tuple)):
        return sum(measure(x) for x in lanelet_obj)
    if lanelet2.BUG_COMPAT:
        raise TypeError(_NOT_A_LANELET)
    return measure(lanelet_obj)


def getLaneletLength2d(lanelet_obj):
    from lanelet2.geometry import length2d

    return _lanelet_length(lanelet_obj, length2d)


def getLaneletLength3d(lanelet_obj):
    from lanelet2.geometry import length3d

    return _lanelet_length(lanelet_obj, length3d)


def lineStringWithWidthToPolygon(linestring):
    """A linestring of exactly two points, widened by its `width` tag.

    Returns None rather than raising when the shape or the tag is missing, which is
    how upstream's bool-and-out-parameter form reads from Python.
    """
    if len(linestring) != 2 or "width" not in linestring.attributes:
        return None
    width = float(linestring.attributes["width"])
    front, back = linestring[0], linestring[-1]
    # Rotating the direction a quarter turn about z gives the left-hand normal.
    direction = _normalised((back.x, back.y, back.z), (front.x, front.y, front.z))
    left = (-direction[1], direction[0], 0.0)
    half = width / 2.0
    corners = [
        tuple(v + n * half for v, n in zip((front.x, front.y, front.z), left)),
        tuple(v + n * half for v, n in zip((back.x, back.y, back.z), left)),
        tuple(v - n * half for v, n in zip((back.x, back.y, back.z), left)),
        tuple(v - n * half for v, n in zip((front.x, front.y, front.z), left)),
    ]
    return Polygon3d(getId(), [Point3d(getId(), *c) for c in corners])


def lineStringToPolygon(linestring):
    """A closed linestring read as a polygon.

    Fewer than four points is only accepted when there are three *distinct* ones --
    upstream compares the first and last ids, so a ring written as four points with
    the ends repeated is fine while a triangle written the same way is not.
    """
    if len(linestring) < 4 and (
        len(linestring) < 3 or linestring[0].id == linestring[-1].id
    ):
        return None
    return Polygon3d(
        getId(), [Point3d(getId(), p.x, p.y, p.z) for p in linestring]
    )


_ROS_ONLY = (
    "getArcCoordinates",
    "getClosestCenterPose",
    "getLaneletAngle",
    "isInLanelet",
    "getLateralDistanceToCenterline",
    "getLateralDistanceToClosestLanelet",
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
    "combineLaneletsShape",
    "generateFineCenterline",
    "getConflictingLanelets",
    "getLaneletLength2d",
    "getLaneletLength3d",
    "getPolygonFromArcLength",
    "lineStringToPolygon",
    "lineStringWithWidthToPolygon",
    "overwriteLaneletsCenterline",
    "getCenterlineWithOffset",
    "getLeftBoundWithOffset",
    "getRightBoundWithOffset",
    "getClosestSegment",
    *_ROS_ONLY,
]
