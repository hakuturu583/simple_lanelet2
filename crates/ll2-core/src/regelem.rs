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
///
/// Upstream this is a C++ class, selected by `RegulatoryElementFactory` from the
/// `subtype` tag. Third parties — `autoware_lanelet2_extension` above all — add
/// their own by registering a factory when their shared library loads, so the set
/// is open. Here a kind is an index into [`registry`], and the built-in six occupy
/// the first six slots as associated constants so that ordinary comparisons read
/// exactly as they did when this was an enum.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegElemKind(u16);

#[allow(non_upper_case_globals)]
impl RegElemKind {
    pub const TrafficLight: RegElemKind = RegElemKind(0);
    pub const RightOfWay: RegElemKind = RegElemKind(1);
    pub const TrafficSign: RegElemKind = RegElemKind(2);
    pub const SpeedLimit: RegElemKind = RegElemKind(3);
    pub const AllWayStop: RegElemKind = RegElemKind(4);
    pub const Generic: RegElemKind = RegElemKind(5);
}

impl RegElemKind {
    /// Its slot in the registry. Stable for the life of the process, which is what
    /// lets the Python layer keep a parallel table of class wrappers.
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// The value of the `subtype` tag that selects this kind when loading a map.
    pub fn rule_name(self) -> &'static str {
        registry::spec(self).rule_name
    }

    /// The Python class name, which is also what `repr` reports.
    pub fn class_name(self) -> &'static str {
        registry::spec(self).class_name
    }

    /// Whether this kind is `other`, or derives from it.
    ///
    /// Upstream's typed accessors (`Lanelet::trafficLights()` and friends) are
    /// `dynamic_pointer_cast`s, so a derived element answers to its base's
    /// accessor: a `SpeedLimit` shows up in `trafficSigns()`, and once the Autoware
    /// extension is loaded an `AutowareTrafficLight` shows up in `trafficLights()`.
    pub fn is_a(self, other: RegElemKind) -> bool {
        let mut current = Some(self);
        while let Some(kind) = current {
            if kind == other {
                return true;
            }
            current = registry::spec(kind).parent;
        }
        false
    }
}

impl std::fmt::Debug for RegElemKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.class_name())
    }
}

/// The open set of regulatory-element kinds.
///
/// Upstream: `lanelet2_core/src/RegulatoryElement.cpp:87-124`
/// (`RegulatoryElementFactory`).
pub mod registry {
    use super::RegElemKind;
    use parking_lot::RwLock;
    use std::collections::HashMap;
    use std::sync::OnceLock;

    /// Rejects a parameter map the typed class would refuse to be built from.
    ///
    /// Upstream's factory does not merely pick a class, it *constructs* one, and those
    /// constructors validate. So a malformed element is rejected at load time with the
    /// constructor's own message, wrapped as
    /// `Creating a regulatory element of type X failed: <message>`.
    pub type Validate = fn(&super::RuleParameterMap) -> Result<(), String>;

    /// What a kind knows about itself.
    #[derive(Clone, Copy)]
    pub struct RegElemSpec {
        /// The `subtype` tag value, and the factory key.
        pub rule_name: &'static str,
        /// The Python class name, which `repr` reports.
        pub class_name: &'static str,
        /// The kind this one derives from, for [`RegElemKind::is_a`].
        pub parent: Option<RegElemKind>,
        /// What the typed constructor insists on, if anything.
        pub validate: Option<Validate>,
    }

    struct Registry {
        /// Append-only, so a kind's index is stable for the life of the process.
        specs: Vec<RegElemSpec>,
        /// Re-assignable, so a later registration takes a rule name over from an
        /// earlier one — upstream's `registry_[name] = factory` is a plain
        /// assignment, and `AutowareTrafficLight` relies on it to claim
        /// `traffic_light` from the stock `TrafficLight`.
        by_rule: HashMap<&'static str, RegElemKind>,
    }

    /// The rule name a regulatory element with no `subtype` of its own gets.
    ///
    /// Upstream rewrites an empty subtype to this *in the attributes*, so it
    /// round-trips back out as a real tag rather than an empty one.
    pub const GENERIC_RULE_NAME: &str = "regulatory_element";

    /// `TrafficLight` refuses an empty `refers`.
    fn validate_traffic_light(parameters: &super::RuleParameterMap) -> Result<(), String> {
        match parameters.get(super::roles::REFERS) {
            Some(values) if !values.is_empty() => Ok(()),
            _ => Err("No traffic light defined!".into()),
        }
    }

    /// `TrafficSign` insists every sign it refers to says which sign it is.
    fn validate_traffic_sign(parameters: &super::RuleParameterMap) -> Result<(), String> {
        let refers = parameters.get(super::roles::REFERS);
        let missing = refers.into_iter().flatten().any(|value| match value {
            super::RuleParameter::LineString(line) | super::RuleParameter::Polygon(line) => {
                !line.attributes().read().contains_key("subtype")
            }
            _ => false,
        });
        if missing {
            return Err("Regulatory element has a traffic sign without subtype attribute!".into());
        }
        Ok(())
    }

    /// `RightOfWay` needs somebody to have the right of way.
    fn validate_right_of_way(parameters: &super::RuleParameterMap) -> Result<(), String> {
        match parameters.get(super::roles::RIGHT_OF_WAY) {
            Some(values) if !values.is_empty() => Ok(()),
            _ => Err("A maneuver must refer to at least one lanelet that has right of way!".into()),
        }
    }

    const BUILTIN: [RegElemSpec; 6] = [
        RegElemSpec {
            rule_name: "traffic_light",
            class_name: "TrafficLight",
            parent: None,
            validate: Some(validate_traffic_light),
        },
        RegElemSpec {
            rule_name: "right_of_way",
            class_name: "RightOfWay",
            parent: None,
            validate: Some(validate_right_of_way),
        },
        RegElemSpec {
            rule_name: "traffic_sign",
            class_name: "TrafficSign",
            parent: None,
            validate: Some(validate_traffic_sign),
        },
        // Measured, not assumed: the reference reports
        // `SpeedLimit.__mro__ == [SpeedLimit, TrafficSign, RegulatoryElement, ...]`
        // and `lanelet.trafficSigns()` returns a `SpeedLimit`. It inherits the sign
        // check with it.
        RegElemSpec {
            rule_name: "speed_limit",
            class_name: "SpeedLimit",
            parent: Some(RegElemKind::TrafficSign),
            validate: Some(validate_traffic_sign),
        },
        // `AllWayStop` accepts an empty parameter map — measured, and not what its
        // constructor's stop-line check would suggest.
        RegElemSpec {
            rule_name: "all_way_stop",
            class_name: "AllWayStop",
            parent: None,
            validate: None,
        },
        // `GenericRegulatoryElement` is the C++ class name, but it is not exposed to
        // Python — the kind is only reachable by loading an element with an empty
        // subtype, and the reference reports it as a plain `RegulatoryElement`.
        RegElemSpec {
            rule_name: GENERIC_RULE_NAME,
            class_name: "RegulatoryElement",
            parent: None,
            validate: None,
        },
    ];

    fn registry() -> &'static RwLock<Registry> {
        static REGISTRY: OnceLock<RwLock<Registry>> = OnceLock::new();
        REGISTRY.get_or_init(|| {
            let specs = BUILTIN.to_vec();
            let by_rule = specs
                .iter()
                .enumerate()
                .map(|(index, spec)| (spec.rule_name, RegElemKind(index as u16)))
                .collect();
            RwLock::new(Registry { specs, by_rule })
        })
    }

    /// Looks up a kind's description.
    pub fn spec(kind: RegElemKind) -> RegElemSpec {
        registry().read().specs[kind.0 as usize]
    }

    /// The rule name a subtype tag maps to is not registered.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct UnknownRule(pub String);

    /// Resolves a `subtype` tag to a kind.
    ///
    /// An unregistered subtype is an **error**, not a fallback to the generic kind:
    /// upstream's factory throws, which is what makes a stock Lanelet2 refuse an
    /// Autoware map until the extension is loaded
    /// (`lanelet2_core/src/RegulatoryElement.cpp:120-124`).
    pub fn resolve(subtype: &str) -> Result<RegElemKind, UnknownRule> {
        let name = if subtype.is_empty() {
            GENERIC_RULE_NAME
        } else {
            subtype
        };
        registry()
            .read()
            .by_rule
            .get(name)
            .copied()
            .ok_or_else(|| UnknownRule(name.to_owned()))
    }

    /// Adds a kind, or re-points an existing rule name at a new one.
    ///
    /// Returns the kind that now owns `rule_name`. Registering the same class twice
    /// is idempotent, so importing an extension module more than once is harmless.
    /// `rule_name` may be `None` for a kind that is never selected by a subtype tag.
    /// The one such case is the object upstream's `SpeedLimit` constructor really
    /// builds: it answers to `SpeedLimit` in Python and to `TrafficSign` in a cast,
    /// and no `subtype` value resolves to it.
    pub fn register(
        rule_name: Option<&str>,
        class_name: &str,
        parent: Option<RegElemKind>,
        validate: Option<Validate>,
    ) -> RegElemKind {
        let mut registry = registry().write();
        if let Some(name) = rule_name
            && let Some(&existing) = registry.by_rule.get(name)
            && registry.specs[existing.0 as usize].class_name == class_name
        {
            return existing;
        }
        // Registration happens once per process, at module-import time, for a
        // handful of classes; leaking the names buys `&'static str` everywhere else.
        let spec = RegElemSpec {
            rule_name: Box::leak(rule_name.unwrap_or("").to_owned().into_boxed_str()),
            class_name: Box::leak(class_name.to_owned().into_boxed_str()),
            parent,
            validate,
        };
        let kind = RegElemKind(registry.specs.len() as u16);
        registry.specs.push(spec);
        if rule_name.is_some() {
            registry.by_rule.insert(spec.rule_name, kind);
        }
        kind
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
        let target = RuleParameter::LineString(
            match regelem
                .parameters_for(roles::REF_LINE)
                .into_iter()
                .next()
                .unwrap()
            {
                RuleParameter::LineString(line) => line,
                _ => unreachable!(),
            },
        );
        assert!(regelem.remove_parameter(roles::REF_LINE, &target));
        assert_eq!(
            regelem.roles(),
            ["ref_line", "refers"],
            "an emptied role is kept"
        );
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
