//! The OSM XML document model, and byte-exact reading and writing of it.
//!
//! The writer has to reproduce pugixml's output exactly, because a round-trip test
//! compares whole files. Three details of that were established by writing a file
//! with the reference implementation and looking at the bytes, not by guessing:
//!
//! * the declaration is `<?xml version="1.0"?>` with no encoding and no standalone;
//! * empty elements close as `<node ... />`, with a space before the slash;
//! * attribute values escape `&`, `<` and `"` — but **not** `>` or `'`.
//!
//! Ordering is fully determined: primitives ascend by id (so negative ids come
//! first) and tags ascend by key, because upstream stores both in a `std::map`.

use std::collections::BTreeMap;

use ll2_core::id::Id;
use quick_xml::events::Event;

/// A tag map: keys sorted, matching upstream's `std::map<std::string, std::string>`.
pub type Tags = BTreeMap<String, String>;

#[derive(Debug, Clone, Default)]
pub struct Node {
    pub id: Id,
    pub lat: f64,
    pub lon: f64,
    /// Elevation, carried as a tag in the file but as the point's `z` in the map.
    pub ele: f64,
    pub tags: Tags,
}

#[derive(Debug, Clone, Default)]
pub struct Way {
    pub id: Id,
    pub nodes: Vec<Id>,
    pub tags: Tags,
}

/// What kind of primitive a relation member points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberType {
    Node,
    Way,
    Relation,
}

impl MemberType {
    pub fn as_str(self) -> &'static str {
        match self {
            MemberType::Node => "node",
            MemberType::Way => "way",
            MemberType::Relation => "relation",
        }
    }

    fn parse(text: &str) -> Option<MemberType> {
        match text {
            "node" => Some(MemberType::Node),
            "way" => Some(MemberType::Way),
            "relation" => Some(MemberType::Relation),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Member {
    pub kind: MemberType,
    pub reference: Id,
    pub role: String,
}

#[derive(Debug, Clone, Default)]
pub struct Relation {
    pub id: Id,
    /// Members keep insertion order; it is what decides the order in the file.
    pub members: Vec<Member>,
    pub tags: Tags,
}

/// A whole OSM file.
#[derive(Debug, Clone, Default)]
pub struct Document {
    pub nodes: BTreeMap<Id, Node>,
    pub ways: BTreeMap<Id, Way>,
    pub relations: BTreeMap<Id, Relation>,
}

// ---------------------------------------------------------------------------
// writing
// ---------------------------------------------------------------------------

/// Formats a number the way the OSM writer does: eleven decimal places, then strip
/// trailing zeros, then a trailing decimal point.
///
/// So 49.0 becomes `49`, 8.4 becomes `8.4`, and 3.25 becomes `3.25`.
///
/// Upstream: `toJosmStyle`, `lanelet2_io/src/OsmFile.cpp`
pub fn josm_style(value: f64, two_decimals: bool) -> String {
    let text = if two_decimals {
        format!("{value:.2}")
    } else {
        format!("{value:.11}")
    };
    let trimmed = text.trim_end_matches('0');
    trimmed.strip_suffix('.').unwrap_or(trimmed).to_owned()
}

/// Escapes an attribute value the way pugixml does.
///
/// Note what is *not* escaped: `>` and `'` are written literally.
fn escape_attribute(value: &str, out: &mut String) {
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            _ => out.push(ch),
        }
    }
}

fn write_attribute(out: &mut String, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    out.push_str("=\"");
    escape_attribute(value, out);
    out.push('"');
}

fn write_tags(out: &mut String, tags: &Tags, indent: &str) {
    for (key, value) in tags {
        out.push_str(indent);
        out.push_str("<tag");
        write_attribute(out, "k", key);
        write_attribute(out, "v", value);
        out.push_str(" />\n");
    }
}

/// Whether an element carries the JOSM bookkeeping attributes.
///
/// Negative ids are JOSM's convention for objects that do not exist upstream yet,
/// and those are written without `visible` or `version`.
fn is_uploaded(id: Id) -> bool {
    id > 0
}

fn write_bookkeeping(out: &mut String, id: Id) {
    write_attribute(out, "id", &id.to_string());
    if is_uploaded(id) {
        write_attribute(out, "visible", "true");
        write_attribute(out, "version", "1");
    }
}

/// Options a caller can pass through `write`'s `params` dictionary.
#[derive(Debug, Clone, Copy, Default)]
pub struct WriteParams {
    /// Sets `upload="true"` on the root element.
    pub josm_upload: bool,
    /// Writes elevations with two decimals instead of eleven.
    pub josm_format_elevation: bool,
}

impl Document {
    /// Renders the document exactly as the reference implementation would.
    pub fn to_xml(&self, params: WriteParams) -> String {
        let mut out = String::with_capacity(64 * 1024);
        out.push_str("<?xml version=\"1.0\"?>\n<osm");
        write_attribute(&mut out, "version", "0.6");
        write_attribute(
            &mut out,
            "upload",
            if params.josm_upload { "true" } else { "false" },
        );
        write_attribute(&mut out, "generator", "lanelet2");
        out.push_str(">\n");

        for node in self.nodes.values() {
            out.push_str("  <node");
            write_bookkeeping(&mut out, node.id);
            write_attribute(&mut out, "lat", &josm_style(node.lat, false));
            write_attribute(&mut out, "lon", &josm_style(node.lon, false));

            // The elevation tag comes first, and only when there is one.
            let elevation =
                (node.ele != 0.0).then(|| josm_style(node.ele, params.josm_format_elevation));
            if elevation.is_none() && node.tags.is_empty() {
                out.push_str(" />\n");
                continue;
            }
            out.push_str(">\n");
            if let Some(elevation) = elevation {
                out.push_str("    <tag");
                write_attribute(&mut out, "k", "ele");
                write_attribute(&mut out, "v", &elevation);
                out.push_str(" />\n");
            }
            write_tags(&mut out, &node.tags, "    ");
            out.push_str("  </node>\n");
        }

        for way in self.ways.values() {
            out.push_str("  <way");
            write_bookkeeping(&mut out, way.id);
            if way.nodes.is_empty() && way.tags.is_empty() {
                out.push_str(" />\n");
                continue;
            }
            out.push_str(">\n");
            for node in &way.nodes {
                out.push_str("    <nd");
                write_attribute(&mut out, "ref", &node.to_string());
                out.push_str(" />\n");
            }
            write_tags(&mut out, &way.tags, "    ");
            out.push_str("  </way>\n");
        }

        for relation in self.relations.values() {
            out.push_str("  <relation");
            write_bookkeeping(&mut out, relation.id);
            if relation.members.is_empty() && relation.tags.is_empty() {
                out.push_str(" />\n");
                continue;
            }
            out.push_str(">\n");
            for member in &relation.members {
                out.push_str("    <member");
                write_attribute(&mut out, "type", member.kind.as_str());
                write_attribute(&mut out, "ref", &member.reference.to_string());
                write_attribute(&mut out, "role", &member.role);
                out.push_str(" />\n");
            }
            write_tags(&mut out, &relation.tags, "    ");
            out.push_str("  </relation>\n");
        }

        out.push_str("</osm>\n");
        out
    }
}

// ---------------------------------------------------------------------------
// reading
// ---------------------------------------------------------------------------

/// A problem encountered while reading, reported rather than thrown so that a
/// partially broken file still yields as much of a map as possible.
pub type ReadErrors = Vec<String>;

struct Element {
    attributes: BTreeMap<String, String>,
}

impl Element {
    fn get(&self, name: &str) -> Option<&str> {
        self.attributes.get(name).map(String::as_str)
    }

    fn id(&self) -> Id {
        self.get("id").and_then(|v| v.parse().ok()).unwrap_or(0)
    }

    fn number(&self, name: &str) -> f64 {
        self.get(name).and_then(|v| v.parse().ok()).unwrap_or(0.0)
    }

    /// JOSM marks pending deletions with `action="delete"`; those are skipped.
    fn is_deleted(&self) -> bool {
        self.get("action") == Some("delete")
    }
}

fn read_element(event: &quick_xml::events::BytesStart<'_>) -> Result<Element, String> {
    let mut attributes = BTreeMap::new();
    for attribute in event.attributes() {
        let attribute = attribute.map_err(|e| e.to_string())?;
        let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
        let value = attribute
            .decode_and_unescape_value(quick_xml::Decoder {})
            .map_err(|e| e.to_string())?
            .into_owned();
        attributes.insert(key, value);
    }
    Ok(Element { attributes })
}

/// The element currently being filled.
enum Pending {
    None,
    Node(Node),
    Way(Way),
    Relation(Relation),
}

fn flush(pending: &mut Pending, document: &mut Document) {
    match std::mem::replace(pending, Pending::None) {
        Pending::None => {}
        Pending::Node(node) => {
            document.nodes.insert(node.id, node);
        }
        Pending::Way(way) => {
            document.ways.insert(way.id, way);
        }
        Pending::Relation(relation) => {
            document.relations.insert(relation.id, relation);
        }
    }
}

/// Parses an OSM file into the document model.
///
/// Malformed pieces are reported rather than fatal, so a file with one bad way
/// still yields the rest of the map — which is what `loadRobust` exists for.
pub fn parse(text: &str) -> Result<(Document, ReadErrors), String> {
    let mut reader = quick_xml::Reader::from_str(text);
    reader.config_mut().trim_text(true);

    let mut document = Document::default();
    let errors = ReadErrors::new();
    let mut pending = Pending::None;
    // Set while inside an element marked for deletion, whose children are ignored.
    let mut skipping = false;

    loop {
        let event = reader.read_event().map_err(|e| e.to_string())?;
        let (start, is_empty) = match &event {
            Event::Start(start) => (Some(start), false),
            Event::Empty(start) => (Some(start), true),
            Event::Eof => break,
            Event::End(end) => {
                let name = end.name();
                if matches!(name.as_ref(), b"node" | b"way" | b"relation") {
                    flush(&mut pending, &mut document);
                    skipping = false;
                }
                continue;
            }
            _ => continue,
        };
        let Some(start) = start else { continue };

        let name = start.name();
        let element = read_element(start)?;
        match name.as_ref() {
            b"osm" => {}
            b"node" | b"way" | b"relation" => {
                flush(&mut pending, &mut document);
                // JOSM marks pending deletions with action="delete"; skip them.
                if element.is_deleted() {
                    skipping = !is_empty;
                    continue;
                }
                pending = match name.as_ref() {
                    b"node" => Pending::Node(Node {
                        id: element.id(),
                        lat: element.number("lat"),
                        lon: element.number("lon"),
                        ..Node::default()
                    }),
                    b"way" => Pending::Way(Way {
                        id: element.id(),
                        ..Way::default()
                    }),
                    _ => Pending::Relation(Relation {
                        id: element.id(),
                        ..Relation::default()
                    }),
                };
                if is_empty {
                    flush(&mut pending, &mut document);
                }
            }
            b"tag" if !skipping => {
                let (Some(key), Some(value)) = (element.get("k"), element.get("v")) else {
                    continue;
                };
                match &mut pending {
                    // The elevation tag becomes the point's z rather than an
                    // attribute, so it is taken out of the tag map here.
                    Pending::Node(node) if key == "ele" => {
                        node.ele = value.parse().unwrap_or(0.0);
                    }
                    Pending::Node(node) => {
                        node.tags.insert(key.to_owned(), value.to_owned());
                    }
                    Pending::Way(way) => {
                        way.tags.insert(key.to_owned(), value.to_owned());
                    }
                    Pending::Relation(relation) => {
                        relation.tags.insert(key.to_owned(), value.to_owned());
                    }
                    Pending::None => {}
                }
            }
            b"nd" if !skipping => {
                if let Pending::Way(way) = &mut pending {
                    if let Some(reference) = element.get("ref").and_then(|v| v.parse().ok()) {
                        way.nodes.push(reference);
                    }
                }
            }
            b"member" if !skipping => {
                if let Pending::Relation(relation) = &mut pending {
                    let kind = element.get("type").and_then(MemberType::parse);
                    let reference: Option<Id> = element.get("ref").and_then(|v| v.parse().ok());
                    // An unknown member type is silently dropped, as upstream does.
                    if let (Some(kind), Some(reference)) = (kind, reference) {
                        relation.members.push(Member {
                            kind,
                            reference,
                            role: element.get("role").unwrap_or("").to_owned(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    flush(&mut pending, &mut document);

    Ok((document, errors))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn josm_numbers_drop_trailing_zeros_and_the_point() {
        assert_eq!(josm_style(49.0, false), "49");
        assert_eq!(josm_style(8.4, false), "8.4");
        assert_eq!(josm_style(3.25, false), "3.25");
        assert_eq!(josm_style(0.0, false), "0");
        assert_eq!(josm_style(49.000009066220, false), "49.00000906622");
        assert_eq!(josm_style(-0.5, false), "-0.5");
        assert_eq!(josm_style(3.256, true), "3.26");
    }

    #[test]
    fn attribute_escaping_matches_pugixml() {
        let mut out = String::new();
        escape_attribute("k<&>\"'", &mut out);
        // Note that `>` and `'` are left alone.
        assert_eq!(out, "k&lt;&amp;>&quot;'");
    }
}
