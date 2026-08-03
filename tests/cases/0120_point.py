"""`Point2d`/`Point3d` — id, attributes, and shared storage.

The load-bearing facts pinned here:

* A point always stores three coordinates. `Point2d(p3d)` is a *view*, not a copy,
  so a write through one handle is visible through the other, and writing `x` on
  the 2D view leaves `z` alone.
* `==` compares the identity of the underlying storage, not the id. Two points
  built separately with the same id are not equal.
* `__hash__` hashes the id, matching upstream. `__eq__` compares storage identity,
  and equal storage always shares an id, so `a == b` still implies equal hash
  (the only direction Python requires); distinct same-id points may collide.
* `.attributes` is a live proxy; mutating it mutates the point.
"""

from canon import emit, expect_raises, run

from lanelet2.core import AttributeMap, BasicPoint3d, ConstPoint2d, ConstPoint3d, Point2d, Point3d


def main():
    attributes = AttributeMap({"type": "line_thin", "subtype": "solid"})
    p = Point3d(1, 1.5, 2.5, 3.5, attributes)

    emit("repr", repr(p))
    emit("str", str(p))
    emit("id", p.id)
    emit("xyz", [p.x, p.y, p.z])
    emit("cls", type(p).__module__ + "." + type(p).__name__)

    emit("repr_no_attrs", repr(Point3d(2, 0.0, 0.0, 0.0)))
    emit("repr_default", repr(Point3d()))
    emit("repr_three_args", repr(Point3d(7, 1.0, 2.0)))
    emit("repr_kwargs", repr(Point3d(id=7, x=1.0, y=2.0, z=3.0)))
    emit("repr_from_basic", repr(Point3d(8, BasicPoint3d(1.0, 2.0, 3.0))))
    emit("repr_from_basic_attrs", repr(Point3d(9, BasicPoint3d(1.0, 2.0, 3.0), {"k": "v"})))

    # A 2D view over the same storage.
    view = Point2d(p)
    emit("view_repr", repr(view))
    emit("view_str", str(view))
    view.x = 9.0
    emit("write_through_x", p.x)
    emit("write_through_keeps_z", p.z)

    emit("basic_point_type", type(p.basicPoint()).__name__)
    basic = p.basicPoint()
    basic.y = 7.0
    emit("basic_point_writes_through", p.y)

    # Only the *other* dimension is accepted as an aliasing constructor.
    expect_raises("ctor_same_dimension", lambda: Point3d(p))

    # Equality is identity of the storage, not of the id.
    emit("eq_self", p == p)
    emit("eq_alias", p == Point3d(view))
    emit("eq_same_id_new_storage", p == Point3d(1, 1.5, 2.5, 3.5))
    emit("eq_cross_dimension", p == view)
    emit("eq_foreign_type", p == "x")
    emit("ne_self", p != p)

    emit("hash_matches_same_id", hash(p) == hash(Point3d(1, 0.0, 0.0, 0.0)))
    emit("hash_matches_alias", hash(p) == hash(Point3d(view)))

    # `.attributes` is a live proxy.
    emit("attributes_type", type(p.attributes).__name__)
    p.attributes["extra"] = "1"
    emit("attributes_after_write", len(p.attributes))
    emit("attributes_repr_in_point", repr(p))
    p.attributes = {"only": "this"}
    emit("attributes_after_replace", [len(p.attributes), p.attributes["only"]])

    p.id = 42
    emit("id_after_write", p.id)

    # The Const flavours are not constructible from Python.
    expect_raises("const3_init", lambda: ConstPoint3d(p), msg=True)
    expect_raises("const2_init", lambda: ConstPoint2d(view), msg=True)


run(main)
