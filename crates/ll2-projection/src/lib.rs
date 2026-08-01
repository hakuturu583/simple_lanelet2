//! Geodetic projections.
//!
//! Ports of the GeographicLib algorithms Lanelet2 uses, rather than a dependency:
//! `geographiclib-rs` covers only the geodesic problem, and the general-purpose
//! projection crates are Snyder-grade, which is millimetres-to-centimetres off at
//! UTM zone edges — enough to show up in a byte-level map comparison.
//!
//! Four projectors, matching what `lanelet2.projection` exposes:
//!
//! * [`SphericalMercator`] — the default, a plain spherical Mercator whose origin
//!   only supplies a scale factor. Not origin-shifted.
//! * [`Geocentric`] — WGS84 earth-centred, earth-fixed, no origin shift.
//! * [`LocalCartesian`] — east/north/up anchored at the origin.
//! * [`Utm`] — UTM with the zone pinned by the origin, and the origin's easting and
//!   northing subtracted by default.

pub mod geocentric;
pub mod local_cartesian;
pub mod mercator;
pub mod transverse_mercator;
pub mod transverse_mercator_projector;
pub mod utm;
pub mod utmups;

pub use geocentric::Geocentric;
pub use local_cartesian::LocalCartesian;
pub use mercator::SphericalMercator;
pub use transverse_mercator_projector::TransverseMercatorProjector;
pub use utm::Utm;

/// A geodetic position: degrees, degrees, metres above the ellipsoid.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GpsPoint {
    pub lat: f64,
    pub lon: f64,
    pub ele: f64,
}

impl GpsPoint {
    pub fn new(lat: f64, lon: f64, ele: f64) -> Self {
        GpsPoint { lat, lon, ele }
    }
}

/// The reference position a map's local coordinates are relative to.
///
/// The `is_default` flag is not exposed to Python but changes behaviour: parsers
/// refuse to load a georeferenced map through a default-constructed origin, because
/// the resulting coordinates would be meaningless.
#[derive(Clone, Copy, Debug)]
pub struct Origin {
    pub position: GpsPoint,
    pub is_default: bool,
}

impl Origin {
    pub fn new(position: GpsPoint) -> Self {
        Origin {
            position,
            is_default: false,
        }
    }

    /// The origin you get from `Origin()`, which parsers reject.
    pub fn default_origin() -> Self {
        Origin {
            position: GpsPoint::default(),
            is_default: true,
        }
    }
}

impl Default for Origin {
    fn default() -> Self {
        Origin::default_origin()
    }
}

/// Something went wrong converting between geodetic and local coordinates.
#[derive(Debug, Clone)]
pub enum ProjectionError {
    Forward(String),
    Reverse(String),
}

impl ProjectionError {
    pub fn message(&self) -> &str {
        match self {
            ProjectionError::Forward(message) | ProjectionError::Reverse(message) => message,
        }
    }
}

impl std::fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// Converts between geodetic positions and a map's local metric coordinates.
pub trait Projector {
    fn forward(&self, point: GpsPoint) -> Result<[f64; 3], ProjectionError>;
    fn reverse(&self, point: [f64; 3]) -> Result<GpsPoint, ProjectionError>;
    fn origin(&self) -> Origin;
}

/// WGS84 ellipsoid parameters, shared by every projection here.
pub mod wgs84 {
    /// Equatorial radius, metres.
    pub const A: f64 = 6_378_137.0;
    /// Flattening.
    pub const F: f64 = 1.0 / 298.257_223_563;
    /// First eccentricity squared, `f * (2 - f)`.
    pub const E2: f64 = F * (2.0 - F);
    /// `1 - e^2`, which is also `(1 - f)^2`.
    pub const E2M: f64 = (1.0 - F) * (1.0 - F);
}

/// One degree in radians — GeographicLib's `Math::degree()`.
///
/// Kept as a named constant because *which way round* it is used matters: degrees to
/// radians multiplies by it, radians to degrees divides by it. Reaching for Rust's
/// `to_degrees()` instead silently multiplies by the rounded reciprocal.
pub(crate) const DEGREE: f64 = std::f64::consts::PI / 180.0;

/// Degree-argument sine and cosine, exact at the multiples of 90°.
///
/// GeographicLib reduces the angle before calling the trigonometric functions so
/// that `sind(180)` is exactly zero rather than 1.2e-16, which matters because the
/// result feeds an `atan2`.
pub(crate) fn sincosd(angle_degrees: f64) -> (f64, f64) {
    if angle_degrees.is_nan() {
        return (f64::NAN, f64::NAN);
    }
    let mut remainder = angle_degrees % 360.0;
    let quadrant = (remainder / 90.0).round();
    remainder -= 90.0 * quadrant;
    let radians = remainder.to_radians();
    let (sin, cos) = radians.sin_cos();
    let quadrant = (quadrant as i64).rem_euclid(4);
    let (mut sin, mut cos) = match quadrant {
        0 => (sin, cos),
        1 => (cos, -sin),
        2 => (-sin, -cos),
        _ => (-cos, sin),
    };
    // Preserve the sign of zero, as GeographicLib does.
    if angle_degrees == 0.0 {
        sin = angle_degrees;
    }
    if sin == 0.0 {
        sin = sin.copysign(angle_degrees);
    }
    cos += 0.0;
    (sin, cos)
}

/// `atan2` in degrees, with GeographicLib's quadrant handling.
pub(crate) fn atan2d(y: f64, x: f64) -> f64 {
    // Swapping the arguments in the steep quadrants keeps the result accurate.
    let mut y = y;
    let mut x = x;
    let mut quadrant = 0;
    if y.abs() > x.abs() {
        std::mem::swap(&mut x, &mut y);
        quadrant = 2;
    }
    if x < 0.0 {
        x = -x;
        quadrant += 1;
    }
    // `to_degrees()` multiplies by 180/pi; GeographicLib *divides* by pi/180. The two
    // constants are not exact reciprocals in binary floating point, so the results
    // differ by one ulp for about 11% of inputs — which is enough to move the last
    // printed digit of a latitude and change a written map by a byte.
    let angle = y.atan2(x) / DEGREE;
    match quadrant {
        0 => angle,
        1 => (if y >= 0.0 { 180.0 } else { -180.0 }) - angle,
        2 => 90.0 - angle,
        _ => angle - 90.0,
    }
}

pub(crate) fn atand(x: f64) -> f64 {
    atan2d(x, 1.0)
}

/// Reduces a longitude to `(-180, 180]`.
pub(crate) fn ang_normalize(angle_degrees: f64) -> f64 {
    let remainder = angle_degrees % 360.0;
    if remainder <= -180.0 {
        remainder + 360.0
    } else if remainder > 180.0 {
        remainder - 360.0
    } else {
        remainder
    }
}

/// The difference of two longitudes, reduced to `(-180, 180]`.
pub(crate) fn ang_diff(from: f64, to: f64) -> f64 {
    ang_normalize(ang_normalize(to) - ang_normalize(from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degree_trig_is_exact_at_the_quadrants() {
        for (angle, expected) in [
            (0.0, (0.0, 1.0)),
            (90.0, (1.0, 0.0)),
            (180.0, (0.0, -1.0)),
            (270.0, (-1.0, 0.0)),
            (360.0, (0.0, 1.0)),
            (-90.0, (-1.0, 0.0)),
        ] {
            let (sin, cos) = sincosd(angle);
            assert_eq!((sin, cos), expected, "sincosd({angle})");
        }
        let (sin, cos) = sincosd(30.0);
        assert!((sin - 0.5).abs() < 1e-15);
        assert!((cos - 3f64.sqrt() / 2.0).abs() < 1e-15);
    }

    #[test]
    fn atan2_in_degrees_covers_every_quadrant() {
        assert_eq!(atan2d(0.0, 1.0), 0.0);
        assert_eq!(atan2d(1.0, 0.0), 90.0);
        assert_eq!(atan2d(0.0, -1.0), 180.0);
        assert_eq!(atan2d(-1.0, 0.0), -90.0);
        assert!((atan2d(1.0, 1.0) - 45.0).abs() < 1e-13);
        assert!((atan2d(-1.0, -1.0) + 135.0).abs() < 1e-13);
    }

    #[test]
    fn longitudes_reduce_to_a_half_open_turn() {
        assert_eq!(ang_normalize(0.0), 0.0);
        assert_eq!(ang_normalize(180.0), 180.0);
        assert_eq!(ang_normalize(-180.0), 180.0);
        assert_eq!(ang_normalize(190.0), -170.0);
        assert_eq!(ang_normalize(-190.0), 170.0);
        assert_eq!(ang_diff(179.0, -179.0), 2.0);
        assert_eq!(ang_diff(-179.0, 179.0), -2.0);
    }

    #[test]
    fn a_default_origin_is_flagged_but_a_zero_one_is_not() {
        assert!(Origin::default().is_default);
        assert!(!Origin::new(GpsPoint::new(0.0, 0.0, 0.0)).is_default);
    }
}
