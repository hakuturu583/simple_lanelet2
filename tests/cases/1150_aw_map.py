"""A real Autoware map, read with the extension registered.

# ORACLE: aw

The Nishi-Shinjuku map carries 33 regulatory elements whose subtypes stock Lanelet2
has no class for. With the extension imported they resolve; without it the map is
refused, which is what 0221_regelem_kinds pins against the PyPI oracle.

This is the end-to-end claim: the same map, the same typed classes, and the same
bytes back out.
"""

import hashlib

from canon import by_id, emit, run

# Registration happens here, before anything is loaded.
import autoware_lanelet2_extension_python.regulatory_elements  # noqa: F401
from lanelet2.io import Origin, loadRobust, write
from lanelet2.projection import UtmProjector

MAP = "/home/masaya/workspace/torchdrivesim/nishishinjuku_autoware_map/lanelet2_map.osm"


def main():
    projector = UtmProjector(Origin(35.6895, 139.6917, 0.0))
    map_, errors = loadRobust(MAP, projector)
    emit("load_errors", list(errors))

    histogram = {}
    for element in by_id(map_.regulatoryElementLayer):
        name = type(element).__name__
        histogram[name] = histogram.get(name, 0) + 1
    emit("types", sorted(histogram.items()))
    emit(
        "counts",
        [
            len(map_.pointLayer),
            len(map_.lineStringLayer),
            len(map_.polygonLayer),
            len(map_.laneletLayer),
            len(map_.areaLayer),
            len(map_.regulatoryElementLayer),
        ],
    )

    # The extension elements are reachable through their typed accessors, not just
    # present under the right class name.
    markings = [r for r in by_id(map_.regulatoryElementLayer)
                if type(r).__name__ == "RoadMarking"]
    emit("marking_count", len(markings))
    emit("marking_first", type(markings[0].roadMarking()).__name__)

    write("out.osm", map_, projector)
    with open("out.osm", "rb") as handle:
        written = handle.read()
    emit("written_bytes", len(written))
    emit("written_sha", hashlib.sha256(written).hexdigest())


run(main)
