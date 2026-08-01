"""The eight regulatory elements the Autoware extension adds.

# ORACLE: aw

Constructor, attributes, roles, repr and accessors for each. Two things are
deliberately *not* exercised here:

* `Crosswalk.crosswalkLanelet()` segfaults the reference — its constructor never
  populates `refers`, so the accessor takes the front of an empty vector. Calling it
  would kill the reference interpreter mid-stream and every later key would read as a
  mismatch. Our own raise is pinned in 1101 instead.
* lanelet reprs, because the pinned oracle is lanelet2 1.2.2, which still recurses
  forever on a lanelet whose regulatory element points back at it.

Which roles a constructor creates is the sharpest signal here: unlike the stock
`TrafficSign`, these create only what they were given, so a `NoStoppingArea` built
with a stop line has a different role set from one built without.
"""

from canon import emit, expect_raises, run

# Importing this is what registers the subtypes. Nothing before this line can see
# them, which is the whole point of the module existing separately.
import autoware_lanelet2_extension_python.regulatory_elements as ext
from lanelet2.core import AttributeMap, LineString3d, Point3d, Polygon3d, getId


def line(offset=0.0, count=2):
    return LineString3d(
        getId(), [Point3d(getId(), offset + i, 0.0, 0.0) for i in range(count)]
    )


def polygon(offset=0.0):
    return Polygon3d(
        getId(),
        [
            Point3d(getId(), offset, 0.0, 0.0),
            Point3d(getId(), offset + 1.0, 0.0, 0.0),
            Point3d(getId(), offset + 1.0, 1.0, 0.0),
        ],
    )


def describe(key, element):
    emit("%s_type" % key, type(element).__name__)
    # Boost.Python splices its own `instance` into every class it exports; that is a
    # binding-technology artifact rather than behaviour, and it is absent from every
    # stock class of ours too.
    emit(
        "%s_mro" % key,
        [t.__name__ for t in type(element).__mro__ if t.__name__ != "instance"][:4],
    )
    emit("%s_attributes" % key, dict(element.attributes))
    emit("%s_roles" % key, sorted(element.parameters.keys()))
    emit(
        "%s_params" % key,
        {k: [type(x).__name__ for x in v] for k, v in element.parameters.items()},
    )


def main():
    # --- RoadMarking -------------------------------------------------------------
    marking = ext.RoadMarking(getId(), AttributeMap(), line())
    describe("marking", marking)
    emit("marking_repr", repr(marking))
    emit("marking_accessor", type(marking.roadMarking()).__name__)

    # --- SpeedBump ---------------------------------------------------------------
    bump = ext.SpeedBump(getId(), AttributeMap(), polygon())
    describe("bump", bump)
    emit("bump_repr", repr(bump))
    emit("bump_accessor", type(bump.speedBump()).__name__)

    # --- NoParkingArea -----------------------------------------------------------
    parking = ext.NoParkingArea(getId(), AttributeMap(), [polygon()])
    describe("parking", parking)
    emit("parking_repr", repr(parking))
    emit("parking_accessor", [type(x).__name__ for x in parking.noParkingAreas()])

    # --- NoStoppingArea, with and without a stop line ----------------------------
    stopping = ext.NoStoppingArea(getId(), AttributeMap(), [polygon()])
    describe("stopping", stopping)
    emit("stopping_accessor", [type(x).__name__ for x in stopping.noStoppingAreas()])
    # Bound to a NoParkingArea& upstream, so this raises rather than printing the
    # wrong class name.
    expect_raises("stopping_repr", lambda: repr(stopping))

    stopping_sl = ext.NoStoppingArea(getId(), AttributeMap(), [polygon()], line(3.0))
    describe("stopping_sl", stopping_sl)
    emit("stopping_sl_stop_line", type(stopping_sl.stopLine()).__name__)

    # --- DetectionArea -----------------------------------------------------------
    detection = ext.DetectionArea(getId(), AttributeMap(), [polygon()], line(3.0))
    describe("detection", detection)
    emit("detection_repr", repr(detection))
    emit("detection_areas", [type(x).__name__ for x in detection.detectionAreas()])
    emit("detection_stop_line", type(detection.stopLine()).__name__)

    # --- AutowareTrafficLight ----------------------------------------------------
    light = ext.AutowareTrafficLight(getId(), AttributeMap(), [line()])
    describe("light", light)
    emit("light_repr", repr(light))
    # Inherited from the stock TrafficLight, and a property rather than a method.
    emit("light_traffic_lights", [type(x).__name__ for x in light.trafficLights])
    emit("light_bulbs", [type(x).__name__ for x in light.lightBulbs()])

    light_sl = ext.AutowareTrafficLight(getId(), AttributeMap(), [line()], line(3.0))
    describe("light_sl", light_sl)

    # --- Crosswalk ---------------------------------------------------------------
    from lanelet2.core import Lanelet

    crossing = Lanelet(getId(), line(), line(5.0))
    crosswalk = ext.Crosswalk(
        getId(), AttributeMap(), crossing, polygon(), [line(7.0)]
    )
    describe("crosswalk", crosswalk)
    emit("crosswalk_areas", [type(x).__name__ for x in crosswalk.crosswalkAreas()])
    emit("crosswalk_stop_lines", [type(x).__name__ for x in crosswalk.stopLines()])

    # Only the first stop line survives upstream: the constructor inserts into a
    # std::map per line, and insert does not overwrite.
    many = ext.Crosswalk(
        getId(), AttributeMap(), crossing, polygon(), [line(9.0), line(11.0)]
    )
    emit("crosswalk_many_stop_lines", len(many.stopLines()))

    # Writes to role `crosswalk` while the getter reads `crosswalk_polygon`, so the
    # addition is invisible.
    before = len(crosswalk.crosswalkAreas())
    crosswalk.addCrosswalkArea(polygon(13.0))
    emit("crosswalk_add_visible", [before, len(crosswalk.crosswalkAreas())])

    # --- VirtualTrafficLight -----------------------------------------------------
    # Its constructor validates for exactly one start_line and its signature gives no
    # way to supply one, so it cannot succeed at all.
    expect_raises(
        "virtual_ctor",
        lambda: ext.VirtualTrafficLight(getId(), AttributeMap(), line()),
        msg=True,
    )


run(main)
