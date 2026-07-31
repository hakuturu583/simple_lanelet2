//! Bug-compatibility mode.
//!
//! Upstream Lanelet2 has a number of outright defects (misnamed keyword arguments,
//! a `__hash__` that contradicts `__eq__`, a filter predicate that is a no-op, ...).
//! By default this library fixes them. Setting the environment variable
//! `LANELET2_BUG_COMPAT` to a truthy value restores the upstream behaviour exactly,
//! which is what the cross-implementation diff tests run against.
//!
//! The flag is read **once, at module initialisation**, and is deliberately not
//! re-read afterwards: some of the switched behaviours are method signatures, and
//! those are fixed when the Python classes are registered.
//!
//! Every switched behaviour goes through a named predicate in this module. Do not
//! call `std::env::var` anywhere else.

use std::sync::OnceLock;

static BUG_COMPAT: OnceLock<bool> = OnceLock::new();

/// The name of the environment variable that enables bug-compatibility mode.
pub const ENV_VAR: &str = "LANELET2_BUG_COMPAT";

fn parse_flag(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Whether upstream-bug-compatible behaviour is enabled.
///
/// Reads `LANELET2_BUG_COMPAT` on first call and caches the result forever.
pub fn bug_compat() -> bool {
    *BUG_COMPAT.get_or_init(|| std::env::var(ENV_VAR).as_deref().map(parse_flag).unwrap_or(false))
}

/// Force the flag to a specific value. Only useful in tests; the first call wins,
/// so this must run before any other `bug_compat()` call in the process.
pub fn force_for_test(value: bool) -> bool {
    *BUG_COMPAT.get_or_init(|| value)
}

// ---------------------------------------------------------------------------
// Named predicates, one per switched behaviour.
//
// Each carries the upstream file:line it reproduces. Keeping them named rather
// than inlining `bug_compat()` makes every switch point greppable and makes the
// compat matrix in tests/compat_matrix.toml auditable against the source.
// ---------------------------------------------------------------------------

/// Upstream `HashBase` hashes the id only, which contradicts `operator==`
/// (identity-based). Fixed mode hashes identity so `__hash__`/`__eq__` agree.
///
/// Upstream: `lanelet2_core/include/lanelet2_core/primitives/Primitive.h:328-330`
pub fn hash_by_id_only() -> bool {
    bug_compat()
}

/// Upstream `makeAreaRepr` hardcodes the display name to `Area`, so
/// `repr(ConstArea(...))` claims to be an `Area`.
///
/// Upstream: `lanelet2_python/python_api/core.cpp:68-76`
pub fn const_area_repr_says_area() -> bool {
    bug_compat()
}

/// Upstream binds `SpeedLimit.__init__` to `TrafficSign::make`, so constructing a
/// `SpeedLimit` from Python actually yields a `TrafficSign`.
///
/// Upstream: `lanelet2_python/python_api/core.cpp:1388`
pub fn speed_limit_ctor_makes_traffic_sign() -> bool {
    bug_compat()
}

/// Upstream's `withoutConflicting` filter computes `allRelations() | ~Conflicting`,
/// which is `0xFF` and therefore matches every relation — the filter does nothing.
///
/// Upstream: `lanelet2_routing/include/lanelet2_routing/internal/Graph.h:164`
pub fn without_conflicting_is_noop() -> bool {
    bug_compat()
}

/// Upstream guards `canPass(Area, Area)` with `!canPass(from) && canPass(to)`,
/// almost certainly a typo for `||`.
///
/// Upstream: `lanelet2_traffic_rules/src/GenericTrafficRules.cpp:238`
pub fn area_to_area_uses_and_guard() -> bool {
    bug_compat()
}

/// Upstream `RoutingGraph::followingRelations` dereferences an optional without
/// checking it. We raise instead of invoking UB; fixed mode skips the entry.
///
/// Upstream: `lanelet2_routing/src/RoutingGraph.cpp:456`
pub fn following_relations_unchecked() -> bool {
    bug_compat()
}

/// Upstream lets you mutate a point through `ConstPoint2d::basicPoint()` because
/// the binding returns an internal reference regardless of constness.
///
/// Upstream: `lanelet2_python/python_api/core.cpp:733, 763`
pub fn const_basic_point_is_mutable() -> bool {
    bug_compat()
}

/// Upstream never registers a Python class for the vector type behind
/// `TrafficSignsWithType.trafficSigns`, so reading the property raises
/// `TypeError: No Python class registered for C++ class std::vector<...>`.
/// The property is simply unusable there.
///
/// Upstream: `lanelet2_python/python_api/core.cpp:1320`
pub fn traffic_signs_property_unreadable() -> bool {
    bug_compat()
}

// --- registration-time switches -------------------------------------------
// These change method signatures, so they are consulted when classes are
// registered rather than when methods are called.

/// Upstream registers `Origin`'s keywords as `(lat, lon, lon)` — the third is
/// misnamed. The consequence is worse than a rejected call: `lon=` fills *both*
/// the longitude and the altitude, and an `alt=` keyword is silently ignored. So
/// `Origin(lat=49, lon=8.4)` yields an altitude of 8.4 metres.
///
/// Upstream: `lanelet2_python/python_api/io.cpp:75`
pub fn origin_alt_kwarg_is_named_lon() -> bool {
    bug_compat()
}

/// Upstream only registers `__div__`, which Python 3 never calls, so `p / 2`
/// raises `TypeError` on every basic point type.
///
/// Upstream: `lanelet2_python/python_api/core.cpp:513-541`
pub fn basic_point_div_only() -> bool {
    bug_compat()
}

/// Upstream names `SpeedLimitInformation`'s first parameter `speedLimitMPS` but
/// interprets it as km/h.
///
/// Upstream: `lanelet2_python/python_api/traffic_rules.cpp`
pub fn speed_limit_info_kwarg_is_mps() -> bool {
    bug_compat()
}

/// Upstream spells `reachableSet`'s third keyword argument `RoutingCostId`.
///
/// Upstream: `lanelet2_python/python_api/routing.cpp`
pub fn reachable_set_kwarg_is_capitalised() -> bool {
    bug_compat()
}

/// Upstream's cost-limit overload of `possiblePaths` declares a fifth keyword
/// argument `routingCostId` that the underlying function does not take.
///
/// Upstream: `lanelet2_python/python_api/routing.cpp`
pub fn possible_paths_has_bogus_kwarg() -> bool {
    bug_compat()
}

#[cfg(test)]
mod tests {
    use super::parse_flag;

    #[test]
    fn truthy_values() {
        for v in ["1", "true", "TRUE", "Yes", "on", " on "] {
            assert!(parse_flag(v), "{v:?} should be truthy");
        }
    }

    #[test]
    fn falsy_values() {
        for v in ["", "0", "false", "no", "off", "2", "maybe"] {
            assert!(!parse_flag(v), "{v:?} should be falsy");
        }
    }
}
