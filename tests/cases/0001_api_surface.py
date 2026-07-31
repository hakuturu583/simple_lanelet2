# EXPECT: partial
"""The full public API surface of all seven submodules.

This is simultaneously the project's TODO list and its permanent regression check.
It is expected to be incomplete for most of the project, so the runner scores it as
a coverage percentage rather than pass/fail: every name the reference exposes and we
do not is a unit of remaining work, and any name that *disappears* after being
implemented shows up as a drop.

Only genuinely public names and the dunders that are part of the API are compared;
``__reduce__``, ``__instance_size__`` and friends are artefacts of the binding
generator, not a compatibility goal (see ``canon.API_DUNDERS``).
"""

import inspect

from canon import emit, public_names, run

import lanelet2

SUBMODULES = ["core", "geometry", "io", "matching", "projection", "routing", "traffic_rules"]


def main():
    for name in SUBMODULES:
        module = getattr(lanelet2, name)
        names = public_names(module)
        emit("module:" + name, names)

        for attr in names:
            member = getattr(module, attr, None)
            if inspect.isclass(member):
                emit("class:{}.{}".format(name, attr), public_names(member))


run(main)
