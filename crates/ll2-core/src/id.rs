//! Primitive ids and the global id counter.
//!
//! Ported from `lanelet2_core/src/LaneletMap.cpp:881-891`.

use std::sync::atomic::{AtomicI64, Ordering};

/// Lanelet2 ids are signed 64-bit. Negative ids are the JOSM "new object" convention.
pub type Id = i64;

/// The id of a primitive that is not (yet) part of a map.
pub const INVAL_ID: Id = 0;

/// Upstream starts the counter at 1000 so that hand-written test ids below that
/// never collide with generated ones.
const FIRST_ID: Id = 1000;

/// The counter behind [`get_id`] and [`register_id`], a type of its own so tests
/// can exercise it on an instance no other test draws from.
struct IdCounter(AtomicI64);

impl IdCounter {
    const fn new() -> Self {
        IdCounter(AtomicI64::new(FIRST_ID))
    }

    fn next(&self) -> Id {
        self.0.fetch_add(1, Ordering::Relaxed)
    }

    fn reserve(&self, id: Id) {
        self.0.fetch_max(id.saturating_add(1), Ordering::Relaxed);
    }
}

static CURRENT_ID: IdCounter = IdCounter::new();

/// Returns a fresh, globally unique id.
pub fn get_id() -> Id {
    CURRENT_ID.next()
}

/// Reserves `id` so that [`get_id`] will never return it.
///
/// Upstream raises the counter to `id + 1` if `id` is at or above the current value,
/// and does nothing otherwise.
pub fn register_id(id: Id) {
    CURRENT_ID.reserve(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_starts_at_1000_and_register_raises_it() {
        let counter = IdCounter::new();
        assert_eq!(counter.next(), FIRST_ID);
        assert_eq!(counter.next(), FIRST_ID + 1);

        counter.reserve(50_000);
        assert_eq!(counter.next(), 50_001);

        // Reserving a lower id must not rewind the counter.
        counter.reserve(10);
        assert_eq!(counter.next(), 50_002);
    }

    /// The global counter is shared with every other test in this binary, which
    /// draw and register ids concurrently, so only what holds regardless of
    /// them is asserted: nothing at or below a registered id comes out after it.
    #[test]
    fn a_registered_id_is_never_handed_out() {
        let registered = get_id() + 1_000_000;
        register_id(registered);
        assert!(get_id() > registered);
    }
}
