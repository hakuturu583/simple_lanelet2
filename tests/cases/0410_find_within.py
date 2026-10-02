"""`findWithin2d` / `findWithin3d` across every layer and query geometry.

Upstream registers these per (layer, geometry) pair -- 50 `findWithin2d` and 30
`findWithin3d` overloads -- independently of `distance`'s overload set. Each call
searches the layer with the geometry's 2D bounding box widened by `maxDist`, then
keeps what lies within `maxDist`: by `distance2d` for `findWithin2d` (so the
elevated linestring below is near), by `distance3d` for `findWithin3d` (so it is
far). An unregistered pair raises `ArgumentError`.

`findWithin3d` against a closed shape (polygon, lanelet, area) follows boost: a
point inside the shape's 2D footprint is at distance zero whatever its height,
anything else is measured in 3D to the nearest edge of the outline.
"""

from canon import by_dist_id, emit, expect_raises, run

import lanelet2.geometry as g
from lanelet2.core import (
    Area,
    BasicPoint2d,
    BasicPoint3d,
    BoundingBox2d,
    BoundingBox3d,
    CompoundLineString3d,
    Lanelet,
    LaneletMap,
    LineString3d,
    Point3d,
    Polygon3d,
    getId,
)


def pt(x, y, z=0.0):
    return Point3d(getId(), x, y, z)


def ls(points):
    return LineString3d(getId(), [pt(*p) for p in points])


def build_map():
    m = LaneletMap()
    m.add(ls([(5.0, 0.0), (5.0, 3.0)]))                    # crosses the road
    m.add(ls([(20.0, 0.0, 10.0), (20.0, 3.0, 10.0)]))      # 2D near, 3D far
    m.add(Lanelet(getId(), ls([(0.0, 3.0), (10.0, 3.0)]), ls([(0.0, 0.0), (10.0, 0.0)])))
    m.add(Polygon3d(getId(), [pt(30.0, 0.0), pt(32.0, 0.0), pt(32.0, 2.0)]))
    m.add(Area(getId(), [ls([(50.0, 0.0), (54.0, 0.0), (54.0, 4.0), (50.0, 4.0), (50.0, 0.0)])]))
    m.add(pt(40.0, 1.0))
    return m


def geometries():
    """Query geometries by the name upstream registers them under."""
    p3 = pt(20.0, 1.5, 0.0)
    line3 = ls([(19.0, 1.0, 10.0), (21.0, 1.0, 10.0)])
    poly3 = Polygon3d(getId(), [pt(4.0, 1.0), pt(6.0, 1.0), pt(6.0, 2.0)])
    compound3 = CompoundLineString3d([ls([(39.0, 0.0), (39.5, 0.0)]), ls([(39.5, 0.0), (41.0, 0.0)])])
    road = Lanelet(getId(), ls([(0.0, 3.0), (10.0, 3.0)]), ls([(0.0, 0.0), (10.0, 0.0)]))
    area = Area(getId(), [ls([(29.0, -1.0), (31.0, -1.0), (31.0, 1.0), (29.0, 1.0), (29.0, -1.0)])])
    return [
        ("BasicPoint2d", BasicPoint2d(5.0, 1.5)),
        ("BasicPoint3d", BasicPoint3d(20.0, 1.5, 0.0)),
        ("Point2d", g.to2D(p3)),
        ("Point3d", p3),
        ("BoundingBox2d", BoundingBox2d(BasicPoint2d(4.0, 1.0), BasicPoint2d(6.0, 2.0))),
        ("BoundingBox3d", BoundingBox3d(BasicPoint3d(19.0, 1.0, 9.0), BasicPoint3d(21.0, 2.0, 11.0))),
        ("LineString2d", g.to2D(line3)),
        ("LineString3d", line3),
        ("Polygon2d", g.to2D(poly3)),
        ("Polygon3d", poly3),
        ("CompoundLineString2d", g.to2D(compound3)),
        ("CompoundLineString3d", compound3),
        ("Lanelet", road),
        ("Area", area),
    ]


def main():
    m = build_map()
    layers = ["pointLayer", "lineStringLayer", "polygonLayer", "laneletLayer", "areaLayer"]
    for layer_name in layers:
        layer = getattr(m, layer_name)
        for geom_name, geom in geometries():
            for fn_name in ("findWithin2d", "findWithin3d"):
                fn = getattr(g, fn_name)
                for max_dist in (0.0, 2.0, 100.0):
                    key = "%s:%s:%s:%s" % (fn_name, layer_name, geom_name, max_dist)

                    def call(fn=fn, layer=layer, geom=geom, max_dist=max_dist):
                        return [[round(d, 6), type(p).__name__]
                                for d, p in by_dist_id(fn(layer, geom, max_dist))]

                    expect_raises(key, call)
    # Closed shapes in 3D: inside the footprint is zero at any height; outside is
    # the 3D distance to the outline, which a sloped shape makes visible.
    probes = [pt(1.0, 1.0, 5.0), pt(1.0, 1.0, -3.0), pt(5.0, 1.0, 0.0), pt(5.0, 1.0, 4.0), pt(1.0, 3.5, 2.0)]
    probe_map = LaneletMap()
    for p in probes:
        probe_map.add(p)
    square = [(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]
    shapes = [
        ("flat_polygon", Polygon3d(getId(), [pt(x, y) for x, y in square])),
        ("sloped_polygon", Polygon3d(getId(), [pt(x, y, x) for x, y in square])),
        ("sloped_lanelet", Lanelet(getId(), ls([(0.0, 2.0, 0.0), (2.0, 2.0, 2.0)]),
                                   ls([(0.0, 0.0, 0.0), (2.0, 0.0, 2.0)]))),
        ("flat_area", Area(getId(), [ls(square + [square[0]])])),
    ]
    for shape_name, shape in shapes:
        found = g.findWithin3d(probe_map.pointLayer, shape, 1000.0)
        emit("closed_3d:" + shape_name,
             sorted([[round(p.x, 3), round(p.y, 3), round(p.z, 3), round(d, 6)] for d, p in found]))
    # maxDist defaults to zero: only what touches the geometry.
    emit("default_max_dist",
         [[round(d, 6), type(p).__name__]
          for d, p in by_dist_id(g.findWithin2d(m.lineStringLayer, BasicPoint2d(5.0, 1.5)))])


run(main)
