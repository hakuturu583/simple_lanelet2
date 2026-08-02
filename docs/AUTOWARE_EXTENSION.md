# autoware_lanelet2_extension — measured behaviour

Ground truth for implementing the extension's types, taken by running the reference
rather than by reading its headers. The reference is the ROS overlay reached by
`# ORACLE: aw` (see `tests/runner.py`); reproduce any line here with

```bash
. /home/masaya/workspace/autoware/install/setup.bash && python3 -c '...'
```

**Probing this reference is not safe in one process.** Calling every public accessor
of every class crashes the interpreter — see the segfault below. Probe one call per
subprocess.

## Registration

Each `lib/*.cpp` ends with a static `RegisterRegulatoryElement<T>`, which runs
`registry_[T::RuleName] = factory` when the shared library loads. It is an
**assignment**, not an insert, so `AutowareTrafficLight` (rule name `traffic_light`)
deliberately takes that key over from the stock `TrafficLight`. Importing
`autoware_lanelet2_extension_python.regulatory_elements` is what loads the library;
there is no explicit init call.

Ten kinds are registered. Only eight have Python classes — `BusStopArea`
(`bus_stop_area`) and `Roundabout` (`roundabout`) affect **loading** but are not
exposed, so a map containing them loads with the extension and fails without it.

| Class | rule name | ctor after `(id, attributes)` |
|---|---|---|
| `AutowareTrafficLight` (← `TrafficLight`) | `traffic_light` | `trafficLights, stopLine=None, lightBulbs=[]` |
| `Crosswalk` | `crosswalk` | `crosswalk_lanelet, crosswalk_area, stop_line` |
| `DetectionArea` | `detection_area` | `detectionAreas, stopLine` |
| `NoParkingArea` | `no_parking_area` | `no_parking_areas` |
| `NoStoppingArea` | `no_stopping_area` | `no_stopping_areas, stopLine=None` |
| `RoadMarking` | `road_marking` | `road_marking` |
| `SpeedBump` | `speed_bump` | `speed_bump` |
| `VirtualTrafficLight` | `virtual_traffic_light` | `virtual_traffic_light` |
| `BusStopArea` — no Python class | `bus_stop_area` | — |
| `Roundabout` — no Python class | `roundabout` | — |

## Without the extension

Stock Lanelet2 **rejects** these maps; it does not fall back to a generic element.
Reproduced exactly as of the registry work — on the 10.5 MB Nishi-Shinjuku map both
the reference and we report 91 errors: 33 rejected elements, 57 dangling references.

```
Errors ocurred while parsing Lanelet Map:
	- Error parsing primitive 20: Creating a regulatory element of type road_marking failed: No regulatory element found that implements rule road_marking
	- Error parsing primitive 30: Failed to get id 20 from map
```

## Measured per-class detail

All constructed with `AttributeMap()`; every one stamps
`type=regulatory_element` plus its own `subtype`. Accessors are **methods**, not
properties — only the `trafficLights`/`stopLine` that `AutowareTrafficLight` inherits
from stock `TrafficLight` are properties.

Constness is not uniform, and has to be copied rather than reasoned about:

| Class | accessor | returns |
|---|---|---|
| `RoadMarking` | `roadMarking()` | `ConstLineString3d` |
| `SpeedBump` | `speedBump()` | `ConstPolygon3d` |
| `NoParkingArea` | `noParkingAreas()` | list of **`Polygon3d`** |
| `DetectionArea` | `detectionAreas()` | list of **`Polygon3d`** |
| `DetectionArea` | `stopLine()` | `ConstLineString3d` |
| `Crosswalk` | `crosswalkAreas()` | list of `ConstPolygon3d` |
| `Crosswalk` | `stopLines()` | list of `ConstLineString3d` |

`SpeedBump`'s parameter map reports its `refers` entry as `ConstPolygon3d` while
`NoParkingArea`'s reports `ConstPolygon3d` too — but the accessors differ, so the
distinction lives in the binding, not in the stored parameter.

### Upstream defects to reproduce

- **`NoStoppingArea.__repr__` raises.** It is bound to `NoParkingArea&`
  (`regulatory_elements.cpp:214`), so it does not merely print the wrong name:

  ```
  Boost.Python.ArgumentError: Python argument types in
      NoStoppingArea.__repr__(NoStoppingArea)
  did not match C++ signature:
      __repr__(lanelet::autoware::format_v2::NoParkingArea {lvalue})
  ```

  A test must use `expect_raises`, not a string comparison.

- **`Crosswalk.crosswalkLanelet()` segfaults on an expired reference.** The
  constructor *does* store the lanelet — `refers` holds a `ConstLanelet` — but holds
  it **weakly**, as everywhere else a regulatory element refers back to one. Upstream
  dereferences that without checking, so passing a temporary and then calling the
  accessor crashes the interpreter. (An earlier note here claimed the constructor
  dropped the lanelet; that was inferred from the crash and is wrong.) Keep the
  lanelet alive and the accessor works. We raise on an expired reference instead.

- **`Crosswalk.addCrosswalkArea` writes role `crosswalk`** while `crosswalkAreas()`
  reads `crosswalk_polygon`, so an added area is invisible to the getter.

- **`Crosswalk`'s stop-line loop uses `std::map::insert` per line**, so only the
  first survives when several are given.

- **`VirtualTrafficLight`'s Python constructor cannot succeed.** The documented
  signature takes only the virtual-traffic-light linestring, but construction
  validates the parameter map and raises:

  ```
  RuntimeError: There must be exactly one start_line defined!
  ```

  So the class is reachable only by loading a map. Pin that with `expect_raises`.

## Projections

- **`MGRSProjector(origin=Origin({0,0}))`** — the origin is stored and then
  **ignored**. `forward` is `UTMUPS::Forward` then `MGRS::Forward(precision=0)`,
  returning `x = fmod(utm.x, 1e5)`, `y = fmod(utm.y, 1e5)`, `z = gps.ele`. The grid
  code is cached in a mutable field and a change is warned about on stderr. `reverse`
  with no code set prints an error and returns `{0,0,0}` — it does **not** throw, and
  that path does not preserve `ele`, unlike the code-set path. Extra methods:
  `setMGRSCode`, `isMGRSCodeSet`, `getProjectedMGRSGrid`.
- **`TransverseMercatorProjector(origin, scale_factor=0.9996)`** — Python exposes
  only the one-argument form. `central_meridian_ = origin.position.lon`; `forward`
  subtracts `origin_y_` but **not** `origin_x_`. That asymmetry is harmless rather
  than a bug: at Δλ = 0 the easting is 0 anyway. Do not "fix" it.

  Upstream uses `TransverseMercatorExact`; we have a Krüger series at maxpow 6. Every
  point of a real map sits within ~0.1° of the central meridian, where the truncation
  error is far below double precision — but measure before committing, sweeping Δλ up
  to 10° at several base latitudes.

## Out of scope, and why it matters

`AutowareOsmParser` overrides node coordinates from `local_x`/`local_y` tags when
present, and reads a `MetaInfo` element for versions. The Nishi-Shinjuku map carries
**36,936** `local_x` tags, so reading it through plain `lanelet2.io.load` yields
coordinates derived from lat/lon instead — plausible, but not what Autoware's own
tooling produces. Say so prominently rather than half-implementing it.

The ROS-dependent halves of `utility.query` and `utility.utilities` need
`geometry_msgs`/`rclpy`. Upstream imports those at module top, so the modules are
unimportable without ROS; our shims must import neither and should raise on **call**.

## Byte-identity on the Nishi-Shinjuku map: achieved

`1001_aw_stock_crosscheck` passes with an **empty** `oracle_skew.toml`: the ROS build
and the PyPI wheel agree on reprs, all four projections, and the load-and-write of
the 594 KB example map down to its sha256. So oracle skew is not a live concern, and
the one byte that used to differ on the 10.5 MB map was ours.

It had two causes, both found and fixed rather than tolerated:

1. **`to_degrees()` multiplies by 180/pi; GeographicLib divides by pi/180.** Those
   constants are not exact reciprocals in binary floating point, and the two forms
   disagree by one ulp for about **11% of inputs**. That was enough to move the 11th
   decimal of one latitude across a rounding boundary (...804995 versus ...805002, a
   difference of 7.1e-15 degrees, about 0.8 nm) and lengthen the written file by a
   byte. Fixed in `atan2d`, now the only place a radian value becomes degrees.

2. **The writer emitted relation members the map does not hold.** A rejected
   regulatory element leaves a placeholder attached to its lanelet but absent from
   every layer; upstream reports that *and drops the member*, whereas we reported it
   and wrote it anyway, producing a file that could not be loaded back. Our message
   was also missing the `Error writing primitive N: ` prefix.

The 10.5 MB map now round-trips **byte-identically** -- same sha256, same 91 load
errors, same 58 write errors. Phase 2's gate can stay worded as byte-identity.

### The residual, and why it has no answer

Over a sweep of 24,000 reverse projections, 5 latitudes differ from the PyPI wheel by
exactly 1 ulp. That number is best read next to two others, measured the same way:

| pair | differing | magnitude |
|---|---|---|
| ours vs PyPI wheel 1.2.3 | **5** / 24,000 | 1 ulp |
| ours vs RoboStack 1.2.2 | 1,635 / 24,000 | 1 ulp |
| PyPI 1.2.3 vs RoboStack 1.2.2 | 1,632 / 24,000 | 1 ulp |

**The two reference builds disagree with each other three hundred times more often than
we disagree with the closer of them.** There is no single bit-exact target to hit;
matching one build would mean reproducing its compiler's instruction selection, which
the other build already contradicts.

What the residual is *not* has been checked rather than assumed:

- **The series coefficients are bit-identical.** Recomputed with GeographicLib's own
  `Math::polyval` and the same tables: every `alp` and `bet`, `a1` and `es` match to
  the last digit.
- **`tauf` and `atand` are exact.** Feeding our `taup` to GeographicLib's `Math::tauf`
  returns our `tau` bit for bit, and its `Math::atand` of that returns our latitude.
- **The forward series is bit-exact** over all 24,000 points, using the same Clenshaw
  recurrence and complex arithmetic as the reverse.
- **It is not FMA contraction.** GCC contracts `a*b - c*d` by default and Rust does
  not, which made the complex multiply a good suspect. Writing it with `mul_add` moved
  the count from 5 to 6 — so that is not what the reference does either.

Bisected, the divergence is one ulp in `taup = sin(xip) / r`, upstream of everything
verified above: the rounding of the reverse Clenshaw recurrence itself. Neither map we
round-trip contains an affected point.
