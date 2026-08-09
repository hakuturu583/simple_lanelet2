# simple_lanelet2

A reimplementation of the [Lanelet2](https://github.com/fzi-forschungszentrum-informatik/Lanelet2)
Python API with a Rust core and PyO3 bindings.

The goal is a **drop-in replacement**: `import lanelet2` keeps working, unchanged, but
installation is a single wheel with no Boost, no GeographicLib and no C++ toolchain.

```python
import lanelet2
from lanelet2.core import Point3d, LineString3d, Lanelet, getId

left = LineString3d(getId(), [Point3d(getId(), 0, 0, 0), Point3d(getId(), 1, 0, 0)])
right = LineString3d(getId(), [Point3d(getId(), 0, 1, 0), Point3d(getId(), 1, 1, 0)])
lanelet = Lanelet(getId(), left, right)
```

**Status.** All seven submodules are implemented. The library exposes **100% of
the reference's public API** — 633 names across every module and class — and
upstream's own test suite passes against it **unmodified**, in both modes.

## Bug-compatibility mode

Upstream has a number of outright defects — a `__hash__` that contradicts `__eq__`, a
misnamed keyword argument on `Origin`, a routing filter that silently does nothing.
By default this library **fixes** them. Setting `LANELET2_BUG_COMPAT` restores
upstream's behaviour exactly:

```bash
LANELET2_BUG_COMPAT=1 python my_script.py   # byte-for-byte upstream behaviour
python my_script.py                         # repaired behaviour (default)
```

The flag is read once at import time and is reported as `lanelet2.BUG_COMPAT`.
Every switched behaviour is listed in [`docs/DIVERGENCE.md`](docs/DIVERGENCE.md) and
enforced by the test harness.

## Container image

A runtime image is published to GHCR for `linux/amd64` and `linux/arm64`:

```bash
# an interpreter with `import lanelet2` already working
docker run --rm -it ghcr.io/hakuturu583/simple_lanelet2:latest

# the working directory is /work, so a map on the host is one mount away
docker run --rm -v "$PWD:/work" ghcr.io/hakuturu583/simple_lanelet2:latest python3 -c "
import lanelet2
from lanelet2.projection import UtmProjector
from lanelet2.io import Origin
m = lanelet2.io.load('map.osm', UtmProjector(Origin(49.0, 8.4)))
print(len(m.laneletLayer), 'lanelets')"
```

Tags are `latest`, `X.Y.Z`, `X.Y` and `X` from releases, plus `main` and
`sha-<commit>` from the tip of the default branch.

It carries a CPython and the wheel and nothing else: no Rust toolchain, no source
tree, no pip — around 40 MB unpacked, of which the extension module is 2.4 MB and
almost all of the rest is CPython itself. That is enforced rather than intended; the
build fails if `pip`, `cargo`, `rustc` or a C compiler can be found in the image, and
its smoke test exercises the map, geometry, projection, routing and I/O paths through
the extension before anything is pushed.

[`Dockerfile`](Dockerfile) builds it in three stages: a Rust toolchain produces one
musl wheel, a second stage unpacks that wheel against the interpreter that will load
it, and the third takes the finished filesystem onto `scratch` — `rm` in a layer
above a base image writes a whiteout and ships the deleted bytes anyway, so copying
what survived is what actually makes it smaller. The image runs as an unprivileged
user with `/work` as its working directory.

The wheel inside it is musllinux and is built only for this image; the manylinux
wheels on PyPI are the ones to install with `pip`.

## Map viewer

**<https://hakuturu583.github.io/simple_lanelet2/>** — drop a Lanelet2 `.osm` on the
page and look at it. The file is parsed and styled by this library compiled to
WebAssembly, in the tab; nothing is uploaded anywhere.

It is a component rather than a page, so it embeds: a Foxglove panel extension or
any application that runs your JavaScript imports `web/viewer.js` and mounts
`<lanelet2-viewer>` — shadow DOM, `ResizeObserver`, no globals, a real `destroy()`
— and a host that can only place a URL, such as a wandb HTML panel or a notebook
cell, frames `web/embed.html` and drives it over `postMessage`.
[`web/EMBEDDING.md`](web/EMBEDDING.md) has both, and
[both are live](https://hakuturu583.github.io/simple_lanelet2/embed-example.html).

None of this is part of the drop-in Lanelet2 surface. Upstream has no viewer to be
compatible with, so `ll2-viz` and `ll2-wasm` are outside the compatibility claim
and have no Python bindings — nothing in the wheel imports them, and the diff
harness does not touch them.

Two crates carry it, and neither needs a browser:

- [`ll2-viz`](crates/ll2-viz) turns a `LaneletMap` into a `Scene` — a flat list of
  styled polylines and polygons in map coordinates — and renders one to SVG. It
  classifies primitives the way the Lanelet2 tagging document and Autoware's
  `lanelet2_extension` describe them, so `line_thin`/`dashed` comes out as a dashed
  hairline, `stop_line` as a red bar, a `crosswalk` lanelet in its own colour.
- [`ll2-wasm`](crates/ll2-wasm) hands a scene across the WebAssembly boundary as
  typed arrays, which is what lets the demo's `<canvas>` draw a city-scale map from
  a few dozen calls per frame.

```rust
// a .osm to an .svg, no browser involved
let svg = ll2_viz::svg_from_osm(&text, &ll2_viz::VizOptions::default())?;
```

```bash
just svg tests/data/mapping_example.osm map.svg   # the same thing from a shell
just scene tests/data/mapping_example.osm s.json  # or the scene itself, to draw elsewhere
just web-serve                                    # the demo, on localhost:8000
```

A `Scene` being renderer-agnostic is meant literally: the SVG writer, the demo's
`<canvas>` and anything else are peers, and the third one costs a serialiser.
[`examples/scene2json.rs`](crates/ll2-viz/examples/scene2json.rs) is that
serialiser — it writes the styled shapes, both palettes and the layer table as
JSON, which is enough to draw the map somewhere this repository has never heard
of.

The viewer differs from `lanelet2.io.load` in one deliberate way. It has no origin
to be given, so it takes the median of the file's own latitudes and longitudes and
projects through UTM from there — and if that collapses the map to a point while the
file's `local_x`/`local_y` tags do not, it uses those instead. That is the case for
Autoware maps written with a placeholder `lat`/`lon`, which would otherwise draw as
nothing. The Python API makes no such guess; see [Autoware maps](#autoware-maps)
below. Both behaviours are reachable from the viewer's *Coordinates* control.

[`web/README.md`](web/README.md) covers building and deploying it.

## Verification

Compatibility is not asserted, it is measured. Every case in `tests/cases/` is run
three ways and the JSON-Lines output is compared:

| run | interpreter | environment |
| --- | --- | --- |
| `REF` | `.venv-ref` | the real `lanelet2==1.2.3` from PyPI |
| `COMPAT` | `.venv` | ours, `LANELET2_BUG_COMPAT=1` |
| `FIXED` | `.venv` | ours, default |

`REF` and `COMPAT` must agree exactly. `COMPAT` and `FIXED` must differ in exactly
the places listed in `tests/compat_matrix.toml` — no more, and no fewer, so neither
an accidental behaviour change nor an unwired repair can slip through.

```bash
just venvs           # create both virtualenvs
just build           # build and install into .venv
just diff            # run the harness
just upstream-tests  # upstream's own tests, unmodified, in both modes
just test-rust       # the Rust unit tests
```

What the harness checks, beyond "it runs":

- the 594 KB example map from the Lanelet2 repository loads, and writing it back
  reproduces the reference's file **byte for byte** — every node, way, relation and
  tag, in the same order — and a second pass is identical again;
- every lanelet's full centerline matches, on that map and on forty procedurally
  generated shapes chosen to exercise the parts of the algorithm a rectangular
  lanelet never reaches;
- the projections agree with GeographicLib to **7e-15 m** across zone edges, the
  Norway and Svalbard zone irregularities, both hemispheres and the antimeridian;
- the traffic-rule tables are swept exhaustively — every participant against every
  way subtype, both directions, both locations — rather than spot-checked;
- the routing graph's whole edge list is compared before any query is.

## Autoware maps

`autoware_lanelet2_extension`'s regulatory elements and its transverse Mercator
projector are provided under the upstream import path, in this same wheel:

```python
import lanelet2
import autoware_lanelet2_extension_python.regulatory_elements  # registers the subtypes
from autoware_lanelet2_extension_python.projection import TransverseMercatorProjector
```

Importing `regulatory_elements` is what makes `road_marking`, `crosswalk`,
`detection_area` and the rest resolvable, and it makes `traffic_light` resolve to
`AutowareTrafficLight`. Before that import a map carrying them is refused, exactly as
stock Lanelet2 refuses it. That is upstream's behaviour, not an accident of packaging:
registration there happens when the extension's shared library loads. Note that it is
process-wide and cannot be undone, so an unrelated module importing the extension
changes what `lanelet2.io.load` produces from that point on.

**Coordinates come from latitude and longitude, not from `local_x`/`local_y`.**
Autoware's C++ `AutowareOsmParser` prefers those tags, and a real Autoware map is full
of them — the Nishi-Shinjuku example carries 36,936. That parser is not reachable from
Python in the reference either, so `lanelet2.io.load` behaves the same in both; our
output on that map is byte-identical to the reference's. But it does mean the numbers
differ from what Autoware's own C++ tooling produces.

`utility.query` and `utility.utilities` are provided apart from their ROS-dependent
halves, which are defined but raise when called — upstream imports `geometry_msgs` and
`rclpy` at module top, so its versions cannot be imported at all without ROS. Several
of upstream's own bindings do not work; those are repaired here and reproduced under
`LANELET2_BUG_COMPAT=1`. See [`docs/DIVERGENCE.md`](docs/DIVERGENCE.md) for the list.

## CI

Four jobs, arranged around the compatibility claim rather than around the test suite:

| job | what it proves | needs |
|---|---|---|
| `check` | it builds, lints, and the wheel installs into an empty venv and imports | nothing |
| `diff` | 20 cases against the PyPI `lanelet2==1.2.3`, plus upstream's vendored tests | a wheel |
| `upstream` | upstream's *own* tests, cloned at HEAD each run, unmodified, both modes | network |
| `oracle` | 31 cases, including the Autoware extension and the two-reference skew check | pixi + colcon |

The map viewer likewise has its own workflow, [`pages.yml`](.github/workflows/pages.yml):
it builds the wasm module, instantiates it in node and puts the example map through
it, drives the demo and both embedding routes in a real Chromium, and deploys to
GitHub Pages from `main`. Pull requests build and test without deploying. Both test
scripts earn their place: `cargo test` runs on the host, where wasm-bindgen's glue is
inert, so it cannot catch a module that fails to instantiate — and instantiating is
not the same as a canvas in a shadow root receiving a wheel event.

The container image has its own workflow rather than a fifth job here, because it
costs a full Rust build per architecture and is wanted on a different set of events
— see [`docker.yml`](.github/workflows/docker.yml). It builds on any pull request
that touches the crates, the Python package or the Dockerfile, and publishes only
from `main` and from releases.

One case is not in that count. `1150_aw_map` loads a real 10.5 MB Autoware map and
checks it writes back byte for byte; the map it was developed against is CC BY-NC
licensed, so it can neither be vendored here nor fetched by CI. Point
`SIMPLE_LL2_AW_MAP` at a real Autoware map to run it. `1160_aw_synthetic_map` covers
the same behaviour on a map small enough to write inline, and does run in CI.

One caveat on what `upstream` proves. Lanelet2's own tests are substantive; the
Autoware extension's Python tests are import smoke tests, and its real suite is C++
gtest that cannot run against a Python implementation. The extension is verified by the
`# ORACLE: aw` diff cases instead — see [`tests/upstream-awext/README.md`](tests/upstream-awext/README.md).

`upstream` fetching at HEAD rather than a pin is deliberate. A vendored copy proves
compatibility with whatever upstream looked like the day it was copied; fetching proves
it against upstream as it stands, and a test they change becomes a signal rather than a
silent drift. It runs on a schedule too, since upstream moves without anyone touching
this repository. Locally: `just upstream-fresh`.

## Licence

BSD-3-Clause, matching upstream Lanelet2. See [`NOTICE`](NOTICE) for vendored assets.
