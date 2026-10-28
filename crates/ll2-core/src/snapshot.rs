//! A self-contained binary snapshot of a map, for `pickle`.
//!
//! Upstream's own binary format is a Boost.Serialization archive whose layout
//! depends on the compiler and Boost version, and upstream's Python objects refuse
//! to pickle at all. This format is ours: it only has to be read back by the same
//! library, so it is versioned and makes no promise beyond that.
//!
//! What it keeps is the object graph, not just the ids. Every point, linestring,
//! lanelet, area and regulatory element is written once, however many primitives
//! share it, and every reference records which view of its target it holds — an
//! inverted bound stays inverted. The six layers are written as they are rather than
//! rebuilt by `add`, so a map whose layers were edited after the fact, or a submap,
//! comes back with exactly the same contents. Computed centerlines are a cache and
//! are not written; custom ones are.
//!
//! A regulatory element's lanelet and area parameters are weak references, and only
//! a layer holds a lanelet or an area strongly. So a parameter is written only when
//! its target is in one of the map's layers: anything else is kept alive, if at all,
//! by something outside the map, which a snapshot cannot carry — restored, it would
//! have no owner and would read back as expired, which is what an expired one
//! already does.

use std::collections::HashMap;
use std::sync::Arc;

use crate::area::{Area, InnerBound};
use crate::attribute::{Attribute, AttributeMap};
use crate::id::{INVAL_ID, Id, register_id};
use crate::lanelet::Lanelet;
use crate::linestring::LineString;
use crate::map::{LaneletMap, LayerKind, Primitive};
use crate::point::Point;
use crate::regelem::{RegElemKind, RegulatoryElement, RuleParameter, RuleParameterMap, registry};

const MAGIC: &[u8; 8] = b"LL2SNAP\0";
const VERSION: u32 = 1;

const LAYERS: [LayerKind; 6] = [
    LayerKind::Point,
    LayerKind::LineString,
    LayerKind::Polygon,
    LayerKind::Lanelet,
    LayerKind::Area,
    LayerKind::RegulatoryElement,
];

const PARAM_POINT: u8 = 0;
const PARAM_LINESTRING: u8 = 1;
const PARAM_POLYGON: u8 = 2;
const PARAM_LANELET: u8 = 3;
const PARAM_AREA: u8 = 4;

/// The storage behind a handle, ignoring which way round the handle views it.
fn data_key(identity: usize) -> usize {
    identity & !1
}

fn base_line(line: &LineString) -> LineString {
    if line.is_inverted() {
        line.invert()
    } else {
        line.clone()
    }
}

fn base_lanelet(lanelet: &Lanelet) -> Lanelet {
    if lanelet.is_inverted() {
        lanelet.invert()
    } else {
        lanelet.clone()
    }
}

/// Every distinct piece of storage reachable from the map, in table order.
#[derive(Default)]
struct Graph {
    points: Vec<Point>,
    point_index: HashMap<usize, u32>,
    lines: Vec<LineString>,
    line_index: HashMap<usize, u32>,
    lanelets: Vec<Lanelet>,
    lanelet_index: HashMap<usize, u32>,
    areas: Vec<Area>,
    area_index: HashMap<usize, u32>,
    regelems: Vec<RegulatoryElement>,
    regelem_index: HashMap<usize, u32>,
}

impl Graph {
    fn point(&mut self, point: &Point) -> u32 {
        let key = point.identity();
        if let Some(&index) = self.point_index.get(&key) {
            return index;
        }
        let index = self.points.len() as u32;
        self.point_index.insert(key, index);
        self.points.push(point.clone());
        index
    }

    fn line(&mut self, line: &LineString) -> u32 {
        let key = data_key(line.identity());
        if let Some(&index) = self.line_index.get(&key) {
            return index;
        }
        let base = base_line(line);
        let index = self.lines.len() as u32;
        self.line_index.insert(key, index);
        self.lines.push(base.clone());
        for point in base.points() {
            self.point(&point);
        }
        index
    }

    fn lanelet(&mut self, lanelet: &Lanelet) -> u32 {
        let key = data_key(lanelet.identity());
        if let Some(&index) = self.lanelet_index.get(&key) {
            return index;
        }
        let base = base_lanelet(lanelet);
        let index = self.lanelets.len() as u32;
        self.lanelet_index.insert(key, index);
        self.lanelets.push(base.clone());
        self.line(&base.left_bound());
        self.line(&base.right_bound());
        if base.has_custom_centerline() {
            self.line(&base.centerline());
        }
        for regelem in base.regulatory_elements() {
            self.regelem(&regelem);
        }
        index
    }

    fn area(&mut self, area: &Area) -> u32 {
        let key = area.identity();
        if let Some(&index) = self.area_index.get(&key) {
            return index;
        }
        let index = self.areas.len() as u32;
        self.area_index.insert(key, index);
        self.areas.push(area.clone());
        for line in area
            .outer_bound()
            .iter()
            .chain(area.inner_bounds().iter().flatten())
        {
            self.line(line);
        }
        for regelem in area.regulatory_elements() {
            self.regelem(&regelem);
        }
        index
    }

    fn regelem(&mut self, regelem: &RegulatoryElement) -> u32 {
        let key = regelem.identity();
        if let Some(&index) = self.regelem_index.get(&key) {
            return index;
        }
        let index = self.regelems.len() as u32;
        self.regelem_index.insert(key, index);
        self.regelems.push(regelem.clone());
        for parameter in regelem.parameters().values().flatten() {
            match parameter {
                RuleParameter::Point(point) => {
                    self.point(point);
                }
                RuleParameter::LineString(line) | RuleParameter::Polygon(line) => {
                    self.line(line);
                }
                // Weak: written only if a layer holds the target; see the module docs.
                RuleParameter::Lanelet(_) | RuleParameter::Area(_) => {}
            }
        }
        index
    }

    fn primitive(&mut self, primitive: &Primitive) {
        match primitive {
            Primitive::Point(point) => {
                self.point(point);
            }
            Primitive::LineString(line) | Primitive::Polygon(line) => {
                self.line(line);
            }
            Primitive::Lanelet(lanelet) => {
                self.lanelet(lanelet);
            }
            Primitive::Area(area) => {
                self.area(area);
            }
            Primitive::RegulatoryElement(regelem) => {
                self.regelem(regelem);
            }
        }
    }
}

struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn len(&mut self, value: usize) {
        self.u32(u32::try_from(value).expect("map too large for a snapshot"));
    }
    fn i64(&mut self, value: i64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn f64(&mut self, value: f64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn str(&mut self, value: &str) {
        self.len(value.len());
        self.0.extend_from_slice(value.as_bytes());
    }
    fn attributes(&mut self, map: &AttributeMap) {
        self.len(map.len());
        for (key, value) in map {
            self.str(key);
            self.str(value.value());
        }
    }
    /// A reference to a linestring: its storage, and which way round it is held.
    fn line_ref(&mut self, graph: &Graph, line: &LineString) {
        self.u32(graph.line_index[&data_key(line.identity())]);
        self.u8(u8::from(line.is_inverted()));
    }
    fn lanelet_ref(&mut self, graph: &Graph, lanelet: &Lanelet) {
        self.u32(graph.lanelet_index[&data_key(lanelet.identity())]);
        self.u8(u8::from(lanelet.is_inverted()));
    }
}

/// Encodes a map, or a submap, into a snapshot.
pub fn encode(map: &LaneletMap) -> Vec<u8> {
    let mut graph = Graph::default();
    let layers: Vec<Vec<Primitive>> = LAYERS.iter().map(|&kind| map.layer(kind).all()).collect();
    for primitive in layers.iter().flatten() {
        graph.primitive(primitive);
    }

    let mut out = Writer(Vec::new());
    out.0.extend_from_slice(MAGIC);
    out.u32(VERSION);
    out.u8(u8::from(map.is_recursive()));

    out.len(graph.points.len());
    for point in &graph.points {
        out.i64(point.id());
        for value in point.xyz() {
            out.f64(value);
        }
        out.attributes(&point.attributes().read());
    }

    out.len(graph.lines.len());
    for line in &graph.lines {
        out.i64(line.id());
        out.attributes(&line.attributes().read());
        let points = line.points();
        out.len(points.len());
        for point in &points {
            out.u32(graph.point_index[&point.identity()]);
        }
    }

    out.len(graph.lanelets.len());
    for lanelet in &graph.lanelets {
        out.i64(lanelet.id());
        out.attributes(&lanelet.attributes().read());
        out.line_ref(&graph, &lanelet.left_bound());
        out.line_ref(&graph, &lanelet.right_bound());
        if lanelet.has_custom_centerline() {
            out.u8(1);
            out.line_ref(&graph, &lanelet.centerline());
        } else {
            out.u8(0);
        }
    }

    out.len(graph.areas.len());
    for area in &graph.areas {
        out.i64(area.id());
        out.attributes(&area.attributes().read());
        let outer = area.outer_bound();
        out.len(outer.len());
        for line in &outer {
            out.line_ref(&graph, line);
        }
        let inner = area.inner_bounds();
        out.len(inner.len());
        for ring in &inner {
            out.len(ring.len());
            for line in ring {
                out.line_ref(&graph, line);
            }
        }
    }

    out.len(graph.regelems.len());
    for regelem in &graph.regelems {
        // By class and rule name rather than registry slot: slots are only stable
        // within one process, and the reading process may register in another order.
        out.str(regelem.kind().class_name());
        out.str(regelem.kind().rule_name());
        out.i64(regelem.id());
        out.attributes(&regelem.attributes().read());
        let parameters = regelem.parameters();
        out.len(parameters.len());
        for (role, values) in &parameters {
            out.str(role);
            let live: Vec<&RuleParameter> = values
                .iter()
                .filter(|value| match value {
                    RuleParameter::Lanelet(weak) => weak.upgrade().is_some_and(|lanelet| {
                        graph
                            .lanelet_index
                            .contains_key(&data_key(lanelet.identity()))
                    }),
                    RuleParameter::Area(weak) => weak
                        .upgrade()
                        .is_some_and(|area| graph.area_index.contains_key(&area.identity())),
                    _ => true,
                })
                .collect();
            out.len(live.len());
            for value in live {
                match value {
                    RuleParameter::Point(point) => {
                        out.u8(PARAM_POINT);
                        out.u32(graph.point_index[&point.identity()]);
                    }
                    RuleParameter::LineString(line) => {
                        out.u8(PARAM_LINESTRING);
                        out.line_ref(&graph, line);
                    }
                    RuleParameter::Polygon(line) => {
                        out.u8(PARAM_POLYGON);
                        out.line_ref(&graph, line);
                    }
                    RuleParameter::Lanelet(weak) => {
                        let lanelet = weak.upgrade().expect("filtered above");
                        out.u8(PARAM_LANELET);
                        out.lanelet_ref(&graph, &lanelet);
                    }
                    RuleParameter::Area(weak) => {
                        let area = weak.upgrade().expect("filtered above");
                        out.u8(PARAM_AREA);
                        out.u32(graph.area_index[&area.identity()]);
                    }
                }
            }
        }
    }

    // Attached elements last: a lanelet's element may refer back to the lanelet.
    for lanelet in &graph.lanelets {
        let regelems = lanelet.regulatory_elements();
        out.len(regelems.len());
        for regelem in &regelems {
            out.u32(graph.regelem_index[&regelem.identity()]);
        }
    }
    for area in &graph.areas {
        let regelems = area.regulatory_elements();
        out.len(regelems.len());
        for regelem in &regelems {
            out.u32(graph.regelem_index[&regelem.identity()]);
        }
    }

    for primitives in &layers {
        out.len(primitives.len());
        for primitive in primitives {
            match primitive {
                Primitive::Point(point) => out.u32(graph.point_index[&point.identity()]),
                Primitive::LineString(line) | Primitive::Polygon(line) => {
                    out.line_ref(&graph, line)
                }
                Primitive::Lanelet(lanelet) => out.lanelet_ref(&graph, lanelet),
                Primitive::Area(area) => out.u32(graph.area_index[&area.identity()]),
                Primitive::RegulatoryElement(regelem) => {
                    out.u32(graph.regelem_index[&regelem.identity()])
                }
            }
        }
    }
    out.0
}

/// Why a snapshot could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotError(pub String);

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn truncated() -> SnapshotError {
    SnapshotError("Map snapshot is truncated or corrupt".into())
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], SnapshotError> {
        let end = self.at.checked_add(count).ok_or_else(truncated)?;
        let slice = self.data.get(self.at..end).ok_or_else(truncated)?;
        self.at = end;
        Ok(slice)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], SnapshotError> {
        Ok(self.take(N)?.try_into().expect("length checked"))
    }
    fn u8(&mut self) -> Result<u8, SnapshotError> {
        Ok(self.take(1)?[0])
    }
    fn flag(&mut self) -> Result<bool, SnapshotError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(truncated()),
        }
    }
    fn u32(&mut self) -> Result<u32, SnapshotError> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    fn len(&mut self) -> Result<usize, SnapshotError> {
        let count = self.u32()? as usize;
        // Every counted item takes at least one byte, so a count beyond what is
        // left cannot be honest; checking it up front bounds every allocation.
        if count > self.data.len() - self.at {
            return Err(truncated());
        }
        Ok(count)
    }
    fn i64(&mut self) -> Result<i64, SnapshotError> {
        Ok(i64::from_le_bytes(self.array()?))
    }
    fn f64(&mut self) -> Result<f64, SnapshotError> {
        Ok(f64::from_le_bytes(self.array()?))
    }
    fn str(&mut self) -> Result<String, SnapshotError> {
        let count = self.len()?;
        String::from_utf8(self.take(count)?.to_vec()).map_err(|_| truncated())
    }
    fn attributes(&mut self) -> Result<AttributeMap, SnapshotError> {
        let count = self.len()?;
        let mut map = AttributeMap::new();
        for _ in 0..count {
            let key = self.str()?;
            map.insert(key, Attribute::new(self.str()?));
        }
        Ok(map)
    }
    fn index<T: Clone>(&mut self, table: &[T]) -> Result<T, SnapshotError> {
        let index = self.u32()? as usize;
        table.get(index).cloned().ok_or_else(truncated)
    }
    fn line_ref(&mut self, lines: &[LineString]) -> Result<LineString, SnapshotError> {
        let line = self.index(lines)?;
        Ok(if self.flag()? { line.invert() } else { line })
    }
    fn lanelet_ref(&mut self, lanelets: &[Lanelet]) -> Result<Lanelet, SnapshotError> {
        let lanelet = self.index(lanelets)?;
        Ok(if self.flag()? {
            lanelet.invert()
        } else {
            lanelet
        })
    }
}

fn reserve(id: Id) {
    if id != INVAL_ID {
        register_id(id);
    }
}

/// Restores a map, or a submap, from a snapshot.
///
/// Fails on anything but this format's own current version, and on a regulatory
/// element whose class is not registered in this process.
pub fn decode(data: &[u8]) -> Result<Arc<LaneletMap>, SnapshotError> {
    decode_with(data, &mut registry::find)
}

/// [`decode`], with the caller choosing how a regulatory element's class and rule
/// name become a kind. The Python layer uses it to load an extension on demand.
pub fn decode_with(
    data: &[u8],
    resolve: &mut dyn FnMut(&str, &str) -> Option<RegElemKind>,
) -> Result<Arc<LaneletMap>, SnapshotError> {
    let mut input = Reader { data, at: 0 };
    if input.take(MAGIC.len()).ok() != Some(MAGIC.as_slice()) {
        return Err(SnapshotError("Not a lanelet2 map snapshot".into()));
    }
    let version = input.u32()?;
    if version != VERSION {
        return Err(SnapshotError(format!(
            "Unsupported map snapshot version {version}; this library reads version {VERSION}"
        )));
    }
    let map = if input.flag()? {
        LaneletMap::new_map()
    } else {
        LaneletMap::new_submap()
    };

    let count = input.len()?;
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        let id = input.i64()?;
        let (x, y, z) = (input.f64()?, input.f64()?, input.f64()?);
        points.push(Point::new(id, x, y, z, input.attributes()?));
    }

    let count = input.len()?;
    let mut lines = Vec::with_capacity(count);
    for _ in 0..count {
        let id = input.i64()?;
        let attributes = input.attributes()?;
        let length = input.len()?;
        let mut members = Vec::with_capacity(length);
        for _ in 0..length {
            members.push(input.index(&points)?);
        }
        lines.push(LineString::new(id, members, attributes));
    }

    let count = input.len()?;
    let mut lanelets = Vec::with_capacity(count);
    for _ in 0..count {
        let id = input.i64()?;
        let attributes = input.attributes()?;
        let left = input.line_ref(&lines)?;
        let right = input.line_ref(&lines)?;
        let lanelet = Lanelet::new(id, left, right, attributes);
        if input.flag()? {
            lanelet.set_centerline(input.line_ref(&lines)?);
        }
        lanelets.push(lanelet);
    }

    let count = input.len()?;
    let mut areas = Vec::with_capacity(count);
    for _ in 0..count {
        let id = input.i64()?;
        let attributes = input.attributes()?;
        let length = input.len()?;
        let mut outer = Vec::with_capacity(length);
        for _ in 0..length {
            outer.push(input.line_ref(&lines)?);
        }
        let rings = input.len()?;
        let mut inner: Vec<InnerBound> = Vec::with_capacity(rings);
        for _ in 0..rings {
            let length = input.len()?;
            let mut ring = Vec::with_capacity(length);
            for _ in 0..length {
                ring.push(input.line_ref(&lines)?);
            }
            inner.push(ring);
        }
        areas.push(Area::new(id, outer, inner, attributes));
    }

    let count = input.len()?;
    let mut regelems = Vec::with_capacity(count);
    for _ in 0..count {
        let class_name = input.str()?;
        let rule_name = input.str()?;
        let kind: RegElemKind = resolve(&class_name, &rule_name).ok_or_else(|| {
            SnapshotError(format!(
                "Regulatory element class {class_name} (rule {rule_name}) is not registered \
                 in this process; import the module that provides it before restoring the map"
            ))
        })?;
        let id = input.i64()?;
        let attributes = input.attributes()?;
        let roles = input.len()?;
        let mut parameters = RuleParameterMap::new();
        for _ in 0..roles {
            let role = input.str()?;
            let length = input.len()?;
            let mut values = Vec::with_capacity(length);
            for _ in 0..length {
                values.push(match input.u8()? {
                    PARAM_POINT => RuleParameter::Point(input.index(&points)?),
                    PARAM_LINESTRING => RuleParameter::LineString(input.line_ref(&lines)?),
                    PARAM_POLYGON => RuleParameter::Polygon(input.line_ref(&lines)?),
                    PARAM_LANELET => {
                        RuleParameter::Lanelet(input.lanelet_ref(&lanelets)?.downgrade())
                    }
                    PARAM_AREA => RuleParameter::Area(input.index(&areas)?.downgrade()),
                    _ => return Err(truncated()),
                });
            }
            parameters.insert(role, values);
        }
        regelems.push(RegulatoryElement::new(kind, id, attributes, parameters));
    }

    for lanelet in &lanelets {
        for _ in 0..input.len()? {
            lanelet.add_regulatory_element(input.index(&regelems)?);
        }
    }
    for area in &areas {
        for _ in 0..input.len()? {
            area.add_regulatory_element(input.index(&regelems)?);
        }
    }

    for kind in LAYERS {
        let layer = map.layer(kind);
        for _ in 0..input.len()? {
            let primitive = match kind {
                LayerKind::Point => Primitive::Point(input.index(&points)?),
                LayerKind::LineString => Primitive::LineString(input.line_ref(&lines)?),
                LayerKind::Polygon => Primitive::Polygon(input.line_ref(&lines)?),
                LayerKind::Lanelet => Primitive::Lanelet(input.lanelet_ref(&lanelets)?),
                LayerKind::Area => Primitive::Area(input.index(&areas)?),
                LayerKind::RegulatoryElement => {
                    Primitive::RegulatoryElement(input.index(&regelems)?)
                }
            };
            reserve(primitive.id());
            layer.insert(primitive);
        }
    }
    if input.at != data.len() {
        return Err(truncated());
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regelem::roles;

    fn point(id: Id, x: f64, y: f64) -> Point {
        let mut attributes = AttributeMap::new();
        attributes.insert("ele".into(), Attribute::new(format!("{x}")));
        Point::new(id, x, y, 0.5, attributes)
    }

    fn line(id: Id, points: Vec<Point>) -> LineString {
        let mut attributes = AttributeMap::new();
        attributes.insert("type".into(), "line_thin".into());
        LineString::new(id, points, attributes)
    }

    /// A lanelet with a custom centerline and a traffic light, its inverted twin
    /// sharing the right bound, and an area whose element refers back to it.
    fn sample(recursive: bool) -> Arc<LaneletMap> {
        let shared = line(11, vec![point(3, 0.0, -1.0), point(4, 1.0, -1.0)]);
        let left = line(10, vec![point(1, 0.0, 1.0), point(2, 1.0, 1.0)]);
        let lanelet = Lanelet::new(20, left, shared.clone(), AttributeMap::new());
        lanelet.set_centerline(line(12, vec![point(5, 0.0, 0.0), point(6, 1.0, 0.0)]));
        let twin = Lanelet::new(
            21,
            shared.invert(),
            line(13, vec![point(7, 0.0, -3.0), point(8, 1.0, -3.0)]),
            AttributeMap::new(),
        )
        .invert();

        let mut parameters = RuleParameterMap::new();
        parameters.insert(
            roles::REFERS.into(),
            vec![RuleParameter::LineString(line(
                30,
                vec![point(31, 0.5, 2.0)],
            ))],
        );
        parameters.insert(
            "ref_lanelet".into(),
            vec![RuleParameter::Lanelet(twin.downgrade())],
        );
        let light = RegulatoryElement::new(
            RegElemKind::TrafficLight,
            40,
            AttributeMap::new(),
            parameters,
        );
        lanelet.add_regulatory_element(light.clone());

        let ring = line(50, vec![point(51, 5.0, 5.0), point(52, 6.0, 5.0)]);
        let closing = line(53, vec![point(52, 6.0, 5.0), point(51, 5.0, 5.0)]);
        let area = Area::new(
            54,
            vec![ring, closing.invert()],
            vec![],
            AttributeMap::new(),
        );
        area.add_regulatory_element(light);

        let map = if recursive {
            LaneletMap::new_map()
        } else {
            LaneletMap::new_submap()
        };
        map.add(Primitive::Lanelet(lanelet));
        map.add(Primitive::Lanelet(twin));
        map.add(Primitive::Area(area));
        map
    }

    /// Everything observable about a map, by id and by view.
    fn describe(map: &LaneletMap) -> Vec<String> {
        let mut out = vec![format!("recursive {}", map.is_recursive())];
        for kind in LAYERS {
            for primitive in map.layer(kind).all() {
                out.push(match &primitive {
                    Primitive::Point(p) => format!("{p:?} {:?}", p.attributes().read()),
                    Primitive::LineString(l) | Primitive::Polygon(l) => format!(
                        "{kind:?} {} inverted {} {:?} {:?}",
                        l.id(),
                        l.is_inverted(),
                        l.points().iter().map(Point::id).collect::<Vec<_>>(),
                        l.attributes().read()
                    ),
                    Primitive::Lanelet(l) => format!(
                        "{} centerline {} {:?} regelems {:?}",
                        l.to_display_string(),
                        l.has_custom_centerline(),
                        l.centerline()
                            .points()
                            .iter()
                            .map(Point::id)
                            .collect::<Vec<_>>(),
                        l.regulatory_elements()
                            .iter()
                            .map(|r| r.to_display_string())
                            .collect::<Vec<_>>()
                    ),
                    Primitive::Area(a) => format!(
                        "area {} {:?} regelems {}",
                        a.id(),
                        a.outer_bound()
                            .iter()
                            .map(|l| (l.id(), l.is_inverted()))
                            .collect::<Vec<_>>(),
                        a.regulatory_elements().len()
                    ),
                    Primitive::RegulatoryElement(r) => {
                        format!("{:?} {}", r.kind(), r.to_display_string())
                    }
                });
            }
        }
        out
    }

    #[test]
    fn a_map_and_a_submap_round_trip() {
        for recursive in [true, false] {
            let map = sample(recursive);
            let restored = decode(&encode(&map)).unwrap();
            assert_eq!(describe(&restored), describe(&map));
            assert_eq!(
                encode(&restored),
                encode(&map),
                "and re-encode to the same bytes"
            );
        }
    }

    #[test]
    fn shared_storage_stays_shared() {
        let restored = decode(&encode(&sample(true))).unwrap();
        let lanelet = restored.lanelets.get(20).unwrap();
        let twin = restored.lanelets.get(21).unwrap();
        let (Primitive::Lanelet(lanelet), Primitive::Lanelet(twin)) = (lanelet, twin) else {
            panic!("lanelets expected")
        };
        assert!(twin.is_inverted());
        // The twin is inverted, so its left bound is its stored right one, inverted.
        assert!(
            lanelet
                .right_bound()
                .is_same_data(&twin.invert().left_bound())
        );

        // Moving a point moves it everywhere it is used.
        let Primitive::Point(shared) = restored.points.get(3).unwrap() else {
            panic!("point expected")
        };
        shared.set_x(42.0);
        assert_eq!(lanelet.right_bound().points()[0].x(), 42.0);

        // The element's weak reference resolves to the very lanelet in the layer, and
        // the area holds the same element as the lanelet.
        let light = &lanelet.regulatory_elements()[0];
        let Some(RuleParameter::Lanelet(weak)) =
            light.parameters_for("ref_lanelet").into_iter().next()
        else {
            panic!("lanelet parameter expected")
        };
        assert!(weak.upgrade().unwrap().is_same_view(&twin));
        let Primitive::Area(area) = restored.areas.get(54).unwrap() else {
            panic!("area expected")
        };
        assert!(area.regulatory_elements()[0].is_same_data(light));
    }

    #[test]
    fn layers_are_restored_as_they_are_not_as_add_would_build_them() {
        let map = LaneletMap::new_map();
        let lanelet = Lanelet::new(
            20,
            line(10, vec![point(1, 0.0, 1.0)]),
            line(11, vec![point(2, 0.0, -1.0)]),
            AttributeMap::new(),
        );
        // Only the lanelet is in a layer; its bounds and points are not.
        map.lanelets.insert(Primitive::Lanelet(lanelet));
        let restored = decode(&encode(&map)).unwrap();
        assert_eq!(restored.lanelets.len(), 1);
        assert_eq!(restored.line_strings.len(), 0);
        assert_eq!(restored.points.len(), 0);
        assert_eq!(describe(&restored), describe(&map));
    }

    #[test]
    fn a_weak_parameter_survives_only_if_the_map_holds_its_target() {
        let map = LaneletMap::new_map();
        let mut parameters = RuleParameterMap::new();
        let gone = Lanelet::new(
            60,
            line(61, vec![point(62, 0.0, 0.0)]),
            line(63, vec![point(64, 0.0, 1.0)]),
            AttributeMap::new(),
        );
        let kept = Lanelet::new(
            70,
            line(71, vec![point(72, 0.0, 0.0)]),
            line(73, vec![point(74, 0.0, 1.0)]),
            AttributeMap::new(),
        );
        let outside = Lanelet::new(
            90,
            line(91, vec![point(92, 0.0, 0.0)]),
            line(93, vec![point(94, 0.0, 1.0)]),
            AttributeMap::new(),
        );
        parameters.insert(
            roles::RIGHT_OF_WAY.into(),
            vec![
                RuleParameter::Lanelet(gone.downgrade()),
                RuleParameter::Lanelet(kept.downgrade()),
                RuleParameter::Lanelet(outside.downgrade()),
            ],
        );
        let regelem =
            RegulatoryElement::new(RegElemKind::RightOfWay, 80, AttributeMap::new(), parameters);
        drop(gone);
        map.add(Primitive::RegulatoryElement(regelem));
        map.add(Primitive::Lanelet(kept));

        let restored = decode(&encode(&map)).unwrap();
        let Primitive::RegulatoryElement(regelem) = restored.regulatory_elements.get(80).unwrap()
        else {
            panic!("regulatory element expected")
        };
        let values = regelem.parameters_for(roles::RIGHT_OF_WAY);
        assert_eq!(
            values.len(),
            1,
            "the expired one and the one outside the map are gone"
        );
        assert_eq!(
            values[0].id(),
            Some(70),
            "and the one in the map is held by it"
        );
        drop(outside);
    }

    #[test]
    fn restoring_reserves_the_ids_it_brings_back() {
        let map = LaneletMap::new_map();
        map.add(Primitive::Point(point(7_000_000, 0.0, 0.0)));
        let bytes = encode(&map);
        decode(&bytes).unwrap();
        assert!(crate::id::get_id() > 7_000_000);
    }

    #[test]
    fn foreign_corrupt_and_newer_snapshots_are_refused() {
        let bytes = encode(&sample(true));
        assert!(decode(b"not a snapshot").is_err());
        assert!(decode(&bytes[..bytes.len() - 1]).is_err());
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(decode(&trailing).is_err());
        let mut newer = bytes.clone();
        newer[MAGIC.len()..MAGIC.len() + 4].copy_from_slice(&(VERSION + 1).to_le_bytes());
        assert!(decode(&newer).err().unwrap().0.contains("version"));
        // Truncated anywhere, never a panic.
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end]).is_err());
        }
    }

    #[test]
    fn an_unregistered_class_names_itself() {
        let bytes = encode(&sample(true));
        let error = decode_with(&bytes, &mut |_, _| None).err().unwrap();
        assert!(error.0.contains("TrafficLight"), "{}", error.0);
    }
}
