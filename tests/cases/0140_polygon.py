"""Polygons.

A polygon is a linestring with polygon traits — upstream literally shares the
storage type — so almost everything carries over. The difference that matters is
that a polygon has no orientation to flip: `invert` is not registered at all.

Points are in clockwise order and the ring is left open, i.e. the first point is
not repeated at the end.
"""

from canon import emit, expect_raises, run

from lanelet2.core import (
    AttributeMap,
    ConstHybridPolygon2d,
    ConstHybridPolygon3d,
    ConstPolygon2d,
    ConstPolygon3d,
    Point3d,
    Polygon2d,
    Polygon3d,
)


def points(ids):
    return [Point3d(i, float(i), float(i % 2), 0.0) for i in ids]


def main():
    attributes = AttributeMap({"type": "parking"})
    poly = Polygon3d(20, points([1, 2, 3]), attributes)

    emit("repr", repr(poly))
    emit("str", str(poly))
    emit("len", len(poly))
    emit("ids", [point.id for point in poly])
    emit("item_type", type(poly[0]).__name__)
    emit("cls", type(poly).__module__ + "." + type(poly).__name__)

    emit("repr_no_attrs", repr(Polygon3d(21, points([1, 2]))))

    # No orientation to flip.
    emit("has_invert", hasattr(poly, "invert"))
    emit("has_inverted", hasattr(poly, "inverted"))

    poly.append(Point3d(4, 4.0, 0.0, 0.0))
    emit("after_append", [point.id for point in poly])
    del poly[0]
    emit("after_delete", [point.id for point in poly])
    poly[0] = Point3d(8, 8.0, 0.0, 0.0)
    emit("after_setitem", [point.id for point in poly])

    two_d = Polygon2d(22, points([1, 2, 3]))
    emit("2d_repr", repr(two_d))
    emit("2d_item_type", type(two_d[0]).__name__)

    const3 = ConstPolygon3d(23, points([1, 2, 3]))
    emit("const3_repr", repr(const3))
    emit("const3_str", str(const3))
    emit("const3_item_type", type(const3[0]).__name__)

    const2 = ConstPolygon2d(24, points([1, 2, 3]))
    emit("const2_repr", repr(const2))

    hybrid3 = ConstHybridPolygon3d(const3)
    emit("hybrid3_repr", repr(hybrid3))
    emit("hybrid3_item_type", type(hybrid3[0]).__name__)

    hybrid2 = ConstHybridPolygon2d(const2)
    emit("hybrid2_repr", repr(hybrid2))
    emit("hybrid2_item_type", type(hybrid2[0]).__name__)

    # A polygon and a linestring over equivalent points are different types and
    # never compare equal.
    emit("eq_self", poly == poly)
    emit("eq_across_dimension", poly == two_d)
    expect_raises("polygon_from_linestring", lambda: Polygon3d(two_d))


run(main)
