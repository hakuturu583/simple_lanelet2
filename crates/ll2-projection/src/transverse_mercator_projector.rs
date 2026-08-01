//! The Autoware extension's transverse Mercator projector.
//!
//! The origin supplies the central meridian and a northing offset — and *only* those.
//! There is no easting offset: upstream computes one and never applies it, which is
//! harmless rather than a bug, because the easting is zero at the central meridian by
//! construction. Reproducing that asymmetry keeps `forward` matching; "fixing" it
//! would break every map whose origin is not exactly on its own central meridian.
//!
//! Upstream uses GeographicLib's `TransverseMercatorExact`, an elliptic-function
//! algorithm; we use the Krüger series already here. Every point of a real map sits
//! within a fraction of a degree of the central meridian, where the truncation error
//! is far below what a coordinate can express — the sweep in `1200_aw_projection`
//! measures the gap rather than assuming it.
//!
//! Upstream: `autoware_lanelet2_extension/lib/transverse_mercator_projector.cpp`

use crate::transverse_mercator::TransverseMercator;
use crate::{GpsPoint, Origin, ProjectionError, Projector};

/// The scale factor upstream defaults to. Python cannot reach the two-argument
/// constructor, so this is the only value a `TransverseMercatorProjector` ever has.
pub const DEFAULT_SCALE_FACTOR: f64 = 0.9996;

pub struct TransverseMercatorProjector {
    origin: Origin,
    projection: TransverseMercator,
    central_meridian: f64,
    origin_northing: f64,
}

impl TransverseMercatorProjector {
    pub fn new(origin: Origin, scale_factor: f64) -> Self {
        let projection = TransverseMercator::new(
            crate::wgs84::A,
            crate::wgs84::F,
            scale_factor,
        );
        let central_meridian = origin.position.lon;
        let (_, origin_northing) =
            projection.forward(central_meridian, origin.position.lat, origin.position.lon);
        TransverseMercatorProjector {
            origin,
            projection,
            central_meridian,
            origin_northing,
        }
    }
}

impl Projector for TransverseMercatorProjector {
    fn forward(&self, point: GpsPoint) -> Result<[f64; 3], ProjectionError> {
        let (x, y) = self
            .projection
            .forward(self.central_meridian, point.lat, point.lon);
        // Only the northing is re-based; see the module comment.
        Ok([x, y - self.origin_northing, point.ele])
    }

    fn reverse(&self, point: [f64; 3]) -> Result<GpsPoint, ProjectionError> {
        let (lat, lon) = self.projection.reverse(
            self.central_meridian,
            point[0],
            point[1] + self.origin_northing,
        );
        Ok(GpsPoint::new(lat, lon, point[2]))
    }

    fn origin(&self) -> Origin {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_origin_is_the_local_zero_in_northing_only() {
        let origin = Origin::new(GpsPoint::new(35.6895, 139.6917, 0.0));
        let projector = TransverseMercatorProjector::new(origin, DEFAULT_SCALE_FACTOR);
        let at_origin = projector.forward(origin.position).unwrap();
        assert!(at_origin[1].abs() < 1e-9, "northing at origin: {}", at_origin[1]);
        // The easting is zero here too, but because the origin *is* the central
        // meridian -- not because an offset was applied.
        assert!(at_origin[0].abs() < 1e-9, "easting at origin: {}", at_origin[0]);
    }

    #[test]
    fn a_point_east_of_the_origin_keeps_its_easting_unshifted() {
        let origin = Origin::new(GpsPoint::new(35.6895, 139.6917, 0.0));
        let projector = TransverseMercatorProjector::new(origin, DEFAULT_SCALE_FACTOR);
        let east = projector
            .forward(GpsPoint::new(35.6895, 139.7917, 0.0))
            .unwrap();
        assert!(east[0] > 8_000.0, "easting {} should be ~9 km", east[0]);
    }

    #[test]
    fn round_trips_across_a_map_sized_extent() {
        let origin = Origin::new(GpsPoint::new(35.6895, 139.6917, 0.0));
        let projector = TransverseMercatorProjector::new(origin, DEFAULT_SCALE_FACTOR);
        let mut worst: f64 = 0.0;
        for lat in [35.6, 35.6895, 35.8] {
            for lon in [139.6, 139.6917, 139.8] {
                let point = GpsPoint::new(lat, lon, 12.5);
                let xyz = projector.forward(point).unwrap();
                let back = projector.reverse(xyz).unwrap();
                worst = worst.max((back.lat - lat).abs()).max((back.lon - lon).abs());
            }
        }
        assert!(worst < 1e-11, "worst round-trip error {worst} degrees");
    }
}
