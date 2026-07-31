"""OSM reading and writing, including byte-exact output.

The writer has to reproduce pugixml's output exactly: the declaration form, the
space before `/>` on empty elements, and an escape set that covers `&`, `<` and `"`
but leaves `>` and `'` alone. All three were established by writing a file with the
reference and reading the bytes.

Everything else about the ordering is forced: primitives ascend by id (so negative
ids come first) and tags ascend by key, because upstream stores both in a std::map.
"""

import os

from canon import by_id, data_path, emit, emit_file, expect_raises, run

from lanelet2.core import (
    AttributeMap,
    Lanelet,
    LineString3d,
    Point3d,
    Polygon3d,
    TrafficLight,
    createMapFromLanelets,
)
from lanelet2.io import Origin, load, loadRobust, write, writeRobust
from lanelet2.projection import UtmProjector


def build_map():
    points = [Point3d(10 + i, float(i), float(i % 2), 0.0 if i != 1 else 3.25) for i in range(4)]
    # An `ele` attribute and a z of zero: the tag comes from the attribute.
    points[0].attributes["ele"] = "ignored"
    # Characters that exercise the escape set.
    points[2].attributes["k<&>\"'"] = "v<&>\"'"

    left = LineString3d(20, [points[0], points[1]])
    left.attributes["type"] = "line_thin"
    left.attributes["subtype"] = "solid"
    right = LineString3d(21, [points[2], points[3]])
    right.attributes["type"] = "line_thin"

    lanelet = Lanelet(30, left, right)
    lanelet.attributes["subtype"] = "road"
    lanelet.addRegulatoryElement(
        TrafficLight(50, AttributeMap(), [LineString3d(22, [points[0], points[3]])]))

    polygon = Polygon3d(40, [points[0], points[1], points[2]])
    polygon.attributes["type"] = "parking"

    map = createMapFromLanelets([lanelet])
    map.add(polygon)
    # A negative id is JOSM's convention for a not-yet-uploaded object, and is
    # written without the visible/version bookkeeping.
    map.add(Point3d(-5, 9.0, 9.0, 0.0))
    return map


def digest(map, label):
    emit(label + ":counts", [len(map.pointLayer), len(map.lineStringLayer),
                             len(map.polygonLayer), len(map.laneletLayer),
                             len(map.areaLayer), len(map.regulatoryElementLayer)])
    emit(label + ":points", [[p.id, round(p.x, 6), round(p.y, 6), round(p.z, 6)]
                             for p in by_id(map.pointLayer)])
    emit(label + ":linestrings", [[ls.id, [p.id for p in ls], dict(ls.attributes.items())]
                                  for ls in by_id(map.lineStringLayer)])
    emit(label + ":polygons", [[p.id, [q.id for q in p], dict(p.attributes.items())]
                               for p in by_id(map.polygonLayer)])
    emit(label + ":lanelets", [[ll.id, ll.leftBound.id, ll.rightBound.id,
                                dict(ll.attributes.items()),
                                [r.id for r in ll.regulatoryElements]]
                               for ll in by_id(map.laneletLayer)])
    emit(label + ":regelems", [[r.id, r.roles, dict(r.attributes.items())]
                               for r in by_id(map.regulatoryElementLayer)])


def main():
    origin = Origin(49.0, 8.4, 0.0)
    projector = UtmProjector(origin)

    write("out.osm", build_map(), projector)
    emit_file("out.osm", "written")

    # Reload what we wrote, and check it survives the trip.
    reloaded = load("out.osm", projector)
    digest(reloaded, "reloaded")

    write("again.osm", reloaded, projector)
    emit_file("again.osm", "rewritten")
    emit("stable", open("out.osm").read() == open("again.osm").read())

    # Writer options.
    write("upload.osm", build_map(), projector, {"josm_upload": "true"})
    emit("upload_header", open("upload.osm").read().splitlines()[1])
    write("ele.osm", build_map(), projector, {"josm_format_elevation": "true"})
    emit("elevation_two_decimals",
         [line.strip() for line in open("ele.osm") if "ele" in line])

    errors = writeRobust("robust.osm", build_map(), projector)
    emit("write_robust_errors", list(errors))

    # Omitting the projector entirely makes the parser refuse: the coordinates it
    # would produce are meaningless. Passing `Origin()` explicitly does *not* --
    # that call goes through the defaulted-argument constructor, which clears the
    # "is default" flag.
    expect_raises("load_no_projector", lambda: load("out.osm"), msg=True)
    emit("load_explicit_empty_origin",
         [round(p.x, 3) for p in by_id(load("out.osm", Origin()).pointLayer)][:3])
    expect_raises("load_missing_file", lambda: load("nope.osm", projector))

    map, load_errors = loadRobust("out.osm", projector)
    emit("load_robust_errors", list(load_errors))
    emit("load_robust_counts", [len(map.pointLayer), len(map.laneletLayer)])

    # The real example map: 594 KB, every primitive compared.
    example = data_path("mapping_example.osm")
    if os.path.exists(example):
        real = load(example, projector)
        digest(real, "example")
        emit("example:areas", [[a.id, [ls.id for ls in a.outerBound],
                                [[ls.id for ls in hole] for hole in a.innerBounds]]
                               for a in by_id(real.areaLayer)])
        # Centerlines exercise the whole geometry stack against real data.
        emit("example:centerlines",
             [[ll.id, [[round(p.x, 6), round(p.y, 6), round(p.z, 6)] for p in ll.centerline]]
              for ll in by_id(real.laneletLayer)])

        # Byte-exact re-emission of a real map, then a stable second pass.
        write("example_out.osm", real, projector)
        emit_file("example_out.osm", "example:written")
        write("example_again.osm", load("example_out.osm", projector), projector)
        emit("example:stable",
             open("example_out.osm").read() == open("example_again.osm").read())


run(main)
