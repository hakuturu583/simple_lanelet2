"""The centerline algorithm, against forty procedurally generated shapes.

This is the single most important case in the project. `calculateCenterline` is not
naive midpoint pairing — the two bounds usually differ in point count and spacing,
so the algorithm advances one side at a time, each step choosing the nearest point
whose resulting centerline segment crosses neither bound, with a triangle-inequality
early exit and special handling for a lanelet closed at its start.

Every downstream number rides on the exact output: `length2d`, arc coordinates,
lane matching, and `RoutingCostTravelTime`. So this case runs before anything that
consumes a centerline, and the shapes are chosen to exercise the parts of the
search that a well-behaved rectangular lanelet never reaches: pinches, reflex
turns, wildly unequal sampling, and shared endpoints.

Coordinates are rounded to 9 decimals on emit so the comparison is about the
algorithm's *choices* — which point it advances to — rather than about the last
bits of a midpoint average.
"""

import math
import random

from canon import emit, run

from lanelet2.core import Lanelet, LineString3d, Point3d, getId


def linestring(points):
    return LineString3d(getId(), [Point3d(getId(), x, y, z) for x, y, z in points])


def centerline_of(left_points, right_points):
    lanelet = Lanelet(getId(), linestring(left_points), linestring(right_points))
    return [[round(p.x, 9), round(p.y, 9), round(p.z, 9)] for p in lanelet.centerline]


def straight(count, offset, length=10.0):
    return [(length * i / (count - 1), offset, 0.0) for i in range(count)]


def arc(count, radius, start=0.0, sweep=math.pi / 2):
    return [
        (
            radius * math.cos(start + sweep * i / (count - 1)),
            radius * math.sin(start + sweep * i / (count - 1)),
            0.0,
        )
        for i in range(count)
    ]


def shapes():
    """Yields (name, left, right) triples. Deterministic by construction."""
    rng = random.Random(20260731)

    # --- evenly sampled parallel bounds, every combination of point counts ---
    for left_count in (2, 3, 5, 9):
        for right_count in (2, 3, 5, 9):
            yield (
                "parallel_{}x{}".format(left_count, right_count),
                straight(left_count, 1.5),
                straight(right_count, -1.5),
            )

    # --- concentric arcs: the inner bound is genuinely shorter ---
    for count in (3, 5, 12):
        yield ("arc_{}".format(count), arc(count, 12.0), arc(count, 8.0))
    yield ("arc_asymmetric_sampling", arc(20, 12.0), arc(3, 8.0))
    yield ("arc_full_half_turn", arc(9, 12.0, sweep=math.pi), arc(9, 8.0, sweep=math.pi))

    # --- a pinch: the bounds converge to nearly touching in the middle ---
    yield (
        "pinched",
        [(0.0, 2.0, 0.0), (5.0, 0.2, 0.0), (10.0, 2.0, 0.0)],
        [(0.0, -2.0, 0.0), (5.0, -0.2, 0.0), (10.0, -2.0, 0.0)],
    )
    yield (
        "widening",
        [(0.0, 0.3, 0.0), (5.0, 2.0, 0.0), (10.0, 6.0, 0.0)],
        [(0.0, -0.3, 0.0), (5.0, -2.0, 0.0), (10.0, -6.0, 0.0)],
    )

    # --- reflex turns, which is where the non-intersection guards earn their keep ---
    yield (
        "zigzag",
        [(0.0, 1.0, 0.0), (2.0, 3.0, 0.0), (4.0, 1.0, 0.0), (6.0, 3.0, 0.0)],
        [(0.0, -1.0, 0.0), (2.0, 1.0, 0.0), (4.0, -1.0, 0.0), (6.0, 1.0, 0.0)],
    )
    yield (
        "sharp_right_turn",
        [(0.0, 1.0, 0.0), (4.0, 1.0, 0.0), (4.0, -4.0, 0.0)],
        [(0.0, -1.0, 0.0), (2.0, -1.0, 0.0), (2.0, -4.0, 0.0)],
    )

    # --- elevation must be averaged even though the search is planar ---
    yield (
        "sloped",
        [(0.0, 1.0, 0.0), (5.0, 1.0, 2.5), (10.0, 1.0, 5.0)],
        [(0.0, -1.0, 1.0), (5.0, -1.0, 3.5), (10.0, -1.0, 6.0)],
    )

    # --- degenerate inputs ---
    yield ("single_point_each", [(0.0, 1.0, 0.0)], [(0.0, -1.0, 0.0)])
    yield ("two_and_one", straight(2, 1.0), [(0.0, -1.0, 0.0)])
    yield (
        "duplicate_points",
        [(0.0, 1.0, 0.0), (5.0, 1.0, 0.0), (5.0, 1.0, 0.0), (10.0, 1.0, 0.0)],
        [(0.0, -1.0, 0.0), (10.0, -1.0, 0.0)],
    )
    yield (
        "zero_width",
        [(0.0, 0.0, 0.0), (5.0, 0.0, 0.0), (10.0, 0.0, 0.0)],
        [(0.0, 0.0, 0.0), (10.0, 0.0, 0.0)],
    )
    yield (
        "bounds_cross",
        [(0.0, 1.0, 0.0), (10.0, -1.0, 0.0)],
        [(0.0, -1.0, 0.0), (10.0, 1.0, 0.0)],
    )

    # --- randomly perturbed corridors, the realistic case ---
    for index in range(8):
        count = rng.randint(2, 10)
        left, right = [], []
        x = 0.0
        for _ in range(count):
            x += rng.uniform(0.5, 3.0)
            left.append((x + rng.uniform(-0.2, 0.2), 1.5 + rng.uniform(-0.6, 0.6), 0.0))
        x = 0.0
        for _ in range(rng.randint(2, 10)):
            x += rng.uniform(0.5, 3.0)
            right.append((x + rng.uniform(-0.2, 0.2), -1.5 + rng.uniform(-0.6, 0.6), 0.0))
        yield ("random_{}".format(index), left, right)


def main():
    for name, left, right in shapes():
        emit("centerline:" + name, centerline_of(left, right))

    # A lanelet closed at its start shares the literal first Point object between
    # its bounds. Upstream detects this by object identity, not by coordinates, and
    # skips the left bound's first point -- so the two cases below differ.
    shared = Point3d(getId(), 0.0, 0.0, 0.0)
    closed = Lanelet(
        getId(),
        LineString3d(getId(), [shared, Point3d(getId(), 5.0, 2.0, 0.0), Point3d(getId(), 10.0, 2.0, 0.0)]),
        LineString3d(getId(), [shared, Point3d(getId(), 5.0, -2.0, 0.0), Point3d(getId(), 10.0, -2.0, 0.0)]),
    )
    emit("closed_at_start", [[round(p.x, 9), round(p.y, 9)] for p in closed.centerline])

    coincident = centerline_of(
        [(0.0, 0.0, 0.0), (5.0, 2.0, 0.0), (10.0, 2.0, 0.0)],
        [(0.0, 0.0, 0.0), (5.0, -2.0, 0.0), (10.0, -2.0, 0.0)],
    )
    emit("coincident_but_distinct_start", [[point[0], point[1]] for point in coincident])

    # The computed centerline carries no id, and neither do its points; that is how
    # a user-assigned centerline is later told apart from a computed one.
    lanelet = Lanelet(getId(), linestring(straight(3, 1.0)), linestring(straight(3, -1.0)))
    emit("computed_centerline_id", lanelet.centerline.id)
    emit("computed_point_ids", [p.id for p in lanelet.centerline])

    # Caching: the same object comes back, and a bound change invalidates it.
    first = [round(p.x, 9) for p in lanelet.centerline]
    emit("cached_repeat", [round(p.x, 9) for p in lanelet.centerline] == first)
    lanelet.leftBound = linestring(straight(3, 4.0))
    emit("after_bound_change", [[round(p.x, 9), round(p.y, 9)] for p in lanelet.centerline])

    # An inverted lanelet reads its centerline backwards.
    forward = Lanelet(getId(), linestring(straight(4, 1.0)), linestring(straight(4, -1.0)))
    emit("forward_xs", [round(p.x, 9) for p in forward.centerline])
    emit("inverted_xs", [round(p.x, 9) for p in forward.invert().centerline])


run(main)
