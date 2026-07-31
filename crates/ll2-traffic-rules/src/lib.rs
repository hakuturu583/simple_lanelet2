//! Traffic rules: who may use a lanelet, in which direction, how fast, and where
//! a lane change is allowed.
//!
//! The rule tables here were **measured against `lanelet2==1.2.3`**, not copied from
//! the C++ in the cloned repository — those two are not the same version, and they
//! disagree. The shipped wheel treats `bus_lane` exactly like `road` for speed
//! limits, where the newer source has no such entry. Since compatibility is with the
//! wheel, the wheel is the authority.
//!
//! Upstream: `lanelet2_traffic_rules/src/{Generic,German}TrafficRules.cpp`

pub mod german;

use ll2_core::attribute::{Attribute, AttributeMap, Velocity};
use ll2_core::compat;
use ll2_core::geometry::lanelet as llgeom;
use ll2_core::lanelet::Lanelet;
use ll2_core::linestring::LineString;

/// The registered locations. Only Germany exists upstream.
pub mod locations {
    pub const GERMANY: &str = "de";
}

/// The participant strings, which are hierarchical and colon-separated.
///
/// The hierarchy is load-bearing: a rule written for `vehicle` applies to
/// `vehicle:car` by prefix match, so the order of the checks matters.
pub mod participants {
    pub const VEHICLE: &str = "vehicle";
    pub const VEHICLE_BUS: &str = "vehicle:bus";
    pub const VEHICLE_CAR: &str = "vehicle:car";
    pub const VEHICLE_CAR_ELECTRIC: &str = "vehicle:car:electric";
    pub const VEHICLE_CAR_COMBUSTION: &str = "vehicle:car:combustion";
    pub const VEHICLE_TRUCK: &str = "vehicle:truck";
    pub const VEHICLE_MOTORCYCLE: &str = "vehicle:motorcycle";
    pub const VEHICLE_TAXI: &str = "vehicle:taxi";
    pub const VEHICLE_EMERGENCY: &str = "vehicle:emergency";
    pub const PEDESTRIAN: &str = "pedestrian";
    pub const BICYCLE: &str = "bicycle";
    pub const TRAIN: &str = "train";

    /// The attribute key that overrides a rule for one participant.
    pub fn tag(participant: &str) -> String {
        format!("participant:{participant}")
    }
}

/// A speed limit and whether it is binding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpeedLimit {
    /// Metres per second.
    pub speed: Velocity,
    pub is_mandatory: bool,
}

impl Default for SpeedLimit {
    /// An unknown limit is zero and mandatory, which `RoutingCostTravelTime` later
    /// reinterprets as *infinite* speed. There is no better sentinel available.
    fn default() -> Self {
        SpeedLimit {
            speed: 0.0,
            is_mandatory: true,
        }
    }
}

impl SpeedLimit {
    pub fn kmh(value: f64, is_mandatory: bool) -> Self {
        SpeedLimit {
            speed: value / 3.6,
            is_mandatory,
        }
    }

    pub fn as_kmh(&self) -> f64 {
        self.speed * 3.6
    }
}

/// Where a lane change is permitted across a boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneChangeType {
    Both,
    ToLeft,
    ToRight,
    None,
}

impl LaneChangeType {
    pub fn allows_left(self) -> bool {
        matches!(self, LaneChangeType::Both | LaneChangeType::ToLeft)
    }

    pub fn allows_right(self) -> bool {
        matches!(self, LaneChangeType::Both | LaneChangeType::ToRight)
    }

    fn mirrored(self) -> Self {
        match self {
            LaneChangeType::ToLeft => LaneChangeType::ToRight,
            LaneChangeType::ToRight => LaneChangeType::ToLeft,
            other => other,
        }
    }
}

fn attribute(map: &AttributeMap, key: &str) -> Option<Attribute> {
    map.get(key).cloned()
}

/// Whether any key in the map starts with `prefix`.
fn has_override(map: &AttributeMap, prefix: &str) -> bool {
    map.keys().any(|key| key.starts_with(prefix))
}

/// Resolves a hierarchical override.
///
/// The scheme is that an attribute key which is a *prefix of* the fully qualified
/// name applies, so `participant:vehicle` covers a `vehicle:car` participant. The
/// first matching key wins; upstream documents that defining overrides at several
/// levels at once is not supported.
fn resolve_override<T>(
    map: &AttributeMap,
    prefix: &str,
    qualified: &str,
    parse: impl Fn(&Attribute) -> Option<T>,
    fallback: T,
) -> T {
    for (key, value) in map {
        if key.len() >= prefix.len() && qualified.starts_with(key.as_str()) {
            if let Some(parsed) = parse(value) {
                return parsed;
            }
        }
    }
    fallback
}

/// The traffic rules for one location and participant.
pub struct TrafficRules {
    location: String,
    participant: String,
    limits: german::CountrySpeedLimits,
}

/// No rules are registered for this combination.
#[derive(Debug)]
pub struct NoSuchRules(pub String);

impl TrafficRules {
    /// Builds the rules for a location and participant.
    ///
    /// A participant under `vehicle` that has no rules of its own falls back to the
    /// plain `vehicle` rules, but keeps its own name — which matters, because the
    /// rules then match attribute overrides written for the specific participant.
    pub fn create(location: &str, participant: &str) -> Result<TrafficRules, NoSuchRules> {
        let registered = [
            participants::VEHICLE,
            participants::PEDESTRIAN,
            participants::BICYCLE,
        ];
        let known = location == locations::GERMANY
            && (registered.contains(&participant)
                || participant.starts_with(participants::VEHICLE));
        if !known {
            return Err(NoSuchRules(format!(
                "No matching traffic rules found for location {location}, participant {participant}"
            )));
        }
        Ok(TrafficRules {
            location: location.to_owned(),
            participant: participant.to_owned(),
            limits: german::speed_limits(),
        })
    }

    pub fn location(&self) -> &str {
        &self.location
    }

    pub fn participant(&self) -> &str {
        &self.participant
    }

    fn is_vehicle(&self) -> bool {
        self.participant.starts_with(participants::VEHICLE)
    }

    /// Whether the participant may use this lanelet, in this direction.
    pub fn can_pass(&self, lanelet: &Lanelet) -> bool {
        if !self.is_driving_direction(lanelet) {
            return false;
        }
        let attributes = lanelet.attributes().read();
        if has_override(&attributes, "participant") {
            return resolve_override(
                &attributes,
                "participant",
                &participants::tag(&self.participant),
                Attribute::as_bool,
                false,
            );
        }
        let subtype = attributes
            .get("subtype")
            .map(|value| value.value().to_owned())
            .unwrap_or_default();
        german::subtype_allows(&subtype, &self.participant)
    }

    /// Whether travelling this way round is allowed at all.
    ///
    /// A lanelet is one-way by default for everything but pedestrians, so the
    /// inverted view of a road is not passable unless it says otherwise.
    fn is_driving_direction(&self, lanelet: &Lanelet) -> bool {
        if !lanelet.is_inverted() {
            return true;
        }
        let attributes = lanelet.attributes().read();
        if let Some(one_way) = attribute(&attributes, "one_way") {
            return !one_way.as_bool().unwrap_or(true);
        }
        if has_override(&attributes, "one_way") {
            let qualified = format!("one_way:{}", self.participant);
            return !resolve_override(&attributes, "one_way", &qualified, Attribute::as_bool, true);
        }
        self.participant == participants::PEDESTRIAN
    }

    pub fn is_one_way(&self, lanelet: &Lanelet) -> bool {
        self.is_driving_direction(lanelet) != self.is_driving_direction(&lanelet.invert())
    }

    /// Whether any attached regulatory element varies with time or conditions.
    pub fn has_dynamic_rules(&self, lanelet: &Lanelet) -> bool {
        lanelet.regulatory_elements().iter().any(|regelem| {
            regelem
                .attributes()
                .read()
                .get("dynamic")
                .and_then(Attribute::as_bool)
                .unwrap_or(false)
        })
    }

    /// The speed limit for this lanelet.
    pub fn speed_limit(&self, lanelet: &Lanelet) -> SpeedLimit {
        let attributes = lanelet.attributes().read();

        if has_override(&attributes, "speed_limit") {
            let limit = resolve_override(
                &attributes,
                "speed_limit:",
                &format!("speed_limit:{}", self.participant),
                Attribute::as_velocity,
                attributes
                    .get("speed_limit")
                    .and_then(Attribute::as_velocity)
                    .unwrap_or(0.0),
            );
            let mandatory = resolve_override(
                &attributes,
                "speed_limit_mandatory",
                &format!("speed_limit_mandatory:{}", self.participant),
                Attribute::as_bool,
                true,
            );
            return SpeedLimit {
                speed: limit,
                is_mandatory: mandatory,
            };
        }

        if self.participant == participants::PEDESTRIAN {
            return self.limits.pedestrian;
        }
        if self.participant == participants::BICYCLE {
            return self.limits.bicycle;
        }
        if !self.is_vehicle() {
            return SpeedLimit::default();
        }
        let location = attributes
            .get("location")
            .map(|value| value.value().to_owned())
            .unwrap_or_else(|| "urban".into());
        let subtype = attributes
            .get("subtype")
            .map(|value| value.value().to_owned())
            .unwrap_or_else(|| "road".into());
        german::vehicle_speed_limit(&self.limits, &location, &subtype).unwrap_or_default()
    }

    /// Where a lane change across this boundary is allowed.
    ///
    /// Attributes on the boundary win over its type. A boundary read backwards has
    /// its left and right swapped, which is applied last.
    pub fn lane_change_type(
        &self,
        boundary: &LineString,
        virtual_is_passable: bool,
    ) -> LaneChangeType {
        let attributes = boundary.attributes().read();
        let truthy = |key: &str| {
            attributes
                .get(key)
                .and_then(Attribute::as_bool)
                .unwrap_or(false)
        };

        let hardcoded = if attributes.contains_key("lane_change") {
            Some(if truthy("lane_change") {
                LaneChangeType::Both
            } else {
                LaneChangeType::None
            })
        } else if attributes.contains_key("lane_change:left") && truthy("lane_change:left") {
            Some(if truthy("lane_change:right") {
                LaneChangeType::Both
            } else {
                LaneChangeType::ToLeft
            })
        } else if attributes.contains_key("lane_change:right") {
            Some(if truthy("lane_change:right") {
                LaneChangeType::ToRight
            } else {
                LaneChangeType::None
            })
        } else {
            None
        };

        let kind = attributes
            .get("type")
            .map(|value| value.value().to_owned())
            .unwrap_or_default();
        let subtype = attributes
            .get("subtype")
            .map(|value| value.value().to_owned())
            .unwrap_or_default();

        let base = match hardcoded {
            Some(kind) => kind,
            // A virtual boundary is freely crossable where the caller says so --
            // and note this path skips the inversion swap below, as upstream does.
            None if virtual_is_passable && kind == "virtual" => return LaneChangeType::Both,
            None => german::change_type(&kind, &subtype, &self.participant),
        };
        if boundary.is_inverted() {
            base.mirrored()
        } else {
            base
        }
    }

    /// Whether a lane change from one lanelet to a neighbour is allowed.
    pub fn can_change_lane(&self, from: &Lanelet, to: &Lanelet) -> bool {
        if !self.can_pass(from) || !self.can_pass(to) {
            return false;
        }
        let is_left = if llgeom::left_of(from, to) {
            true
        } else if llgeom::right_of(from, to) {
            false
        } else {
            return false;
        };
        let boundary = if is_left {
            from.right_bound()
        } else {
            from.left_bound()
        };
        let kind = self.lane_change_type(&boundary, false);
        if is_left {
            kind.allows_right()
        } else {
            kind.allows_left()
        }
    }

    /// Whether a vehicle may drive straight from one lanelet into the next.
    pub fn can_pass_between(&self, from: &Lanelet, to: &Lanelet) -> bool {
        llgeom::follows(from, to) && self.can_pass(from) && self.can_pass(to)
    }

    /// Whether an area may be entered from a lanelet, or vice versa.
    ///
    /// Upstream's guard here reads `!canPass(from) && canPass(to)`, which is almost
    /// certainly a typo for `||` — it lets an impassable area be entered whenever
    /// the destination is impassable too. Repaired by default.
    pub fn can_pass_area(&self, from_passable: bool, to_passable: bool) -> bool {
        if compat::area_to_area_uses_and_guard() {
            from_passable || !to_passable
        } else {
            from_passable && to_passable
        }
    }
}

impl std::fmt::Display for TrafficRules {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "location: {}, participant: {}",
            self.location, self.participant
        )
    }
}

impl std::fmt::Display for SpeedLimit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "speedLimit: {}km/h, mandatory: {}",
            ll2_core::fmt::ostream_double(self.as_kmh()),
            if self.is_mandatory { "yes" } else { "no" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll2_core::point::Point;

    fn line(attributes: &[(&str, &str)]) -> LineString {
        let mut map = AttributeMap::new();
        for (key, value) in attributes {
            map.insert((*key).into(), Attribute::new(*value));
        }
        LineString::new(
            1,
            vec![
                Point::new(0, 0.0, 0.0, 0.0, AttributeMap::new()),
                Point::new(0, 10.0, 0.0, 0.0, AttributeMap::new()),
            ],
            map,
        )
    }

    fn lanelet(attributes: &[(&str, &str)]) -> Lanelet {
        let mut map = AttributeMap::new();
        for (key, value) in attributes {
            map.insert((*key).into(), Attribute::new(*value));
        }
        Lanelet::new(1, line(&[]), line(&[]), map)
    }

    fn vehicle() -> TrafficRules {
        TrafficRules::create(locations::GERMANY, participants::VEHICLE).unwrap()
    }

    #[test]
    fn a_vehicle_may_use_a_road_but_only_forwards() {
        let rules = vehicle();
        let road = lanelet(&[("subtype", "road")]);
        assert!(rules.can_pass(&road));
        assert!(!rules.can_pass(&road.invert()));
        assert!(rules.is_one_way(&road));
    }

    #[test]
    fn a_pedestrian_may_walk_a_walkway_both_ways() {
        let rules = TrafficRules::create(locations::GERMANY, participants::PEDESTRIAN).unwrap();
        let walkway = lanelet(&[("subtype", "walkway")]);
        assert!(rules.can_pass(&walkway));
        assert!(rules.can_pass(&walkway.invert()));
        assert!(!rules.is_one_way(&walkway));
        assert!(!vehicle().can_pass(&walkway));
    }

    #[test]
    fn the_participant_hierarchy_matches_by_prefix() {
        let car = TrafficRules::create(locations::GERMANY, participants::VEHICLE_CAR).unwrap();
        assert_eq!(
            car.participant(),
            "vehicle:car",
            "the requested name is kept"
        );
        assert!(car.can_pass(&lanelet(&[("subtype", "road")])));

        // An override written for `vehicle` applies to `vehicle:car`.
        assert!(!car.can_pass(&lanelet(&[
            ("subtype", "road"),
            ("participant:vehicle", "no")
        ])));
    }

    #[test]
    fn an_emergency_lane_needs_the_specific_participant() {
        let lane = lanelet(&[("subtype", "emergency_lane")]);
        assert!(!vehicle().can_pass(&lane));
        let emergency =
            TrafficRules::create(locations::GERMANY, participants::VEHICLE_EMERGENCY).unwrap();
        assert!(emergency.can_pass(&lane));
    }

    #[test]
    fn speed_limits_follow_location_and_subtype() {
        let rules = vehicle();
        let kmh = |attributes: &[(&str, &str)]| rules.speed_limit(&lanelet(attributes)).as_kmh();
        assert!((kmh(&[("subtype", "road")]) - 50.0).abs() < 1e-9);
        assert!((kmh(&[("subtype", "road"), ("location", "nonurban")]) - 100.0).abs() < 1e-9);
        assert!((kmh(&[("subtype", "highway")]) - 130.0).abs() < 1e-9);
        assert!(
            !rules
                .speed_limit(&lanelet(&[("subtype", "highway")]))
                .is_mandatory
        );
        assert!((kmh(&[("subtype", "play_street")]) - 7.0).abs() < 1e-9);
        // The shipped wheel treats a bus lane like a road, though the newer source
        // in the cloned repository does not.
        assert!((kmh(&[("subtype", "bus_lane")]) - 50.0).abs() < 1e-9);
        // Anything unrecognised is zero, the sentinel for "unknown".
        assert_eq!(kmh(&[("subtype", "parking")]), 0.0);
    }

    #[test]
    fn an_explicit_speed_limit_is_read_as_kilometres_per_hour() {
        let rules = vehicle();
        let kmh = |attributes: &[(&str, &str)]| rules.speed_limit(&lanelet(attributes)).as_kmh();
        assert!((kmh(&[("subtype", "road"), ("speed_limit", "30")]) - 30.0).abs() < 1e-9);
        assert!((kmh(&[("subtype", "road"), ("speed_limit", "30 mph")]) - 48.28032).abs() < 1e-5);
        assert!(
            !rules
                .speed_limit(&lanelet(&[
                    ("subtype", "road"),
                    ("speed_limit_mandatory", "false")
                ]))
                .is_mandatory
        );
    }

    #[test]
    fn lane_changes_follow_the_boundary_markings() {
        let rules = vehicle();
        let kind = |attributes: &[(&str, &str)]| rules.lane_change_type(&line(attributes), false);
        assert_eq!(
            kind(&[("type", "line_thin"), ("subtype", "dashed")]),
            LaneChangeType::Both
        );
        assert_eq!(
            kind(&[("type", "line_thin"), ("subtype", "solid")]),
            LaneChangeType::None
        );
        assert_eq!(
            kind(&[("type", "line_thin"), ("subtype", "dashed_solid")]),
            LaneChangeType::ToRight
        );
        assert_eq!(
            kind(&[("type", "line_thin"), ("subtype", "solid_dashed")]),
            LaneChangeType::ToLeft
        );
        // An inverted boundary swaps left and right.
        assert_eq!(
            rules.lane_change_type(
                &line(&[("type", "line_thin"), ("subtype", "dashed_solid")]).invert(),
                false
            ),
            LaneChangeType::ToLeft
        );
        // An explicit attribute overrides the markings.
        assert_eq!(
            kind(&[
                ("type", "line_thin"),
                ("subtype", "solid"),
                ("lane_change", "yes")
            ]),
            LaneChangeType::Both
        );
    }

    #[test]
    fn only_registered_combinations_exist() {
        assert!(TrafficRules::create("de", "vehicle").is_ok());
        assert!(TrafficRules::create("de", "vehicle:bus").is_ok());
        assert!(TrafficRules::create("de", "train").is_err());
        assert!(TrafficRules::create("us", "vehicle").is_err());
    }

    #[test]
    fn text_forms_match_upstream() {
        assert_eq!(vehicle().to_string(), "location: de, participant: vehicle");
        assert_eq!(
            SpeedLimit::kmh(50.0, true).to_string(),
            "speedLimit: 50km/h, mandatory: yes"
        );
        assert_eq!(
            SpeedLimit::kmh(130.0, false).to_string(),
            "speedLimit: 130km/h, mandatory: no"
        );
    }
}
