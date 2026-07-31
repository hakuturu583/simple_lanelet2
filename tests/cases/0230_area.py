"""Areas: one outer ring, any number of holes, no direction of travel.

Each ring is a *list* of linestrings chained end to start, so the inner bounds are
nested. `ConstArea.__repr__` claims to be an `Area` upstream, because the repr
helper takes a display name and then ignores it — repaired by default.
"""

from canon import emit, expect_raises, run

from lanelet2.core import (
    Area,
    AttributeMap,
    ConstArea,
    LineString3d,
    Point3d,
)


def line(id, points):
    return LineString3d(id, [Point3d(id * 100 + i, x, y, 0.0) for i, (x, y) in enumerate(points)])


def square(id, size=4.0):
    return line(id, [(0.0, 0.0), (size, 0.0), (size, size), (0.0, size)])


def main():
    area = Area(10, [square(11)], [], AttributeMap({"subtype": "parking"}))
    emit("repr", repr(area))
    emit("id", area.id)
    emit("cls", type(area).__module__ + "." + type(area).__name__)
    emit("outer_ids", [ls.id for ls in area.outerBound])
    emit("outer_types", [type(ls).__name__ for ls in area.outerBound])
    emit("inner_type", type(area.innerBounds).__name__)
    emit("inner_len", len(area.innerBounds))
    emit("inner_repr", repr(area.innerBounds))
    emit("outer_polygon_type", type(area.outerBoundPolygon()).__name__)
    emit("outer_polygon_ids", area.outerBoundPolygon().ids())
    emit("outer_polygon_len", len(area.outerBoundPolygon()))
    emit("inner_polygons", len(area.innerBoundPolygons()))

    emit("repr_no_attrs", repr(Area(12, [square(13)])))

    # A hole.
    holed = Area(14, [square(15, 10.0)], [[line(16, [(2.0, 2.0), (4.0, 2.0), (4.0, 4.0)])]])
    emit("holed_repr", repr(holed))
    emit("holed_inner_len", len(holed.innerBounds))
    emit("holed_inner_ids", [ls.id for ls in holed.innerBounds[0]])
    emit("holed_inner_polygons", [poly.ids() for poly in holed.innerBoundPolygons()])

    # Rings are recomputed eagerly when a bound is assigned.
    holed.outerBound = [square(17, 6.0)]
    emit("after_outer_assign", holed.outerBoundPolygon().ids())
    holed.innerBounds = [[line(18, [(1.0, 1.0), (2.0, 1.0), (2.0, 2.0)])]]
    emit("after_inner_assign", [poly.ids() for poly in holed.innerBoundPolygons()])

    area.attributes["extra"] = "1"
    emit("attributes_after_write", len(area.attributes))
    area.id = 42
    emit("id_after_write", area.id)

    emit("regelems_empty", area.regulatoryElements)
    emit("eq_self", area == area)
    emit("eq_distinct", Area(20, [square(21)]) == Area(20, [square(21)]))

    # Upstream's repr helper hardcodes the display name.
    const = ConstArea(30, [square(31)])
    emit("const_repr", repr(const))
    emit("const_outer_types", [type(ls).__name__ for ls in const.outerBound])
    emit("const_inner_type", type(const.innerBounds).__name__)

    expect_raises("deprecated_inner_bound_polygon", lambda: area.innerBoundPolygon(), msg=True)


run(main)
