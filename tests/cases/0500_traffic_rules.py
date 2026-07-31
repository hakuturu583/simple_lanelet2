"""Traffic rules: passability, direction, speed limits and lane changes.

The tables are swept exhaustively rather than spot-checked, because they are
tables: every registered participant against every way subtype, both directions,
both locations, plus the attribute overrides and the full lane-change matrix.

One thing worth stating plainly: an unrecognised subtype yields a speed limit of
*zero*, not "no limit". That zero is a sentinel which the travel-time routing cost
later reads as infinite speed.
"""

from canon import by_id, data_path, emit, expect_raises, run

import lanelet2.traffic_rules as t
from lanelet2.core import Lanelet, LineString3d, Point3d, getId
from lanelet2.io import Origin, load
from lanelet2.projection import UtmProjector

SUBTYPES = ["", "road", "highway", "play_street", "emergency_lane", "exit", "walkway",
            "crosswalk", "stairs", "shared_walkway", "bicycle_lane", "bus_lane",
            "parking", "freespace", "traffic_island", "pedestrian_lane", "lane"]
LOCATIONS = ["", "urban", "nonurban"]
PARTICIPANTS = ["vehicle", "vehicle:car", "vehicle:bus", "vehicle:truck", "vehicle:emergency",
                "vehicle:taxi", "vehicle:motorcycle", "pedestrian", "bicycle"]
BOUNDARIES = [("line_thin", "dashed"), ("line_thin", "solid"), ("line_thin", "dashed_solid"),
              ("line_thin", "solid_dashed"), ("line_thin", "solid_solid"),
              ("line_thick", "dashed"), ("line_thick", "solid"),
              ("curbstone", "low"), ("curbstone", "high"), ("virtual", ""),
              ("road_border", ""), ("guard_rail", "")]


def line(points, attributes=None):
    ls = LineString3d(getId(), [Point3d(getId(), x, y, 0.0) for x, y in points])
    for key, value in (attributes or {}).items():
        ls.attributes[key] = value
    return ls


def lanelet(subtype="", location="", extra=None):
    ll = Lanelet(getId(), line([(0.0, 1.0), (10.0, 1.0)]), line([(0.0, -1.0), (10.0, -1.0)]))
    if subtype:
        ll.attributes["subtype"] = subtype
    if location:
        ll.attributes["location"] = location
    for key, value in (extra or {}).items():
        ll.attributes[key] = value
    return ll


def main():
    emit("locations", [t.Locations.Germany])
    emit("participants", sorted(n for n in dir(t.Participants) if not n.startswith("_")))
    emit("participant_values",
         [getattr(t.Participants, n) for n in
          sorted(n for n in dir(t.Participants) if not n.startswith("_"))])

    rules = t.create(t.Locations.Germany, t.Participants.Vehicle)
    emit("location_is_a_method", rules.location())
    emit("participant_is_a_method", rules.participant())
    emit("str", str(rules))

    # A participant under `vehicle` with no rules of its own falls back to the
    # vehicle rules but keeps its own name.
    bus = t.create(t.Locations.Germany, "vehicle:bus")
    emit("fallback_keeps_the_name", [bus.location(), bus.participant()])
    expect_raises("unregistered_participant",
                  lambda: t.create(t.Locations.Germany, "train"), msg=True)
    expect_raises("unregistered_location", lambda: t.create("us", "vehicle"), msg=True)

    # --- the passability and direction tables --------------------------------
    for participant in PARTICIPANTS:
        r = t.create(t.Locations.Germany, participant)
        for subtype in SUBTYPES:
            ll = lanelet(subtype)
            emit("pass:{}:{}".format(participant, subtype),
                 [r.canPass(ll), r.canPass(ll.invert()), r.isOneWay(ll)])

    # --- the speed-limit table -----------------------------------------------
    for participant in PARTICIPANTS:
        r = t.create(t.Locations.Germany, participant)
        for subtype in SUBTYPES:
            for location in LOCATIONS:
                limit = r.speedLimit(lanelet(subtype, location))
                emit("speed:{}:{}:{}".format(participant, subtype, location),
                     [round(limit.speedLimitKmH, 9), round(limit.speedLimitMPS, 9),
                      limit.isMandatory])

    empty = t.SpeedLimitInformation()
    emit("speed_limit_default",
         [empty.speedLimit, empty.speedLimitKmH, empty.speedLimitMPS, empty.isMandatory])
    emit("speed_limit_str", str(empty))
    # The two-argument constructor is registered but unreachable upstream.
    expect_raises("speed_limit_two_args", lambda: t.SpeedLimitInformation(50.0, True))

    # --- attribute overrides -------------------------------------------------
    OVERRIDES = [
        {"participant:vehicle": "no"},
        {"participant:vehicle:car": "no"},
        {"participant:pedestrian": "yes"},
        {"one_way": "no"},
        {"one_way:vehicle": "no"},
        {"speed_limit": "30"},
        {"speed_limit": "30 mph"},
        {"speed_limit": "10 m/s"},
        {"speed_limit:vehicle": "20"},
        {"speed_limit_mandatory": "false"},
        {"speed_limit": "40", "speed_limit_mandatory": "false"},
    ]
    car = t.create(t.Locations.Germany, "vehicle:car")
    for extra in OVERRIDES:
        ll = lanelet("road", "", extra)
        limit = car.speedLimit(ll)
        emit("override:{}".format(sorted(extra.items())),
             [car.canPass(ll), car.isOneWay(ll),
              round(limit.speedLimitKmH, 6), limit.isMandatory])

    # --- lane changes --------------------------------------------------------
    for participant in ["vehicle", "pedestrian", "bicycle"]:
        r = t.create(t.Locations.Germany, participant)
        for kind, subtype in BOUNDARIES:
            attributes = {"type": kind}
            if subtype:
                attributes["subtype"] = subtype
            shared = line([(0.0, 0.0), (10.0, 0.0)], attributes)
            upper = Lanelet(getId(), line([(0.0, 2.0), (10.0, 2.0)]), shared)
            lower = Lanelet(getId(), shared, line([(0.0, -2.0), (10.0, -2.0)]))
            for ll in (upper, lower):
                ll.attributes["subtype"] = "road"
            emit("change:{}:{}:{}".format(participant, kind, subtype),
                 [r.canChangeLane(upper, lower), r.canChangeLane(lower, upper)])

    # An explicit attribute overrides the markings.
    for attributes in [{"lane_change": "yes"}, {"lane_change": "no"},
                       {"lane_change:left": "yes"}, {"lane_change:right": "yes"},
                       {"lane_change:left": "yes", "lane_change:right": "yes"}]:
        marks = dict(attributes)
        marks.update({"type": "line_thin", "subtype": "solid"})
        shared = line([(0.0, 0.0), (10.0, 0.0)], marks)
        upper = Lanelet(getId(), line([(0.0, 2.0), (10.0, 2.0)]), shared)
        lower = Lanelet(getId(), shared, line([(0.0, -2.0), (10.0, -2.0)]))
        for ll in (upper, lower):
            ll.attributes["subtype"] = "road"
        emit("change_override:{}".format(sorted(attributes.items())),
             [rules.canChangeLane(upper, lower), rules.canChangeLane(lower, upper)])

    # --- dynamic rules and succession ----------------------------------------
    emit("dynamic_none", rules.hasDynamicRules(lanelet("road")))

    first = lanelet("road")
    emit("can_pass_pair_unrelated", rules.canPass(first, lanelet("road")))

    # --- against the real map ------------------------------------------------
    real = load(data_path("mapping_example.osm"), UtmProjector(Origin(49.0, 8.4, 0.0)))
    for participant in ["vehicle", "pedestrian", "bicycle"]:
        r = t.create(t.Locations.Germany, participant)
        rows = []
        for ll in by_id(real.laneletLayer)[:120]:
            limit = r.speedLimit(ll)
            rows.append([ll.id, r.canPass(ll), r.canPass(ll.invert()), r.isOneWay(ll),
                         round(limit.speedLimitKmH, 6), limit.isMandatory,
                         r.hasDynamicRules(ll)])
        emit("real:{}".format(participant), rows)


run(main)
