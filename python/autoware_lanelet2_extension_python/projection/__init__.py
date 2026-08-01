"""The projections `autoware_lanelet2_extension` adds.

Unlike the regulatory elements, these register nothing — a projector is passed in
explicitly, so importing this module changes no global state.
"""

import lanelet2  # noqa: F401

from lanelet2._lanelet2 import awext_projection as _ext

MGRSProjector = _ext.MGRSProjector
TransverseMercatorProjector = _ext.TransverseMercatorProjector

__all__ = ["MGRSProjector", "TransverseMercatorProjector"]
