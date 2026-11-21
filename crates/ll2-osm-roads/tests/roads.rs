//! Small hand-built Lanelet2 maps through `to_road_osm`.

use std::collections::HashMap;

use ll2_core::id::Id;
use ll2_io::osm::{Document, Member, MemberType, Node, Relation, Tags, Way};
use ll2_osm_roads::{Options, to_road_osm};

const LAT0: f64 = 35.0;
const LON0: f64 = 139.0;
const W: f64 = 3.5;

/// Builds a document in metres around (LAT0, LON0); a point used twice is one node.
#[derive(Default)]
struct Map {
    doc: Document,
    next: Id,
    at: HashMap<(i64, i64), Id>,
}

impl Map {
    fn id(&mut self) -> Id {
        self.next += 1;
        self.next
    }

    fn node(&mut self, x: f64, y: f64) -> Id {
        let key = ((x * 1000.0).round() as i64, (y * 1000.0).round() as i64);
        if let Some(&id) = self.at.get(&key) {
            return id;
        }
        let id = self.id();
        let lat = LAT0 + y / 111_320.0;
        let lon = LON0 + x / (111_320.0 * LAT0.to_radians().cos());
        self.doc.nodes.insert(
            id,
            Node {
                id,
                lat,
                lon,
                ele: 0.0,
                tags: Tags::new(),
            },
        );
        self.at.insert(key, id);
        id
    }

    fn way(&mut self, points: &[(f64, f64)], kind: &str) -> Id {
        let nodes = points.iter().map(|&(x, y)| self.node(x, y)).collect();
        let id = self.id();
        let tags = Tags::from([("type".to_string(), kind.to_string())]);
        self.doc.ways.insert(id, Way { id, nodes, tags });
        id
    }

    fn lanelet(&mut self, left: Id, right: Id, tags: &[(&str, &str)], regelems: &[Id]) -> Id {
        let id = self.id();
        let mut members = vec![
            Member {
                kind: MemberType::Way,
                reference: left,
                role: "left".into(),
            },
            Member {
                kind: MemberType::Way,
                reference: right,
                role: "right".into(),
            },
        ];
        for &r in regelems {
            members.push(Member {
                kind: MemberType::Relation,
                reference: r,
                role: "regulatory_element".into(),
            });
        }
        let mut t = Tags::from([
            ("type".to_string(), "lanelet".to_string()),
            ("subtype".to_string(), "road".to_string()),
        ]);
        for (k, v) in tags {
            t.insert(k.to_string(), v.to_string());
        }
        self.doc.relations.insert(
            id,
            Relation {
                id,
                members,
                tags: t,
            },
        );
        id
    }

    fn traffic_light(&mut self) -> Id {
        let id = self.id();
        let tags = Tags::from([
            ("type".to_string(), "regulatory_element".to_string()),
            ("subtype".to_string(), "traffic_light".to_string()),
        ]);
        self.doc.relations.insert(
            id,
            Relation {
                id,
                members: vec![],
                tags,
            },
        );
        id
    }

    /// A two-way road along x from `x0` to `x1` at y = 0: one eastbound and one westbound
    /// lanelet sharing the centre line, every way drawn west to east.
    fn two_way_x(&mut self, x0: f64, x1: f64) {
        let centre = self.way(&[(x0, 0.0), (x1, 0.0)], "line_thin");
        let south = self.way(&[(x0, -W), (x1, -W)], "road_border");
        let north = self.way(&[(x0, W), (x1, W)], "road_border");
        self.lanelet(centre, south, &[], &[]);
        self.lanelet(centre, north, &[], &[]);
    }

    fn two_way_y(&mut self, y0: f64, y1: f64) {
        let centre = self.way(&[(0.0, y0), (0.0, y1)], "line_thin");
        let east = self.way(&[(W, y0), (W, y1)], "road_border");
        let west = self.way(&[(-W, y0), (-W, y1)], "road_border");
        self.lanelet(centre, east, &[], &[]);
        self.lanelet(centre, west, &[], &[]);
    }
}

fn roads(doc: &Document) -> Vec<&Way> {
    doc.ways
        .values()
        .filter(|w| w.tags.contains_key("lanes"))
        .collect()
}

#[test]
fn a_two_way_road_cut_into_lanelets_is_one_way() {
    let mut m = Map::default();
    m.two_way_x(0.0, 100.0);
    m.two_way_x(100.0, 200.0);
    let (out, report) = to_road_osm(&m.doc, &Options::default()).unwrap();
    assert_eq!(report.vehicle_lanelets, 4);
    let r = roads(&out);
    assert_eq!(r.len(), 1, "{r:?}");
    let t = &r[0].tags;
    assert_eq!(t["lanes"], "2");
    assert_eq!(t["lanes:forward"], "1");
    assert_eq!(t["lanes:backward"], "1");
    assert!(!t.contains_key("oneway"));
    let width: f64 = t["width"].parse().unwrap();
    assert!((width - 2.0 * W).abs() < 0.1, "{width}");
    // the way runs the whole 200 m
    let first = &out.nodes[&r[0].nodes[0]];
    let last = &out.nodes[r[0].nodes.last().unwrap()];
    let dx = (last.lon - first.lon).abs() * 111_320.0 * LAT0.to_radians().cos();
    assert!((dx - 200.0).abs() < 1.0, "{dx}");
}

#[test]
fn opposite_lanes_on_two_coincident_centre_lines_are_one_road() {
    let mut m = Map::default();
    let east_centre = m.way(&[(0.0, 0.0), (100.0, 0.0)], "line_thin");
    let west_centre = m.way(&[(0.0, 0.05), (100.0, 0.05)], "line_thin");
    let south = m.way(&[(0.0, -W), (100.0, -W)], "road_border");
    let north = m.way(&[(0.0, W), (100.0, W)], "road_border");
    m.lanelet(east_centre, south, &[], &[]);
    m.lanelet(west_centre, north, &[], &[]);
    let (out, _) = to_road_osm(&m.doc, &Options::default()).unwrap();
    let r = roads(&out);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].tags["lanes:forward"], "1");
    assert_eq!(r[0].tags["lanes:backward"], "1");
}

#[test]
fn an_intersection_is_one_signalised_node() {
    let mut m = Map::default();
    let d = 20.0;
    m.two_way_x(-120.0, -d);
    m.two_way_x(d, 120.0);
    m.two_way_y(-120.0, -d);
    m.two_way_y(d, 120.0);
    let tl = m.traffic_light();
    let straight = [("turn_direction", "straight")];
    // east-west through lanes
    let c = m.way(&[(-d, 0.0), (d, 0.0)], "virtual");
    let s = m.way(&[(-d, -W), (d, -W)], "virtual");
    let n = m.way(&[(-d, W), (d, W)], "virtual");
    m.lanelet(c, s, &straight, &[tl]);
    m.lanelet(c, n, &straight, &[tl]);
    // north-south through lanes
    let c = m.way(&[(0.0, -d), (0.0, d)], "virtual");
    let e = m.way(&[(W, -d), (W, d)], "virtual");
    let w = m.way(&[(-W, -d), (-W, d)], "virtual");
    m.lanelet(c, e, &straight, &[tl]);
    m.lanelet(c, w, &straight, &[tl]);

    let (out, report) = to_road_osm(&m.doc, &Options::default()).unwrap();
    assert_eq!(report.junction_lanelets, 4);
    assert_eq!(report.junctions, 1);
    assert_eq!(report.traffic_signals, 1);
    let r = roads(&out);
    assert_eq!(r.len(), 4);
    // the four arms meet at one node, the signalised one
    let mut ends: HashMap<Id, usize> = HashMap::new();
    for w in &r {
        *ends.entry(w.nodes[0]).or_default() += 1;
        *ends.entry(*w.nodes.last().unwrap()).or_default() += 1;
    }
    let (&centre, _) = ends
        .iter()
        .find(|&(_, &k)| k == 4)
        .expect("a node shared by all four arms");
    assert_eq!(out.nodes[&centre].tags["highway"], "traffic_signals");
}

#[test]
fn one_way_roads_get_the_requested_ref() {
    let mut m = Map::default();
    let left = m.way(&[(0.0, 0.0), (100.0, 0.0)], "line_thin");
    let right = m.way(&[(0.0, -W), (100.0, -W)], "line_thin");
    m.lanelet(left, right, &[], &[]);
    let (plain, _) = to_road_osm(&m.doc, &Options::default()).unwrap();
    assert_eq!(roads(&plain)[0].tags["oneway"], "yes");
    assert!(!roads(&plain)[0].tags.contains_key("ref"));
    let options = Options {
        oneway_ref: Some("L2".into()),
    };
    let (tagged, _) = to_road_osm(&m.doc, &options).unwrap();
    assert_eq!(roads(&tagged)[0].tags["ref"], "L2");
}

#[test]
fn a_map_without_coordinates_is_refused() {
    assert!(to_road_osm(&Document::default(), &Options::default()).is_err());
}
