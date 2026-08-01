"""The extension's public API surface.

# ORACLE: aw

The counterpart of `0001_api_surface` for `autoware_lanelet2_extension_python`. Names
are what a user's import statement actually depends on, so they are compared exactly
rather than sampled -- a class that quietly stops being exported breaks callers no
less than one that changes behaviour.

The only expected differences are ROS types that leak into upstream's module
namespaces from its own imports: `Point`, `Pose`, `Quaternion` and `serialize_message`.
"""

import importlib
import inspect

from canon import emit, public_names, run

PACKAGE = "autoware_lanelet2_extension_python"

#: ROS types that upstream's `from geometry_msgs.msg import ...` leaks into its module
#: namespaces. They are classes, so expanding them would put keys in the reference's
#: stream that ours cannot have -- a structural mismatch no ledger can explain. Their
#: absence from the module listings is recorded in divergence.toml instead.
ROS_TYPES = {"Point", "Pose", "Quaternion", "serialize_message"}
SUBMODULES = ["regulatory_elements", "projection", "utility.query", "utility.utilities"]


def main():
    # Importing this first is what registers the subtypes; the others do not depend on
    # it, but the order is what a user would write.
    importlib.import_module("%s.regulatory_elements" % PACKAGE)

    for name in SUBMODULES:
        module = importlib.import_module("%s.%s" % (PACKAGE, name))
        names = public_names(module)
        emit("module:" + name, names)

        for attribute in names:
            member = getattr(module, attribute, None)
            if attribute in ROS_TYPES:
                continue
            if inspect.isclass(member):
                emit("class:{}.{}".format(name, attribute), public_names(member))


run(main)
