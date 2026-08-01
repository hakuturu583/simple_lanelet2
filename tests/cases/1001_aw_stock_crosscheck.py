"""Do the two references agree on plain, extension-free Lanelet2?

# ORACLE: skew
# TOL: abs=1e-9 rel=1e-12

Nothing here touches `autoware_lanelet2_extension`. The point is the opposite: to
establish that the ROS build and the PyPI wheel behave identically on the ground both
of them cover, so that when an Autoware case does disagree, the disagreement is
attributable to us rather than to the gap between the two builds.

This is the check the project was missing when it first noticed the Nishi-Shinjuku
map writing 11,744,007 bytes under one implementation and 11,744,008 under another.
"""

import hashlib

from canon import by_id, data_path, emit, run

from lanelet2.core import (
    AttributeMap,
    BasicPoint3d,
    GPSPoint,
    LineString3d,
    Point3d,
    getId,
)
from lanelet2.io import Origin, load, write
from lanelet2.projection import (
    GeocentricProjector,
    LocalCartesianProjector,
    MercatorProjector,
    UtmProjector,
)


def main():
    # --- formatting and identity -------------------------------------------------
    point = Point3d(getId(), 1.0, 2.0, 3.0)
    emit("point_repr", repr(point))
    emit("point_str", str(point))
    emit("odd_floats", [repr(Point3d(getId(), v, 0.0, 0.0)) for v in
                        (0.0, -0.0, 1e-7, 1.234567891e8, 0.5, 1e300)])
    line = LineString3d(getId(), [point, Point3d(getId(), 4.0, 5.0, 6.0)])
    emit("line_repr", repr(line))
    emit("attrs_repr", repr(AttributeMap({"type": "line_thin", "subtype": "solid"})))

    # --- projection ---------------------------------------------------------------
    origin = Origin(49.0, 8.4, 0.0)
    for name, projector in (
        ("utm", UtmProjector(origin)),
        ("mercator", MercatorProjector(origin)),
        ("local_cartesian", LocalCartesianProjector(origin)),
        ("geocentric", GeocentricProjector()),
    ):
        forward, reverse = [], []
        for lat, lon, ele in (
            (49.0, 8.4, 0.0),
            (49.0001, 8.4001, 12.5),
            (48.9, 8.3, -5.0),
            (0.0, 0.0, 0.0),
            (-33.0, 151.0, 100.0),
            (60.0, 5.0, 0.0),
        ):
            xyz = projector.forward(GPSPoint(lat, lon, ele))
            forward.append([xyz.x, xyz.y, xyz.z])
            back = projector.reverse(BasicPoint3d(xyz.x, xyz.y, xyz.z))
            reverse.append([back.lat, back.lon, back.ele])
        emit("proj_%s_forward" % name, forward)
        emit("proj_%s_reverse" % name, reverse)

    # --- a real map, loaded and written back --------------------------------------
    projector = UtmProjector(origin)
    map_ = load(data_path("mapping_example.osm"), projector)
    emit(
        "map_counts",
        [
            len(map_.pointLayer),
            len(map_.lineStringLayer),
            len(map_.polygonLayer),
            len(map_.laneletLayer),
            len(map_.areaLayer),
            len(map_.regulatoryElementLayer),
        ],
    )
    emit("map_lanelet_reprs", [repr(x)[:120] for x in by_id(map_.laneletLayer)][:20])
    emit("map_regelem_reprs", [repr(x)[:120] for x in by_id(map_.regulatoryElementLayer)][:20])

    write("out.osm", map_, projector)
    with open("out.osm", "rb") as handle:
        written = handle.read()
    emit("written_bytes", len(written))
    emit("written_sha", hashlib.sha256(written).hexdigest())


run(main)
