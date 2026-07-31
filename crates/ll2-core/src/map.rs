//! Map layers, `LaneletMap` and `LaneletSubmap`.
//!
//! A map is six layers keyed by id. Adding a primitive to a `LaneletMap` pulls in
//! everything it references — a lanelet drags in its bounds, their points, and its
//! regulatory elements — assigning fresh ids to anything that has none. A
//! `LaneletSubmap` does not recurse; it still assigns an id to the object handed to
//! it, but records nothing else.
//!
//! Upstream: `lanelet2_core/src/LaneletMap.cpp:110-130, 340-380, 545-700`

use std::collections::BTreeMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::area::Area;
use crate::geometry::bbox::{self, BoundingBox2d};
use crate::geometry::distance;
use crate::id::{INVAL_ID, Id, get_id, register_id};
use crate::lanelet::Lanelet;
use crate::linestring::LineString;
use crate::point::Point;
use crate::regelem::{RegulatoryElement, RuleParameter};

/// Anything a layer can hold.
///
/// Polygons and linestrings share their storage type, so the variant is what tells
/// them apart.
#[derive(Clone)]
pub enum Primitive {
    Point(Point),
    LineString(LineString),
    Polygon(LineString),
    Lanelet(Lanelet),
    Area(Area),
    RegulatoryElement(RegulatoryElement),
}

impl Primitive {
    pub fn id(&self) -> Id {
        match self {
            Primitive::Point(p) => p.id(),
            Primitive::LineString(l) | Primitive::Polygon(l) => l.id(),
            Primitive::Lanelet(l) => l.id(),
            Primitive::Area(a) => a.id(),
            Primitive::RegulatoryElement(r) => r.id(),
        }
    }

    pub fn set_id(&self, id: Id) {
        match self {
            Primitive::Point(p) => p.set_id(id),
            Primitive::LineString(l) | Primitive::Polygon(l) => l.set_id(id),
            Primitive::Lanelet(l) => l.set_id(id),
            Primitive::Area(a) => a.set_id(id),
            Primitive::RegulatoryElement(r) => r.set_id(id),
        }
    }

    pub fn kind(&self) -> LayerKind {
        match self {
            Primitive::Point(_) => LayerKind::Point,
            Primitive::LineString(_) => LayerKind::LineString,
            Primitive::Polygon(_) => LayerKind::Polygon,
            Primitive::Lanelet(_) => LayerKind::Lanelet,
            Primitive::Area(_) => LayerKind::Area,
            Primitive::RegulatoryElement(_) => LayerKind::RegulatoryElement,
        }
    }

    pub fn bounding_box_2d(&self) -> BoundingBox2d {
        match self {
            Primitive::Point(p) => bbox::of_point_2d(p),
            Primitive::LineString(l) | Primitive::Polygon(l) => bbox::of_linestring_2d(l),
            Primitive::Lanelet(l) => bbox::of_lanelet_2d(l),
            Primitive::Area(a) => bbox::of_area_2d(a),
            Primitive::RegulatoryElement(r) => {
                let mut box2d = BoundingBox2d::empty();
                for values in r.parameters().values() {
                    for value in values {
                        box2d.extend(&rule_parameter_bbox(value));
                    }
                }
                box2d
            }
        }
    }

    pub fn distance_2d(&self, point: [f64; 2]) -> f64 {
        match self {
            Primitive::Point(p) => distance::distance_2d_point_to_point_primitive(point, p),
            Primitive::LineString(l) => distance::distance_2d_point_to_linestring(point, l),
            Primitive::Polygon(l) => distance::distance_2d_point_to_polygon(point, l),
            Primitive::Lanelet(l) => distance::distance_2d_point_to_lanelet(point, l),
            Primitive::Area(a) => distance::distance_2d_point_to_area(point, a),
            Primitive::RegulatoryElement(_) => self.bounding_box_2d().distance_to(point),
        }
    }
}

fn rule_parameter_bbox(parameter: &RuleParameter) -> BoundingBox2d {
    match parameter {
        RuleParameter::Point(p) => bbox::of_point_2d(p),
        RuleParameter::LineString(l) | RuleParameter::Polygon(l) => bbox::of_linestring_2d(l),
        RuleParameter::Lanelet(weak) => weak
            .upgrade()
            .map(|l| bbox::of_lanelet_2d(&l))
            .unwrap_or_default(),
        RuleParameter::Area(weak) => weak
            .upgrade()
            .map(|a| bbox::of_area_2d(&a))
            .unwrap_or_default(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerKind {
    Point,
    LineString,
    Polygon,
    Lanelet,
    Area,
    RegulatoryElement,
}

/// Raised on a lookup that finds nothing, or one made with `InvalId`.
#[derive(Debug)]
pub struct NoSuchPrimitive(pub String);

/// One layer of a map: primitives of a single kind, keyed by id.
///
/// Upstream backs this with `std::unordered_map`, so its iteration order is not
/// reproducible even between two runs. A `BTreeMap` here makes iteration
/// id-ordered, which is a strictly stronger guarantee and costs nothing.
pub struct Layer {
    kind: LayerKind,
    items: RwLock<BTreeMap<Id, Primitive>>,
}

impl Layer {
    fn new(kind: LayerKind) -> Self {
        Layer {
            kind,
            items: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn kind(&self) -> LayerKind {
        self.kind
    }

    pub fn len(&self) -> usize {
        self.items.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `InvalId` never exists, even if something with that id were stored.
    pub fn exists(&self, id: Id) -> bool {
        id != INVAL_ID && self.items.read().contains_key(&id)
    }

    pub fn get(&self, id: Id) -> Result<Primitive, NoSuchPrimitive> {
        if id == INVAL_ID {
            return Err(NoSuchPrimitive(
                "Tried to lookup an element with id InvalId!".into(),
            ));
        }
        self.items
            .read()
            .get(&id)
            .cloned()
            .ok_or_else(|| NoSuchPrimitive(format!("Primitive with id {id} does not exist")))
    }

    /// Every element, ordered by id.
    pub fn all(&self) -> Vec<Primitive> {
        self.items.read().values().cloned().collect()
    }

    /// Stores a primitive, keeping whatever is already under that id.
    ///
    /// Upstream uses `std::unordered_map::insert`, which does not overwrite. That
    /// is load-bearing: adding a lanelet or an area re-offers its boundaries, and
    /// some of those handles are inverted views of linestrings the map already
    /// holds the right way round.
    pub fn insert(&self, primitive: Primitive) {
        self.items
            .write()
            .entry(primitive.id())
            .or_insert(primitive);
    }

    /// A fresh id. Upstream simply draws from the global counter.
    pub fn unique_id(&self) -> Id {
        get_id()
    }

    /// Everything whose bounding box meets `area`, ordered by id.
    pub fn search(&self, area: &BoundingBox2d) -> Vec<Primitive> {
        self.all()
            .into_iter()
            .filter(|primitive| primitive.bounding_box_2d().intersects(area))
            .collect()
    }

    /// The `count` nearest primitives, closest first.
    ///
    /// Ties are broken by id so the result does not depend on iteration order,
    /// which upstream's does.
    pub fn nearest(&self, point: [f64; 2], count: usize) -> Vec<Primitive> {
        let mut scored: Vec<(f64, Id, Primitive)> = self
            .all()
            .into_iter()
            .map(|primitive| (primitive.distance_2d(point), primitive.id(), primitive))
            .collect();
        scored.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        scored
            .into_iter()
            .take(count)
            .map(|(_, _, primitive)| primitive)
            .collect()
    }

    /// The primitives in this layer that reference `id`.
    pub fn find_usages(&self, id: Id) -> Vec<Primitive> {
        self.all()
            .into_iter()
            .filter(|primitive| references(primitive, id))
            .collect()
    }
}

/// Whether `primitive` directly references something with the given id.
fn references(primitive: &Primitive, id: Id) -> bool {
    match primitive {
        Primitive::Point(_) => false,
        Primitive::LineString(line) | Primitive::Polygon(line) => {
            line.points().iter().any(|point| point.id() == id)
        }
        Primitive::Lanelet(lanelet) => {
            lanelet.left_bound().id() == id
                || lanelet.right_bound().id() == id
                || lanelet
                    .regulatory_elements()
                    .iter()
                    .any(|regelem| regelem.id() == id)
        }
        Primitive::Area(area) => {
            area.outer_bound().iter().any(|line| line.id() == id)
                || area
                    .inner_bounds()
                    .iter()
                    .flatten()
                    .any(|line| line.id() == id)
                || area
                    .regulatory_elements()
                    .iter()
                    .any(|regelem| regelem.id() == id)
        }
        Primitive::RegulatoryElement(regelem) => regelem.find(id).is_some(),
    }
}

/// The six layers of a map.
pub struct LaneletMap {
    pub points: Layer,
    pub line_strings: Layer,
    pub polygons: Layer,
    pub lanelets: Layer,
    pub areas: Layer,
    pub regulatory_elements: Layer,
    /// A submap records only what it is handed, never what that references.
    recursive: bool,
}

impl LaneletMap {
    pub fn new_map() -> Arc<Self> {
        Arc::new(LaneletMap::with_recursion(true))
    }

    pub fn new_submap() -> Arc<Self> {
        Arc::new(LaneletMap::with_recursion(false))
    }

    fn with_recursion(recursive: bool) -> Self {
        LaneletMap {
            points: Layer::new(LayerKind::Point),
            line_strings: Layer::new(LayerKind::LineString),
            polygons: Layer::new(LayerKind::Polygon),
            lanelets: Layer::new(LayerKind::Lanelet),
            areas: Layer::new(LayerKind::Area),
            regulatory_elements: Layer::new(LayerKind::RegulatoryElement),
            recursive,
        }
    }

    pub fn is_recursive(&self) -> bool {
        self.recursive
    }

    pub fn layer(&self, kind: LayerKind) -> &Layer {
        match kind {
            LayerKind::Point => &self.points,
            LayerKind::LineString => &self.line_strings,
            LayerKind::Polygon => &self.polygons,
            LayerKind::Lanelet => &self.lanelets,
            LayerKind::Area => &self.areas,
            LayerKind::RegulatoryElement => &self.regulatory_elements,
        }
    }

    /// Assigns a fresh id if the primitive has none, otherwise reserves the one it
    /// has so that `getId` never hands it out again, then stores it.
    fn record(&self, primitive: Primitive) {
        let layer = self.layer(primitive.kind());
        if primitive.id() == INVAL_ID {
            primitive.set_id(layer.unique_id());
        } else {
            register_id(primitive.id());
        }
        layer.insert(primitive);
    }

    /// Adds a primitive, and — on a full map — everything it references.
    pub fn add(&self, primitive: Primitive) {
        self.record(primitive.clone());
        if !self.recursive {
            return;
        }
        match primitive {
            Primitive::Point(_) => {}
            Primitive::LineString(line) | Primitive::Polygon(line) => {
                for point in line.points() {
                    self.record(Primitive::Point(point));
                }
            }
            Primitive::Lanelet(lanelet) => {
                self.add(Primitive::LineString(lanelet.left_bound()));
                self.add(Primitive::LineString(lanelet.right_bound()));
                for regelem in lanelet.regulatory_elements() {
                    self.add(Primitive::RegulatoryElement(regelem));
                }
            }
            Primitive::Area(area) => {
                for line in area.outer_bound() {
                    self.add(Primitive::LineString(line));
                }
                for line in area.inner_bounds().into_iter().flatten() {
                    self.add(Primitive::LineString(line));
                }
                for regelem in area.regulatory_elements() {
                    self.add(Primitive::RegulatoryElement(regelem));
                }
            }
            Primitive::RegulatoryElement(regelem) => {
                for values in regelem.parameters().values() {
                    for value in values {
                        match value {
                            RuleParameter::Point(point) => {
                                self.record(Primitive::Point(point.clone()))
                            }
                            RuleParameter::LineString(line) => {
                                self.add(Primitive::LineString(line.clone()))
                            }
                            RuleParameter::Polygon(polygon) => {
                                self.add(Primitive::Polygon(polygon.clone()))
                            }
                            // Lanelets and areas are referenced weakly and are not
                            // pulled into the map by their regulatory element.
                            RuleParameter::Lanelet(_) | RuleParameter::Area(_) => {}
                        }
                    }
                }
            }
        }
    }
}

/// The linestring inside a linestring-or-polygon primitive, if it is one.
pub fn as_linestring(primitive: &Primitive) -> Option<&LineString> {
    match primitive {
        Primitive::LineString(line) | Primitive::Polygon(line) => Some(line),
        _ => None,
    }
}

pub fn as_point(primitive: &Primitive) -> Option<&Point> {
    match primitive {
        Primitive::Point(point) => Some(point),
        _ => None,
    }
}

pub fn as_lanelet(primitive: &Primitive) -> Option<&Lanelet> {
    match primitive {
        Primitive::Lanelet(lanelet) => Some(lanelet),
        _ => None,
    }
}

pub fn as_area(primitive: &Primitive) -> Option<&Area> {
    match primitive {
        Primitive::Area(area) => Some(area),
        _ => None,
    }
}

pub fn as_regulatory_element(primitive: &Primitive) -> Option<&RegulatoryElement> {
    match primitive {
        Primitive::RegulatoryElement(regelem) => Some(regelem),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute::AttributeMap;

    fn point(id: Id, x: f64, y: f64) -> Point {
        Point::new(id, x, y, 0.0, AttributeMap::new())
    }

    fn lanelet(id: Id) -> Lanelet {
        Lanelet::new(
            id,
            LineString::new(
                INVAL_ID,
                vec![point(INVAL_ID, 0.0, 1.0), point(INVAL_ID, 1.0, 1.0)],
                AttributeMap::new(),
            ),
            LineString::new(
                INVAL_ID,
                vec![point(INVAL_ID, 0.0, -1.0), point(INVAL_ID, 1.0, -1.0)],
                AttributeMap::new(),
            ),
            AttributeMap::new(),
        )
    }

    #[test]
    fn adding_a_lanelet_pulls_in_its_bounds_and_points() {
        let map = LaneletMap::new_map();
        map.add(Primitive::Lanelet(lanelet(INVAL_ID)));

        assert_eq!(map.lanelets.len(), 1);
        assert_eq!(map.line_strings.len(), 2);
        assert_eq!(map.points.len(), 4);
        // Everything was given an id on the way in.
        assert!(map.points.all().iter().all(|p| p.id() != INVAL_ID));
        assert!(map.lanelets.all().iter().all(|l| l.id() != INVAL_ID));
    }

    #[test]
    fn a_submap_records_only_what_it_is_handed() {
        let submap = LaneletMap::new_submap();
        submap.add(Primitive::Lanelet(lanelet(INVAL_ID)));

        assert_eq!(submap.lanelets.len(), 1);
        assert_eq!(submap.line_strings.len(), 0);
        assert_eq!(submap.points.len(), 0);
        assert!(
            submap.lanelets.all()[0].id() != INVAL_ID,
            "but it still gets an id"
        );
    }

    #[test]
    fn lookups_reject_the_invalid_id_and_missing_elements() {
        let map = LaneletMap::new_map();
        map.add(Primitive::Point(point(7, 0.0, 0.0)));

        assert!(map.points.exists(7));
        assert!(!map.points.exists(INVAL_ID));
        assert!(!map.points.exists(8));
        assert!(map.points.get(7).is_ok());
        assert!(map.points.get(INVAL_ID).is_err());
        assert!(map.points.get(8).is_err());
    }

    #[test]
    fn re_adding_an_id_keeps_the_first_primitive() {
        let map = LaneletMap::new_map();
        let first = point(7, 1.0, 0.0);
        map.add(Primitive::Point(first.clone()));
        map.add(Primitive::Point(point(7, 99.0, 0.0)));

        let stored = map.points.get(7).unwrap();
        assert_eq!(map.points.len(), 1);
        assert!(as_point(&stored).unwrap().is_same_data(&first));
    }

    #[test]
    fn adding_an_existing_id_reserves_it_against_the_global_counter() {
        let map = LaneletMap::new_map();
        map.add(Primitive::Point(point(900_000, 0.0, 0.0)));
        assert!(get_id() > 900_000);
    }

    #[test]
    fn find_usages_reports_the_linestrings_a_point_belongs_to() {
        let map = LaneletMap::new_map();
        let shared = point(INVAL_ID, 0.0, 0.0);
        let line = LineString::new(
            INVAL_ID,
            vec![shared.clone(), point(INVAL_ID, 1.0, 0.0)],
            AttributeMap::new(),
        );
        map.add(Primitive::LineString(line));

        assert_eq!(map.line_strings.find_usages(shared.id()).len(), 1);
        assert!(map.line_strings.find_usages(123_456).is_empty());
    }

    #[test]
    fn nearest_returns_the_closest_first() {
        let map = LaneletMap::new_map();
        for (id, x) in [(1, 5.0), (2, 1.0), (3, 3.0)] {
            map.add(Primitive::Point(point(id, x, 0.0)));
        }
        let ids: Vec<Id> = map
            .points
            .nearest([0.0, 0.0], 2)
            .iter()
            .map(Primitive::id)
            .collect();
        assert_eq!(ids, [2, 3]);
    }

    #[test]
    fn search_returns_everything_meeting_the_box() {
        let map = LaneletMap::new_map();
        for (id, x) in [(1, 0.0), (2, 5.0), (3, 10.0)] {
            map.add(Primitive::Point(point(id, x, 0.0)));
        }
        let found: Vec<Id> = map
            .points
            .search(&BoundingBox2d {
                min: [-1.0, -1.0],
                max: [6.0, 1.0],
            })
            .iter()
            .map(Primitive::id)
            .collect();
        assert_eq!(found, [1, 2]);
    }
}
