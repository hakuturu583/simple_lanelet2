"""Map layers, `LaneletMap` and `LaneletSubmap`.

Adding to a full map pulls in everything the primitive references and assigns ids to
anything that has none; a submap records only what it is handed, though it still
assigns that object an id.

Layer iteration order is *not* reproducible upstream — the layers are
`unordered_map` — so every listing here goes through `canon.by_id`.
"""

from canon import by_id, emit, expect_raises, run

from lanelet2.core import (
    AttributeMap,
    BasicPoint2d,
    BoundingBox2d,
    Lanelet,
    LaneletMap,
    LaneletSubmap,
    LineString3d,
    Point3d,
    Polygon3d,
    TrafficLight,
    createMapFromLanelets,
    createMapFromPoints,
    createSubmapFromLanelets,
    getId,
)


def line(id, points):
    return LineString3d(id, [Point3d(id * 100 + i, x, y, 0.0) for i, (x, y) in enumerate(points)])


def lanelet(id, offset=0.0):
    return Lanelet(
        id,
        line(id * 10 + 1, [(offset, 1.0), (offset + 1.0, 1.0)]),
        line(id * 10 + 2, [(offset, -1.0), (offset + 1.0, -1.0)]),
    )


def layer_sizes(map):
    return [
        len(map.pointLayer),
        len(map.lineStringLayer),
        len(map.polygonLayer),
        len(map.laneletLayer),
        len(map.areaLayer),
        len(map.regulatoryElementLayer),
    ]


def main():
    first = lanelet(3)
    map = createMapFromLanelets([first])
    emit("cls", type(map).__module__ + "." + type(map).__name__)
    emit("layer_sizes", layer_sizes(map))
    emit("lanelet_ids", [ll.id for ll in by_id(map.laneletLayer)])
    emit("linestring_ids", [ls.id for ls in by_id(map.lineStringLayer)])
    emit("point_ids", [p.id for p in by_id(map.pointLayer)])
    emit("layer_item_types", [type(ll).__name__ for ll in by_id(map.laneletLayer)])

    emit("exists", map.laneletLayer.exists(3))
    emit("exists_missing", map.laneletLayer.exists(999))
    emit("exists_invalid", map.laneletLayer.exists(0))
    emit("getitem", map.laneletLayer[3].id)
    emit("get", map.laneletLayer.get(3).id)
    emit("contains_id", 3 in map.laneletLayer)
    emit("contains_primitive", first in map.laneletLayer)
    expect_raises("get_missing", lambda: map.laneletLayer.get(999))
    expect_raises("get_invalid", lambda: map.laneletLayer.get(0))

    emit("nearest", [ll.id for ll in map.laneletLayer.nearest(BasicPoint2d(0.0, 0.0), 1)])
    emit("search", [ll.id for ll in by_id(map.laneletLayer.search(
        BoundingBox2d(BasicPoint2d(-1.0, -1.0), BasicPoint2d(2.0, 2.0))))])
    emit("search_empty", [ll.id for ll in map.laneletLayer.search(
        BoundingBox2d(BasicPoint2d(100.0, 100.0), BasicPoint2d(101.0, 101.0)))])
    emit("unique_id_is_fresh", map.laneletLayer.uniqueId() > 0)

    emit("find_usages_point", [ls.id for ls in by_id(
        map.lineStringLayer.findUsages(first.leftBound[0]))])
    emit("find_usages_bound", [ll.id for ll in by_id(
        map.laneletLayer.findUsages(first.leftBound))])
    emit("find_usages_none", [ls.id for ls in map.lineStringLayer.findUsages(
        Point3d(987654, 0.0, 0.0, 0.0))])

    # Adding assigns an id to anything that has none, and recurses.
    fresh = Lanelet(0, LineString3d(0, [Point3d(0, 5.0, 1.0, 0.0), Point3d(0, 6.0, 1.0, 0.0)]),
                    LineString3d(0, [Point3d(0, 5.0, -1.0, 0.0), Point3d(0, 6.0, -1.0, 0.0)]))
    map.add(fresh)
    emit("after_add_sizes", layer_sizes(map))
    emit("added_got_an_id", fresh.id != 0)
    emit("added_bounds_got_ids", [fresh.leftBound.id != 0, fresh.rightBound.id != 0])
    emit("added_points_got_ids", all(p.id != 0 for p in fresh.leftBound))

    # A regulatory element is pulled in along with its own linestrings.
    with_light = lanelet(50)
    with_light.addRegulatoryElement(
        TrafficLight(51, AttributeMap(), [line(52, [(0.0, 3.0), (1.0, 3.0)])]))
    light_map = createMapFromLanelets([with_light])
    emit("regelem_map_sizes", layer_sizes(light_map))
    emit("regelem_ids", [r.id for r in by_id(light_map.regulatoryElementLayer)])
    emit("regelem_types", [type(r).__name__ for r in by_id(light_map.regulatoryElementLayer)])

    # A submap records only what it is handed.
    submap = createSubmapFromLanelets([lanelet(60)])
    emit("submap_cls", type(submap).__name__)
    emit("submap_sizes", layer_sizes(submap))
    emit("submap_to_map_sizes", layer_sizes(submap.laneletMap()))

    # An empty map, and adding to it directly.
    empty = LaneletMap()
    emit("empty_sizes", layer_sizes(empty))
    empty.add(Polygon3d(70, [Point3d(71, 0.0, 0.0, 0.0), Point3d(72, 1.0, 0.0, 0.0),
                             Point3d(73, 1.0, 1.0, 0.0)]))
    emit("after_polygon_sizes", layer_sizes(empty))
    emit("polygon_ids", [p.id for p in by_id(empty.polygonLayer)])

    emit("empty_submap_sizes", layer_sizes(LaneletSubmap()))

    points = createMapFromPoints([Point3d(getId(), 0.0, 0.0, 0.0) for _ in range(3)])
    emit("point_map_sizes", layer_sizes(points))


run(main)
