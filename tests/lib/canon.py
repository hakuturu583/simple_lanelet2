"""Canonical output helpers shared by every test case.

A test case is a standalone, deterministic script that imports ``lanelet2``, the
standard library and this module -- nothing else. It writes JSON Lines to stdout via
:func:`emit`; ``tests/runner.py`` runs the same script under the reference
implementation and under ours and compares the streams key by key.

JSON Lines rather than formatted text on purpose: it keeps float *formatting* out of
the comparison entirely (the runner compares parsed numbers with an explicit
tolerance) while ``repr``/``str`` output, which we *do* want to match byte for byte,
is compared as plain strings.

This module must stay importable on any CPython the reference wheel supports, so:
standard library only, no f-string ``=`` specifiers, no ``match``.
"""

import json
import os
import sys
import traceback

__all__ = [
    "emit",
    "emit_file",
    "expect_raises",
    "by_id",
    "by_dist_id",
    "data_path",
    "run",
]

#: Dunder methods that are genuinely part of the Lanelet2 API surface. Everything
#: else that ``dir()`` reports (``__reduce__``, ``__instance_size__``, ...) is an
#: artefact of the binding generator and is not a compatibility goal.
API_DUNDERS = frozenset(
    [
        "__init__",
        "__eq__",
        "__ne__",
        "__lt__",
        "__hash__",
        "__str__",
        "__repr__",
        "__len__",
        "__getitem__",
        "__setitem__",
        "__delitem__",
        "__iter__",
        "__next__",
        "__contains__",
        "__call__",
        "__add__",
        "__sub__",
        "__mul__",
        "__rmul__",
        "__div__",
        "__truediv__",
        "__neg__",
    ]
)


def emit(key, value):
    """Write one canonical ``(key, value)`` record to stdout."""
    sys.stdout.write(json.dumps({"k": key, "v": value}, sort_keys=True) + "\n")


def emit_file(path, key=None):
    """Emit the full text of a file, for byte-exact comparison."""
    with open(path, "r", encoding="utf-8") as handle:
        emit(key or ("file:" + os.path.basename(path)), handle.read())


def expect_raises(key, fn, msg=False):
    """Call ``fn`` and emit the exception it raised.

    Emits ``[type_name]``, or ``[type_name, message]`` when ``msg`` is true. If no
    exception is raised, emits ``["<no-exception>", repr(result)]`` so the mismatch
    is visible rather than silently passing.
    """
    try:
        result = fn()
    except BaseException as exc:  # noqa: BLE001 - reporting is the whole point
        name = type(exc).__name__
        emit(key, [name, str(exc)] if msg else [name])
        return
    emit(key, ["<no-exception>", repr(result)])


def by_id(iterable):
    """Sort primitives by id.

    Mandatory for anything that iterates a map layer: upstream layers are
    ``std::unordered_map``, so their iteration order is not reproducible even
    between two runs of the reference implementation.
    """
    return sorted(iterable, key=lambda primitive: primitive.id)


def by_dist_id(pairs):
    """Sort ``(distance, primitive)`` pairs deterministically.

    ``findNearest``/``findWithin`` sort with ``std::sort``, which is unstable, so
    equal-distance results come back in an arbitrary order. Break ties by id.
    """
    return sorted(pairs, key=lambda pair: (round(pair[0], 9), pair[1].id))


def data_path(name):
    """Absolute path of a file in ``tests/data``."""
    root = os.environ.get("LANELET2_DATA")
    if not root:
        raise RuntimeError("LANELET2_DATA is not set; run cases through tests/runner.py")
    return os.path.join(root, name)


def public_names(obj):
    """Sorted API surface of a module or class."""
    names = []
    for name in dir(obj):
        if not name.startswith("_") or name in API_DUNDERS:
            names.append(name)
    return sorted(names)


def run(main):
    """Run a case's ``main`` and make any failure a visible record, not a traceback."""
    try:
        main()
    except BaseException as exc:  # noqa: BLE001 - reporting is the whole point
        emit("__fatal__", [type(exc).__name__, str(exc)])
        sys.stdout.flush()
        sys.stderr.write(traceback.format_exc())
        raise SystemExit(1)
    sys.stdout.flush()
