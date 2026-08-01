"""Which two references we are actually measuring against.

# ORACLE: skew

The extension oracle is a ROS overlay workspace on the developer's machine, not a
pinned wheel, so it can change under us without anything else noticing. This case
runs under *both* references and reports what each one is, so that the difference
between them is written down rather than assumed.

Everything here differs by construction — different interpreter, different build,
only one of them has the extension — so every key is an entry in oracle_skew.toml.
That is the point: the entries are the description of the two oracles, and the
runner's anti-rot check means they cannot quietly stop being true.
"""

import sys

from canon import emit, run


def main():
    emit("python", ".".join(str(part) for part in sys.version_info[:2]))

    import lanelet2

    emit("lanelet2_module", lanelet2.__file__)

    # The key set has to be the same on both sides — a stream that stops early is a
    # structural mismatch, which no ledger can explain away — so an absent extension
    # reports empty lists rather than skipping the keys.
    try:
        import autoware_lanelet2_extension_python.projection as projection
        import autoware_lanelet2_extension_python.regulatory_elements as regelems
    except ImportError:
        projection = regelems = None

    emit("extension", "absent" if regelems is None else "present")
    emit(
        "extension_regelems",
        [] if regelems is None else sorted(n for n in dir(regelems) if n[0].isupper()),
    )
    emit(
        "extension_projections",
        []
        if projection is None
        else sorted(n for n in dir(projection) if n[0].isupper()),
    )


run(main)
