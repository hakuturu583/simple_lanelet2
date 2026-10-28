"""Which classes accept weak references.

Every Boost.Python instance carries a weak-reference slot, so upstream objects can
key a ``WeakKeyDictionary`` or sit in a ``WeakValueDictionary``. The two exceptions
are the enums, which are ``int`` subclasses. Downstream caches rely on this: a
binding without the slot raises ``TypeError: cannot create weak reference`` the
first time one is used.
"""

import inspect
import weakref

from canon import emit, run

import lanelet2
from lanelet2.core import LaneletMap, Point3d

SUBMODULES = ["core", "geometry", "io", "matching", "projection", "routing", "traffic_rules"]


def main():
    for name in SUBMODULES:
        module = getattr(lanelet2, name)
        classes = sorted(
            attr
            for attr, member in vars(module).items()
            if inspect.isclass(member) and member.__module__.startswith("lanelet2")
        )
        emit(
            "no_weakref:" + name,
            [attr for attr in classes if getattr(module, attr).__weakrefoffset__ == 0],
        )

    # And the slot works, not merely exists.
    native = LaneletMap()
    ref = weakref.ref(native)
    emit("map_ref_alive", ref() is native)
    del native
    emit("map_ref_dead", ref() is None)
    cache = weakref.WeakKeyDictionary({Point3d(1, 0.0, 0.0, 0.0): "point"})
    emit("weak_key_after_collect", len(cache))


run(main)
