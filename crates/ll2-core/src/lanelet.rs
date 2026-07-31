//! Lanelets.
//!
//! A lanelet is a pair of bounds plus a lazily computed centerline. `invert()` is
//! again an O(1) flag flip, but here it does more than reverse: it *swaps* the
//! bounds as well, because the left bound of a lanelet driven backwards is the
//! reverse of its right bound. Writes are mapped back the same way, so
//! `setLeftBound` on an inverted handle assigns the inverted linestring to the
//! underlying *right* bound.
//!
//! Upstream: `lanelet2_core/include/lanelet2_core/primitives/Lanelet.h:150-370`,
//! `lanelet2_core/src/Lanelet.cpp:237-305`

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use parking_lot::RwLock;

use crate::attribute::AttributeMap;
use crate::centerline;
use crate::fmt::make_repr;
use crate::id::{Id, INVAL_ID};
use crate::linestring::LineString;
use crate::refs::{Attrs, attrs};

pub struct LaneletData {
    id: AtomicI64,
    left: RwLock<LineString>,
    right: RwLock<LineString>,
    attributes: Attrs,
    /// Filled on first use, and cleared whenever a bound changes — unless the user
    /// set a centerline explicitly, which is detected purely by it having an id.
    centerline: RwLock<Option<LineString>>,
}

/// A handle to a lanelet, carrying its own direction of travel.
#[derive(Clone)]
pub struct Lanelet {
    data: Arc<LaneletData>,
    inverted: bool,
}

impl Lanelet {
    pub fn new(id: Id, left: LineString, right: LineString, attributes: AttributeMap) -> Self {
        Lanelet {
            data: Arc::new(LaneletData {
                id: AtomicI64::new(id),
                left: RwLock::new(left),
                right: RwLock::new(right),
                attributes: attrs(attributes),
                centerline: RwLock::new(None),
            }),
            inverted: false,
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

    pub fn is_inverted(&self) -> bool {
        self.inverted
    }

    pub fn invert(&self) -> Lanelet {
        Lanelet {
            data: self.data.clone(),
            inverted: !self.inverted,
        }
    }

    /// Inverting swaps *and* reverses the bounds.
    pub fn left_bound(&self) -> LineString {
        if self.inverted {
            self.data.right.read().invert()
        } else {
            self.data.left.read().clone()
        }
    }

    pub fn right_bound(&self) -> LineString {
        if self.inverted {
            self.data.left.read().invert()
        } else {
            self.data.right.read().clone()
        }
    }

    pub fn set_left_bound(&self, bound: LineString) {
        if self.inverted {
            self.set_data_right(bound.invert());
        } else {
            self.set_data_left(bound);
        }
    }

    pub fn set_right_bound(&self, bound: LineString) {
        if self.inverted {
            self.set_data_left(bound.invert());
        } else {
            self.set_data_right(bound);
        }
    }

    /// Upstream only resets the cache when the bound actually changes, comparing
    /// by storage identity *and* orientation.
    fn set_data_left(&self, bound: LineString) {
        let mut slot = self.data.left.write();
        if *slot != bound {
            self.reset_cache();
            *slot = bound;
        }
    }

    fn set_data_right(&self, bound: LineString) {
        let mut slot = self.data.right.write();
        if *slot != bound {
            self.reset_cache();
            *slot = bound;
        }
    }

    /// The centerline, computed on first use and cached.
    ///
    /// The computed result and all of its points carry `InvalId`.
    pub fn centerline(&self) -> LineString {
        let cached = self.data.centerline.read().clone();
        let centerline = match cached {
            Some(existing) => existing,
            None => {
                // Compute without holding the cache lock: the calculation reads the
                // bounds and their points, and must not be nested inside a write.
                let (left, right) = (self.data.left.read().clone(), self.data.right.read().clone());
                let computed = centerline::calculate(&left, &right);
                let mut slot = self.data.centerline.write();
                slot.get_or_insert(computed).clone()
            }
        };
        if self.inverted {
            centerline.invert()
        } else {
            centerline
        }
    }

    pub fn set_centerline(&self, line: LineString) {
        let stored = if self.inverted { line.invert() } else { line };
        *self.data.centerline.write() = Some(stored);
    }

    /// A centerline counts as user-supplied purely by having a non-zero id, since
    /// the computed one never does.
    pub fn has_custom_centerline(&self) -> bool {
        matches!(&*self.data.centerline.read(), Some(line) if line.id() != INVAL_ID)
    }

    /// Discards a computed centerline, keeping a user-supplied one.
    pub fn reset_cache(&self) {
        if !self.has_custom_centerline() {
            *self.data.centerline.write() = None;
        }
    }

    /// The lanelet outline: the left bound followed by the reversed right bound.
    pub fn boundary(&self) -> Vec<LineString> {
        vec![self.left_bound(), self.right_bound().invert()]
    }

    pub fn is_same_data(&self, other: &Lanelet) -> bool {
        Arc::ptr_eq(&self.data, &other.data)
    }

    /// Upstream's `operator==`: same storage *and* same direction.
    pub fn is_same_view(&self, other: &Lanelet) -> bool {
        self.is_same_data(other) && self.inverted == other.inverted
    }

    pub fn identity(&self) -> usize {
        (Arc::as_ptr(&self.data) as usize) | usize::from(self.inverted)
    }

    /// `str(lanelet)`:
    /// `[id: 1, inverted, left id: 2 (inverted), right id: 3 (inverted)]`.
    pub fn to_display_string(&self) -> String {
        let (left, right) = (self.left_bound(), self.right_bound());
        let mark = |line: &LineString| if line.is_inverted() { " (inverted)" } else { "" };
        format!(
            "[id: {}{}, left id: {}{}, right id: {}{}]",
            self.id(),
            if self.inverted { ", inverted" } else { "" },
            left.id(),
            mark(&left),
            right.id(),
            mark(&right),
        )
    }

    /// `repr(lanelet)`, given the bounds and optional arguments already rendered.
    pub fn repr(
        &self,
        class_name: &str,
        left_repr: &str,
        right_repr: &str,
        attributes_repr: &str,
        regelems_repr: &str,
    ) -> String {
        make_repr(
            class_name,
            &[
                self.id().to_string(),
                left_repr.to_owned(),
                right_repr.to_owned(),
                attributes_repr.to_owned(),
                regelems_repr.to_owned(),
            ],
        )
    }
}

impl PartialEq for Lanelet {
    fn eq(&self, other: &Self) -> bool {
        self.is_same_view(other)
    }
}

impl Eq for Lanelet {}

impl std::fmt::Debug for Lanelet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_display_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::point::Point;

    fn line(id: Id, points: &[(f64, f64)]) -> LineString {
        LineString::new(
            id,
            points
                .iter()
                .map(|&(x, y)| Point::new(INVAL_ID, x, y, 0.0, AttributeMap::new()))
                .collect(),
            AttributeMap::new(),
        )
    }

    fn lanelet() -> Lanelet {
        Lanelet::new(
            1,
            line(2, &[(0.0, 1.0), (1.0, 1.0)]),
            line(3, &[(0.0, -1.0), (1.0, -1.0)]),
            AttributeMap::new(),
        )
    }

    #[test]
    fn inverting_swaps_and_reverses_the_bounds() {
        let ll = lanelet();
        let inverted = ll.invert();

        assert_eq!(inverted.left_bound().id(), 3);
        assert!(inverted.left_bound().is_inverted());
        assert_eq!(inverted.right_bound().id(), 2);
        assert!(inverted.right_bound().is_inverted());
        assert_ne!(ll, inverted);
        assert_eq!(ll, inverted.invert());
    }

    #[test]
    fn writing_a_bound_through_an_inverted_handle_is_mapped_back() {
        let ll = lanelet();
        let replacement = line(9, &[(0.0, -5.0), (1.0, -5.0)]);

        ll.invert().set_left_bound(replacement);

        // Assigning the left bound of the inverse assigns the underlying right one.
        assert_eq!(ll.right_bound().id(), 9);
        assert_eq!(ll.left_bound().id(), 2);
    }

    #[test]
    fn the_centerline_is_cached_and_reset_when_a_bound_changes() {
        let ll = lanelet();
        let first = ll.centerline();
        assert!(ll.centerline().is_same_data(&first), "second call is cached");
        assert!(!ll.has_custom_centerline());

        ll.set_left_bound(line(4, &[(0.0, 3.0), (1.0, 3.0)]));
        assert!(!ll.centerline().is_same_data(&first), "cache was invalidated");
    }

    #[test]
    fn assigning_the_same_bound_does_not_invalidate_the_cache() {
        let ll = lanelet();
        let first = ll.centerline();
        ll.set_left_bound(ll.left_bound());
        assert!(ll.centerline().is_same_data(&first));
    }

    #[test]
    fn a_centerline_with_an_id_counts_as_custom_and_survives_a_reset() {
        let ll = lanelet();
        ll.set_centerline(line(42, &[(0.0, 0.0), (1.0, 0.0)]));
        assert!(ll.has_custom_centerline());

        ll.set_left_bound(line(4, &[(0.0, 3.0), (1.0, 3.0)]));
        assert_eq!(ll.centerline().id(), 42, "a custom centerline is kept");

        // A centerline without an id is indistinguishable from a computed one.
        let other = lanelet();
        other.set_centerline(line(INVAL_ID, &[(0.0, 0.0)]));
        assert!(!other.has_custom_centerline());
    }

    #[test]
    fn the_centerline_of_an_inverted_lanelet_runs_backwards() {
        let ll = lanelet();
        let forward: Vec<f64> = ll.centerline().points().iter().map(|p| p.x()).collect();
        let backward: Vec<f64> = ll.invert().centerline().points().iter().map(|p| p.x()).collect();
        assert_eq!(backward, forward.iter().rev().copied().collect::<Vec<_>>());
    }

    #[test]
    fn text_format_marks_every_inversion() {
        let ll = lanelet();
        assert_eq!(
            ll.to_display_string(),
            "[id: 1, left id: 2, right id: 3]"
        );
        assert_eq!(
            ll.invert().to_display_string(),
            "[id: 1, inverted, left id: 3 (inverted), right id: 2 (inverted)]"
        );
    }
}
