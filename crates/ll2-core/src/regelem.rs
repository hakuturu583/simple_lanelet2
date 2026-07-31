//! Regulatory elements.
//!
//! A regulatory element is an id, an attribute map, and a map from *role names* to
//! lists of primitives. The typed subclasses (`TrafficLight`, `RightOfWay`, ...)
//! are conveniences over that same parameter map: `TrafficLight.trafficLights` is
//! the `refers` role, its `stopLine` is the single entry under `ref_line`.
//!
//! Lanelets and areas are referenced **weakly**. A lanelet owns the regulatory
//! elements attached to it, so an owning reference back would leak the entire map.
//!
//! Upstream: `lanelet2_core/include/lanelet2_core/primitives/RegulatoryElement.h`,
//! `lanelet2_core/src/BasicRegulatoryElements.cpp`

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use parking_lot::RwLock;

use crate::area::WeakArea;
use crate::attribute::AttributeMap;
use crate::id::Id;
use crate::lanelet::WeakLanelet;
use crate::linestring::LineString;
use crate::point::Point;
use crate::refs::{Attrs, attrs};

/// The role names upstream gives a fast path. Any string may be used as a role.
pub mod roles {
    pub const REFERS: &str = "refers";
    pub const REF_LINE: &str = "ref_line";
    pub const YIELD: &str = "yield";
    pub const RIGHT_OF_WAY: &str = "right_of_way";
    pub const CANCELS: &str = "cancels";
    pub const CANCEL_LINE: &str = "cancel_line";
}

/// Which typed regulatory element this is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegElemKind {
    TrafficLight,
    RightOfWay,
    TrafficSign,
    SpeedLimit,
    AllWayStop,
    Generic,
}

impl RegElemKind {
    /// The value of the `subtype` tag that selects this kind when loading a map.
    pub fn rule_name(self) -> &'static str {
        match self {
            RegElemKind::TrafficLight => "traffic_light",
            RegElemKind::RightOfWay => "right_of_way",
            RegElemKind::TrafficSign => "traffic_sign",
            RegElemKind::SpeedLimit => "speed_limit",
            RegElemKind::AllWayStop => "all_way_stop",
            RegElemKind::Generic => "",
        }
    }

    pub fn from_rule_name(name: &str) -> RegElemKind {
        match name {
            "traffic_light" => RegElemKind::TrafficLight,
            "right_of_way" => RegElemKind::RightOfWay,
            "traffic_sign" => RegElemKind::TrafficSign,
            "speed_limit" => RegElemKind::SpeedLimit,
            "all_way_stop" => RegElemKind::AllWayStop,
            _ => RegElemKind::Generic,
        }
    }

    /// The Python class name, which is also what `repr` reports.
    pub fn class_name(self) -> &'static str {
        match self {
            RegElemKind::TrafficLight => "TrafficLight",
            RegElemKind::RightOfWay => "RightOfWay",
            RegElemKind::TrafficSign => "TrafficSign",
            RegElemKind::SpeedLimit => "SpeedLimit",
            RegElemKind::AllWayStop => "AllWayStop",
            RegElemKind::Generic => "GenericRegulatoryElement",
        }
    }
}

/// One entry in a regulatory element's parameter map.
///
/// Polygons share `LineString` storage with linestrings, so the variant is what
/// distinguishes them — it decides whether Python sees a `LineString3d` or a
/// `Polygon3d`.
#[derive(Clone)]
pub enum RuleParameter {
    Point(Point),
    LineString(LineString),
    Polygon(LineString),
    Lanelet(WeakLanelet),
    Area(WeakArea),
}

impl RuleParameter {
    /// Whether two parameters reference the very same primitive.
    pub fn is_same_data(&self, other: &RuleParameter) -> bool {
        match (self, other) {
            (RuleParameter::Point(a), RuleParameter::Point(b)) => a.is_same_data(b),
            (
                RuleParameter::LineString(a) | RuleParameter::Polygon(a),
                RuleParameter::LineString(b) | RuleParameter::Polygon(b),
            ) => a.is_same_data(b),
            (RuleParameter::Lanelet(a), RuleParameter::Lanelet(b)) => {
                match (a.upgrade(), b.upgrade()) {
                    (Some(a), Some(b)) => a.is_same_data(&b),
                    _ => false,
                }
            }
            (RuleParameter::Area(a), RuleParameter::Area(b)) => match (a.upgrade(), b.upgrade()) {
                (Some(a), Some(b)) => a.is_same_data(&b),
                _ => false,
            },
            _ => false,
        }
    }

    /// The id of the referenced primitive, or `None` if a weak reference expired.
    pub fn id(&self) -> Option<Id> {
        match self {
            RuleParameter::Point(point) => Some(point.id()),
            RuleParameter::LineString(line) | RuleParameter::Polygon(line) => Some(line.id()),
            RuleParameter::Lanelet(weak) => weak.upgrade().map(|l| l.id()),
            RuleParameter::Area(weak) => weak.upgrade().map(|a| a.id()),
        }
    }
}

/// A role name to primitives mapping.
///
/// A `BTreeMap`, matching upstream's `std::map`-backed storage: roles iterate in
/// sorted order, and that order shows up in `repr` and in written OSM files.
pub type RuleParameterMap = BTreeMap<String, Vec<RuleParameter>>;

pub struct RegElemData {
    id: AtomicI64,
    attributes: Attrs,
    parameters: RwLock<RuleParameterMap>,
}

/// A handle to a regulatory element.
#[derive(Clone)]
pub struct RegulatoryElement {
    data: Arc<RegElemData>,
    kind: RegElemKind,
}

impl RegulatoryElement {
    pub fn new(
        kind: RegElemKind,
        id: Id,
        attributes: AttributeMap,
        parameters: RuleParameterMap,
    ) -> Self {
        RegulatoryElement {
            data: Arc::new(RegElemData {
                id: AtomicI64::new(id),
                attributes: attrs(attributes),
                parameters: RwLock::new(parameters),
            }),
            kind,
        }
    }

    pub fn kind(&self) -> RegElemKind {
        self.kind
    }

    pub fn id(&self) -> Id {
        self.data.id.load(Ordering::Relaxed)
    }

    pub fn set_id(&self, id: Id) {
        self.data.id.store(id, Ordering::Relaxed);
    }

    pub fn attributes(&self) -> &Attrs {
        &self.data.attributes
    }

    pub fn parameters(&self) -> RuleParameterMap {
        self.data.parameters.read().clone()
    }

    /// The role names present, in sorted order.
    pub fn roles(&self) -> Vec<String> {
        self.data.parameters.read().keys().cloned().collect()
    }

    /// `len(regelem)` is the number of *roles*, not of referenced primitives.
    pub fn len(&self) -> usize {
        self.data.parameters.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The first parameter with the given id, across all roles.
    pub fn find(&self, id: Id) -> Option<RuleParameter> {
        self.data
            .parameters
            .read()
            .values()
            .flatten()
            .find(|parameter| parameter.id() == Some(id))
            .cloned()
    }

    pub fn parameters_for(&self, role: &str) -> Vec<RuleParameter> {
        self.data
            .parameters
            .read()
            .get(role)
            .cloned()
            .unwrap_or_default()
    }

    /// Assigns a role's contents.
    ///
    /// An emptied role is *kept*, not removed: upstream's roles are created by the
    /// constructor and never disappear afterwards, so `removeStopLine` leaves
    /// `ref_line` present but empty.
    pub fn set_parameters_for(&self, role: &str, values: Vec<RuleParameter>) {
        self.data.parameters.write().insert(role.to_owned(), values);
    }

    /// Appends to a role, creating it if absent.
    pub fn push_parameter(&self, role: &str, value: RuleParameter) {
        self.data
            .parameters
            .write()
            .entry(role.to_owned())
            .or_default()
            .push(value);
    }

    /// Removes the first entry under `role` that is *the same object* as `target`,
    /// reporting whether anything was removed.
    ///
    /// Identity, not id: upstream compares primitives with `operator==`, so passing
    /// a freshly built copy with a matching id removes nothing.
    pub fn remove_parameter(&self, role: &str, target: &RuleParameter) -> bool {
        let mut parameters = self.data.parameters.write();
        let Some(values) = parameters.get_mut(role) else {
            return false;
        };
        let Some(index) = values.iter().position(|value| value.is_same_data(target)) else {
            return false;
        };
        values.remove(index);
        true
    }

    pub fn is_same_data(&self, other: &RegulatoryElement) -> bool {
        Arc::ptr_eq(&self.data, &other.data)
    }

    pub fn identity(&self) -> usize {
        Arc::as_ptr(&self.data) as usize
    }

    /// `str(regelem)`: `[id: 5, parameters: {refers: 10 11 }{ref_line: 12 }]`.
    ///
    /// Upstream: `lanelet2_core/src/RegulatoryElement.cpp:175`
    pub fn to_display_string(&self) -> String {
        let parameters = self.data.parameters.read();
        if parameters.is_empty() {
            return format!("[id: {}]", self.id());
        }
        let mut out = format!("[id: {}, parameters: ", self.id());
        for (role, values) in parameters.iter() {
            out.push('{');
            out.push_str(role);
            out.push_str(": ");
            for value in values {
                if let Some(id) = value.id() {
                    out.push_str(&id.to_string());
                    out.push(' ');
                }
            }
            out.push('}');
        }
        out.push(']');
        out
    }
}

impl PartialEq for RegulatoryElement {
    fn eq(&self, other: &Self) -> bool {
        self.is_same_data(other)
    }
}

impl Eq for RegulatoryElement {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::linestring::LineString;

    fn line(id: Id) -> LineString {
        LineString::new(id, Vec::new(), AttributeMap::new())
    }

    fn traffic_light() -> RegulatoryElement {
        let mut parameters = RuleParameterMap::new();
        parameters.insert(
            roles::REFERS.into(),
            vec![RuleParameter::LineString(line(10))],
        );
        parameters.insert(
            roles::REF_LINE.into(),
            vec![RuleParameter::LineString(line(11))],
        );
        RegulatoryElement::new(
            RegElemKind::TrafficLight,
            5,
            AttributeMap::new(),
            parameters,
        )
    }

    #[test]
    fn length_counts_roles_not_primitives() {
        let regelem = traffic_light();
        regelem.push_parameter(roles::REFERS, RuleParameter::LineString(line(12)));
        assert_eq!(regelem.len(), 2, "two roles, three primitives");
        assert_eq!(regelem.roles(), ["ref_line", "refers"], "roles are sorted");
    }

    #[test]
    fn find_searches_across_every_role() {
        let regelem = traffic_light();
        assert!(regelem.find(10).is_some());
        assert!(regelem.find(11).is_some());
        assert!(regelem.find(99).is_none());
    }

    #[test]
    fn removing_the_last_entry_leaves_the_role_in_place() {
        let regelem = traffic_light();
        let target = RuleParameter::LineString(match regelem.parameters_for(roles::REF_LINE)
            .into_iter()
            .next()
            .unwrap()
        {
            RuleParameter::LineString(line) => line,
            _ => unreachable!(),
        });
        assert!(regelem.remove_parameter(roles::REF_LINE, &target));
        assert_eq!(regelem.roles(), ["ref_line", "refers"], "an emptied role is kept");
        assert!(regelem.parameters_for(roles::REF_LINE).is_empty());
        assert!(!regelem.remove_parameter(roles::REF_LINE, &target));
    }

    #[test]
    fn lanelets_are_held_weakly_so_a_cycle_cannot_leak() {
        use crate::lanelet::Lanelet;

        let regelem = RegulatoryElement::new(
            RegElemKind::RightOfWay,
            6,
            AttributeMap::new(),
            RuleParameterMap::new(),
        );
        {
            let lanelet = Lanelet::new(7, line(1), line(2), AttributeMap::new());
            lanelet.add_regulatory_element(regelem.clone());
            regelem.push_parameter(
                roles::RIGHT_OF_WAY,
                RuleParameter::Lanelet(lanelet.downgrade()),
            );
            assert_eq!(regelem.find(7).and_then(|p| p.id()), Some(7));
        }
        // The lanelet owned the regulatory element; the way back was weak, so the
        // lanelet is gone and the parameter no longer resolves.
        assert_eq!(regelem.parameters_for(roles::RIGHT_OF_WAY).len(), 1);
        assert_eq!(regelem.parameters_for(roles::RIGHT_OF_WAY)[0].id(), None);
    }

    #[test]
    fn text_format_lists_roles_with_their_ids() {
        assert_eq!(
            traffic_light().to_display_string(),
            "[id: 5, parameters: {ref_line: 11 }{refers: 10 }]"
        );
    }
}
