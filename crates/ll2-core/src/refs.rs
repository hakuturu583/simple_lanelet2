//! Shared, independently-lockable cells for the two things upstream hands Python a
//! live interior reference to.
//!
//! `primitive.attributes`, `bbox.min`/`bbox.max` and `point.basicPoint()` are all
//! `return_internal_reference` in the Boost.Python bindings, and upstream's own test
//! suite mutates through them — `primitive.attributes["k"] = "v"` changes
//! `len(primitive.attributes)`. Handing Python a copy would be a real behavioural
//! divergence, so these must be live proxies.
//!
//! Rather than an enum over every possible owner (which would have to grow with each
//! new primitive type), each owner simply *holds* one of these shared cells. A proxy
//! object then holds a clone of the same `Arc`, which both keeps the storage alive —
//! exactly what `return_internal_reference`'s lifetime tie achieves — and makes every
//! write visible to the owner with no back-pointer bookkeeping.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::attribute::AttributeMap;

/// A shared 3D coordinate triple.
///
/// Points always store three coordinates even when viewed in 2D, so a `Point2d` and
/// a `Point3d` over the same data share one of these. That is what makes
/// `to2D(p).basicPoint().x = 5` write the first component and leave `z` untouched,
/// matching upstream's Eigen `Map` over the first two of three doubles.
pub type Coords = Arc<RwLock<[f64; 3]>>;

/// A shared attribute map.
pub type Attrs = Arc<RwLock<AttributeMap>>;

pub fn coords(x: f64, y: f64, z: f64) -> Coords {
    Arc::new(RwLock::new([x, y, z]))
}

pub fn attrs(map: AttributeMap) -> Attrs {
    Arc::new(RwLock::new(map))
}

pub fn empty_attrs() -> Attrs {
    Arc::new(RwLock::new(AttributeMap::new()))
}

/// A read-only view of a coordinate cell.
///
/// Upstream lets you write through `ConstPoint2d::basicPoint()` because the binding
/// returns an internal reference regardless of constness. That hole is closed by
/// default and reopened in bug-compatibility mode, which is why the mutability of a
/// coordinate view is carried by the view rather than by the cell.
#[derive(Clone)]
pub struct CoordView {
    cell: Coords,
    mutable: bool,
}

impl CoordView {
    pub fn new(cell: Coords, mutable: bool) -> Self {
        CoordView { cell, mutable }
    }

    pub fn is_mutable(&self) -> bool {
        self.mutable
    }

    pub fn get(&self, index: usize) -> f64 {
        self.cell.read()[index]
    }

    /// Returns `false` if this view is read-only, leaving the cell untouched.
    #[must_use]
    pub fn set(&self, index: usize, value: f64) -> bool {
        if !self.mutable {
            return false;
        }
        self.cell.write()[index] = value;
        true
    }

    pub fn xyz(&self) -> [f64; 3] {
        *self.cell.read()
    }

    pub fn cell(&self) -> &Coords {
        &self.cell
    }

    /// Whether two views address the same storage.
    pub fn is_same_cell(&self, other: &CoordView) -> bool {
        Arc::ptr_eq(&self.cell, &other.cell)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_over_one_cell_see_each_others_writes() {
        let cell = coords(1.0, 2.0, 3.0);
        let a = CoordView::new(cell.clone(), true);
        let b = CoordView::new(cell, true);

        assert!(a.set(0, 9.0));
        assert_eq!(b.get(0), 9.0);
        assert_eq!(b.xyz(), [9.0, 2.0, 3.0]);
        assert!(a.is_same_cell(&b));
    }

    #[test]
    fn a_read_only_view_refuses_writes_without_disturbing_the_cell() {
        let cell = coords(1.0, 2.0, 3.0);
        let writable = CoordView::new(cell.clone(), true);
        let readonly = CoordView::new(cell, false);

        assert!(!readonly.set(0, 9.0));
        assert_eq!(writable.get(0), 1.0);
    }
}
