"""A Lanelet2-compatible Python API backed by a Rust core.

Drop-in replacement for the ``lanelet2`` package shipped with
https://github.com/fzi-forschungszentrum-informatik/Lanelet2

Upstream ships seven separate Boost.Python extension modules. We build a single
extension containing seven genuine submodules and register them in ``sys.modules``
here, so ``import lanelet2.core`` and ``from lanelet2.core import Point3d`` work the
same way.

Setting the environment variable ``LANELET2_BUG_COMPAT`` to a truthy value before the
first import restores upstream's known defects exactly; see ``docs/DIVERGENCE.md``.
The flag is latched at import time, so changing ``os.environ`` afterwards has no
effect. Its effective value is available as ``lanelet2.BUG_COMPAT``.
"""

import sys as _sys

from . import _lanelet2 as _native

# Alphabetical, matching upstream's own __init__.py exactly.
__all__ = [
    "core",
    "geometry",
    "io",
    "matching",
    "projection",
    "routing",
    "traffic_rules",
]

#: Whether upstream-bug-compatible behaviour is active in this process.
BUG_COMPAT: bool = _native.BUG_COMPAT

for _name in __all__:
    _submodule = getattr(_native, _name)
    _sys.modules[f"{__name__}.{_name}"] = _submodule
    globals()[_name] = _submodule

del _name, _submodule, _sys
