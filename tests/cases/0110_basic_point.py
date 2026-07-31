"""`BasicPoint2d`/`BasicPoint3d` — plain coordinates, no id, no attributes.

Two things here are less obvious than they look. `str()` goes through Eigen, which
renders a column vector with every entry right-aligned to the width of the widest.
And `==` is *identity*, not coordinate equality: upstream registers no comparison
operator, so two separately built points with identical coordinates are unequal.
"""

from canon import emit, expect_raises, run

from lanelet2.core import BasicPoint2d, BasicPoint3d, BoundingBox2d, BoundingBox3d


def main():
    p = BasicPoint3d(1.0, 2.0, 3.0)
    emit("repr3", repr(p))
    emit("str3", str(p))
    emit("xyz", [p.x, p.y, p.z])

    q = BasicPoint2d(1.5, -2.5)
    emit("repr2", repr(q))
    emit("str2", str(q))
    emit("xy", [q.x, q.y])

    # Eigen right-aligns to the widest entry, which only shows up with mixed widths.
    emit("str_aligned", str(BasicPoint3d(1.0, -22.5, 333.0)))
    emit("str_scientific", str(BasicPoint3d(1e7, 1.0, 2.0)))

    p.x = 9.0
    emit("after_write", [p.x, p.y, p.z])

    emit("add", repr(BasicPoint3d(1, 2, 3) + BasicPoint3d(1, 1, 1)))
    emit("sub", repr(BasicPoint3d(1, 2, 3) - BasicPoint3d(1, 1, 1)))
    emit("mul_float", repr(BasicPoint3d(1, 2, 3) * 2.0))
    emit("mul_int", repr(BasicPoint3d(1, 2, 3) * 2))
    emit("rmul", repr(2.0 * BasicPoint3d(1, 2, 3)))
    emit("add2", repr(BasicPoint2d(1, 2) + BasicPoint2d(3, 4)))

    # Upstream registers only __div__, which Python 3 never calls.
    expect_raises("truediv", lambda: BasicPoint3d(1, 2, 3) / 2.0)
    # No __neg__ is registered at all.
    expect_raises("neg", lambda: -BasicPoint3d(1, 2, 3))
    # Mixing dimensions has no registered overload.
    expect_raises("add_mixed", lambda: BasicPoint2d(1, 2) + BasicPoint3d(1, 2, 3))

    emit("eq_identical_values", BasicPoint3d(1, 2, 3) == BasicPoint3d(1, 2, 3))
    same = BasicPoint3d(1, 2, 3)
    emit("eq_self", same == same)

    box = BoundingBox2d(BasicPoint2d(0.0, 0.0), BasicPoint2d(1.0, 1.0))
    emit("bbox_repr", repr(box))
    emit("bbox_str", str(box))
    emit("bbox_min", [box.min.x, box.min.y])
    # .min is a live view onto the box, so writing through it moves the box.
    box.min.x = 5.0
    emit("bbox_after_write", repr(box))

    box3 = BoundingBox3d(BasicPoint3d(0.0, 0.0, 0.0), BasicPoint3d(1.0, 2.0, 3.0))
    emit("bbox3_repr", repr(box3))
    emit("bbox3_max", [box3.max.x, box3.max.y, box3.max.z])


run(main)
