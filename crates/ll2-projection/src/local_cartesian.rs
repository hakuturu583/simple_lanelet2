//! Local east/north/up coordinates anchored at the origin.
//!
//! Exact, not a small-angle approximation: the point is converted to earth-centred
//! coordinates, the origin's are subtracted, and the difference is rotated into the
//! local frame.
//!
//! Upstream: `lanelet2_projection/src/LocalCartesian.cpp`, GeographicLib
//! `LocalCartesian`

use crate::geocentric::Geocentric;
use crate::{GpsPoint, Origin, ProjectionError, Projector, sincosd};

pub struct LocalCartesian {
    origin: Origin,
    /// The origin in earth-centred coordinates.
    anchor: [f64; 3],
    /// Row-major rotation from local east/north/up to earth-centred.
    rotation: [f64; 9],
}

impl LocalCartesian {
    pub fn new(origin: Origin) -> Self {
        let (sin_phi, cos_phi) = sincosd(origin.position.lat);
        let (sin_lambda, cos_lambda) = sincosd(origin.position.lon);
        LocalCartesian {
            anchor: Geocentric::forward(origin.position),
            rotation: [
                -sin_lambda,
                -cos_lambda * sin_phi,
                cos_lambda * cos_phi,
                cos_lambda,
                -sin_lambda * sin_phi,
                sin_lambda * cos_phi,
                0.0,
                cos_phi,
                sin_phi,
            ],
            origin,
        }
    }
}

impl Projector for LocalCartesian {
    fn forward(&self, point: GpsPoint) -> Result<[f64; 3], ProjectionError> {
        let ecef = Geocentric::forward(point);
        let d = [
            ecef[0] - self.anchor[0],
            ecef[1] - self.anchor[1],
            ecef[2] - self.anchor[2],
        ];
        let r = &self.rotation;
        Ok([
            r[0] * d[0] + r[3] * d[1] + r[6] * d[2],
            r[1] * d[0] + r[4] * d[1] + r[7] * d[2],
            r[2] * d[0] + r[5] * d[1] + r[8] * d[2],
        ])
    }

    fn reverse(&self, point: [f64; 3]) -> Result<GpsPoint, ProjectionError> {
        let r = &self.rotation;
        let ecef = [
            self.anchor[0] + r[0] * point[0] + r[1] * point[1] + r[2] * point[2],
            self.anchor[1] + r[3] * point[0] + r[4] * point[1] + r[5] * point[2],
            self.anchor[2] + r[6] * point[0] + r[7] * point[1] + r[8] * point[2],
        ];
        Ok(Geocentric::reverse(ecef))
    }

    fn origin(&self) -> Origin {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projector() -> LocalCartesian {
        LocalCartesian::new(Origin::new(GpsPoint::new(49.0, 8.4, 100.0)))
    }

    #[test]
    fn the_origin_maps_to_zero() {
        let projected = projector()
            .forward(GpsPoint::new(49.0, 8.4, 100.0))
            .unwrap();
        assert!(projected.iter().all(|v| v.abs() < 1e-8), "{projected:?}");
    }

    #[test]
    fn the_axes_point_east_north_and_up() {
        let projector = projector();
        let east = projector.forward(GpsPoint::new(49.0, 8.5, 100.0)).unwrap();
        assert!(east[0] > 7000.0 && east[1].abs() < 10.0);

        let north = projector.forward(GpsPoint::new(49.1, 8.4, 100.0)).unwrap();
        assert!(north[1] > 11_000.0 && north[0].abs() < 10.0);

        let up = projector.forward(GpsPoint::new(49.0, 8.4, 150.0)).unwrap();
        assert!((up[2] - 50.0).abs() < 1e-6 && up[0].abs() < 1e-6);
    }

    #[test]
    fn round_trips_to_sub_nanometre() {
        let projector = projector();
        let mut worst = 0.0f64;
        for lat in [48.0, 49.0, 50.0, 0.0, -30.0, 80.0] {
            for lon in [7.0, 8.4, 10.0, -170.0, 100.0] {
                for ele in [-100.0, 0.0, 2000.0] {
                    let point = GpsPoint::new(lat, lon, ele);
                    let back = projector.reverse(projector.forward(point).unwrap()).unwrap();
                    worst = worst
                        .max((back.lat - point.lat).abs())
                        .max((back.lon - point.lon).abs())
                        .max((back.ele - point.ele).abs() / 1e5);
                }
            }
        }
        assert!(worst < 1e-9, "worst round-trip error {worst}");
    }
}
