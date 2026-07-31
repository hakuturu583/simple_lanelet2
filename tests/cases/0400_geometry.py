"""The geometry module: 36 functions, checked against real and synthetic shapes.

Upstream registers these through Boost.Python's overload chain -- `distance` alone
has 55 registrations -- so the type combinations matter as much as the numbers. The
sweeps below walk the accepted argument shapes for each function rather than
checking one case per name.

Two conventions that are easy to invert and are pinned explicitly: arc-coordinate
`distance` is positive to the *left* of travel, and `leftOf`/`rightOf`/`follows`
compare object identity rather than coordinates.
"""

import math

from canon import by_id, data_path, emit, expect_raises, run

import lanelet2.geometry as g
from lanelet2.core import (
    BasicPoint2d,
    ConstHybridLineString3d,
    ConstHybridPolygon2d,
    ConstPolygon2d,
    BasicPoint3d,
    BoundingBox2d,
    BoundingBox3d,
    ConstLineString2d,
    ConstLineString3d,
    Lanelet,
    LineString3d,
    Point3d,
    Polygon3d,
    createMapFromLanelets,
    getId,
)
from lanelet2.io import Origin, load
from lanelet2.projection import UtmProjector


def ls(points, id=None):
    return LineString3d(id or getId(), [Point3d(getId(), x, y, z) for x, y, z in points])


def flat(points):
    return [(x, y, 0.0) for x, y in points]


def corridor(x0, x1, y=1.0):
    left = ls(flat([(x0, y), (x1, y)]))
    right = ls(flat([(x0, -y), (x1, -y)]))
    return Lanelet(getId(), left, right)


def r(value, places=9):
    return round(value, places)


def vertical_for_3d():
    return Lanelet(getId(), ls(flat([(4.0, -5.0), (4.0, 5.0)])),
                   ls(flat([(6.0, -5.0), (6.0, 5.0)])))


def main():
    line = ls(flat([(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)]))
    line2d = g.to2D(line)
    diagonal = ls([(0.0, 0.0, 0.0), (10.0, 10.0, 5.0)])

    # --- conversions ---------------------------------------------------------
    emit("to2d_types", [type(g.to2D(x)).__name__ for x in
                        [Point3d(getId(), 1.0, 2.0, 3.0), BasicPoint3d(1.0, 2.0, 3.0), line,
                         Polygon3d(getId(), [Point3d(getId(), 0.0, 0.0, 0.0),
                                             Point3d(getId(), 1.0, 0.0, 0.0),
                                             Point3d(getId(), 1.0, 1.0, 0.0)])]])
    emit("to3d_types", [type(g.to3D(x)).__name__ for x in
                        [g.to2D(Point3d(getId(), 1.0, 2.0, 3.0)), BasicPoint2d(1.0, 2.0), line2d]])
    # Something with no counterpart comes back unchanged.
    emit("to2d_passthrough", type(g.to2D(BasicPoint2d(1.0, 2.0))).__name__)
    shared = Point3d(getId(), 1.0, 2.0, 3.0)
    emit("to2d_shares_storage", [g.to2D(shared).x, shared.z])

    # --- distance ------------------------------------------------------------
    a3, b3 = BasicPoint3d(0.0, 0.0, 0.0), BasicPoint3d(3.0, 4.0, 12.0)
    a2, b2 = BasicPoint2d(0.0, 0.0), BasicPoint2d(3.0, 4.0)
    emit("distance_pp3", r(g.distance(a3, b3)))
    emit("distance_pp2", r(g.distance(a2, b2)))
    emit("distance_point_line", r(g.distance(BasicPoint2d(5.0, 3.0), line2d)))
    emit("distance_line_point", r(g.distance(line2d, BasicPoint2d(5.0, 3.0))))
    emit("distance_line_line", r(g.distance(line2d, g.to2D(diagonal))))
    emit("distance_point_primitive", r(g.distance(Point3d(getId(), 0.0, 0.0, 0.0),
                                                  Point3d(getId(), 3.0, 4.0, 0.0))))

    llt = corridor(0.0, 10.0)
    emit("distance_lanelet_point", r(g.distance(llt, BasicPoint2d(5.0, 5.0))))
    emit("distance_point_lanelet", r(g.distance(BasicPoint2d(5.0, 5.0), llt)))
    emit("distance_inside_lanelet", r(g.distance(llt, BasicPoint2d(5.0, 0.0))))

    emit("distance_to_lines", r(g.distanceToLines(BasicPoint2d(5.0, 3.0), [line2d])))

    emit("equals_same", g.equals(BasicPoint2d(1.0, 2.0), BasicPoint2d(1.0, 2.0)))
    emit("equals_different", g.equals(BasicPoint2d(1.0, 2.0), BasicPoint2d(1.0, 2.5)))

    # --- bounding boxes ------------------------------------------------------
    box = g.boundingBox2d(line2d)
    emit("bbox2d", [r(box.min.x), r(box.min.y), r(box.max.x), r(box.max.y)])
    box3 = g.boundingBox3d(diagonal)
    emit("bbox3d", [r(box3.min.x), r(box3.min.y), r(box3.min.z),
                    r(box3.max.x), r(box3.max.y), r(box3.max.z)])
    llt_box = g.boundingBox2d(llt)
    emit("bbox2d_lanelet", [r(llt_box.min.x), r(llt_box.min.y),
                            r(llt_box.max.x), r(llt_box.max.y)])
    point2d = g.to2D(Point3d(getId(), 1.0, 2.0, 3.0))
    emit("bbox2d_point", [r(g.boundingBox2d(point2d).min.x), r(g.boundingBox2d(point2d).max.y)])
    expect_raises("bbox2d_rejects_3d", lambda: g.boundingBox2d(line))

    # --- lengths and interpolation -------------------------------------------
    emit("length2d_line", r(g.length(line2d)))
    emit("length3d_line", r(g.length(line)))
    emit("length2d_lanelet", r(g.length2d(llt)))
    emit("length3d_lanelet", r(g.length3d(llt)))
    emit("approximated_length", r(g.approximatedLength2d(llt)))
    emit("distance_to_centerline2d", r(g.distanceToCenterline2d(llt, BasicPoint2d(5.0, 3.0))))
    emit("distance_to_centerline3d", r(g.distanceToCenterline3d(llt, BasicPoint3d(5.0, 3.0, 4.0))))

    for at in [-5.0, 0.0, 0.5, 5.0, 10.0, 15.0, 20.0, 99.0]:
        point = g.interpolatedPointAtDistance(line2d, at)
        emit("interp:{}".format(at), [r(point.x), r(point.y)])
        emit("nearest:{}".format(at), g.nearestPointAtDistance(line2d, at).id)

    emit("project", [r(v) for v in [g.project(line2d, BasicPoint2d(5.0, 3.0)).x,
                                    g.project(line2d, BasicPoint2d(5.0, 3.0)).y]])
    pair = g.projectedPoint3d(line, diagonal)
    emit("projected_point_3d", [[r(pair[0].x), r(pair[0].y), r(pair[0].z)],
                                [r(pair[1].x), r(pair[1].y), r(pair[1].z)]])

    # --- arc coordinates -----------------------------------------------------
    straight = g.to2D(ls(flat([(0.0, 0.0), (10.0, 0.0)])))
    for x, y in [(4.0, 2.0), (4.0, -2.0), (-1.0, 1.0), (11.0, 0.0), (0.0, 0.0)]:
        arc = g.toArcCoordinates(straight, BasicPoint2d(x, y))
        emit("arc:{}:{}".format(x, y), [r(arc.length), r(arc.distance)])
    corner_arc = g.toArcCoordinates(line2d, BasicPoint2d(11.0, 5.0))
    emit("arc_corner", [r(corner_arc.length), r(corner_arc.distance)])

    emit("arc_repr", repr(g.ArcCoordinates(1.0, -0.5)))
    emit("arc_str", str(g.ArcCoordinates(1.0, -0.5)))
    emit("arc_default", repr(g.ArcCoordinates()))
    emit("arc_eq", g.ArcCoordinates(1.0, 2.0) == g.ArcCoordinates(1.0, 2.0))
    emit("arc_fields", [g.ArcCoordinates(1.5, -2.5).length, g.ArcCoordinates(1.5, -2.5).distance])

    back = g.fromArcCoordinates(straight, g.ArcCoordinates(4.0, 2.0))
    emit("from_arc", [r(back.x), r(back.y)])
    expect_raises("from_arc_degenerate",
                  lambda: g.fromArcCoordinates(g.to2D(ls(flat([(0.0, 0.0)]))),
                                               g.ArcCoordinates(1.0, 0.0)), msg=True)

    # --- curvature and area --------------------------------------------------
    emit("curvature_circle", r(g.curvature2d(BasicPoint2d(1.0, 0.0), BasicPoint2d(0.0, 1.0),
                                             BasicPoint2d(-1.0, 0.0))))
    emit("signed_curvature_left", r(g.signedCurvature2d(BasicPoint2d(1.0, 0.0), BasicPoint2d(0.0, 1.0),
                                                        BasicPoint2d(-1.0, 0.0))))
    emit("signed_curvature_right", r(g.signedCurvature2d(BasicPoint2d(-1.0, 0.0), BasicPoint2d(0.0, 1.0),
                                                         BasicPoint2d(1.0, 0.0))))
    emit("curvature_collinear", g.curvature2d(BasicPoint2d(0.0, 0.0), BasicPoint2d(1.0, 0.0),
                                              BasicPoint2d(2.0, 0.0)))
    emit("curvature_coincident", g.curvature2d(BasicPoint2d(0.0, 0.0), BasicPoint2d(0.0, 0.0),
                                               BasicPoint2d(2.0, 0.0)))

    # `area` is registered for ConstHybridPolygon2d alone; a list of basic points
    # has no overload, and neither does a Polygon2d.
    corners = [(0.0, 0.0), (0.0, 4.0), (4.0, 4.0), (4.0, 0.0)]
    clockwise = ConstHybridPolygon2d(ConstPolygon2d(
        getId(), [Point3d(getId(), x, y, 0.0) for x, y in corners]))
    anticlockwise = ConstHybridPolygon2d(ConstPolygon2d(
        getId(), [Point3d(getId(), x, y, 0.0) for x, y in reversed(corners)]))
    emit("area_clockwise", r(g.area(clockwise)))
    emit("area_counter_clockwise", r(g.area(anticlockwise)))
    expect_raises("area_rejects_list",
                  lambda: g.area([BasicPoint2d(x, y) for x, y in corners]))

    # --- predicates ----------------------------------------------------------
    crossing = g.to2D(ls(flat([(5.0, -5.0), (5.0, 5.0)])))
    emit("intersects2d_crossing", g.intersects2d(line2d, crossing))
    emit("intersects2d_apart", g.intersects2d(line2d, g.to2D(ls(flat([(0.0, 100.0), (10.0, 100.0)])))))
    emit("intersects2d_boxes", g.intersects2d(BoundingBox3d(BasicPoint3d(0.0, 0.0, 0.0),
                                                            BasicPoint3d(2.0, 2.0, 2.0)),
                                              BoundingBox3d(BasicPoint3d(1.0, 1.0, 1.0),
                                                            BasicPoint3d(3.0, 3.0, 3.0))))
    # intersects3d takes hybrid or compound linestrings, not a plain LineString3d.
    def hybrid3(points):
        return ConstHybridLineString3d(ConstLineString3d(
            getId(), [Point3d(getId(), x, y, z) for x, y, z in points]))
    # The linestring overloads leave heightTolerance without a default, so they
    # are unreachable from a two-argument call.
    emit("intersects3d_lines",
         g.intersects3d(hybrid3([(0.0, 0.0, 0.0), (10.0, 0.0, 0.0)]),
                        hybrid3([(5.0, -5.0, 0.0), (5.0, 5.0, 0.0)]), 3.0))
    emit("intersects3d_far_apart_in_z",
         g.intersects3d(hybrid3([(0.0, 0.0, 0.0), (10.0, 0.0, 0.0)]),
                        hybrid3([(5.0, -5.0, 100.0), (5.0, 5.0, 100.0)]), 3.0))
    expect_raises("intersects3d_lines_need_a_tolerance",
                  lambda: g.intersects3d(hybrid3([(0.0, 0.0, 0.0), (10.0, 0.0, 0.0)]),
                                         hybrid3([(5.0, -5.0, 0.0), (5.0, 5.0, 0.0)])))
    emit("intersects3d_lanelets", g.intersects3d(llt, vertical_for_3d()))

    emit("inside_middle", g.inside(llt, BasicPoint2d(5.0, 0.0)))
    emit("inside_boundary", g.inside(llt, BasicPoint2d(5.0, 1.0)))
    emit("inside_outside", g.inside(llt, BasicPoint2d(5.0, 2.0)))

    # Adjacency is identity: two lanelets sharing a boundary *object*.
    shared_bound = ls(flat([(0.0, 0.0), (10.0, 0.0)]))
    upper = Lanelet(getId(), ls(flat([(0.0, 2.0), (10.0, 2.0)])), shared_bound)
    lower = Lanelet(getId(), shared_bound, ls(flat([(0.0, -2.0), (10.0, -2.0)])))
    emit("left_of", g.leftOf(upper, lower))
    emit("right_of", g.rightOf(lower, upper))
    emit("left_of_coincident_but_distinct",
         g.leftOf(upper, Lanelet(getId(), ls(flat([(0.0, 0.0), (10.0, 0.0)])),
                                 ls(flat([(0.0, -2.0), (10.0, -2.0)])))))
    emit("overlaps_neighbours", g.overlaps2d(upper, lower))
    emit("overlaps_self", g.overlaps2d(upper, upper))

    vertical = Lanelet(getId(), ls(flat([(4.0, -5.0), (4.0, 5.0)])),
                       ls(flat([(6.0, -5.0), (6.0, 5.0)])))
    emit("overlaps_crossing", g.overlaps2d(llt, vertical))
    emit("overlaps3d_crossing", g.overlaps3d(llt, vertical))
    emit("overlaps_far", g.overlaps2d(llt, corridor(100.0, 110.0)))
    emit("intersect_centerlines",
         [[r(p.x), r(p.y)] for p in g.intersectCenterlines2d(llt, vertical)])

    # Succession needs the *same* end point objects.
    mid_left = Point3d(getId(), 10.0, 1.0, 0.0)
    mid_right = Point3d(getId(), 10.0, -1.0, 0.0)
    first = Lanelet(getId(),
                    LineString3d(getId(), [Point3d(getId(), 0.0, 1.0, 0.0), mid_left]),
                    LineString3d(getId(), [Point3d(getId(), 0.0, -1.0, 0.0), mid_right]))
    second = Lanelet(getId(),
                     LineString3d(getId(), [mid_left, Point3d(getId(), 20.0, 1.0, 0.0)]),
                     LineString3d(getId(), [mid_right, Point3d(getId(), 20.0, -1.0, 0.0)]))
    emit("follows", g.follows(first, second))
    emit("follows_reversed", g.follows(second, first))
    emit("follows_coincident", g.follows(first, corridor(10.0, 20.0)))

    emit("intersection", [[r(p.x), r(p.y)] for p in g.intersection(line2d, crossing)])

    # --- map queries ---------------------------------------------------------
    map = createMapFromLanelets([corridor(0.0, 10.0), corridor(20.0, 30.0), corridor(40.0, 50.0)])
    found = g.findNearest(map.laneletLayer, BasicPoint2d(0.0, 0.0), 2)
    emit("find_nearest", [[r(d), p.id] for d, p in found])
    within = g.findWithin2d(map.laneletLayer, BasicPoint2d(25.0, 0.0), 0.0)
    emit("find_within_touching", [[r(d), p.id] for d, p in within])
    within = g.findWithin2d(map.laneletLayer, BasicPoint2d(15.0, 0.0), 10.0)
    emit("find_within_ranged", sorted([[r(d), p.id] for d, p in within]))

    # --- against the real map ------------------------------------------------
    real = load(data_path("mapping_example.osm"), UtmProjector(Origin(49.0, 8.4, 0.0)))
    lanelets = by_id(real.laneletLayer)[:60]
    emit("real:lengths", [[ll.id, r(g.length2d(ll), 6), r(g.length3d(ll), 6),
                           r(g.approximatedLength2d(ll), 6)] for ll in lanelets])
    emit("real:bboxes", [[ll.id, r(g.boundingBox2d(ll).min.x, 6), r(g.boundingBox2d(ll).max.y, 6)]
                         for ll in lanelets])
    probe = BasicPoint2d(0.0, 0.0)
    emit("real:distances", [[ll.id, r(g.distance(ll, probe), 6),
                             r(g.distanceToCenterline2d(ll, probe), 6)] for ll in lanelets])
    emit("real:arc", [[ll.id, r(g.toArcCoordinates(g.to2D(ll.centerline), probe).length, 6),
                       r(g.toArcCoordinates(g.to2D(ll.centerline), probe).distance, 6)]
                      for ll in lanelets])
    emit("real:inside", [[ll.id, g.inside(ll, probe)] for ll in lanelets])
    emit("real:follows", [[a.id, b.id, g.follows(a, b), g.leftOf(a, b)]
                          for a in lanelets[:20] for b in lanelets[:20] if a.id != b.id])


run(main)
