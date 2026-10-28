"""Pickling a map: an addition, so every result here is a `pickle:` key.

Upstream refuses to pickle any of its objects (Boost.Python: "Pickling of ... is
not enabled"), so the reference reports the error under each `pickle:` key and the
divergence table allows exactly those. Everything else is emitted by both sides and
must agree: it pins the map the pickle is taken of.

The snapshot keeps the object graph, not just the ids: shared storage stays shared,
inverted views stay inverted, layers are restored as they were, and a submap stays a
submap. It is read back in a fresh interpreter too, which is the case that matters
for multiprocessing.
"""

import pickle
import subprocess
import sys

from canon import data_path, emit, run

import lanelet2
from lanelet2.core import (
    AttributeMap,
    Lanelet,
    LaneletMap,
    LaneletSubmap,
    LineString3d,
    Point3d,
    TrafficLight,
)
from lanelet2.io import Origin
from lanelet2.projection import UtmProjector

LAYERS = [
    "pointLayer",
    "lineStringLayer",
    "polygonLayer",
    "laneletLayer",
    "areaLayer",
    "regulatoryElementLayer",
]


def describe(map):
    """Everything a layer shows, by id, including which way round things are held."""
    return {name: sorted(repr(item) for item in getattr(map, name)) for name in LAYERS}


def line(id, points):
    return LineString3d(id, [Point3d(id * 100 + i, x, y, 0.5) for i, (x, y) in enumerate(points)])


def sample():
    shared = line(11, [(0.0, -1.0), (1.0, -1.0)])
    lanelet = Lanelet(20, line(10, [(0.0, 1.0), (1.0, 1.0)]), shared,
                      AttributeMap({"subtype": "road"}))
    lanelet.centerline = line(12, [(0.0, 0.0), (1.0, 0.0)])
    twin = Lanelet(21, shared.invert(), line(13, [(0.0, -3.0), (1.0, -3.0)])).invert()
    lanelet.addRegulatoryElement(
        TrafficLight(30, AttributeMap(), [line(31, [(0.5, 2.0), (0.6, 2.0)])]))
    map = LaneletMap()
    map.add(lanelet)
    map.add(twin)
    return map


def attempt(key, fn):
    try:
        emit(key, fn())
    except Exception as exc:  # noqa: BLE001 - the reference raises; that is the record
        emit(key, ["raised", type(exc).__name__])


def in_fresh_interpreter(payload):
    script = (
        "import pickle, sys\n"
        "m = pickle.loads(sys.stdin.buffer.read())\n"
        "print(type(m).__name__, len(m.laneletLayer), len(m.pointLayer))\n"
    )
    done = subprocess.run([sys.executable, "-c", script], input=payload,
                          capture_output=True, check=True)
    return done.stdout.decode().split()


AUTOWARE_MAP = """\
import pickle, sys
from autoware_lanelet2_extension_python.regulatory_elements import RoadMarking
from lanelet2.core import AttributeMap, Lanelet, LaneletMap, LineString3d, Point3d
def line(id):
    return LineString3d(id, [Point3d(id * 10, 0.0, id, 0.0), Point3d(id * 10 + 1, 1.0, id, 0.0)])
lanelet = Lanelet(1, line(2), line(3))
lanelet.addRegulatoryElement(RoadMarking(4, AttributeMap(), line(5)))
map = LaneletMap()
map.add(lanelet)
sys.stdout.buffer.write(pickle.dumps(map))
"""

AUTOWARE_READ = """\
import pickle, sys
map = pickle.loads(sys.stdin.buffer.read())
print(type(map.regulatoryElementLayer[4]).__name__,
      "autoware_lanelet2_extension_python.regulatory_elements" in sys.modules)
"""


def autoware_in_a_process_that_never_imported_it():
    """The reading process has not imported the extension; the snapshot needs it."""
    payload = subprocess.run([sys.executable, "-c", AUTOWARE_MAP],
                             capture_output=True, check=True).stdout
    done = subprocess.run([sys.executable, "-c", AUTOWARE_READ], input=payload,
                          capture_output=True, check=True)
    return done.stdout.decode().split()


def main():
    built = sample()
    emit("built", describe(built))

    attempt("pickle:built_roundtrip",
            lambda: describe(pickle.loads(pickle.dumps(built))) == describe(built))
    attempt("pickle:type", lambda: type(pickle.loads(pickle.dumps(built))).__name__)

    def sharing():
        restored = pickle.loads(pickle.dumps(built))
        lanelet, twin = restored.laneletLayer[20], restored.laneletLayer[21]
        restored.pointLayer[1100].x = 42.0
        return [twin.invert().leftBound[-1].x, lanelet.rightBound[0].x,
                lanelet.centerline.id, twin.inverted()]
    attempt("pickle:sharing", sharing)

    submap = LaneletSubmap()
    submap.add(built.laneletLayer[20])
    emit("submap", describe(submap))
    attempt("pickle:submap", lambda: [type(pickle.loads(pickle.dumps(submap))).__name__,
                                      describe(pickle.loads(pickle.dumps(submap)))
                                      == describe(submap)])

    loaded = lanelet2.io.load(data_path("mapping_example.osm"), UtmProjector(Origin(49, 8.4)))
    emit("loaded_sizes", [len(getattr(loaded, name)) for name in LAYERS])
    attempt("pickle:loaded_roundtrip",
            lambda: describe(pickle.loads(pickle.dumps(loaded))) == describe(loaded))
    attempt("pickle:fresh_interpreter", lambda: in_fresh_interpreter(pickle.dumps(loaded)))

    attempt("pickle:autoware_extension", autoware_in_a_process_that_never_imported_it)
    attempt("pickle:empty", lambda: describe(pickle.loads(pickle.dumps(LaneletMap())))
            == describe(LaneletMap()))
    attempt("pickle:corrupt",
            lambda: lanelet2.core._map_from_snapshot(b"LL2SNAP\0\x01\x00\x00\x00"))


run(main)
