//! Areas.
//!
//! An area is a region bounded by one outer ring and any number of inner rings
//! (holes). Each ring is a *list* of linestrings chained end to start, which is why
//! the bounds are nested vectors rather than single linestrings.
//!
//! Unlike a lanelet's centerline, the ring polygons are recomputed eagerly whenever
//! a bound is assigned rather than lazily on read. An area has no direction of
//! travel, so there is no `invert`.
//!
//! Upstream: `lanelet2_core/include/lanelet2_core/primitives/Area.h:85-108`

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use parking_lot::RwLock;

use crate::attribute::AttributeMap;
use crate::compound::CompoundLineString;
use crate::id::Id;
use crate::linestring::LineString;
use crate::refs::{Attrs, attrs};
use crate::regelem::RegulatoryElement;

/// A hole: one closed ring, assembled from consecutive linestrings.
pub type InnerBound = Vec<LineString>;

pub struct AreaData {
    id: AtomicI64,
    outer: RwLock<Vec<LineString>>,
    inner: RwLock<Vec<InnerBound>>,
    attributes: Attrs,
    regelems: RwLock<Vec<RegulatoryElement>>,
    outer_polygon: RwLock<CompoundLineString>,
    inner_polygons: RwLock<Vec<CompoundLineString>>,
}

/// A handle to an area.
#[derive(Clone)]
pub struct Area {
    data: Arc<AreaData>,
}

impl Area {
    pub fn new(
        id: Id,
        outer: Vec<LineString>,
        inner: Vec<InnerBound>,
        attributes: AttributeMap,
    ) -> Self {
        let outer_polygon = CompoundLineString::new(outer.clone());
        let inner_polygons = inner
            .iter()
            .map(|hole| CompoundLineString::new(hole.clone()))
            .collect();
        Area {
            data: Arc::new(AreaData {
                id: AtomicI64::new(id),
                outer: RwLock::new(outer),
                inner: RwLock::new(inner),
                attributes: attrs(attributes),
                regelems: RwLock::new(Vec::new()),
                outer_polygon: RwLock::new(outer_polygon),
                inner_polygons: RwLock::new(inner_polygons),
            }),
        }
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

    pub fn set_attributes(&self, map: AttributeMap) {
        *self.data.attributes.write() = map;
    }

    pub fn outer_bound(&self) -> Vec<LineString> {
        self.data.outer.read().clone()
    }

    /// Assigning a bound recomputes its ring polygon immediately.
    pub fn set_outer_bound(&self, outer: Vec<LineString>) {
        *self.data.outer_polygon.write() = CompoundLineString::new(outer.clone());
        *self.data.outer.write() = outer;
    }

    pub fn inner_bounds(&self) -> Vec<InnerBound> {
        self.data.inner.read().clone()
    }

    pub fn set_inner_bounds(&self, inner: Vec<InnerBound>) {
        *self.data.inner_polygons.write() = inner
            .iter()
            .map(|hole| CompoundLineString::new(hole.clone()))
            .collect();
        *self.data.inner.write() = inner;
    }

    pub fn outer_bound_polygon(&self) -> CompoundLineString {
        self.data.outer_polygon.read().clone()
    }

    pub fn inner_bound_polygons(&self) -> Vec<CompoundLineString> {
        self.data.inner_polygons.read().clone()
    }

    pub fn regulatory_elements(&self) -> Vec<RegulatoryElement> {
        self.data.regelems.read().clone()
    }

    pub fn add_regulatory_element(&self, regelem: RegulatoryElement) {
        self.data.regelems.write().push(regelem);
    }

    pub fn remove_regulatory_element(&self, regelem: &RegulatoryElement) -> bool {
        let mut regelems = self.data.regelems.write();
        match regelems.iter().position(|held| held.is_same_data(regelem)) {
            Some(index) => {
                regelems.remove(index);
                true
            }
            None => false,
        }
    }

    /// A non-owning handle, for a regulatory element's reference back to the areas
    /// it governs.
    pub fn downgrade(&self) -> WeakArea {
        WeakArea {
            data: Arc::downgrade(&self.data),
        }
    }

    pub fn is_same_data(&self, other: &Area) -> bool {
        Arc::ptr_eq(&self.data, &other.data)
    }

    pub fn identity(&self) -> usize {
        Arc::as_ptr(&self.data) as usize
    }
}

/// A non-owning handle to an area.
#[derive(Clone)]
pub struct WeakArea {
    data: std::sync::Weak<AreaData>,
}

impl WeakArea {
    pub fn upgrade(&self) -> Option<Area> {
        self.data.upgrade().map(|data| Area { data })
    }
}

impl PartialEq for Area {
    fn eq(&self, other: &Self) -> bool {
        self.is_same_data(other)
    }
}

impl Eq for Area {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point::Point;

    fn line(id: Id, count: usize) -> LineString {
        LineString::new(
            id,
            (0..count)
                .map(|i| Point::new(id * 100 + i as i64, i as f64, 0.0, 0.0, AttributeMap::new()))
                .collect(),
            AttributeMap::new(),
        )
    }

    #[test]
    fn ring_polygons_are_recomputed_when_a_bound_is_assigned() {
        let area = Area::new(1, vec![line(10, 3)], Vec::new(), AttributeMap::new());
        assert_eq!(area.outer_bound_polygon().ids(), [10]);

        area.set_outer_bound(vec![line(11, 2), line(12, 2)]);
        assert_eq!(area.outer_bound_polygon().ids(), [11, 12]);
        assert_eq!(area.outer_bound().len(), 2);
    }

    #[test]
    fn holes_each_get_their_own_ring_polygon() {
        let area = Area::new(
            1,
            vec![line(10, 4)],
            vec![vec![line(20, 3)], vec![line(30, 2), line(31, 2)]],
            AttributeMap::new(),
        );
        let holes = area.inner_bound_polygons();
        assert_eq!(holes.len(), 2);
        assert_eq!(holes[0].ids(), [20]);
        assert_eq!(holes[1].ids(), [30, 31]);
    }

    #[test]
    fn regulatory_elements_can_be_attached_and_detached() {
        use crate::regelem::{RegElemKind, RegulatoryElement, RuleParameterMap};

        let area = Area::new(1, vec![line(10, 3)], Vec::new(), AttributeMap::new());
        let regelem = RegulatoryElement::new(
            RegElemKind::Generic,
            5,
            AttributeMap::new(),
            RuleParameterMap::new(),
        );

        area.add_regulatory_element(regelem.clone());
        assert_eq!(area.regulatory_elements().len(), 1);
        assert!(area.remove_regulatory_element(&regelem));
        assert!(area.regulatory_elements().is_empty());
        assert!(!area.remove_regulatory_element(&regelem));
    }

    #[test]
    fn a_weak_handle_stops_resolving_once_the_area_is_gone() {
        let weak = {
            let area = Area::new(1, vec![line(10, 3)], Vec::new(), AttributeMap::new());
            let weak = area.downgrade();
            assert!(weak.upgrade().is_some());
            weak
        };
        assert!(weak.upgrade().is_none());
    }
}
