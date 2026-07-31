//! WGS84 earth-centred, earth-fixed coordinates.
//!
//! The forward direction is closed form. The reverse is the exact, non-iterative
//! solution GeographicLib uses (Vermeille's quartic, arranged to avoid cancellation
//! near the centre and the poles) rather than a Bowring iteration: Bowring converges
//! to about a nanometre too, but the last bits differ, and the diff tolerance would
//! have to be loosened for no reason.
//!
//! Upstream: `lanelet2_projection/src/Geocentric.cpp`, GeographicLib `Geocentric`

use crate::{GpsPoint, Origin, ProjectionError, Projector, atan2d, sincosd, wgs84};

/// WGS84 ECEF. There is no origin shift, so `forward` returns absolute
/// earth-centred coordinates.
pub struct Geocentric {
    origin: Origin,
}

impl Default for Geocentric {
    fn default() -> Self {
        Geocentric::new()
    }
}

impl Geocentric {
    /// Upstream gives this projector a dummy non-default origin purely so that the
    /// map parser's "you must pass an origin" guard does not fire.
    pub fn new() -> Self {
        Geocentric {
            origin: Origin::new(GpsPoint::new(90.0, 0.0, -6_356_752.3)),
        }
    }

    pub fn forward(point: GpsPoint) -> [f64; 3] {
        let (sin_phi, cos_phi) = sincosd(point.lat);
        let (sin_lambda, cos_lambda) = sincosd(point.lon);
        let n = wgs84::A / (1.0 - wgs84::E2 * sin_phi * sin_phi).sqrt();
        [
            (n + point.ele) * cos_phi * cos_lambda,
            (n + point.ele) * cos_phi * sin_lambda,
            (n * wgs84::E2M + point.ele) * sin_phi,
        ]
    }

    /// The exact inverse.
    ///
    /// Follows GeographicLib's arrangement step for step; the intermediate names
    /// are kept so the two can be compared side by side.
    pub fn reverse(xyz: [f64; 3]) -> GpsPoint {
        let [x, y, z] = xyz;
        let e2 = wgs84::E2;
        let e2m = wgs84::E2M;
        let e4a = e2 * e2;
        let e2a = e2.abs();

        let r = f64::hypot(x, y);
        let (sin_lambda, cos_lambda) = if r != 0.0 { (y / r, x / r) } else { (0.0, 1.0) };

        let p = (r / wgs84::A).powi(2);
        let q = e2m * (z / wgs84::A).powi(2);
        let big_r = (p + q - e4a) / 6.0;

        let (sin_phi, cos_phi, height);
        if !(e4a * q == 0.0 && big_r <= 0.0) {
            // Multiplying through by powers of `big_r` avoids a division by zero
            // when it is zero.
            let s = e4a * p * q / 4.0;
            let r2 = big_r * big_r;
            let r3 = big_r * r2;
            let disc = s * (2.0 * r3 + s);
            let mut u = big_r;
            if disc >= 0.0 {
                let mut t3 = s + r3;
                t3 += if t3 < 0.0 { -disc.sqrt() } else { disc.sqrt() };
                let t = t3.cbrt();
                u += t + if t != 0.0 { r2 / t } else { 0.0 };
            } else {
                // The discriminant is negative only very close to the centre.
                let angle = (-disc).sqrt().atan2(-(s + r3));
                u += 2.0 * big_r * (angle / 3.0).cos();
            }
            let v = (u * u + e4a * q).sqrt();
            let uv = if u < 0.0 { e4a * q / (v - u) } else { u + v };
            let w = (e2a * (uv - q) / (2.0 * v)).max(0.0);
            let k = uv / ((uv + w * w).sqrt() + w);
            let k1 = k;
            let k2 = k + e2;
            let d = k1 * r / k2;
            let h = f64::hypot(z / k1, r / k2);
            sin_phi = (z / k1) / h;
            cos_phi = (r / k2) / h;
            height = (1.0 - e2m / k1) * f64::hypot(d, z);
        } else {
            // On the equatorial disc inside the ellipsoid.
            let zz = (e4a - p).max(0.0).sqrt() / (e2a * (1.0 - e2).sqrt());
            let xx = (p / e4a - 1.0).max(0.0).sqrt() / e2a;
            let h = f64::hypot(zz, xx);
            sin_phi = if h != 0.0 { zz / h } else { 1.0 };
            cos_phi = if h != 0.0 { xx / h } else { 0.0 };
            let sin_phi = if z < 0.0 { -sin_phi } else { sin_phi };
            height = -wgs84::A * (1.0 - e2) / (1.0 - e2 * sin_phi * sin_phi).sqrt() * h;
            return GpsPoint::new(
                atan2d(sin_phi, cos_phi),
                atan2d(sin_lambda, cos_lambda),
                height,
            );
        }

        GpsPoint::new(
            atan2d(sin_phi, cos_phi),
            atan2d(sin_lambda, cos_lambda),
            height,
        )
    }
}

impl Projector for Geocentric {
    fn forward(&self, point: GpsPoint) -> Result<[f64; 3], ProjectionError> {
        Ok(Geocentric::forward(point))
    }

    fn reverse(&self, point: [f64; 3]) -> Result<GpsPoint, ProjectionError> {
        Ok(Geocentric::reverse(point))
    }

    fn origin(&self) -> Origin {
        self.origin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_equator_and_the_poles_land_where_they_should() {
        let equator = Geocentric::forward(GpsPoint::new(0.0, 0.0, 0.0));
        assert!((equator[0] - wgs84::A).abs() < 1e-6);
        assert!(equator[1].abs() < 1e-9 && equator[2].abs() < 1e-9);

        let pole = Geocentric::forward(GpsPoint::new(90.0, 0.0, 0.0));
        let polar_radius = wgs84::A * (1.0 - wgs84::F);
        assert!(pole[0].abs() < 1e-6 && pole[1].abs() < 1e-6);
        assert!((pole[2] - polar_radius).abs() < 1e-6);
    }

    #[test]
    fn round_trips_to_sub_nanometre_over_a_wide_grid() {
        // A longitude of exactly -180 comes back as +180, which is the same
        // meridian, so compare longitudes the short way round.
        let lon_error = |a: f64, b: f64| {
            let diff = (a - b).abs() % 360.0;
            diff.min(360.0 - diff)
        };
        let mut worst = 0.0f64;
        for lat in (-89..=89).step_by(7) {
            for lon in (-180..=180).step_by(11) {
                for ele in [-500.0, 0.0, 1234.5, 20_000.0] {
                    let point = GpsPoint::new(lat as f64, lon as f64, ele);
                    let back = Geocentric::reverse(Geocentric::forward(point));
                    worst = worst
                        .max((back.lat - point.lat).abs())
                        .max(lon_error(back.lon, point.lon))
                        .max((back.ele - point.ele).abs() / 1e5);
                }
            }
        }
        assert!(worst < 1e-9, "worst round-trip error {worst}");
    }
}
