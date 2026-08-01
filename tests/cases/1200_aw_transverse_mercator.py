"""The extension's transverse Mercator projector.

# ORACLE: aw
# TOL: abs=1e-6 rel=1e-12

Upstream uses GeographicLib's `TransverseMercatorExact`, an elliptic-function
algorithm; we reuse the Krüger series already in the crate. Whether that is close
enough is a measurement, not an opinion, so the sweep runs out to ten degrees from
the central meridian -- far beyond any real map, which sits within a fraction of a
degree of it. The observed worst case is about 7 nanometres in northing, two orders
below the tolerance declared above; the runner prints the actual deviation each run,
so drift would be visible rather than silently absorbed.

The origin supplies the central meridian and a northing offset and nothing else.
There is deliberately no easting offset: upstream computes one and never applies it,
which is invisible when the origin sits on its own central meridian -- as it always
does, since the origin *defines* the meridian.
"""

from canon import emit, run

from autoware_lanelet2_extension_python.projection import TransverseMercatorProjector
from lanelet2.core import BasicPoint3d, GPSPoint
from lanelet2.io import Origin

BASE_LON = 139.6917


def main():
    for base_lat in (0.0, 35.6895, 60.0, 80.0):
        projector = TransverseMercatorProjector(Origin(base_lat, BASE_LON, 0.0))
        origin = projector.origin().position
        emit("origin_%g" % base_lat, [origin.lat, origin.lon, origin.ele])

        forward, reverse = [], []
        for dlon in (0.0, 1e-4, 1e-3, 0.01, 0.1, 0.5, 1.0, 3.0, 5.0, 10.0):
            for dlat in (0.0, 0.1, -0.1, 1.0, -1.0, 5.0, -5.0):
                lat, lon = base_lat + dlat, BASE_LON + dlon
                if not -84.0 < lat < 84.0:
                    continue
                xyz = projector.forward(GPSPoint(lat, lon, 12.5))
                forward.append([xyz.x, xyz.y, xyz.z])
                back = projector.reverse(BasicPoint3d(xyz.x, xyz.y, xyz.z))
                reverse.append([back.lat, back.lon, back.ele])
        emit("forward_%g" % base_lat, forward)
        emit("reverse_%g" % base_lat, reverse)

    # The origin projects to zero northing by subtraction, and to zero easting because
    # it is the central meridian -- two different reasons for the same number.
    at = TransverseMercatorProjector(Origin(35.6895, BASE_LON, 0.0)).forward(
        GPSPoint(35.6895, BASE_LON, 0.0)
    )
    emit("origin_projects_to", [at.x, at.y, at.z])


run(main)
