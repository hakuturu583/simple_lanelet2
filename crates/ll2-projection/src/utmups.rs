//! UTM zone selection and the false-origin offsets.
//!
//! Two irregularities in the zone grid are load-bearing and easy to miss: zone 32
//! is widened westward over southern Norway, and the zones over Svalbard are
//! 12° wide instead of 6°. Getting either wrong silently shifts every coordinate in
//! an affected map by hundreds of kilometres.
//!
//! UPS — the polar stereographic system used beyond ±84°/±80° — is not implemented;
//! Lanelet2 maps do not reach the poles. Asking for a position there is an error
//! rather than a silent fallback.
//!
//! Upstream: GeographicLib `UTMUPS`, `MGRS::LatitudeBand`

use crate::transverse_mercator::TransverseMercator;
use crate::{ProjectionError, ang_normalize};

/// False easting applied to every UTM zone, metres.
pub const FALSE_EASTING: f64 = 500_000.0;
/// False northing applied in the southern hemisphere, metres.
pub const FALSE_NORTHING_SOUTH: f64 = 10_000_000.0;

/// The MGRS latitude band index, `floor(lat / 8)` clamped to the band range.
fn latitude_band(lat: f64) -> i32 {
    let band = (lat / 8.0).floor() as i32;
    band.clamp(-10, 9)
}

/// The UTM zone a position falls in, including the Norway and Svalbard exceptions.
pub fn standard_zone(lat: f64, lon: f64) -> Result<i32, ProjectionError> {
    if !(-80.0..84.0).contains(&lat) {
        return Err(ProjectionError::Forward(format!(
            "Latitude {lat} is outside the UTM band; UPS is not supported"
        )));
    }
    let mut ilon = ang_normalize(lon).floor() as i32;
    if ilon == 180 {
        ilon = -180;
    }
    let mut zone = (ilon + 186) / 6;
    let band = latitude_band(lat);
    if band == 7 && zone == 31 && ilon >= 3 {
        // Southern Norway: zone 32 is widened west to keep the country whole.
        zone = 32;
    } else if band == 9 && (0..42).contains(&ilon) {
        // Svalbard: four 12°-wide zones instead of eight 6°-wide ones.
        zone = 2 * ((ilon + 183) / 12) + 1;
    }
    Ok(zone)
}

/// The central meridian of a zone, in degrees.
pub fn central_meridian(zone: i32) -> f64 {
    6.0 * zone as f64 - 183.0
}

/// Projects into the zone the position naturally falls in.
///
/// Returns the zone, whether it is northern, and the easting/northing with the
/// false origin already applied.
pub fn forward(lat: f64, lon: f64) -> Result<(i32, bool, f64, f64), ProjectionError> {
    let zone = standard_zone(lat, lon)?;
    forward_into(lat, lon, zone)
}

/// Projects into a specific zone, which may not be the natural one.
pub fn forward_into(lat: f64, lon: f64, zone: i32) -> Result<(i32, bool, f64, f64), ProjectionError> {
    if !(1..=60).contains(&zone) {
        return Err(ProjectionError::Forward(format!("Zone {zone} is out of range")));
    }
    let northp = lat >= 0.0;
    let (x, y) = TransverseMercator::utm().forward(central_meridian(zone), lat, lon);
    Ok((
        zone,
        northp,
        x + FALSE_EASTING,
        y + if northp { 0.0 } else { FALSE_NORTHING_SOUTH },
    ))
}

/// Recovers a geodetic position from zone-relative coordinates.
pub fn reverse(zone: i32, northp: bool, x: f64, y: f64) -> Result<(f64, f64), ProjectionError> {
    if !(1..=60).contains(&zone) {
        return Err(ProjectionError::Reverse(format!("Zone {zone} is out of range")));
    }
    let (lat, lon) = TransverseMercator::utm().reverse(
        central_meridian(zone),
        x - FALSE_EASTING,
        y - if northp { 0.0 } else { FALSE_NORTHING_SOUTH },
    );
    Ok((lat, lon))
}

/// Re-expresses coordinates from one zone in another.
///
/// Round-trips through geodetic coordinates, which is what GeographicLib does when
/// the zones differ. The returned zone is the one actually used: if the position is
/// too far outside `zone_out` to be represented there, it comes back different, and
/// the caller decides whether that is acceptable.
pub fn transfer(
    zone_in: i32,
    northp_in: bool,
    x_in: f64,
    y_in: f64,
    zone_out: i32,
    northp_out: bool,
) -> Result<(f64, f64, i32), ProjectionError> {
    if zone_in == zone_out && northp_in == northp_out {
        return Ok((x_in, y_in, zone_out));
    }
    let (lat, lon) = reverse(zone_in, northp_in, x_in, y_in)?;
    let (zone, northp, x, y) = forward_into(lat, lon, zone_out)?;
    if northp != northp_out {
        // Crossing the equator inside a fixed zone: re-apply the caller's false
        // northing so the result stays continuous.
        let shift = if northp_out { -FALSE_NORTHING_SOUTH } else { FALSE_NORTHING_SOUTH };
        return Ok((x, y + shift, zone));
    }
    Ok((x, y, zone))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_are_six_degrees_wide_away_from_the_exceptions() {
        assert_eq!(standard_zone(0.0, 0.0).unwrap(), 31);
        assert_eq!(standard_zone(0.0, 5.9).unwrap(), 31);
        assert_eq!(standard_zone(0.0, 6.0).unwrap(), 32);
        assert_eq!(standard_zone(0.0, -180.0).unwrap(), 1);
        assert_eq!(standard_zone(0.0, 179.0).unwrap(), 60);
        assert_eq!(standard_zone(49.0, 8.4).unwrap(), 32);
        assert_eq!(standard_zone(-33.0, 151.0).unwrap(), 56);
    }

    #[test]
    fn southern_norway_widens_zone_32_westward() {
        // Band 7 is 56..64 degrees north.
        assert_eq!(standard_zone(60.0, 3.0).unwrap(), 32);
        assert_eq!(standard_zone(60.0, 5.0).unwrap(), 32);
        assert_eq!(standard_zone(60.0, 2.9).unwrap(), 31, "west of the exception");
        assert_eq!(standard_zone(55.0, 5.0).unwrap(), 31, "south of the band");
        assert_eq!(standard_zone(64.0, 5.0).unwrap(), 31, "north of the band");
    }

    #[test]
    fn svalbard_uses_twelve_degree_zones() {
        // Band 9 is 72..80 degrees north.
        assert_eq!(standard_zone(78.0, 5.0).unwrap(), 31);
        assert_eq!(standard_zone(78.0, 10.0).unwrap(), 33);
        assert_eq!(standard_zone(78.0, 20.0).unwrap(), 33);
        assert_eq!(standard_zone(78.0, 25.0).unwrap(), 35);
        assert_eq!(standard_zone(78.0, 35.0).unwrap(), 37);
        assert_eq!(standard_zone(78.0, -5.0).unwrap(), 30, "west of the exception");
    }

    #[test]
    fn the_poles_are_rejected_rather_than_silently_wrong() {
        assert!(standard_zone(85.0, 0.0).is_err());
        assert!(standard_zone(-81.0, 0.0).is_err());
    }

    #[test]
    fn the_false_origin_puts_the_central_meridian_at_five_hundred_kilometres() {
        let (zone, northp, x, y) = forward(49.0, 9.0).unwrap();
        assert_eq!((zone, northp), (32, true));
        assert!((x - FALSE_EASTING).abs() < 1e-6, "x = {x}");
        assert!(y > 5_000_000.0);

        let (_, northp, _, y) = forward(-33.0, 151.0).unwrap();
        assert!(!northp);
        assert!(y > 6_000_000.0, "southern northings count down from 10,000 km");
    }

    #[test]
    fn round_trips_through_the_false_origin() {
        let mut worst = 0.0f64;
        for lat in [-79.0, -33.0, 0.0, 49.0, 83.0] {
            for lon in [-179.0, -60.0, 0.0, 8.4, 151.0] {
                let (zone, northp, x, y) = forward(lat, lon).unwrap();
                let (lat2, lon2) = reverse(zone, northp, x, y).unwrap();
                worst = worst.max((lat2 - lat).abs()).max((lon2 - lon).abs());
            }
        }
        assert!(worst < 1e-11, "worst round-trip error {worst} degrees");
    }

    #[test]
    fn transferring_into_a_neighbouring_zone_keeps_the_position() {
        // A point in zone 33, expressed in zone 32's frame.
        let (zone_in, northp, x, y) = forward(49.0, 13.0).unwrap();
        assert_eq!(zone_in, 33);
        let (x32, y32, zone) = transfer(zone_in, northp, x, y, 32, true).unwrap();
        assert_eq!(zone, 32);
        let (lat, lon) = reverse(32, true, x32, y32).unwrap();
        assert!((lat - 49.0).abs() < 1e-9 && (lon - 13.0).abs() < 1e-9);
    }
}
