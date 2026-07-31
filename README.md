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

> **Status: early.** The package layout, the bug-compatibility switch and the
> verification harness are in place; the API surface is being filled in phase by
> phase. Run `just coverage` to see how much of the reference API is implemented.

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
just venvs      # create both virtualenvs
just build      # build and install into .venv
just diff       # run the harness
just coverage   # API-surface burn-down
```

## Licence

BSD-3-Clause, matching upstream Lanelet2. See [`NOTICE`](NOTICE) for vendored assets.
