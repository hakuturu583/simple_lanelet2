"""Lanelets: bounds, inversion, the centerline cache, and the outline polygon.

Inverting swaps *and* reverses the bounds, and writes are mapped back through that
swap — assigning the left bound of an inverted handle assigns the underlying right
one. The centerline is cached, and a bound change discards it unless the user
assigned a centerline of their own, which is detected purely by it having an id.
"""

from canon import emit, expect_raises, run

from lanelet2.core import AttributeMap, ConstLanelet, Lanelet, LineString3d, Point3d


def line(id, points):
    return LineString3d(id, [Point3d(id * 100 + i, x, y, 0.0) for i, (x, y) in enumerate(points)])


def lanelet(id=3):
    return Lanelet(id, line(1, [(0.0, 1.0), (1.0, 1.0)]), line(2, [(0.0, -1.0), (1.0, -1.0)]))


def main():
    ll = Lanelet(3, line(1, [(0.0, 1.0), (1.0, 1.0)]), line(2, [(0.0, -1.0), (1.0, -1.0)]),
                 AttributeMap({"subtype": "road"}))
    emit("repr", repr(ll))
    emit("str", str(ll))
    emit("id", ll.id)
    emit("cls", type(ll).__module__ + "." + type(ll).__name__)
    emit("bound_ids", [ll.leftBound.id, ll.rightBound.id])
    emit("bound_types", [type(ll.leftBound).__name__, type(ll.rightBound).__name__])
    emit("centerline_type", type(ll.centerline).__name__)
    emit("repr_no_attrs", repr(lanelet()))

    inverted = ll.invert()
    emit("inverted_repr", repr(inverted))
    emit("inverted_str", str(inverted))
    emit("inverted_flag", [inverted.inverted(), ll.inverted()])
    emit("inverted_bound_ids", [inverted.leftBound.id, inverted.rightBound.id])
    emit("inverted_bounds_are_inverted",
         [inverted.leftBound.inverted(), inverted.rightBound.inverted()])
    emit("eq_inverse", ll == inverted)
    emit("double_inverse_eq", ll == inverted.invert())
    emit("hash_matches_inverse", hash(ll) == hash(inverted))

    # Writing a bound through the inverted handle assigns the underlying other one.
    victim = lanelet()
    victim.invert().leftBound = line(9, [(0.0, -5.0), (1.0, -5.0)])
    emit("write_through_inverse", [victim.leftBound.id, victim.rightBound.id])

    # The outline is the left bound followed by the reversed right bound, with the
    # shared seam points collapsed.
    emit("polygon3d_type", type(ll.polygon3d()).__name__)
    emit("polygon2d_type", type(ll.polygon2d()).__name__)
    emit("polygon3d_member_ids", ll.polygon3d().ids())
    emit("polygon3d_point_ids", [p.id for p in ll.polygon3d()])
    emit("polygon3d_len", len(ll.polygon3d()))

    # Centerline caching.
    cached = lanelet()
    first = [[round(p.x, 9), round(p.y, 9)] for p in cached.centerline]
    emit("centerline", first)
    emit("centerline_id", cached.centerline.id)
    emit("centerline_point_ids", [p.id for p in cached.centerline])
    cached.leftBound = line(4, [(0.0, 3.0), (1.0, 3.0)])
    emit("centerline_after_bound_change",
         [[round(p.x, 9), round(p.y, 9)] for p in cached.centerline])

    # A centerline the user assigned has an id, so it survives a bound change.
    custom = lanelet()
    custom.centerline = line(42, [(0.0, 0.0), (1.0, 0.0)])
    emit("custom_centerline_id", custom.centerline.id)
    custom.leftBound = line(5, [(0.0, 9.0), (1.0, 9.0)])
    emit("custom_centerline_survives", custom.centerline.id)

    # resetCache discards a computed centerline but keeps a custom one.
    computed = lanelet()
    _ = computed.centerline
    computed.resetCache()
    emit("after_reset_cache", [[round(p.x, 9), round(p.y, 9)] for p in computed.centerline])
    custom.resetCache()
    emit("custom_survives_reset_cache", custom.centerline.id)

    ll.attributes["extra"] = "1"
    emit("attributes_after_write", len(ll.attributes))
    ll.id = 42
    emit("id_after_write", ll.id)

    emit("regulatory_elements_empty", ll.regulatoryElements)
    emit("traffic_lights_empty", ll.trafficLights())

    expect_raises("const_init", lambda: ConstLanelet(ll), msg=True)


run(main)
