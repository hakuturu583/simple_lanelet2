"""The MGRS projector.

# ORACLE: aw
# TOL: abs=1e-6 rel=1e-12

The grid alphabet is swept exhaustively rather than spot-checked, because an error in
it is silent: a wrong column set shifts a map by 100 km and a wrong row repetition by
2000 km, with nothing raising. The sweep covers every latitude band, both zone
parities and all three column alphabets.

Decoding is the hard half. A row letter repeats every twenty squares, so a reference
alone does not say which repetition it means; the band letter resolves it. We decode
by search -- projecting each candidate back and keeping the one whose latitude lands
in the band -- so a wrong answer would have to land in the right band to survive.

Two behaviours are reproduced rather than repaired: the origin is accepted and then
ignored, and nothing throws. A `reverse` with no code set prints to stderr and returns
a zero point, and on that path alone the elevation is dropped.
"""

from canon import emit, run

from autoware_lanelet2_extension_python.projection import MGRSProjector
from lanelet2.core import BasicPoint3d, GPSPoint
from lanelet2.io import Origin


def main():
    # --- the grid alphabet, swept ------------------------------------------------
    codes = []
    for band in range(20):
        lat = (band - 10) * 8.0 + 4.0
        if not -80.0 <= lat < 84.0:
            continue
        for zone in range(1, 61):
            lon = 6.0 * zone - 183.0 + 1.5
            projector = MGRSProjector(Origin(0.0, 0.0, 0.0))
            projector.forward(GPSPoint(lat, lon, 0.0))
            codes.append(projector.getProjectedMGRSGrid())
    emit("grid_codes", codes)
    emit("grid_code_count", len(codes))

    # --- forward, and what it does with the origin -------------------------------
    for label, origin in (
        ("ignored", Origin(35.6895, 139.6917, 0.0)),
        ("zero", Origin(0.0, 0.0, 0.0)),
    ):
        projector = MGRSProjector(origin)
        projected = []
        for lat, lon, ele in (
            (35.6895, 139.6917, 12.5),
            (35.6900, 139.6920, 0.0),
            (35.6890, 139.6910, -3.25),
        ):
            xyz = projector.forward(GPSPoint(lat, lon, ele))
            projected.append([xyz.x, xyz.y, xyz.z])
        # The origin changes nothing: MGRS names a global grid.
        emit("forward_%s" % label, projected)
        emit("grid_%s" % label, projector.getProjectedMGRSGrid())

    # --- round trip through the latched grid -------------------------------------
    projector = MGRSProjector(Origin(0.0, 0.0, 0.0))
    emit("code_set_before", projector.isMGRSCodeSet())
    xyz = projector.forward(GPSPoint(35.6895, 139.6917, 12.5))
    back = projector.reverse(BasicPoint3d(xyz.x, xyz.y, xyz.z))
    emit("round_trip", [back.lat, back.lon, back.ele])

    # --- an explicitly set code wins ---------------------------------------------
    explicit = MGRSProjector(Origin(0.0, 0.0, 0.0))
    explicit.setMGRSCode("54SUE")
    emit("code_set_after", explicit.isMGRSCodeSet())
    recovered = explicit.reverse(BasicPoint3d(xyz.x, xyz.y, xyz.z))
    emit("explicit_reverse", [recovered.lat, recovered.lon, recovered.ele])


run(main)
