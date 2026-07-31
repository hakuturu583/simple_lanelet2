//! Attributes and attribute maps.
//!
//! Python only ever sees attribute *values* as `str`; the `Attribute` type itself is
//! not exposed, it is converted to and from `str` at the binding boundary. The
//! parsing helpers here exist because the traffic-rules module interprets attribute
//! values as booleans, numbers and velocities, and it has to interpret them the same
//! way upstream does.
//!
//! Upstream: `lanelet2_core/src/Attribute.cpp`,
//! `lanelet2_core/include/lanelet2_core/Attribute.h`

use std::collections::BTreeMap;

/// A speed in metres per second.
///
/// Upstream carries a `boost::units` quantity; the unit only matters at the
/// boundaries, so a plain scalar in SI units is enough.
pub type Velocity = f64;

const KMH_TO_MPS: f64 = 1.0 / 3.6;
const MPH_TO_MPS: f64 = 0.44704;

/// An attribute value.
///
/// Always a string underneath — the typed accessors parse on demand and return
/// `None` when the value does not parse, exactly like upstream's `Optional<T>`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Attribute(String);

impl Attribute {
    pub fn new(value: impl Into<String>) -> Self {
        Attribute(value.into())
    }

    pub fn value(&self) -> &str {
        &self.0
    }

    pub fn set_value(&mut self, value: impl Into<String>) {
        self.0 = value.into();
    }

    /// Upstream tries `boost::lexical_cast<bool>` first, which accepts only `"0"`
    /// and `"1"`, then falls back to the literals. Anything else is not a bool —
    /// note that this includes `"True"`, which is capitalised differently.
    ///
    /// Upstream: `lanelet2_core/src/Attribute.cpp:61-79`
    pub fn as_bool(&self) -> Option<bool> {
        match self.0.as_str() {
            "0" => Some(false),
            "1" => Some(true),
            "true" | "yes" => Some(true),
            "false" | "no" => Some(false),
            _ => None,
        }
    }

    pub fn as_double(&self) -> Option<f64> {
        lexical_cast_f64(&self.0)
    }

    pub fn as_int(&self) -> Option<i32> {
        self.0.parse().ok()
    }

    pub fn as_id(&self) -> Option<i64> {
        self.0.parse().ok()
    }

    /// Parses a speed, returning metres per second.
    ///
    /// The important quirk: **a bare number is interpreted as km/h**, not m/s. Only
    /// when the string does not parse as a number in full does upstream take the
    /// numeric prefix and look for a unit suffix.
    ///
    /// Upstream: `lanelet2_core/src/Attribute.cpp:123`
    pub fn as_velocity(&self) -> Option<Velocity> {
        if let Some(number) = lexical_cast_f64(&self.0) {
            return Some(number * KMH_TO_MPS);
        }

        let text = self.0.trim_start();
        let split = text
            .char_indices()
            .take_while(|&(index, ch)| {
                ch.is_ascii_digit()
                    || ch == '.'
                    || ((ch == '+' || ch == '-') && index == 0)
                    || ch == 'e'
                    || ch == 'E'
            })
            .count();
        let (number, unit) = text.split_at(split);
        let number: f64 = number.parse().ok()?;

        let unit = unit.trim_start();
        if unit.starts_with("m/s") || unit.starts_with("mps") {
            Some(number)
        } else if unit.starts_with("km/h") || unit.starts_with("kmh") {
            Some(number * KMH_TO_MPS)
        } else if unit.starts_with("m/h") || unit.starts_with("mph") {
            Some(number * MPH_TO_MPS)
        } else if unit.is_empty() {
            Some(number * KMH_TO_MPS)
        } else {
            None
        }
    }
}

/// `boost::lexical_cast<double>` semantics: the *entire* string must be a number,
/// with no surrounding whitespace and no trailing text.
fn lexical_cast_f64(text: &str) -> Option<f64> {
    if text.is_empty() || text.trim() != text {
        return None;
    }
    text.parse().ok()
}

impl From<&str> for Attribute {
    fn from(value: &str) -> Self {
        Attribute::new(value)
    }
}

impl From<String> for Attribute {
    fn from(value: String) -> Self {
        Attribute(value)
    }
}

/// A primitive's attributes.
///
/// A `BTreeMap`, not a `HashMap`: upstream's `AttributeMap` is backed by
/// `std::map<std::string, Attribute>`, so iteration order is sorted by key and is
/// part of the observable API — it determines `repr` output and, through it, the
/// order tags appear in a written OSM file.
///
/// Upstream: `lanelet2_core/include/lanelet2_core/utility/HybridMap.h:71`
pub type AttributeMap = BTreeMap<String, Attribute>;

/// Well-known attribute keys.
///
/// Upstream: `lanelet2_core/include/lanelet2_core/Attribute.h:204-235`
pub mod keys {
    pub const TYPE: &str = "type";
    pub const SUBTYPE: &str = "subtype";
    pub const ONE_WAY: &str = "one_way";
    pub const PARTICIPANT: &str = "participant";
    pub const SPEED_LIMIT: &str = "speed_limit";
    pub const SPEED_LIMIT_MANDATORY: &str = "speed_limit_mandatory";
    pub const LOCATION: &str = "location";
    pub const DYNAMIC: &str = "dynamic";
    pub const LANE_CHANGE: &str = "lane_change";
    pub const LANE_CHANGE_LEFT: &str = "lane_change:left";
    pub const LANE_CHANGE_RIGHT: &str = "lane_change:right";
    pub const AREA: &str = "area";
    pub const ELEVATION: &str = "ele";
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(value: &str) -> Attribute {
        Attribute::new(value)
    }

    #[test]
    fn bools_accept_only_upstream_spellings() {
        assert_eq!(attr("1").as_bool(), Some(true));
        assert_eq!(attr("0").as_bool(), Some(false));
        assert_eq!(attr("true").as_bool(), Some(true));
        assert_eq!(attr("yes").as_bool(), Some(true));
        assert_eq!(attr("false").as_bool(), Some(false));
        assert_eq!(attr("no").as_bool(), Some(false));
        // Capitalisation is not accepted upstream, because the literal comparison
        // is exact and lexical_cast<bool> only handles "0"/"1".
        assert_eq!(attr("True").as_bool(), None);
        assert_eq!(attr("").as_bool(), None);
        assert_eq!(attr("2").as_bool(), None);
    }

    #[test]
    fn doubles_reject_trailing_text_and_padding() {
        assert_eq!(attr("1.5").as_double(), Some(1.5));
        assert_eq!(attr("-2e3").as_double(), Some(-2000.0));
        assert_eq!(attr(" 1.5").as_double(), None);
        assert_eq!(attr("1.5 kmh").as_double(), None);
        assert_eq!(attr("").as_double(), None);
    }

    #[test]
    fn a_bare_number_is_kilometres_per_hour() {
        assert_eq!(attr("36").as_velocity(), Some(10.0));
        assert_eq!(attr("36 km/h").as_velocity(), Some(10.0));
        assert_eq!(attr("36kmh").as_velocity(), Some(10.0));
        assert_eq!(attr("10 m/s").as_velocity(), Some(10.0));
        assert_eq!(attr("10mps").as_velocity(), Some(10.0));
        assert_eq!(attr("10 mph").as_velocity(), Some(4.4704));
        assert_eq!(attr("fast").as_velocity(), None);
        assert_eq!(attr("30 furlongs").as_velocity(), None);
    }
}
