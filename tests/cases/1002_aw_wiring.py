"""The `# ORACLE: aw` plumbing itself, checked on stock behaviour.

# ORACLE: aw

Every later Autoware case is scored REF(ROS) vs COMPAT(.venv-aw) vs FIXED(.venv-aw),
so this one exercises that path before anything depends on it, using only behaviour
that has nothing to do with the extension.

It doubles as the staleness canary for `.venv-aw`. That venv holds a *built wheel*
rather than an editable install, so unlike `.venv` it does not follow the source —
`just venv-aw` has to be re-run. A change that alters behaviour without a rebuild
shows up here rather than as a confusing mismatch inside a Phase 2 case.
"""

from canon import by_id, data_path, emit, run

from lanelet2.core import AttributeMap, GPSPoint, Point3d, getId
from lanelet2.io import Origin, load
from lanelet2.projection import UtmProjector


def main():
    emit("id_counter_starts_at", getId())
    emit("point_repr", repr(Point3d(getId(), 1.0, 2.0, 3.0)))
    emit("attrs_repr", repr(AttributeMap({"type": "line_thin"})))

    projector = UtmProjector(Origin(49.0, 8.4, 0.0))
    xyz = projector.forward(GPSPoint(49.0001, 8.4001, 12.5))
    emit("forward", [xyz.x, xyz.y, xyz.z])

    map_ = load(data_path("mapping_example.osm"), projector)
    emit("counts", [len(map_.pointLayer), len(map_.laneletLayer)])
    emit("first_lanelets", [repr(x)[:100] for x in by_id(map_.laneletLayer)][:5])


run(main)
