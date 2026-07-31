//! The German rule tables.
//!
//! Measured against `lanelet2==1.2.3` rather than transcribed from the C++ in the
//! cloned repository: those are different versions and they disagree, most visibly
//! on `bus_lane`, which the wheel treats like a road and the newer source does not.

use crate::{LaneChangeType, SpeedLimit, participants};

/// The speed limits a country defines for each kind of way.
#[derive(Clone, Copy, Debug)]
pub struct CountrySpeedLimits {
    pub vehicle_urban_road: SpeedLimit,
    pub vehicle_nonurban_road: SpeedLimit,
    pub vehicle_urban_highway: SpeedLimit,
    pub vehicle_nonurban_highway: SpeedLimit,
    pub play_street: SpeedLimit,
    pub pedestrian: SpeedLimit,
    pub bicycle: SpeedLimit,
}

/// Germany's limits. The motorway figures are *advisory*, not binding.
pub fn speed_limits() -> CountrySpeedLimits {
    CountrySpeedLimits {
        vehicle_urban_road: SpeedLimit::kmh(50.0, true),
        vehicle_nonurban_road: SpeedLimit::kmh(100.0, true),
        vehicle_urban_highway: SpeedLimit::kmh(130.0, false),
        vehicle_nonurban_highway: SpeedLimit::kmh(130.0, false),
        play_street: SpeedLimit::kmh(7.0, true),
        pedestrian: SpeedLimit::kmh(5.0, true),
        bicycle: SpeedLimit::kmh(20.0, true),
    }
}

/// The speed limit for a vehicle, by location and way subtype.
///
/// Anything not listed has no limit *and no sentinel of its own* — the caller
/// substitutes zero, which downstream reads as "unknown".
pub fn vehicle_speed_limit(
    limits: &CountrySpeedLimits,
    location: &str,
    subtype: &str,
) -> Option<SpeedLimit> {
    Some(match (location, subtype) {
        ("urban", "road") | ("urban", "bus_lane") => limits.vehicle_urban_road,
        ("nonurban", "road") | ("nonurban", "bus_lane") => limits.vehicle_nonurban_road,
        ("urban", "highway") => limits.vehicle_urban_highway,
        ("nonurban", "highway") => limits.vehicle_nonurban_highway,
        ("urban", "play_street") | ("nonurban", "play_street") => limits.play_street,
        ("urban", "exit") => limits.vehicle_urban_road,
        _ => return None,
    })
}

/// Which participants a way subtype admits.
///
/// The entries are matched by *prefix*, so a `vehicle` entry admits `vehicle:car`
/// but a `vehicle:emergency` entry does not admit a plain `vehicle`. A subtype that
/// is not listed at all admits nobody.
///
/// A bus lane carries a road's speed limit but admits only buses, taxis and
/// emergency vehicles — so a plain `vehicle` is refused while `vehicle:bus` is not.
pub fn subtype_allows(subtype: &str, participant: &str) -> bool {
    let allowed: &[&str] = match subtype {
        // An absent subtype defaults to a road for vehicles only.
        "" => &[participants::VEHICLE],
        "road" => &[participants::VEHICLE, participants::BICYCLE],
        // A bus lane admits exactly the vehicles allowed to use one.
        "bus_lane" => &[
            participants::VEHICLE_BUS,
            participants::VEHICLE_EMERGENCY,
            participants::VEHICLE_TAXI,
        ],
        "highway" => &[participants::VEHICLE],
        "bicycle_lane" => &[participants::BICYCLE],
        "play_street" => &[
            participants::PEDESTRIAN,
            participants::BICYCLE,
            participants::VEHICLE,
        ],
        "emergency_lane" => &[participants::VEHICLE_EMERGENCY],
        "exit" => &[
            participants::PEDESTRIAN,
            participants::BICYCLE,
            participants::VEHICLE,
        ],
        "walkway" | "crosswalk" | "stairs" => &[participants::PEDESTRIAN],
        "shared_walkway" => &[participants::PEDESTRIAN, participants::BICYCLE],
        _ => &[],
    };
    allowed.iter().any(|entry| participant.starts_with(entry))
}

/// Where a lane change is allowed across a boundary of this type.
///
/// A bicycle gets the vehicle rules first and falls back to the pedestrian ones,
/// which is how it may cross a low curbstone but not a solid line.
pub fn change_type(kind: &str, subtype: &str, participant: &str) -> LaneChangeType {
    if participant.starts_with(participants::VEHICLE) {
        return vehicle_change_type(kind, subtype);
    }
    if participant == participants::PEDESTRIAN {
        return pedestrian_change_type(kind, subtype);
    }
    if participant == participants::BICYCLE {
        let vehicle = vehicle_change_type(kind, subtype);
        if vehicle != LaneChangeType::None {
            return vehicle;
        }
        return pedestrian_change_type(kind, subtype);
    }
    LaneChangeType::None
}

fn vehicle_change_type(kind: &str, subtype: &str) -> LaneChangeType {
    match (kind, subtype) {
        ("line_thin" | "line_thick", "dashed") => LaneChangeType::Both,
        ("line_thin" | "line_thick", "dashed_solid") => LaneChangeType::ToRight,
        ("line_thin" | "line_thick", "solid_dashed") => LaneChangeType::ToLeft,
        _ => LaneChangeType::None,
    }
}

fn pedestrian_change_type(kind: &str, subtype: &str) -> LaneChangeType {
    match (kind, subtype) {
        ("curbstone", "low") => LaneChangeType::Both,
        _ => LaneChangeType::None,
    }
}

/// The speed a German traffic-sign code prescribes, in km/h.
///
/// `de274-30` and friends carry the limit in the code; a few signs are named
/// outright.
pub fn traffic_sign_speed_kmh(sign: &str) -> Option<f64> {
    match sign {
        "de274" | "de274_1" => Some(30.0),
        "de310" => Some(50.0),
        "de274_1-20" => Some(20.0),
        _ => sign
            .strip_prefix("de274-")
            .and_then(|value| value.parse().ok()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtype_entries_match_by_participant_prefix() {
        assert!(subtype_allows("road", "vehicle"));
        assert!(subtype_allows("road", "vehicle:car"));
        assert!(subtype_allows("road", "bicycle"));
        assert!(!subtype_allows("road", "pedestrian"));

        // The specific entry does not admit the general participant.
        assert!(subtype_allows("emergency_lane", "vehicle:emergency"));
        assert!(!subtype_allows("emergency_lane", "vehicle"));
        assert!(!subtype_allows("emergency_lane", "vehicle:car"));

        // An unlisted subtype admits nobody.
        assert!(!subtype_allows("parking", "vehicle"));
        assert!(!subtype_allows("freespace", "pedestrian"));
        // A bus lane has a road's speed limit but admits only the vehicles
        // entitled to use one.
        assert!(!subtype_allows("bus_lane", "vehicle"));
        assert!(!subtype_allows("bus_lane", "vehicle:car"));
        assert!(subtype_allows("bus_lane", "vehicle:bus"));
        assert!(subtype_allows("bus_lane", "vehicle:taxi"));
        assert!(subtype_allows("bus_lane", "vehicle:emergency"));
        assert!(vehicle_speed_limit(&speed_limits(), "urban", "bus_lane").is_some());
    }

    #[test]
    fn a_bicycle_falls_back_to_the_pedestrian_rules() {
        assert_eq!(
            change_type("curbstone", "low", "bicycle"),
            LaneChangeType::Both
        );
        assert_eq!(
            change_type("curbstone", "low", "vehicle"),
            LaneChangeType::None
        );
        assert_eq!(
            change_type("line_thin", "dashed", "bicycle"),
            LaneChangeType::Both,
            "the vehicle rules are tried first"
        );
        assert_eq!(
            change_type("line_thin", "dashed", "train"),
            LaneChangeType::None
        );
    }

    #[test]
    fn sign_codes_carry_their_limit() {
        assert_eq!(traffic_sign_speed_kmh("de274-30"), Some(30.0));
        assert_eq!(traffic_sign_speed_kmh("de274-120"), Some(120.0));
        assert_eq!(traffic_sign_speed_kmh("de274"), Some(30.0));
        assert_eq!(traffic_sign_speed_kmh("de310"), Some(50.0));
        assert_eq!(traffic_sign_speed_kmh("nonsense"), None);
    }
}
