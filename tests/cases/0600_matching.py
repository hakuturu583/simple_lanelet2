"""Matching an object to the lanelets it may be on.

Both matchers emit *both* orientations of every candidate, because which way the
object is travelling is exactly what is in question;
`removeNonRuleCompliantMatches` is what prunes the wrong-way ones.

The probabilistic variant ranks by squared Mahalanobis distance, combining the
position covariance with a von Mises concentration on the heading -- so an object
sitting exactly on the centerline but pointing backwards still scores badly.
"""

import math

from canon import by_id, data_path, emit, expect_raises, run

import lanelet2.matching as m
import lanelet2.traffic_rules as t
from lanelet2.core import BasicPoint2d, Lanelet, LineString3d, Point3d, createMapFromLanelets, getId
from lanelet2.io import Origin, load
from lanelet2.projection import UtmProjector


def line(points):
    return LineString3d(getId(), [Point3d(getId(), x, y, 0.0) for x, y in points])


def road(y, x0=0.0, x1=20.0):
    ll = Lanelet(getId(), line([(x0, y + 1.0), (x1, y + 1.0)]),
                 line([(x0, y - 1.0), (x1, y - 1.0)]))
    ll.attributes["subtype"] = "road"
    return ll


def describe(matches, probabilistic=False):
    """Rows sorted so that ties do not depend on the layer's iteration order.

    Upstream sorts with std::sort -- unstable -- over an unordered_map, so
    equal-scoring candidates come back in an arbitrary order that is not
    reproducible even between two runs of the reference.
    """
    rows = []
    for found in matches:
        row = [found.lanelet.id, found.lanelet.inverted(), round(found.distance, 9)]
        if probabilistic:
            row.append(round(found.mahalanobisDistSq, 6))
        rows.append(row)
    key = (lambda row: (row[3], row[2], row[0], row[1])) if probabilistic \
        else (lambda row: (row[2], row[0], row[1]))
    return sorted(rows, key=key)


def main():
    pose = m.Pose2d(1.0, 2.0, 0.5)
    emit("pose_str", str(pose))
    emit("covariance_str", str(m.PositionCovariance2d(1.0, 2.0, 0.5)))
    emit("covariance_str_wide", str(m.PositionCovariance2d(100.0, 0.25, -3.0)))

    obj = m.Object2d(5, pose, [BasicPoint2d(0.0, 0.0), BasicPoint2d(1.0, 0.0)])
    emit("object_fields", [obj.objectId, [[p.x, p.y] for p in obj.absoluteHull]])
    obj.objectId = 9
    obj.pose = m.Pose2d(3.0, 4.0, 0.0)
    emit("object_after_write", [obj.objectId, str(obj.pose)])

    with_cov = m.ObjectWithCovariance2d(6, pose, [], m.PositionCovariance2d(1.0, 1.0, 0.0), 2.0)
    emit("object_cov_fields", [with_cov.objectId, with_cov.vonMisesKappa,
                               str(with_cov.positionCovariance)])
    emit("object_cov_is_an_object2d", isinstance(with_cov, m.Object2d))

    # --- deterministic -------------------------------------------------------
    map = createMapFromLanelets([road(0.0), road(10.0), road(-10.0)])
    plain = m.Object2d(1, m.Pose2d(10.0, 0.0, 0.0), [])
    emit("deterministic_close", describe(m.getDeterministicMatches(map, plain, 1.0)))
    emit("deterministic_wide", describe(m.getDeterministicMatches(map, plain, 20.0)))
    emit("deterministic_none", describe(m.getDeterministicMatches(map, plain, 0.0)))

    # A footprint is measured instead of the position when it is present.
    hull = m.Object2d(2, m.Pose2d(10.0, 5.0, 0.0),
                      [BasicPoint2d(9.0, 0.0), BasicPoint2d(11.0, 0.0)])
    emit("deterministic_hull", describe(m.getDeterministicMatches(map, hull, 0.5)))

    rules = t.create(t.Locations.Germany, t.Participants.Vehicle)
    kept = m.removeNonRuleCompliantMatches(m.getDeterministicMatches(map, plain, 1.0), rules)
    emit("rule_compliant", describe(kept))

    # --- probabilistic -------------------------------------------------------
    for yaw, label in [(0.0, "aligned"), (math.pi, "reversed"),
                       (math.pi / 2, "crossways"), (0.2, "slightly_off")]:
        obj = m.ObjectWithCovariance2d(3, m.Pose2d(10.0, 0.0, yaw), [],
                                       m.PositionCovariance2d(1.0, 1.0, 0.0), 2.0)
        emit("probabilistic:" + label,
             describe(m.getProbabilisticMatches(map, obj, 1.0), probabilistic=True))

    # Lateral offset and a kappa of zero (no heading information at all).
    offset = m.ObjectWithCovariance2d(4, m.Pose2d(10.0, 0.5, math.pi), [],
                                      m.PositionCovariance2d(1.0, 1.0, 0.0), 0.0)
    emit("probabilistic_no_heading_weight",
         describe(m.getProbabilisticMatches(map, offset, 1.0), probabilistic=True))

    anisotropic = m.ObjectWithCovariance2d(5, m.Pose2d(10.0, 0.5, 0.0), [],
                                           m.PositionCovariance2d(4.0, 0.25, 0.1), 1.0)
    emit("probabilistic_anisotropic",
         describe(m.getProbabilisticMatches(map, anisotropic, 1.0), probabilistic=True))

    probabilistic_kept = m.removeNonRuleCompliantMatches(
        m.getProbabilisticMatches(map, m.ObjectWithCovariance2d(
            7, m.Pose2d(10.0, 0.0, 0.0), [], m.PositionCovariance2d(1.0, 1.0, 0.0), 2.0), 1.0),
        rules)
    emit("probabilistic_rule_compliant", describe(probabilistic_kept, probabilistic=True))

    # A covariance with no spread, or a singular one, cannot be inverted.
    for label, cov in [("zero", m.PositionCovariance2d(0.0, 0.0, 0.0)),
                       ("singular", m.PositionCovariance2d(1.0, 1.0, 1.0))]:
        bad = m.ObjectWithCovariance2d(8, m.Pose2d(10.0, 0.0, 0.0), [], cov, 1.0)
        expect_raises("probabilistic_" + label,
                      lambda bad=bad: m.getProbabilisticMatches(map, bad, 1.0), msg=True)

    expect_raises("remove_wrong_list_type",
                  lambda: m.removeNonRuleCompliantMatches([1, 2, 3], rules), msg=True)

    # --- against the real map ------------------------------------------------
    real = load(data_path("mapping_example.osm"), UtmProjector(Origin(49.0, 8.4, 0.0)))
    probes = [(p.x, p.y) for p in by_id(real.pointLayer)[:12]]
    for index, (x, y) in enumerate(probes):
        obj = m.Object2d(index, m.Pose2d(x, y, 0.3), [])
        emit("real:deterministic:{}".format(index),
             describe(m.getDeterministicMatches(real, obj, 2.0)))
        cov = m.ObjectWithCovariance2d(index, m.Pose2d(x, y, 0.3), [],
                                       m.PositionCovariance2d(2.0, 2.0, 0.0), 1.5)
        emit("real:probabilistic:{}".format(index),
             describe(m.getProbabilisticMatches(real, cov, 2.0), probabilistic=True))


run(main)
