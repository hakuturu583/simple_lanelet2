"""How a `subtype` tag becomes a typed regulatory element, and what happens when it
cannot.

Upstream resolves the tag through `RegulatoryElementFactory`, which **throws** for a
name nobody registered — a generic element is the fallback for an *empty* subtype
only, not for an unknown one. That is why a stock Lanelet2 refuses an Autoware map
until `autoware_lanelet2_extension` is loaded, and the failure is two-part: the
element is rejected, and then every lanelet referencing it reports separately that
the id is missing and gets a bare placeholder in its place.

The typed accessors are `dynamic_pointer_cast`s rather than equality tests, so a
derived element answers to its base's accessor: a `SpeedLimit` appears in
`trafficSigns()`.
"""

from canon import by_id, emit, expect_raises, run

import lanelet2
from lanelet2.io import Origin, load, loadRobust
from lanelet2.projection import UtmProjector

HEADER = """<?xml version="1.0" encoding="UTF-8"?>
<osm version="0.6" generator="case0221">
  <node id="1" lat="49.0000000" lon="8.4000000"/>
  <node id="2" lat="49.0001000" lon="8.4000000"/>
  <node id="3" lat="49.0000000" lon="8.4001000"/>
  <node id="4" lat="49.0001000" lon="8.4001000"/>
  <way id="10"><nd ref="1"/><nd ref="2"/><tag k="type" v="line_thin"/></way>
  <way id="11"><nd ref="3"/><nd ref="4"/><tag k="type" v="line_thin"/></way>
  <way id="12"><nd ref="1"/><nd ref="3"/><tag k="type" v="traffic_sign"/>"""\
    """<tag k="subtype" v="de274"/></way>
"""

LANELET = """  <relation id="30">
    <member type="way" role="left" ref="10"/>
    <member type="way" role="right" ref="11"/>
{members}    <tag k="type" v="lanelet"/>
    <tag k="subtype" v="road"/>
  </relation>
</osm>
"""


def write_map(name, regelems, refs):
    """Builds a map whose one lanelet refers to the given regulatory elements."""
    members = "".join(
        '    <member type="relation" role="regulatory_element" ref="%d"/>\n' % ref
        for ref in refs
    )
    with open(name, "w") as handle:
        handle.write(HEADER + regelems + LANELET.format(members=members))
    return name


def regelem(id_, subtype, extra=""):
    subtype_tag = '<tag k="subtype" v="%s"/>' % subtype if subtype is not None else ""
    return (
        '  <relation id="%d">\n'
        '    <member type="way" role="refers" ref="12"/>\n'
        '    <tag k="type" v="regulatory_element"/>%s%s\n'
        "  </relation>\n" % (id_, subtype_tag, extra)
    )


def digest(regelems):
    return sorted((r.id, type(r).__name__) for r in regelems)


def main():
    projector = UtmProjector(Origin(49.0, 8.4, 0.0))

    # --- an unregistered subtype is rejected, exactly as upstream's factory is ----
    unknown = write_map("unknown.osm", regelem(20, "road_marking"), [20])
    map_, errors = loadRobust(unknown, projector)
    emit("unknown_errors", list(errors))
    emit("unknown_layer_ids", [r.id for r in by_id(map_.regulatoryElementLayer)])
    lanelet = map_.laneletLayer[30]
    # The referencing lanelet still gets an element: a placeholder carrying only the
    # id, with no attributes, that never entered the map's own layer.
    emit("unknown_attached", digest(lanelet.regulatoryElements))
    emit(
        "unknown_attached_attributes",
        [dict(r.attributes) for r in lanelet.regulatoryElements],
    )
    expect_raises("unknown_load", lambda: load(unknown, projector), msg=True)

    # --- a derived kind answers to its base's accessor ---------------------------
    typed = write_map(
        "typed.osm",
        regelem(20, "speed_limit", '<tag k="sign_type" v="de274"/>')
        + regelem(21, "traffic_sign", '<tag k="sign_type" v="de206"/>')
        + regelem(22, "traffic_light"),
        [20, 21, 22],
    )
    map_ = load(typed, projector)
    lanelet = map_.laneletLayer[30]
    emit("typed_all", digest(lanelet.regulatoryElements))
    emit("typed_signs", digest(lanelet.trafficSigns()))
    emit("typed_limits", digest(lanelet.speedLimits()))
    emit("typed_lights", digest(lanelet.trafficLights()))
    emit("typed_row", digest(lanelet.rightOfWay()))
    emit("typed_repr", repr(map_.regulatoryElementLayer[20])[:80])

    # --- the factory *constructs* the class, so its constructor validates ---------
    # A malformed element is rejected at load time under the same wrapper message as
    # an unknown one, carrying the constructor's own complaint.
    for label, subtype, members, sign_subtype in (
        ("light_empty", "traffic_light", [], True),
        ("sign_no_subtype", "traffic_sign", ["refers"], False),
        ("limit_no_subtype", "speed_limit", ["refers"], False),
        ("row_no_lanelets", "right_of_way", [], True),
        # Measured: an empty AllWayStop is accepted, despite its stop-line rule.
        ("allway_empty", "all_way_stop", [], True),
    ):
        sign = '<tag k="subtype" v="de206"/>' if sign_subtype else ""
        body = "".join(
            '    <member type="way" role="%s" ref="12"/>\n' % role for role in members
        )
        path = write_map(
            "%s.osm" % label,
            '  <relation id="20">\n%s'
            '    <tag k="type" v="regulatory_element"/><tag k="subtype" v="%s"/>\n'
            "  </relation>\n" % (body, subtype),
            [20],
        )
        # The referred way carries the sign subtype, so rewrite the header for it.
        source = open(path).read().replace(
            '<tag k="subtype" v="de274"/></way>', "%s</way>" % sign
        )
        with open(path, "w") as handle:
            handle.write(source)
        _, errors = loadRobust(path, projector)
        emit("validate_%s" % label, list(errors))

    # --- what `SpeedLimit(...)` really builds ------------------------------------
    # Upstream binds the constructor to TrafficSign::make, so the object answers to
    # `SpeedLimit` in Python but fails a cast to one: it shows up in `trafficSigns()`
    # — displayed as a SpeedLimit — and not in `speedLimits()`.
    from lanelet2.core import (
        AttributeMap,
        Lanelet,
        LineString3d,
        Point3d,
        SpeedLimit,
        TrafficSign,
        TrafficSignsWithType,
        getId,
    )

    def line(*offsets):
        return LineString3d(
            getId(), [Point3d(getId(), x, 0.0, 0.0) for x in offsets]
        )

    host = Lanelet(getId(), line(0.0, 1.0), line(0.0, 1.0))
    limit = SpeedLimit(getId(), AttributeMap(), TrafficSignsWithType([line(2.0, 3.0)], "de274"))
    sign = TrafficSign(getId(), AttributeMap(), TrafficSignsWithType([line(4.0, 5.0)], "de206"))
    host.addRegulatoryElement(limit)
    host.addRegulatoryElement(sign)
    emit("ctor_limit_type", type(limit).__name__)
    emit("ctor_limit_subtype", dict(limit.attributes).get("subtype"))
    emit("ctor_signs", [type(x).__name__ for x in host.trafficSigns()])
    emit("ctor_limits", [type(x).__name__ for x in host.speedLimits()])

    # --- no `subtype` tag at all is its own error, distinct from an unknown one --
    missing = write_map("missing.osm", regelem(20, None), [20])
    expect_raises("missing_load", lambda: load(missing, projector), msg=True)

    # --- a present-but-empty subtype is the generic fallback ---------------------
    # This is the one path that reaches `GenericRegulatoryElement`; upstream writes
    # the resolved name back into the attributes, so it round-trips as a real tag.
    empty = write_map("empty.osm", regelem(20, ""), [20])
    map_, errors = loadRobust(empty, projector)
    emit("empty_errors", list(errors))
    emit("empty_layer_ids", [r.id for r in by_id(map_.regulatoryElementLayer)])
    if 20 in [r.id for r in by_id(map_.regulatoryElementLayer)]:
        element = map_.regulatoryElementLayer[20]
        emit("empty_type", type(element).__name__)
        emit("empty_attributes", dict(element.attributes))
        emit("empty_repr", repr(element)[:60])


run(main)
