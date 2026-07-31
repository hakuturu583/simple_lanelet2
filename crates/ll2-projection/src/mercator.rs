//! Spherical Mercator, the default projector.
//!
//! Worth knowing before using it: the origin supplies only a *scale factor*, taken
//! from the cosine of its latitude. There is no shift, so `forward(origin)` is not
//! the local origin — it is wherever the whole-earth Mercator puts it, typically
//! hundreds of kilometres from zero.
//!
//! Upstream: `lanelet2_io/include/lanelet2_io/Projection.h:54-73`
//! (`SphericalMercatorProjector`, bound to Python as `MercatorProjector`)

use std::f64::consts::PI;

use crate::{GpsPoint, Origin, ProjectionError, Projector};

/// The sphere radius upstream uses, which is WGS84's equatorial radius.
const EARTH_RADIUS: f64 = 6_378_137.0;

pub struct SphericalMercator {
    origin: Origin,
    scale: f64,
}

impl SphericalMercator {
    pub fn new(origin: Origin) -> Self {
        SphericalMercator {
            scale: (origin.position.lat * PI / 180.0).cos(),
            origin,
        }
    }
}

impl Projector for SphericalMercator {
    fn forward(&self, point: GpsPoint) -> Result<[f64; 3], ProjectionError> {
        Ok([
            self.scale * point.lon * PI * EARTH_RADIUS / 180.0,
            self.scale * EARTH_RADIUS * ((90.0 + point.lat) * PI / 360.0).tan().ln(),
            point.ele,
        ])
    }

    fn reverse(&self, point: [f64; 3]) -> Result<GpsPoint, ProjectionError> {
        Ok(GpsPoint::new(
            360.0 * (point[1] / (EARTH_RADIUS * self.scale)).exp().atan() / PI - 90.0,
            point[0] * 180.0 / (PI * EARTH_RADIUS * self.scale),
            point[2],
        ))
    }

    fn origin(&self) -> Origin {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_equator_maps_to_zero_northing() {
        let projector = SphericalMercator::new(Origin::new(GpsPoint::new(0.0, 0.0, 0.0)));
        let projected = projector.forward(GpsPoint::new(0.0, 0.0, 0.0)).unwrap();
        assert!(projected[0].abs() < 1e-9 && projected[1].abs() < 1e-9);
    }

    #[test]
    fn the_origin_is_a_scale_factor_and_not_a_shift() {
        let projector = SphericalMercator::new(Origin::new(GpsPoint::new(49.0, 8.4, 0.0)));
        let at_origin = projector.forward(GpsPoint::new(49.0, 8.4, 0.0)).unwrap();
        assert!(
            at_origin[0].abs() > 100_000.0,
            "the origin does not project to zero: {at_origin:?}"
        );
    }

    #[test]
    fn round_trips_over_the_usable_latitude_band() {
        let projector = SphericalMercator::new(Origin::new(GpsPoint::new(49.0, 8.4, 0.0)));
        let mut worst = 0.0f64;
        for lat in (-80..=80).step_by(7) {
            for lon in (-180..=180).step_by(13) {
                let point = GpsPoint::new(lat as f64, lon as f64, 12.5);
                let back = projector
                    .reverse(projector.forward(point).unwrap())
                    .unwrap();
                worst = worst
                    .max((back.lat - point.lat).abs())
                    .max((back.lon - point.lon).abs())
                    .max((back.ele - point.ele).abs());
            }
        }
        assert!(worst < 1e-11, "worst round-trip error {worst}");
    }
}
