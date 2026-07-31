# TOL: abs=1e-6 rel=1e-12
"""The four projectors.

Two behaviours are easy to assume wrongly and are pinned here. `MercatorProjector`
takes only a *scale factor* from its origin, not a shift, so `forward(origin)` is
nowhere near zero. And `origin()` is a method, not a property.

UTM is the one that has to be numerically right: the zone and hemisphere are fixed
by the origin, and the Krüger series has to hold up at zone edges, across the Norway
and Svalbard zone irregularities, and in the southern hemisphere.

The tolerance is 1e-6 m on projected coordinates -- a micrometre -- and the default
1e-12 relative elsewhere.
"""

from canon import emit, expect_raises, run

from lanelet2.core import BasicPoint3d, GPSPoint
from lanelet2.io import Origin
from lanelet2.projection import (
    GeocentricProjector,
    LocalCartesianProjector,
    MercatorProjector,
    Projector,
    UtmProjector,
)

# Zone edges, the Norway and Svalbard exceptions, both hemispheres, the antimeridian.
GRID = [
    (49.0, 8.4), (49.0, 9.0), (49.0, 6.1), (49.0, 11.9),
    (0.0, 8.4), (-33.87, 8.4), (60.0, 8.4), (83.0, 8.4), (-79.0, 8.4),
    (49.01, 8.41), (48.99, 8.39), (49.0, 8.400001),
]


def sweep(name, projector, points):
    for lat, lon in points:
        for ele in (0.0, 123.5):
            projected = projector.forward(GPSPoint(lat, lon, ele))
            emit("{}:fwd:{}:{}:{}".format(name, lat, lon, ele),
                 [projected.x, projected.y, projected.z])
            back = projector.reverse(projected)
            emit("{}:rev:{}:{}:{}".format(name, lat, lon, ele),
                 [back.lat, back.lon, back.ele])


def main():
    origin = Origin(49.0, 8.4, 0.0)
    emit("origin_position", [origin.position.lat, origin.position.lon, origin.position.ele])
    emit("origin_default", [Origin().position.lat, Origin().position.lon])
    emit("origin_from_gps", Origin(GPSPoint(1.0, 2.0, 3.0)).position.ele)

    # Upstream registers the third keyword under the same name as the second, so
    # `lon=` silently fills the altitude too and `alt=` is ignored: there,
    # Origin(lat=49, lon=8.4, alt=100) ends up 8.4 metres above the ellipsoid.
    emit("origin_alt_kwarg", Origin(lat=49.0, lon=8.4, alt=100.0).position.ele)
    emit("origin_lon_kwarg_only", Origin(lat=49.0, lon=8.4).position.ele)
    emit("origin_positional", Origin(49.0, 8.4, 100.0).position.ele)
    expect_raises("origin_bogus_kwarg", lambda: Origin(lat=49.0, bogus=1.0))

    emit("gps_repr", repr(GPSPoint(49.0, 8.4, 100.0)))
    emit("gps_alt_alias", GPSPoint(1.0, 2.0, 3.0).alt)

    utm = UtmProjector(origin)
    emit("utm_origin_is_zero", [round(v, 9) for v in
                                [utm.forward(GPSPoint(49.0, 8.4, 0.0)).x,
                                 utm.forward(GPSPoint(49.0, 8.4, 0.0)).y]])
    emit("utm_origin_method", [utm.origin().position.lat, utm.origin().position.lon])
    sweep("utm", utm, GRID)

    raw = UtmProjector(origin, False, False)
    sweep("utm_nooffset", raw, [(49.0, 8.4), (49.0, 9.0), (60.0, 5.0)])

    # Fixing the zone means a point in the next zone is transferred, not rejected.
    emit("utm_next_zone", [utm.forward(GPSPoint(49.0, 13.0, 0.0)).x,
                           utm.forward(GPSPoint(49.0, 13.0, 0.0)).y])
    strict = UtmProjector(origin, True, True)
    expect_raises("utm_strict_next_zone", lambda: strict.forward(GPSPoint(49.0, 13.0, 0.0)), msg=True)

    # The Norway and Svalbard zone irregularities.
    for lat, lon in [(60.0, 4.0), (78.0, 10.0), (78.0, 25.0)]:
        local = UtmProjector(Origin(lat, lon, 0.0), False, False)
        projected = local.forward(GPSPoint(lat, lon, 0.0))
        emit("utm_zone_exception:{}:{}".format(lat, lon), [projected.x, projected.y])

    mercator = MercatorProjector(origin)
    # The origin supplies a scale factor only, so this is far from zero.
    at_origin = mercator.forward(GPSPoint(49.0, 8.4, 0.0))
    emit("mercator_origin_not_zero", [at_origin.x, at_origin.y])
    sweep("mercator", mercator, GRID[:8])

    geocentric = GeocentricProjector()
    emit("geocentric_origin", [geocentric.origin().position.lat,
                               geocentric.origin().position.lon,
                               geocentric.origin().position.ele])
    sweep("geocentric", geocentric, GRID)

    local = LocalCartesianProjector(origin)
    emit("local_origin_is_zero",
         [round(v, 9) for v in [local.forward(GPSPoint(49.0, 8.4, 0.0)).x,
                                local.forward(GPSPoint(49.0, 8.4, 0.0)).y,
                                local.forward(GPSPoint(49.0, 8.4, 0.0)).z]])
    sweep("local", local, GRID)

    expect_raises("projector_init", lambda: Projector(), msg=True)
    expect_raises("reverse_wrong_type", lambda: utm.reverse(GPSPoint(0.0, 0.0, 0.0)))
    emit("reverse_basic_point", [round(v, 6) for v in
                                 [utm.reverse(BasicPoint3d(0.0, 0.0, 0.0)).lat,
                                  utm.reverse(BasicPoint3d(0.0, 0.0, 0.0)).lon]])


run(main)
