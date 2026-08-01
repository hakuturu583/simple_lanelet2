//! Turning an OSM document into a `LaneletMap`.
//!
//! The order matters: nodes, then ways, then lanelets, then areas, then regulatory
//! elements, and only then are the elements attached to the lanelets and areas that
//! reference them — because a regulatory element may name a lanelet defined after
//! it in the file.
//!
//! Upstream: `lanelet2_io/src/OsmHandlerLoad.cpp`

use std::collections::HashMap;
use std::sync::Arc;

use ll2_core::area::{Area, InnerBound};
use ll2_core::attribute::{Attribute, AttributeMap};
use ll2_core::geometry::distance::{middle_point, signed_distance_2d};
use ll2_core::id::Id;
use ll2_core::lanelet::Lanelet;
use ll2_core::linestring::LineString;
use ll2_core::map::{LaneletMap, Primitive};
use ll2_core::point::Point;
use ll2_core::regelem::{
    RegElemKind, RegulatoryElement, RuleParameter, RuleParameterMap, registry,
};
use ll2_projection::{GpsPoint, Projector};

use crate::osm::{Document, MemberType, Tags};

fn attributes_of(tags: &Tags) -> AttributeMap {
    tags.iter()
        .map(|(key, value)| (key.clone(), Attribute::new(value.clone())))
        .collect()
}

fn flat(line: &LineString) -> Vec<[f64; 2]> {
    line.points()
        .iter()
        .map(|point| {
            let [x, y, _] = point.xyz();
            [x, y]
        })
        .collect()
}

/// Orients a pair of boundaries so that the first really is on the left.
///
/// A heuristic, and upstream says so: it compares each boundary against the other's
/// middle point and flips whichever disagrees.
///
/// Upstream: `geometry::align`, `impl/LineString.h:809-829`
fn align(left: LineString, right: LineString) -> (LineString, LineString) {
    if (left.len() <= 1 && right.len() <= 1) || left.is_empty() || right.is_empty() {
        return (left, right);
    }
    let mut left = left;
    let mut right = right;

    if let Some(middle) = middle_point(&flat(&right)) {
        let right_of_left = signed_distance_2d(&flat(&left), middle) < 0.0;
        if !right_of_left && left.len() > 1 {
            left = left.invert();
        }
    }
    if let Some(middle) = middle_point(&flat(&left)) {
        let left_of_right = signed_distance_2d(&flat(&right), middle) > 0.0;
        if !left_of_right && right.len() > 1 {
            right = right.invert();
        }
    }
    (left, right)
}

/// Twice the signed area of a ring, by the shoelace formula.
///
/// Negative means clockwise in a standard x-right, y-up frame, which is the winding
/// Lanelet2 requires of a boundary.
fn signed_area_2x(ring: &[LineString]) -> f64 {
    let points: Vec<[f64; 2]> = ring.iter().flat_map(flat).collect();
    if points.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for index in 0..points.len() {
        let a = points[index];
        let b = points[(index + 1) % points.len()];
        sum += a[0] * b[1] - b[0] * a[1];
    }
    sum
}

/// Reverses a ring: every member is inverted and their order is flipped.
fn reverse_ring(ring: &mut [LineString]) {
    for line in ring.iter_mut() {
        *line = line.invert();
    }
    ring.reverse();
}

/// Chains a ring's linestrings end to start, inverting members as needed, and then
/// forces the required winding.
///
/// Members are matched by *point id*, not by object identity, which matters because
/// a file can reference the same coordinates through separately-parsed nodes. The
/// winding check is what makes the assembled ring reproducible: without it, whether
/// a ring comes out clockwise depends on which member happened to be listed first.
///
/// Upstream: `assembleBoundary`, `OsmHandlerLoad.cpp:332-376`
fn assemble_ring(parts: Vec<LineString>, id: Id) -> (Vec<InnerBound>, Vec<String>) {
    let mut remaining = parts;
    let mut rings: Vec<InnerBound> = Vec::new();
    let mut errors = Vec::new();
    let mut current: InnerBound = Vec::new();

    while !remaining.is_empty() {
        if current.is_empty() {
            current.push(remaining.remove(0));
        } else {
            let Some(last_id) = current.last().and_then(LineString::back).map(|p| p.id()) else {
                break;
            };
            let next = remaining.iter().position(|candidate| {
                candidate.back().is_some_and(|p| p.id() == last_id)
                    || candidate.front().is_some_and(|p| p.id() == last_id)
            });
            let Some(index) = next else {
                errors.push(format!(
                    "Error parsing primitive {id}: Could not complete boundary around linestring {}",
                    current.last().map(LineString::id).unwrap_or_default()
                ));
                current.clear();
                continue;
            };
            let piece = remaining.remove(index);
            // Chain end to start: if the piece already ends where we are, it runs
            // the wrong way round.
            let oriented = if piece.back().is_some_and(|p| p.id() == last_id) {
                piece.invert()
            } else {
                piece
            };
            current.push(oriented);
        }

        let closed = match (
            current.last().and_then(LineString::back),
            current.first().and_then(LineString::front),
        ) {
            (Some(tail), Some(head)) => tail.id() == head.id(),
            _ => false,
        };
        if closed {
            if signed_area_2x(&current) > 0.0 {
                reverse_ring(&mut current);
            }
            rings.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        errors.push(format!(
            "Error parsing primitive {id}: Failed to generate boundary (self-intersecting?)"
        ));
    }
    (rings, errors)
}

/// Builds a map from a parsed document, collecting problems as it goes.
pub fn to_map(document: &Document, projector: &dyn Projector) -> (Arc<LaneletMap>, Vec<String>) {
    let map = LaneletMap::new_map();
    let mut errors = Vec::new();

    // --- nodes -> points ---------------------------------------------------
    let mut points: HashMap<Id, Point> = HashMap::new();
    for node in document.nodes.values() {
        match projector.forward(GpsPoint::new(node.lat, node.lon, node.ele)) {
            Err(error) => errors.push(format!(
                "Error projecting point with id {}: {}",
                node.id,
                error.message()
            )),
            Ok([x, y, z]) => {
                let point = Point::new(node.id, x, y, z, attributes_of(&node.tags));
                points.insert(node.id, point.clone());
                map.add(Primitive::Point(point));
            }
        }
    }

    // --- ways -> linestrings and polygons ----------------------------------
    let mut lines: HashMap<Id, LineString> = HashMap::new();
    let mut polygon_ids: Vec<Id> = Vec::new();
    for way in document.ways.values() {
        let mut resolved = Vec::with_capacity(way.nodes.len());
        let mut missing = false;
        for reference in &way.nodes {
            match points.get(reference) {
                Some(point) => resolved.push(point.clone()),
                None => missing = true,
            }
        }
        if missing {
            errors.push(format!(
                "Error reading primitive with id {} from file: Way references nonexisting points",
                way.id
            ));
            resolved.clear();
        }
        if resolved.is_empty() {
            errors.push(format!(
                "Error reading primitive with id {} from file: Ways must have at least one point!",
                way.id
            ));
            continue;
        }

        let attributes = attributes_of(&way.tags);
        let is_area = way
            .tags
            .get("area")
            .map(|value| Attribute::new(value.clone()).as_bool().unwrap_or(false))
            .unwrap_or(false);
        let line = LineString::new(way.id, resolved, attributes);
        lines.insert(way.id, line.clone());
        if is_area {
            polygon_ids.push(way.id);
            map.add(Primitive::Polygon(line));
        } else {
            map.add(Primitive::LineString(line));
        }
    }

    // --- relations ---------------------------------------------------------
    let relation_type = |id: Id| -> Option<&str> {
        document
            .relations
            .get(&id)
            .and_then(|relation| relation.tags.get("type"))
            .map(String::as_str)
    };

    let mut lanelets: HashMap<Id, Lanelet> = HashMap::new();
    let mut areas: HashMap<Id, Area> = HashMap::new();

    for relation in document.relations.values() {
        if relation_type(relation.id) != Some("lanelet") {
            continue;
        }
        let bound = |role: &str| -> Option<LineString> {
            let matching: Vec<&crate::osm::Member> = relation
                .members
                .iter()
                .filter(|member| member.role == role && member.kind == MemberType::Way)
                .collect();
            if matching.len() != 1 {
                return None;
            }
            lines.get(&matching[0].reference).cloned()
        };
        let (Some(left), Some(right)) = (bound("left"), bound("right")) else {
            errors.push(format!(
                "Lanelet {} must have exactly one left and one right boundary",
                relation.id
            ));
            continue;
        };
        let (left, right) = align(left, right);
        let lanelet = Lanelet::new(relation.id, left, right, attributes_of(&relation.tags));
        // A centerline in the file is an explicit one, and keeps its id, which is
        // what later marks it as user-supplied rather than computed.
        if let Some(centerline) = bound("centerline") {
            lanelet.set_centerline(centerline);
        }
        lanelets.insert(relation.id, lanelet.clone());
        map.add(Primitive::Lanelet(lanelet));
    }

    for relation in document.relations.values() {
        if relation_type(relation.id) != Some("multipolygon") {
            continue;
        }
        let collect = |role: &str| -> Vec<LineString> {
            relation
                .members
                .iter()
                .filter(|member| member.role == role && member.kind == MemberType::Way)
                .filter_map(|member| lines.get(&member.reference).cloned())
                .collect()
        };
        let (outer_rings, outer_errors) = assemble_ring(collect("outer"), relation.id);
        errors.extend(outer_errors);
        if outer_rings.len() != 1 {
            errors.push(format!(
                "Error parsing primitive {}: Areas must have exactly one outer ring!",
                relation.id
            ));
            continue;
        }
        let (inner_rings, inner_errors) = assemble_ring(collect("inner"), relation.id);
        errors.extend(inner_errors);
        let area = Area::new(
            relation.id,
            outer_rings.into_iter().next().unwrap_or_default(),
            inner_rings,
            attributes_of(&relation.tags),
        );
        areas.insert(relation.id, area.clone());
        map.add(Primitive::Area(area));
    }

    let mut regelems: HashMap<Id, RegulatoryElement> = HashMap::new();
    for relation in document.relations.values() {
        if relation_type(relation.id) != Some("regulatory_element") {
            continue;
        }
        let Some(subtype) = relation.tags.get("subtype") else {
            errors.push(format!(
                "Error parsing primitive {}: Regulatory element has no 'subtype' tag.",
                relation.id
            ));
            continue;
        };

        let mut parameters = RuleParameterMap::new();
        for member in &relation.members {
            let parameter = match member.kind {
                MemberType::Node => points
                    .get(&member.reference)
                    .cloned()
                    .map(RuleParameter::Point),
                MemberType::Way => lines.get(&member.reference).cloned().map(|line| {
                    if polygon_ids.contains(&member.reference) {
                        RuleParameter::Polygon(line)
                    } else {
                        RuleParameter::LineString(line)
                    }
                }),
                MemberType::Relation => match relation_type(member.reference) {
                    Some("lanelet") => lanelets
                        .get(&member.reference)
                        .map(|lanelet| RuleParameter::Lanelet(lanelet.downgrade())),
                    Some("multipolygon") => areas
                        .get(&member.reference)
                        .map(|area| RuleParameter::Area(area.downgrade())),
                    Some("regulatory_element") => {
                        errors.push(format!(
                            "Regulatory element {} references regulatory element {}, which is not supported",
                            relation.id, member.reference
                        ));
                        None
                    }
                    _ => None,
                },
            };
            if let Some(parameter) = parameter {
                parameters
                    .entry(member.role.clone())
                    .or_default()
                    .push(parameter);
            }
        }

        // An unregistered subtype is fatal for this element, exactly as upstream's
        // factory is: `RegulatoryElementFactory::create` throws, the loader catches
        // it and records the failure, and the element never enters the map.
        // Registering the Autoware extension is what makes these names resolve.
        let kind = match registry::resolve(subtype) {
            Ok(kind) => kind,
            Err(_) => {
                errors.push(format!(
                    "Error parsing primitive {}: Creating a regulatory element of type {subtype} \
                     failed: No regulatory element found that implements rule {subtype}",
                    relation.id
                ));
                continue;
            }
        };

        let mut attributes = attributes_of(&relation.tags);
        if subtype.is_empty() {
            // The factory writes the resolved name back into the attributes, so an
            // empty subtype round-trips out as a real tag rather than an empty one.
            attributes.insert(
                "subtype".to_owned(),
                registry::GENERIC_RULE_NAME.to_owned().into(),
            );
        }

        let regelem = RegulatoryElement::new(kind, relation.id, attributes, parameters);
        regelems.insert(relation.id, regelem.clone());
        map.add(Primitive::RegulatoryElement(regelem));
    }

    // --- attach regulatory elements ---------------------------------------
    for relation in document.relations.values() {
        for member in &relation.members {
            if member.role != "regulatory_element" || member.kind != MemberType::Relation {
                continue;
            }
            let regelem = match regelems.get(&member.reference) {
                Some(regelem) => regelem.clone(),
                None => {
                    // Upstream's `getOrGetDummy` reports the failure against the
                    // *referencing* primitive and substitutes a bare generic element
                    // carrying only the id — no attributes, and never added to the
                    // map's own layer. Skipping instead would leave the lanelet with
                    // fewer elements than the reference has.
                    errors.push(format!(
                        "Error parsing primitive {}: Failed to get id {} from map",
                        relation.id, member.reference
                    ));
                    RegulatoryElement::new(
                        RegElemKind::Generic,
                        member.reference,
                        AttributeMap::new(),
                        RuleParameterMap::new(),
                    )
                }
            };
            if let Some(lanelet) = lanelets.get(&relation.id) {
                lanelet.add_regulatory_element(regelem);
            } else if let Some(area) = areas.get(&relation.id) {
                area.add_regulatory_element(regelem);
            }
        }
    }

    (map, errors)
}
