"""An Autoware-style map, built to order, loaded and written back.

# ORACLE: aw

`1150_aw_map` does this on a real 10.5 MB Autoware map, which is the stronger check --
but that map is CC BY-NC licensed, so it can be neither vendored here nor fetched by
CI, and the case skips itself without `SIMPLE_LL2_AW_MAP`. This one covers the same
behaviour on a map small enough to write inline, so the claim is exercised everywhere
rather than only on one machine.

What has to hold: the extension's subtypes resolve to their classes, the elements
attach to the lanelets that reference them, and the map writes back byte for byte.
"""

import hashlib

from canon import by_id, emit, run

# Registration happens here, before anything is loaded.
import autoware_lanelet2_extension_python.regulatory_elements  # noqa: F401
from lanelet2.io import Origin, loadRobust, write
from lanelet2.projection import UtmProjector

# Two lanelets sharing a traffic light, so the deduplication in `query.trafficLights`
# has something to deduplicate, plus one of each extension subtype that a map can
# actually carry. The areas need `area=yes`: without it they load as linestrings, the
# element that refers to them cannot be written back, and the reference says so only at
# write time.
OSM = """<?xml version="1.0" encoding="UTF-8"?>
<osm version="0.6" generator="case1160">
  <node id="1" lat="49.0000000" lon="8.4000000"/>
  <node id="2" lat="49.0002000" lon="8.4000000"/>
  <node id="3" lat="49.0000000" lon="8.4002000"/>
  <node id="4" lat="49.0002000" lon="8.4002000"/>
  <node id="5" lat="49.0004000" lon="8.4000000"/>
  <node id="6" lat="49.0004000" lon="8.4002000"/>
  <way id="10"><nd ref="1"/><nd ref="2"/><tag k="type" v="line_thin"/></way>
  <way id="11"><nd ref="3"/><nd ref="4"/><tag k="type" v="line_thin"/></way>
  <way id="12"><nd ref="2"/><nd ref="5"/><tag k="type" v="line_thin"/></way>
  <way id="13"><nd ref="4"/><nd ref="6"/><tag k="type" v="line_thin"/></way>
  <way id="20"><nd ref="1"/><nd ref="3"/><tag k="type" v="light_bulbs"/></way>
  <way id="21"><nd ref="2"/><nd ref="4"/><tag k="type" v="stop_line"/></way>
  <way id="22"><nd ref="1"/><nd ref="2"/><tag k="type" v="road_marking"/></way>
  <way id="23"><nd ref="3"/><nd ref="4"/><nd ref="6"/><tag k="type" v="detection_area"/><tag k="area" v="yes"/></way>
  <way id="24"><nd ref="1"/><nd ref="3"/><nd ref="4"/><tag k="type" v="no_parking_area"/><tag k="area" v="yes"/></way>
  <relation id="30">
    <member type="way" role="refers" ref="20"/>
    <member type="way" role="ref_line" ref="21"/>
    <tag k="type" v="regulatory_element"/><tag k="subtype" v="traffic_light"/>
  </relation>
  <relation id="31">
    <member type="way" role="refers" ref="22"/>
    <tag k="type" v="regulatory_element"/><tag k="subtype" v="road_marking"/>
  </relation>
  <relation id="32">
    <member type="way" role="refers" ref="23"/>
    <member type="way" role="ref_line" ref="21"/>
    <tag k="type" v="regulatory_element"/><tag k="subtype" v="detection_area"/>
  </relation>
  <relation id="33">
    <member type="way" role="refers" ref="24"/>
    <tag k="type" v="regulatory_element"/><tag k="subtype" v="no_parking_area"/>
  </relation>
  <relation id="40">
    <member type="way" role="left" ref="10"/>
    <member type="way" role="right" ref="11"/>
    <member type="relation" role="regulatory_element" ref="30"/>
    <member type="relation" role="regulatory_element" ref="31"/>
    <member type="relation" role="regulatory_element" ref="33"/>
    <tag k="type" v="lanelet"/><tag k="subtype" v="road"/>
  </relation>
  <relation id="41">
    <member type="way" role="left" ref="12"/>
    <member type="way" role="right" ref="13"/>
    <member type="relation" role="regulatory_element" ref="30"/>
    <member type="relation" role="regulatory_element" ref="32"/>
    <tag k="type" v="lanelet"/><tag k="subtype" v="road"/>
  </relation>
</osm>
"""


def main():
    with open("synthetic.osm", "w") as handle:
        handle.write(OSM)

    projector = UtmProjector(Origin(49.0, 8.4, 0.0))
    map_, errors = loadRobust("synthetic.osm", projector)
    emit("load_errors", list(errors))

    histogram = {}
    for element in by_id(map_.regulatoryElementLayer):
        name = type(element).__name__
        histogram[name] = histogram.get(name, 0) + 1
    emit("types", sorted(histogram.items()))

    for lanelet in by_id(map_.laneletLayer):
        emit(
            "attached_%d" % lanelet.id,
            sorted((r.id, type(r).__name__) for r in lanelet.regulatoryElements),
        )

    # The shared traffic light must appear once across both lanelets, not once each.
    import autoware_lanelet2_extension_python.utility.query as query

    lanelets = query.laneletLayer(map_)
    emit("query_lights", sorted(x.id for x in query.trafficLights(lanelets)))
    emit("query_autoware_lights", sorted(x.id for x in query.autowareTrafficLights(lanelets)))
    emit("query_detection_areas", sorted(x.id for x in query.detectionAreas(lanelets)))
    emit("query_no_parking", sorted(x.id for x in query.noParkingAreas(lanelets)))

    # Typed accessors reach through to the roles.
    marking = map_.regulatoryElementLayer[31]
    emit("marking_refers", type(marking.roadMarking()).__name__)

    write("out.osm", map_, projector)
    with open("out.osm", "rb") as handle:
        written = handle.read()
    emit("written_bytes", len(written))
    emit("written_sha", hashlib.sha256(written).hexdigest())


run(main)
