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

static CURRENT_ID: AtomicI64 = AtomicI64::new(FIRST_ID);

/// Returns a fresh, globally unique id.
pub fn get_id() -> Id {
    CURRENT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Reserves `id` so that [`get_id`] will never return it.
///
/// Upstream raises the counter to `id + 1` if `id` is at or above the current value,
/// and does nothing otherwise.
pub fn register_id(id: Id) {
    CURRENT_ID.fetch_max(id.saturating_add(1), Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_starts_at_1000_and_register_raises_it() {
        // Runs in its own process-wide static, so assert relatively rather than
        // against absolute values (other tests in this binary also draw ids).
        let a = get_id();
        assert!(a >= FIRST_ID);
        assert_eq!(get_id(), a + 1);

        register_id(50_000);
        assert_eq!(get_id(), 50_001);

        // Registering a lower id must not rewind the counter.
        register_id(10);
        assert_eq!(get_id(), 50_002);
    }
}
