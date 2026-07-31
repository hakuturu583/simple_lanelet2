//! Turning a `LaneletMap` back into an OSM document.
//!
//! Everything about the output order is decided by the document model — primitives
//! ascend by id, tags by key — so the only thing to get right here is *what* goes
//! in: which tags are synthesised, and in which order a relation's members appear.
//!
//! Upstream: `lanelet2_io/src/OsmHandlerWrite.cpp`

use ll2_core::attribute::AttributeMap;
use ll2_core::id::Id;
use ll2_core::map::{LaneletMap, Primitive};
use ll2_core::regelem::RuleParameter;
use ll2_projection::Projector;

use crate::osm::{Document, Member, MemberType, Node, Relation, Tags, Way};

fn tags_of(attributes: &AttributeMap) -> Tags {
    attributes
        .iter()
        .map(|(key, value)| (key.clone(), value.value().to_owned()))
        .collect()
}

/// Builds a document from a map, collecting problems as it goes.
pub fn from_map(map: &LaneletMap, projector: &dyn Projector) -> (Document, Vec<String>) {
    let mut document = Document::default();
    let mut errors = Vec::new();

    for primitive in map.points.all() {
        let Primitive::Point(point) = primitive else {
            continue;
        };
        let [x, y, z] = point.xyz();
        match projector.reverse([x, y, z]) {
            Err(error) => errors.push(format!(
                "Error projecting point with id {}: {}",
                point.id(),
                error.message()
            )),
            Ok(gps) => {
                document.nodes.insert(
                    point.id(),
                    Node {
                        id: point.id(),
                        lat: gps.lat,
                        lon: gps.lon,
                        ele: gps.ele,
                        tags: tags_of(&point.attributes().read()),
                    },
                );
            }
        }
    }

    let mut add_way = |id: Id, line: &ll2_core::linestring::LineString, is_polygon: bool| {
        // An inverted handle is written in its canonical order; the file has no
        // notion of a reversed way.
        let points = if line.is_inverted() {
            line.invert().points()
        } else {
            line.points()
        };
        let mut tags = tags_of(&line.attributes().read());
        if is_polygon {
            // An existing `area` tag wins, matching upstream's `emplace`.
            tags.entry("area".into()).or_insert_with(|| "true".into());
        }
        document.ways.insert(
            id,
            Way {
                id,
                nodes: points.iter().map(ll2_core::point::Point::id).collect(),
                tags,
            },
        );
    };

    for primitive in map.line_strings.all() {
        if let Primitive::LineString(line) = primitive {
            add_way(line.id(), &line, false);
        }
    }
    for primitive in map.polygons.all() {
        if let Primitive::Polygon(polygon) = primitive {
            add_way(polygon.id(), &polygon, true);
        }
    }

    for primitive in map.lanelets.all() {
        let Primitive::Lanelet(lanelet) = primitive else {
            continue;
        };
        let mut members = vec![
            Member {
                kind: MemberType::Way,
                reference: lanelet.left_bound().id(),
                role: "left".into(),
            },
            Member {
                kind: MemberType::Way,
                reference: lanelet.right_bound().id(),
                role: "right".into(),
            },
        ];
        // Only a centerline the user assigned is written; a computed one would be
        // regenerated on load anyway, and has no id to reference.
        if lanelet.has_custom_centerline() {
            members.push(Member {
                kind: MemberType::Way,
                reference: lanelet.centerline().id(),
                role: "centerline".into(),
            });
        }
        for regelem in lanelet.regulatory_elements() {
            members.push(Member {
                kind: MemberType::Relation,
                reference: regelem.id(),
                role: "regulatory_element".into(),
            });
        }
        let mut tags = tags_of(&lanelet.attributes().read());
        tags.insert("type".into(), "lanelet".into());
        document.relations.insert(
            lanelet.id(),
            Relation {
                id: lanelet.id(),
                members,
                tags,
            },
        );
    }

    for primitive in map.areas.all() {
        let Primitive::Area(area) = primitive else {
            continue;
        };
        let mut members: Vec<Member> = area
            .outer_bound_polygon()
            .ids()
            .into_iter()
            .map(|reference| Member {
                kind: MemberType::Way,
                reference,
                role: "outer".into(),
            })
            .collect();
        for hole in area.inner_bound_polygons() {
            members.extend(hole.ids().into_iter().map(|reference| Member {
                kind: MemberType::Way,
                reference,
                role: "inner".into(),
            }));
        }
        for regelem in area.regulatory_elements() {
            members.push(Member {
                kind: MemberType::Relation,
                reference: regelem.id(),
                role: "regulatory_element".into(),
            });
        }
        let mut tags = tags_of(&area.attributes().read());
        tags.insert("type".into(), "multipolygon".into());
        document.relations.insert(
            area.id(),
            Relation {
                id: area.id(),
                members,
                tags,
            },
        );
    }

    for primitive in map.regulatory_elements.all() {
        let Primitive::RegulatoryElement(regelem) = primitive else {
            continue;
        };
        let mut members = Vec::new();
        for (role, values) in regelem.parameters() {
            for value in values {
                let member = match &value {
                    RuleParameter::Point(point) => Some((MemberType::Node, point.id())),
                    RuleParameter::LineString(line) | RuleParameter::Polygon(line) => {
                        Some((MemberType::Way, line.id()))
                    }
                    RuleParameter::Lanelet(weak) => weak
                        .upgrade()
                        .map(|lanelet| (MemberType::Relation, lanelet.id())),
                    RuleParameter::Area(weak) => {
                        weak.upgrade().map(|area| (MemberType::Relation, area.id()))
                    }
                };
                match member {
                    None => errors.push(format!(
                        "Regulatory element {} has a lanelet/area that is not in the map!",
                        regelem.id()
                    )),
                    Some((kind, reference)) => members.push(Member {
                        kind,
                        reference,
                        role: role.clone(),
                    }),
                }
            }
        }
        let mut tags = tags_of(&regelem.attributes().read());
        tags.insert("type".into(), "regulatory_element".into());
        document.relations.insert(
            regelem.id(),
            Relation {
                id: regelem.id(),
                members,
                tags,
            },
        );
    }

    // Every referenced primitive must be present, or the file will not reload.
    for way in document.ways.values() {
        for reference in &way.nodes {
            if !document.nodes.contains_key(reference) {
                errors.push("Way has a point that is not in the map!".into());
            }
        }
    }
    for relation in document.relations.values() {
        for member in &relation.members {
            let present = match member.kind {
                MemberType::Node => document.nodes.contains_key(&member.reference),
                MemberType::Way => document.ways.contains_key(&member.reference),
                MemberType::Relation => document.relations.contains_key(&member.reference),
            };
            if !present {
                errors.push(format!(
                    "Relation has a member with id {} that is not in the map!",
                    member.reference
                ));
            }
        }
    }

    (document, errors)
}
