"""Linestrings, and what `invert()` really means.

`invert()` is an O(1) flag flip over the *same* storage, not a copy. That has three
consequences worth pinning: reading is reversed, `ls != ls.invert()` even though
every point is shared, and writes are mapped back through the inversion — appending
to an inverted handle prepends to the underlying data, and assigning to index 0
overwrites the last underlying point.

The 2D, const and hybrid flavours are all views on that same storage; only what they
hand back from `__getitem__` differs.
"""

from canon import emit, expect_raises, run

from lanelet2.core import (
    AttributeMap,
    ConstHybridLineString2d,
    ConstHybridLineString3d,
    ConstLineString2d,
    ConstLineString3d,
    LineString2d,
    LineString3d,
    Point3d,
)


def points(ids):
    return [Point3d(i, float(i), 0.0, 0.0) for i in ids]


def ids_of(linestring):
    return [point.id for point in linestring]


def main():
    attributes = AttributeMap({"type": "line_thin", "subtype": "solid"})
    ls = LineString3d(10, points([1, 2, 3]), attributes)

    emit("repr", repr(ls))
    emit("str", str(ls))
    emit("len", len(ls))
    emit("ids", ids_of(ls))
    emit("id", ls.id)
    emit("cls", type(ls).__module__ + "." + type(ls).__name__)
    emit("item_type", type(ls[0]).__name__)

    emit("getitem_first", repr(ls[0]))
    emit("getitem_negative", repr(ls[-1]))
    expect_raises("getitem_oob", lambda: ls[5], msg=True)
    expect_raises("getitem_slice", lambda: ls[0:1])

    inverted = ls.invert()
    emit("inverted_repr", repr(inverted))
    emit("inverted_str", str(inverted))
    emit("inverted_ids", ids_of(inverted))
    emit("inverted_flag", inverted.inverted())
    emit("original_flag", ls.inverted())
    emit("eq_inverse", ls == inverted)
    emit("ne_inverse", ls != inverted)
    emit("double_inverse_eq", ls == inverted.invert())
    emit("hash_matches_inverse", hash(ls) == hash(inverted))

    # Appending through the inverted handle prepends to the storage, so the new
    # point still reads back last from the inverted view.
    inverted.append(Point3d(9, 9.0, 0.0, 0.0))
    emit("after_append_original", ids_of(ls))
    emit("after_append_inverted", ids_of(inverted))

    # Index 0 of the inverse is the last point of the original.
    victim = LineString3d(11, points([1, 2, 3]))
    victim.invert()[0] = Point3d(7, 7.0, 0.0, 0.0)
    emit("after_setitem_inverted", ids_of(victim))

    victim2 = LineString3d(12, points([1, 2, 3]))
    del victim2.invert()[0]
    emit("after_delitem_inverted", ids_of(victim2))

    victim3 = LineString3d(13, points([1, 2, 3]))
    victim3.append(Point3d(4, 4.0, 0.0, 0.0))
    del victim3[0]
    emit("after_plain_append_and_delete", ids_of(victim3))

    emit("empty_repr", repr(LineString3d(14, [])))
    emit("empty_str", str(LineString3d(14, [])))
    emit("default_repr", repr(LineString3d()))
    emit("id_only_repr", repr(LineString3d(5)))

    # The 2D view shares storage with the 3D one.
    plain = LineString3d(20, points([1, 2]))
    two_d = LineString2d(plain)
    emit("2d_repr", repr(two_d))
    emit("2d_item_type", type(two_d[0]).__name__)
    two_d[0].x = 99.0
    emit("2d_write_through", plain[0].x)
    emit("eq_across_dimension", plain == two_d)

    const3 = ConstLineString3d(30, points([1, 2, 3]))
    emit("const3_repr", repr(const3))
    emit("const3_str", str(const3))
    emit("const3_item_type", type(const3[0]).__name__)
    expect_raises("const3_from_mutable", lambda: ConstLineString3d(plain))

    const2 = ConstLineString2d(const3)
    emit("const2_repr", repr(const2))
    emit("const3_from_const2", repr(ConstLineString3d(const2)))

    hybrid3 = ConstHybridLineString3d(const3)
    emit("hybrid3_repr", repr(hybrid3))
    emit("hybrid3_str", str(hybrid3))
    emit("hybrid3_item_type", type(hybrid3[0]).__name__)
    emit("hybrid3_items", [repr(item) for item in hybrid3])

    hybrid2 = ConstHybridLineString2d(const2)
    emit("hybrid2_repr", repr(hybrid2))
    emit("hybrid2_item_type", type(hybrid2[0]).__name__)

    # `.attributes` is a live proxy here too.
    ls.attributes["extra"] = "1"
    emit("attributes_after_write", len(ls.attributes))
    emit("attributes_shared_with_inverse", len(inverted.attributes))


run(main)
