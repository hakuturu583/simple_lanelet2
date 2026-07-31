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

## Licence

BSD-3-Clause, matching upstream Lanelet2. See [`NOTICE`](NOTICE) for vendored assets.
