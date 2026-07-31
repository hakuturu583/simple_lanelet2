# Divergences from upstream Lanelet2

Two separate things live in this document, and they are not the same:

1. **Repaired defects** — upstream bugs that this library fixes by default. Setting
   `LANELET2_BUG_COMPAT=1` restores upstream's behaviour exactly, so nothing here
   costs you compatibility if you need it. Each is enforced by
   `tests/compat_matrix.toml`, which the harness checks in both directions.
2. **Out of scope** — functionality we have declined to implement at all, in either
   mode. These are listed in `tests/divergence.toml`.

Behaviour that is merely *surprising* but has no defensible alternative is
reproduced in both modes and listed at the end.

---

## 1. Repaired defects

The switch for each is a named predicate in
[`crates/ll2-core/src/compat.rs`](../crates/ll2-core/src/compat.rs), which also
carries the upstream `file:line` it reproduces.

Repairs come in two kinds. **Runtime** switches are a branch inside a method.
**Registration** switches change a method's signature, so the choice is made once
when the Python classes are registered — which is why the flag is latched at import
time and re-reading `os.environ` afterwards has no effect.

| # | Upstream behaviour | Default (repaired) | Kind |
| --- | --- | --- | --- |
| 1 | `Origin`'s keywords are registered as `(lat, lon, lon)`: the third is misnamed, so `lon=` silently fills the *altitude* as well and `alt=` is accepted and ignored — `Origin(lat=49, lon=8.4)` ends up 8.4 m above the ellipsoid | third keyword named `alt` | registration |
| 2 | Only `__div__` is registered on basic points; Python 3 never calls it, so `p / 2` raises `TypeError` | `__truediv__` implemented | registration |
| 3 | `SpeedLimitInformation`'s first parameter is named `speedLimitMPS` but is interpreted as km/h, and the two-argument constructor is unreachable | only the no-argument form exists, so the confusion cannot arise | registration |
| 4 | `reachableSet`'s third keyword argument is spelled `RoutingCostId` | `routingCostId` | registration |
| 5 | The cost-limit overload of `possiblePaths` declares a fifth keyword argument the function does not accept | removed | registration |
| 5a | `TrafficSignsWithType.trafficSigns` raises `TypeError`: no Python class is registered for the vector behind it, so the property is unusable | returns the list of signs | runtime |
| 6 | `repr(ConstArea(...))` claims to be an `Area` | `ConstArea(...)` | runtime |
| 7 | `SpeedLimit.__init__` is bound to `TrafficSign::make`, so it constructs a `TrafficSign` | constructs a `SpeedLimit` | runtime |
| 8 | The `withoutConflicting` edge filter computes `allRelations() \| ~Conflicting` = `0xFF`, matching every relation — it does nothing | conflicting edges are actually excluded | runtime |
| 9 | `canPass(Area, Area)` guards with `!canPass(from) && canPass(to)`, almost certainly a typo | `\|\|` | runtime |
| 10 | `followingRelations` dereferences an optional without checking it | missing entries are skipped | runtime |
| 11 | `__hash__` hashes the id only, contradicting the identity-based `__eq__` and violating Python's hash/eq contract | hashes identity, so `__hash__` and `__eq__` agree | runtime |
| 12 | `ConstPoint2d::basicPoint()` returns a mutable internal reference, so you can write through a `Const` handle | `Const` types return a read-only view | runtime |
| 13 | The shipped `.pyi` stubs disagree with the actual bindings in a dozen places | generated from the implementation | — |

Item 13 has no runtime effect and therefore no `compat_matrix.toml` entry.

## 2. Out of scope

Not implemented in either mode. Listed with reasons in `tests/divergence.toml`.

- **`.bin` I/O.** Upstream's binary format is a Boost.Serialization archive, whose
  layout depends on the compiler and Boost version. There is nothing stable to
  target. `load`/`write` on a `.bin` path raise `RuntimeError`.
- **`exportGraphML` / `exportGraphViz`.** Reproducing them means reproducing
  Boost's GraphML and GraphViz writers byte for byte, which buys nothing for map
  handling.
- **Python-subclassable `RoutingCost`.** `RoutingCostDistance` and
  `RoutingCostTravelTime` are provided; deriving your own cost model in Python is
  not supported.
- **`lanelet2_validation`.** Never had Python bindings upstream either.
- **Mutating a linestring while iterating it.** `__iter__` snapshots the points,
  where upstream yields live references.

## 3. Additions

- **`lanelet2.BUG_COMPAT`** (`bool`) — whether bug-compatibility mode is active in
  this process. Upstream has no such attribute.
- **`RoutingGraph.edges()`** — the whole edge list as
  `(fromId, fromInverted, toId, toInverted, relation, costId, cost)`. Not part of
  upstream's surface; it exists so that the graph *builder* can be tested directly
  rather than through a query's view of it.
- **`TrafficRules.laneChangeType(boundary, virtualIsPassable=False)`** — the
  boundary rule the routing graph consults, exposed because it is cheap and makes
  the lane-change tables inspectable.

## 3a. A note on versions

The Lanelet2 source checked out for reference is **not the same version as the
`lanelet2==1.2.3` wheel** this library is compatible with, and the two disagree on
real behaviour: in the wheel a `bus_lane` carries a road's speed limit, while the
newer source has no such entry and yields zero. Where they differ, the wheel wins,
because the wheel is what compatibility is measured against. Several tables in
`ll2-traffic-rules` and the whole acceptance matrix in `accept.rs` were therefore
*measured* against the wheel rather than transcribed from the C++.

## 4. Reproduced in both modes

Surprising, but intended upstream, and with no better answer available:

- `makeRepr` drops empty string arguments entirely rather than emitting an empty
  slot. This is the mechanism that produces `Point3d(1000, 1, 2, 3)` instead of
  `Point3d(1000, 1, 2, 3, )` when the attribute map is empty.
- Doubles in `__repr__`/`__str__` use C++ `ostream` default formatting — six
  significant digits, `0` rather than `0.0`. `ArcCoordinates.__repr__` uses
  `std::to_string`, which is six *decimal places*.
- An unknown `(location, subtype)` combination yields a speed limit of 0 km/h.
  `RoutingCostTravelTime` treats that as a sentinel for infinite speed. There is no
  defensible alternative value.
- `AllWayStop.stopLines()` is not index-aligned with `lanelets()`; only lanelets
  that actually have a stop line contribute an entry.
- Every C++ exception surfaces as `RuntimeError`, because upstream registers no
  exception translators. The one exception is `AttributeMap.__getitem__`, which
  raises `KeyError`.
- `leftOf`, `rightOf` and `follows` compare object *identity*, not geometry. Two
  coincident but distinct linestrings are not "left of" each other.
