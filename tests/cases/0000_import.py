"""The package must present itself exactly like upstream's seven-extension package.

Upstream ships ``lanelet2`` as a package of seven separate Boost.Python ``.so``
files. We build one extension with seven submodules, so this case pins the things
that make the difference invisible: ``__all__``, the submodules' ``__name__``, their
presence in ``sys.modules``, and the ``__module__`` of a class defined inside one.
"""

import sys

from canon import emit, run

import lanelet2

SUBMODULES = ["core", "geometry", "io", "matching", "projection", "routing", "traffic_rules"]


def main():
    emit("all", list(lanelet2.__all__))

    for name in SUBMODULES:
        module = getattr(lanelet2, name)
        emit("name:" + name, module.__name__)
        emit("in_sys_modules:" + name, "lanelet2." + name in sys.modules)
        emit("is_same_object:" + name, sys.modules["lanelet2." + name] is module)

    # `import lanelet2.core` and `from lanelet2.core import ...` must resolve to the
    # very same module object as the attribute access above.
    import lanelet2.core

    emit("import_stmt_matches", lanelet2.core is sys.modules["lanelet2.core"])

    # A class registered inside a submodule must report the dotted module path, so
    # that repr(type(x)) reads "<class 'lanelet2.core.Point3d'>".
    emit("getId_module", lanelet2.core.getId.__module__)

    # Deliberate addition: our bug-compatibility switch. Absent upstream.
    emit("bug_compat_attr", getattr(lanelet2, "BUG_COMPAT", None))


run(main)
