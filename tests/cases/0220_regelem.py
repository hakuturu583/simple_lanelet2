"""Regulatory elements: the typed subclasses over one parameter map.

Which roles a constructor creates is observable through `.roles`, including roles it
leaves empty, so each type is checked. Two details worth pinning: constructing a
typed element stamps `type`/`subtype` attributes onto it, and `len()` counts *roles*
rather than referenced primitives.

The `repr` of a lanelet holding a regulatory element that points back at the lanelet
would recurse forever; upstream breaks the cycle by rendering the inner lanelet
without its elements.
"""

from canon import emit, expect_raises, run

from lanelet2.core import (
    AllWayStop,
    AttributeMap,
    Lanelet,
    LaneletWithStopLine,
    LineString3d,
    ManeuverType,
    Point3d,
    RegulatoryElement,
    RightOfWay,
    SpeedLimit,
    TrafficLight,
    TrafficSign,
    TrafficSignsWithType,
)


def line(id, subtype=None):
    ls = LineString3d(id, [Point3d(id * 100, 0.0, 0.0, 0.0), Point3d(id * 100 + 1, 1.0, 0.0, 0.0)])
    if subtype is not None:
        ls.attributes["subtype"] = subtype
    return ls


def lanelet(id):
    return Lanelet(id, line(id * 10 + 1), line(id * 10 + 2))


def main():
    light = TrafficLight(5, AttributeMap(), [line(6)], line(7))
    emit("tl_repr", repr(light))
    emit("tl_str", str(light))
    emit("tl_cls", type(light).__module__ + "." + type(light).__name__)
    emit("tl_bases", [base.__name__ for base in type(light).__bases__])
    emit("tl_isinstance_base", isinstance(light, RegulatoryElement))
    emit("tl_attributes", dict(light.attributes.items()))
    emit("tl_roles", light.roles)
    emit("tl_len", len(light))
    emit("tl_id", light.id)
    emit("tl_parameters_type", type(light.parameters).__name__)
    emit("tl_parameters_repr", repr(light.parameters))
    emit("tl_traffic_lights", [type(x).__name__ for x in light.trafficLights])
    emit("tl_stop_line", repr(light.stopLine))
    emit("tl_find", repr(light.find(6)))
    emit("tl_find_missing", repr(light.find(999)))

    light.removeStopLine()
    emit("tl_after_remove_stop_line", [light.roles, repr(light.stopLine)])
    light.addTrafficLight(line(8))
    emit("tl_after_add", len(light.trafficLights))
    emit("tl_remove_returns", light.removeTrafficLight(line(8)))

    expect_raises("tl_no_lights", lambda: TrafficLight(9, AttributeMap(), []), msg=True)

    # A separately built element with the same id is a different object.
    emit("tl_eq_distinct", TrafficLight(5, AttributeMap(), [line(6)])
         == TrafficLight(5, AttributeMap(), [line(6)]))

    row_lanelets = [lanelet(2)]
    yield_lanelets = [lanelet(3)]
    row = RightOfWay(10, AttributeMap(), row_lanelets, yield_lanelets)
    emit("row_attributes", dict(row.attributes.items()))
    emit("row_roles", row.roles)
    emit("row_right_of_way", [x.id for x in row.rightOfWayLanelets()])
    emit("row_yield", [x.id for x in row.yieldLanelets()])
    emit("row_maneuver_row", str(row.getManeuver(row_lanelets[0])))
    emit("row_maneuver_yield", str(row.getManeuver(yield_lanelets[0])))
    emit("row_maneuver_unknown", str(row.getManeuver(lanelet(99))))
    emit("maneuver_values", [str(ManeuverType.Yield), str(ManeuverType.RightOfWay),
                             str(ManeuverType.Unknown)])

    # Either every lanelet has a stop line, or none does.
    expect_raises(
        "aws_mixed_stop_lines",
        lambda: AllWayStop(
            12,
            AttributeMap(),
            [LaneletWithStopLine(lanelet(4), line(11)), LaneletWithStopLine(lanelet(5))],
        ),
        msg=True,
    )
    stopped = [LaneletWithStopLine(lanelet(4), line(11)), LaneletWithStopLine(lanelet(5), line(17))]
    stop = AllWayStop(12, AttributeMap(), stopped)
    emit("aws_attributes", dict(stop.attributes.items()))
    emit("aws_roles", stop.roles)
    emit("aws_lanelets", [x.id for x in stop.lanelets()])
    # Only lanelets that have a stop line contribute one, so these are not aligned.
    emit("aws_stop_lines", [x.id for x in stop.stopLines()])
    emit("aws_signs", [x.id for x in stop.trafficSigns()])

    signs = TrafficSignsWithType([line(13, "de274")], "de274")
    emit("tswt_type", signs.type)
    # Upstream registers no Python class for the underlying vector, so reading this
    # property raises there and works here. Captured rather than emitted directly:
    # an exception would truncate the stream in one mode and not the other, turning
    # a single-key difference into a whole-case mismatch.
    expect_raises("tswt_signs", lambda: [x.id for x in signs.trafficSigns])

    sign = TrafficSign(14, AttributeMap(), signs)
    emit("sign_attributes", dict(sign.attributes.items()))
    emit("sign_roles", sign.roles)
    emit("sign_type", sign.type())
    emit("sign_traffic_signs", [x.id for x in sign.trafficSigns()])
    emit("sign_cancel_types", sign.cancelTypes())

    limit = SpeedLimit(15, AttributeMap(), TrafficSignsWithType([line(16, "de274")], "de274"))
    emit("limit_cls", type(limit).__name__)
    emit("limit_bases", [base.__name__ for base in type(limit).__bases__])
    emit("limit_attributes", dict(limit.attributes.items()))

    # Attaching to a lanelet, and the repr cycle that creates.
    host = lanelet(20)
    host.addRegulatoryElement(light)
    emit("host_regelem_count", len(host.regulatoryElements))
    emit("host_regelem_types", [type(x).__name__ for x in host.regulatoryElements])
    emit("host_traffic_lights", len(host.trafficLights()))
    emit("host_repr", repr(host))

    cycle = lanelet(21)
    back_reference = RightOfWay(22, AttributeMap(), [cycle], [lanelet(23)])
    cycle.addRegulatoryElement(back_reference)
    emit("cycle_repr", repr(cycle))

    emit("host_remove", host.removeRegulatoryElement(light))
    emit("host_after_remove", len(host.regulatoryElements))
    emit("host_remove_again", host.removeRegulatoryElement(light))

    expect_raises("base_init", lambda: RegulatoryElement(), msg=True)


run(main)
