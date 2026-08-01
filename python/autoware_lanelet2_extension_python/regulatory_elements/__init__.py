"""The regulatory elements `autoware_lanelet2_extension` adds.

Importing this module is what makes their subtypes resolvable, exactly as loading
upstream's shared library is. Until then `lanelet2.io.load` refuses a map carrying
them, which is what stock Lanelet2 does and therefore what we must do too.

The classes themselves live in the `lanelet2` extension module, because PyO3 classes
are static Rust types and the subtype-to-class table has to reach them. That is a
packaging detail; the observable behaviour is upstream's.
"""

import lanelet2  # noqa: F401  (upstream's shim imports it first too)

from lanelet2._lanelet2 import awext_regulatory_elements as _ext

_ext.__register__()

AutowareTrafficLight = _ext.AutowareTrafficLight
Crosswalk = _ext.Crosswalk
DetectionArea = _ext.DetectionArea
NoParkingArea = _ext.NoParkingArea
NoStoppingArea = _ext.NoStoppingArea
RoadMarking = _ext.RoadMarking
SpeedBump = _ext.SpeedBump
VirtualTrafficLight = _ext.VirtualTrafficLight

__all__ = [
    "AutowareTrafficLight",
    "Crosswalk",
    "DetectionArea",
    "NoParkingArea",
    "NoStoppingArea",
    "RoadMarking",
    "SpeedBump",
    "VirtualTrafficLight",
]
