"""Registering the extension is a moment in time, not a property of the process.

# ORACLE: aw

Upstream registers its rule names when its shared library loads, and every regulatory
element latches its concrete class at construction. Two consequences follow, and both
are surprising enough to be worth pinning rather than leaving as folklore:

* before the import, a map carrying the extension's subtypes is **refused**, exactly
  as stock Lanelet2 refuses it — the classes existing in the same binary as
  `lanelet2` changes nothing;
* after the import, elements loaded *earlier* keep the class they were built with. A
  `traffic_light` read before the import stays a plain `TrafficLight` even though
  `traffic_light` now resolves to `AutowareTrafficLight`.

The second is the one that bites: an unrelated module importing the extension changes
what `lanelet2.io.load` produces from that point on, process-wide.
"""

from canon import emit, expect_raises, run

from lanelet2.io import Origin, load, loadRobust
from lanelet2.projection import UtmProjector

OSM = """<?xml version="1.0" encoding="UTF-8"?>
<osm version="0.6" generator="case1101">
  <node id="1" lat="49.0000000" lon="8.4000000"/>
  <node id="2" lat="49.0001000" lon="8.4000000"/>
  <node id="3" lat="49.0000000" lon="8.4001000"/>
  <node id="4" lat="49.0001000" lon="8.4001000"/>
  <way id="10"><nd ref="1"/><nd ref="2"/><tag k="type" v="line_thin"/></way>
  <way id="11"><nd ref="3"/><nd ref="4"/><tag k="type" v="line_thin"/></way>
  <way id="12"><nd ref="1"/><nd ref="3"/><tag k="type" v="light_bulbs"/></way>
  <relation id="20">
    <member type="way" role="refers" ref="12"/>
    <tag k="type" v="regulatory_element"/><tag k="subtype" v="{subtype}"/>
  </relation>
  <relation id="30">
    <member type="way" role="left" ref="10"/>
    <member type="way" role="right" ref="11"/>
    <member type="relation" role="regulatory_element" ref="20"/>
    <tag k="type" v="lanelet"/><tag k="subtype" v="road"/>
  </relation>
</osm>
"""


def write_map(name, subtype):
    with open(name, "w") as handle:
        handle.write(OSM.format(subtype=subtype))
    return name


def main():
    projector = UtmProjector(Origin(49.0, 8.4, 0.0))
    marking = write_map("marking.osm", "road_marking")
    light = write_map("light.osm", "traffic_light")

    # --- before the import -------------------------------------------------------
    expect_raises("before_marking", lambda: load(marking, projector), msg=True)
    _, errors = loadRobust(marking, projector)
    emit("before_marking_errors", list(errors))

    before = load(light, projector)
    before_element = before.regulatoryElementLayer[20]
    emit("before_light_type", type(before_element).__name__)

    # --- the import --------------------------------------------------------------
    import autoware_lanelet2_extension_python.regulatory_elements  # noqa: F401

    # --- after the import --------------------------------------------------------
    after = load(marking, projector)
    emit("after_marking_type", type(after.regulatoryElementLayer[20]).__name__)

    after_light = load(light, projector)
    emit("after_light_type", type(after_light.regulatoryElementLayer[20]).__name__)

    # The map read before the import is untouched by it: its element was built as a
    # stock TrafficLight and stays one.
    emit("before_light_type_again", type(before.regulatoryElementLayer[20]).__name__)
    emit(
        "before_light_same_object",
        type(before_element).__name__ == type(before.regulatoryElementLayer[20]).__name__,
    )


run(main)
