//! Writes a Lanelet2 map as a plain OpenStreetMap road network.
//!
//! A Lanelet2 `.osm` file is OSM XML, but its ways are lane *bounds* and its roads are
//! `type=lanelet` relations, so a tool that reads OSM roads (`highway=*` centre lines that
//! meet at shared nodes) finds nothing in it. [`to_road_osm`] rewrites the map in that
//! vocabulary:
//!
//! * Laterally adjacent vehicle lanelets (sharing a bound) form one carriageway, written as
//!   one `highway=*` way along the middle of the outermost bounds, with `lanes`,
//!   `lanes:forward`/`lanes:backward` or `oneway=yes`, `width` and `maxspeed`.
//! * Lanelets with a `turn_direction` are the inside of an intersection. They are not
//!   written; every carriageway that enters or leaves them ends at one junction node.
//!   Carriageways that follow each other directly share their end node.
//! * Traffic lights become `highway=traffic_signals` on the junction node, stop signs
//!   `highway=stop` (with `direction`) on the approach, crosswalk lanelets
//!   `highway=footway` + `footway=crossing` ways with a `highway=crossing` node where they
//!   cross a road, and walkway lanelets `highway=footway` ways.
//! * A carriageway that passes over another one more than [`LEVEL_DZ`] higher gets
//!   `bridge=yes` + `layer=1`; one that passes under the local ground level gets
//!   `tunnel=yes` + `layer=-1`.
//!
//! The geometry is the map's own (no lane is re-derived from tags), so the result is as
//! close to the lanelets as a centre-line model allows.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ll2_core::geometry::distance::{middle_point, signed_distance_2d};
use ll2_core::id::Id;
use ll2_io::osm::{Document, Node, Tags, Way};
use ll2_projection::{GpsPoint, LocalCartesian, Origin, Projector};

/// Vehicle lanelet subtypes.
pub const VEHICLE_SUBTYPES: [&str; 4] = ["road", "highway", "bus_lane", "play_street"];
/// Two lanelets whose bound ends are this close (m) follow each other even without shared
/// points.
pub const END_TOL: f64 = 0.1;
/// Overlapping carriageways this far apart in z (m) are on two levels.
pub const LEVEL_DZ: f64 = 3.0;
/// Bounds of two opposite lanelets closer than this on average (m) are one centre line
/// drawn twice.
pub const COINCIDENT_M: f64 = 0.5;
/// Spacing of the written centre line (m).
const STEP: f64 = 5.0;
/// A one-way road with a speed limit at or below this (km/h) is written as
/// `highway=service`: a station rotary, a bus bay, a car park lane. Consumers keep such
/// roads out of the street junctions next to them.
pub const SERVICE_MAX_KMH: f64 = 15.0;

/// Choices a caller can make.
#[derive(Debug, Default, Clone)]
pub struct Options {
    /// A `ref` for every one-way carriageway. Lanelet2 maps carry no street names, and some
    /// consumers only pair the two carriageways of a divided road when they share a name or
    /// a ref (carla-digitaltwins' TwinModel, for one); with a common ref they pair them by
    /// geometry alone. Off by default: the ref is not data from the map.
    pub oneway_ref: Option<String>,
}

/// What the conversion made.
#[derive(Debug, Default, Clone)]
pub struct Report {
    pub vehicle_lanelets: usize,
    pub junction_lanelets: usize,
    pub roads: usize,
    pub junctions: usize,
    pub traffic_signals: usize,
    pub stops: usize,
    pub crossings: usize,
    pub footways: usize,
    pub bridges: usize,
    pub tunnels: usize,
    pub dead_ends: usize,
    /// Carriageways left out because they start and end in the same intersection, within
    /// its turn lanelets (a short stretch a turn lane runs alongside).
    pub inside_junctions: usize,
    /// Ways written as `highway=service` (`SERVICE_MAX_KMH`).
    pub service: usize,
}

type P3 = [f64; 3];

struct Lanelet {
    id: Id,
    left: Vec<P3>,
    right: Vec<P3>,
    left_way: Id,
    right_way: Id,
    left_ends: (Id, Id),
    right_ends: (Id, Id),
    tags: Tags,
    regelems: Vec<Id>,
}

impl Lanelet {
    fn centerline(&self) -> Vec<P3> {
        mid_line(&self.left, &self.right)
    }
    fn is_junction(&self) -> bool {
        self.tags.contains_key("turn_direction")
    }
    fn speed(&self) -> Option<f64> {
        self.tags
            .get("speed_limit")
            .and_then(|v| v.trim().parse().ok())
    }
}

// ----------------------------------------------------------------------------- geometry

fn dist2(a: &P3, b: &P3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn length(line: &[P3]) -> f64 {
    line.windows(2).map(|w| dist2(&w[0], &w[1])).sum()
}

/// `n` points evenly spaced by arc length.
fn resample(line: &[P3], n: usize) -> Vec<P3> {
    let total = length(line);
    if line.len() < 2 || total == 0.0 {
        return vec![line[0]; n.max(2)];
    }
    let mut out = Vec::with_capacity(n);
    let mut seg = 0;
    let mut acc = 0.0;
    for k in 0..n {
        let target = total * k as f64 / (n - 1) as f64;
        while seg + 1 < line.len() - 1 && acc + dist2(&line[seg], &line[seg + 1]) < target {
            acc += dist2(&line[seg], &line[seg + 1]);
            seg += 1;
        }
        let d = dist2(&line[seg], &line[seg + 1]);
        let t = if d > 0.0 {
            ((target - acc) / d).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (a, b) = (line[seg], line[seg + 1]);
        out.push([
            a[0] + t * (b[0] - a[0]),
            a[1] + t * (b[1] - a[1]),
            a[2] + t * (b[2] - a[2]),
        ]);
    }
    out
}

fn samples_for(a: &[P3], b: &[P3]) -> usize {
    ((length(a).max(length(b)) / STEP).ceil() as usize + 1).max(2)
}

fn mid_line(a: &[P3], b: &[P3]) -> Vec<P3> {
    let n = samples_for(a, b);
    resample(a, n)
        .iter()
        .zip(resample(b, n))
        .map(|(p, q)| {
            [
                (p[0] + q[0]) / 2.0,
                (p[1] + q[1]) / 2.0,
                (p[2] + q[2]) / 2.0,
            ]
        })
        .collect()
}

fn mean_width(a: &[P3], b: &[P3]) -> f64 {
    let n = samples_for(a, b);
    let (ra, rb) = (resample(a, n), resample(b, n));
    ra.iter().zip(&rb).map(|(p, q)| dist2(p, q)).sum::<f64>() / n as f64
}

fn direction(line: &[P3]) -> [f64; 2] {
    let (a, b) = (line[0], line[line.len() - 1]);
    [b[0] - a[0], b[1] - a[1]]
}

/// Signed lateral offset of `p` from `line` (positive on the left).
fn lateral(line: &[P3], p: &P3) -> f64 {
    let mut best = (f64::INFINITY, 0.0);
    for w in line.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dy * dy;
        let t = if l2 > 0.0 {
            (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (qx, qy) = (a[0] + t * dx, a[1] + t * dy);
        let d = ((p[0] - qx).powi(2) + (p[1] - qy).powi(2)).sqrt();
        if d < best.0 {
            let cross = dx * (p[1] - a[1]) - dy * (p[0] - a[0]);
            best = (d, if cross >= 0.0 { d } else { -d });
        }
    }
    best.1
}

fn bbox(a: &[P3], b: &[P3]) -> [f64; 4] {
    let mut r = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in a.iter().chain(b) {
        r = [
            r[0].min(p[0]),
            r[1].min(p[1]),
            r[2].max(p[0]),
            r[3].max(p[1]),
        ];
    }
    r
}

fn boxes_touch(a: &[f64; 4], b: &[f64; 4], margin: f64) -> bool {
    a[0] - margin <= b[2] && b[0] - margin <= a[2] && a[1] - margin <= b[3] && b[1] - margin <= a[3]
}

/// Two bounds that lie on top of each other along most of their length.
fn coincident_lines(a: &[P3], b: &[P3]) -> bool {
    let (la, lb) = (length(a), length(b));
    if la == 0.0 || lb == 0.0 || la.min(lb) / la.max(lb) < 0.7 {
        return false;
    }
    let near = |from: &[P3], to: &[P3]| {
        let pts = resample(from, 11);
        pts[1..10].iter().map(|p| lateral(to, p).abs()).sum::<f64>() / 9.0 < COINCIDENT_M
    };
    near(a, b) && near(b, a)
}

/// Intersection of segments `ab` and `cd`: the parameters along each.
fn seg_intersection(a: &P3, b: &P3, c: &P3, d: &P3) -> Option<(f64, f64)> {
    let r = [b[0] - a[0], b[1] - a[1]];
    let s = [d[0] - c[0], d[1] - c[1]];
    let den = r[0] * s[1] - r[1] * s[0];
    if den.abs() < 1e-12 {
        return None;
    }
    let qp = [c[0] - a[0], c[1] - a[1]];
    let t = (qp[0] * s[1] - qp[1] * s[0]) / den;
    let u = (qp[0] * r[1] - qp[1] * r[0]) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some((t, u))
}

fn lerp(a: &P3, b: &P3, t: f64) -> P3 {
    [
        a[0] + t * (b[0] - a[0]),
        a[1] + t * (b[1] - a[1]),
        a[2] + t * (b[2] - a[2]),
    ]
}

/// Convex hull of the points' x, y (Andrew's monotone chain), counter-clockwise.
fn convex_hull(pts: &[P3]) -> Vec<[f64; 2]> {
    let mut p: Vec<[f64; 2]> = pts.iter().map(|q| [q[0], q[1]]).collect();
    p.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    p.dedup();
    if p.len() < 3 {
        return p;
    }
    let cross = |o: &[f64; 2], a: &[f64; 2], b: &[f64; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    let mut h: Vec<[f64; 2]> = Vec::with_capacity(2 * p.len());
    for pass in 0..2 {
        let start = h.len();
        let it: Box<dyn Iterator<Item = &[f64; 2]>> = if pass == 0 {
            Box::new(p.iter())
        } else {
            Box::new(p.iter().rev())
        };
        for q in it {
            while h.len() >= start + 2 && cross(&h[h.len() - 2], &h[h.len() - 1], q) <= 0.0 {
                h.pop();
            }
            h.push(*q);
        }
        h.pop();
    }
    h
}

/// Whether `p` lies inside a counter-clockwise convex polygon, or within `END_TOL` of it.
fn in_convex(hull: &[[f64; 2]], p: &P3) -> bool {
    hull.len() >= 3
        && (0..hull.len()).all(|i| {
            let (a, b) = (hull[i], hull[(i + 1) % hull.len()]);
            let len = (b[0] - a[0]).hypot(b[1] - a[1]);
            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) >= -END_TOL * len
        })
}

// ----------------------------------------------------------------------------- union-find

struct Dsu(Vec<usize>);

impl Dsu {
    fn new(n: usize) -> Self {
        Dsu((0..n).collect())
    }
    fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.0[r] != r {
            r = self.0[r];
        }
        let mut y = x;
        while self.0[y] != r {
            let next = self.0[y];
            self.0[y] = r;
            y = next;
        }
        r
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.0[ra.max(rb)] = ra.min(rb);
        }
    }
}

// ----------------------------------------------------------------------------- reading

fn coords(doc: &Document, way: &Way, local: &HashMap<Id, P3>) -> Option<Vec<P3>> {
    let pts: Option<Vec<P3>> = way.nodes.iter().map(|n| local.get(n).copied()).collect();
    pts.filter(|p| p.len() >= 2 && doc.ways.contains_key(&way.id))
}

fn member(rel: &ll2_io::osm::Relation, role: &str) -> Option<Id> {
    rel.members
        .iter()
        .find(|m| m.role == role)
        .map(|m| m.reference)
}

fn lanelets(doc: &Document, local: &HashMap<Id, P3>, subtypes: &[&str]) -> Vec<Lanelet> {
    let mut out = Vec::new();
    for rel in doc.relations.values() {
        if rel.tags.get("type").map(String::as_str) != Some("lanelet") {
            continue;
        }
        let subtype = rel
            .tags
            .get("subtype")
            .map(String::as_str)
            .unwrap_or("road");
        if !subtypes.contains(&subtype) {
            continue;
        }
        if subtypes == VEHICLE_SUBTYPES
            && rel.tags.get("participant:vehicle").map(String::as_str) == Some("no")
        {
            continue;
        }
        let (Some(lw), Some(rw)) = (member(rel, "left"), member(rel, "right")) else {
            continue;
        };
        let (Some(lway), Some(rway)) = (doc.ways.get(&lw), doc.ways.get(&rw)) else {
            continue;
        };
        let (Some(mut left), Some(mut right)) =
            (coords(doc, lway, local), coords(doc, rway, local))
        else {
            continue;
        };
        let mut lnodes = lway.nodes.clone();
        let mut rnodes = rway.nodes.clone();
        // the bounds as Lanelet2 reads them: each one flipped until the other's middle point
        // lies on the correct side of it (upstream `geometry::align`), which also fixes the
        // lanelet's direction
        let flat = |v: &[P3]| v.iter().map(|p| [p[0], p[1]]).collect::<Vec<[f64; 2]>>();
        if let Some(m) = middle_point(&flat(&right)) {
            if signed_distance_2d(&flat(&left), m) >= 0.0 {
                left.reverse();
                lnodes.reverse();
            }
        }
        if let Some(m) = middle_point(&flat(&left)) {
            if signed_distance_2d(&flat(&right), m) <= 0.0 {
                right.reverse();
                rnodes.reverse();
            }
        }
        out.push(Lanelet {
            id: rel.id,
            left,
            right,
            left_way: lw,
            right_way: rw,
            left_ends: (lnodes[0], *lnodes.last().unwrap()),
            right_ends: (rnodes[0], *rnodes.last().unwrap()),
            tags: rel.tags.clone(),
            regelems: rel
                .members
                .iter()
                .filter(|m| m.role == "regulatory_element")
                .map(|m| m.reference)
                .collect(),
        });
    }
    out
}

/// `next[i]`: the lanelets whose bounds start where lanelet `i`'s end.
fn successors(ll: &[Lanelet]) -> Vec<Vec<usize>> {
    let mut by_start: HashMap<(Id, Id), Vec<usize>> = HashMap::new();
    for (i, l) in ll.iter().enumerate() {
        by_start
            .entry((l.left_ends.0, l.right_ends.0))
            .or_default()
            .push(i);
    }
    ll.iter()
        .enumerate()
        .map(|(i, a)| {
            let exact: Vec<usize> = by_start
                .get(&(a.left_ends.1, a.right_ends.1))
                .map(|v| v.iter().copied().filter(|&j| j != i).collect())
                .unwrap_or_default();
            if !exact.is_empty() {
                return exact;
            }
            let (al, ar) = (a.left[a.left.len() - 1], a.right[a.right.len() - 1]);
            ll.iter()
                .enumerate()
                .filter(|(j, b)| {
                    *j != i && dist2(&al, &b.left[0]) < END_TOL && dist2(&ar, &b.right[0]) < END_TOL
                })
                .map(|(j, _)| j)
                .collect()
        })
        .collect()
}

// ----------------------------------------------------------------------------- conversion

struct Carriageway {
    members: Vec<usize>,
    forward: Vec<bool>, // per member: drives along the way
    line: Vec<P3>,
    lanes_fwd: usize,
    lanes_bwd: usize,
    width: f64,
    speed: Option<f64>,
    highway_subtype: bool,
}

/// Builds the plain-OSM road network of a Lanelet2 document.
///
/// Intersections are found by Autoware's `turn_direction` lanelet tag; a map without it
/// (plain Lanelet2) is written as carriageways that meet at shared nodes.
pub fn to_road_osm(doc: &Document, options: &Options) -> Result<(Document, Report), String> {
    if doc.nodes.is_empty() {
        return Err("the map has no nodes".into());
    }
    let (lat0, lon0) = {
        let n = doc.nodes.len() as f64;
        let lat = doc.nodes.values().map(|p| p.lat).sum::<f64>() / n;
        let lon = doc.nodes.values().map(|p| p.lon).sum::<f64>() / n;
        if lat == 0.0 && lon == 0.0 {
            return Err("the nodes carry no lat/lon".into());
        }
        (lat, lon)
    };
    let proj = LocalCartesian::new(Origin::new(GpsPoint::new(lat0, lon0, 0.0)));
    let mut local: HashMap<Id, P3> = HashMap::new();
    for n in doc.nodes.values() {
        let p = proj
            .forward(GpsPoint::new(n.lat, n.lon, 0.0))
            .map_err(|e| e.message().to_string())?;
        local.insert(n.id, [p[0], p[1], n.ele]);
    }

    let ll = lanelets(doc, &local, &VEHICLE_SUBTYPES);
    let next = successors(&ll);
    let mut report = Report {
        vehicle_lanelets: ll.len(),
        ..Report::default()
    };

    // -- carriageways: plain lanelets sharing a bound -------------------------------------
    let plain: Vec<usize> = (0..ll.len()).filter(|&i| !ll[i].is_junction()).collect();
    report.junction_lanelets = ll.len() - plain.len();
    let mut dsu = Dsu::new(ll.len());
    let mut by_way: HashMap<Id, Vec<usize>> = HashMap::new();
    for &i in &plain {
        by_way.entry(ll[i].left_way).or_default().push(i);
        by_way.entry(ll[i].right_way).or_default().push(i);
    }
    for group in by_way.values() {
        for w in group.windows(2) {
            dsu.union(w[0], w[1]);
        }
    }
    // opposite lanes drawn with two coincident centre lines (separate ways on top of each
    // other) are one road too; a real median keeps them apart
    let boxes: Vec<[f64; 4]> = ll.iter().map(|l| bbox(&l.left, &l.right)).collect();
    for (a, &i) in plain.iter().enumerate() {
        for &j in &plain[a + 1..] {
            if !boxes_touch(&boxes[i], &boxes[j], COINCIDENT_M) {
                continue;
            }
            let (di, dj) = (direction(&ll[i].left), direction(&ll[j].left));
            if di[0] * dj[0] + di[1] * dj[1] >= 0.0 {
                continue;
            }
            let coincident = [&ll[i].left, &ll[i].right].iter().any(|a| {
                [&ll[j].left, &ll[j].right]
                    .iter()
                    .any(|b| coincident_lines(a, b))
            });
            if coincident {
                dsu.union(i, j);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &i in &plain {
        groups.entry(dsu.find(i)).or_default().push(i);
    }
    let mut ways: Vec<Carriageway> = Vec::new();
    let mut way_of: HashMap<usize, usize> = HashMap::new();
    for members in groups.into_values() {
        // the reference: the lanelet with the lowest id; the others by their side of it
        let r = *members.iter().min_by_key(|&&i| ll[i].id).unwrap();
        let rc = ll[r].centerline();
        let rd = direction(&rc);
        let mut placed: Vec<(f64, usize, bool)> = members
            .iter()
            .map(|&i| {
                let c = ll[i].centerline();
                let d = direction(&c);
                (
                    lateral(&rc, &c[c.len() / 2]),
                    i,
                    d[0] * rd[0] + d[1] * rd[1] >= 0.0,
                )
            })
            .collect();
        placed.sort_by(|a, b| b.0.total_cmp(&a.0)); // leftmost first
        let (_, li, lfwd) = placed[0];
        let (_, ri, rfwd) = placed[placed.len() - 1];
        let mut left = if lfwd {
            ll[li].left.clone()
        } else {
            ll[li].right.iter().rev().copied().collect()
        };
        let mut right = if rfwd {
            ll[ri].right.clone()
        } else {
            ll[ri].left.iter().rev().copied().collect()
        };
        let fwd = placed.iter().filter(|p| p.2).count();
        let bwd = placed.len() - fwd;
        // a road with lanes in both directions runs along the lanes of the reference's
        // direction; a one-way road along its lanes
        if fwd == 0 {
            std::mem::swap(&mut left, &mut right);
            left.reverse();
            right.reverse();
        }
        let line = mid_line(&left, &right);
        let width = mean_width(&left, &right);
        let speed = members
            .iter()
            .filter_map(|&i| ll[i].speed())
            .reduce(f64::max);
        let k = ways.len();
        let mut fw = Vec::new();
        let mut ms = Vec::new();
        for (_, i, f) in &placed {
            way_of.insert(*i, k);
            ms.push(*i);
            fw.push(if fwd == 0 { !*f } else { *f });
        }
        let highway_subtype = members
            .iter()
            .any(|&i| ll[i].tags.get("subtype").map(String::as_str) == Some("highway"));
        let (lanes_fwd, lanes_bwd) = if fwd == 0 { (bwd, 0) } else { (fwd, bwd) };
        ways.push(Carriageway {
            members: ms,
            forward: fw,
            line,
            lanes_fwd,
            lanes_bwd,
            width,
            speed,
            highway_subtype,
        });
    }
    report.roads = ways.len();

    // -- end nodes: carriageway ends and junction lanelets, joined by succession -----------
    // keys: 2 per carriageway (start, end), then 1 per junction lanelet
    let jkey = |i: usize| 2 * ways.len() + i;
    let mut ends = Dsu::new(2 * ways.len() + ll.len());
    let exit_key = |i: usize, ways: &Vec<Carriageway>, way_of: &HashMap<usize, usize>| -> usize {
        match way_of.get(&i) {
            Some(&k) => {
                let pos = ways[k].members.iter().position(|&m| m == i).unwrap();
                2 * k + usize::from(ways[k].forward[pos])
            }
            None => jkey(i),
        }
    };
    let entry_key = |i: usize, ways: &Vec<Carriageway>, way_of: &HashMap<usize, usize>| -> usize {
        match way_of.get(&i) {
            Some(&k) => {
                let pos = ways[k].members.iter().position(|&m| m == i).unwrap();
                2 * k + usize::from(!ways[k].forward[pos])
            }
            None => jkey(i),
        }
    };
    for (i, succ) in next.iter().enumerate() {
        for &j in succ {
            ends.union(exit_key(i, &ways, &way_of), entry_key(j, &ways, &way_of));
        }
    }
    // junction lanelets that cross each other belong to the same intersection
    let jl: Vec<usize> = (0..ll.len()).filter(|&i| ll[i].is_junction()).collect();
    let jlines: Vec<Vec<P3>> = jl.iter().map(|&i| ll[i].centerline()).collect();
    for a in 0..jl.len() {
        for b in a + 1..jl.len() {
            let crosses = jlines[a].windows(2).any(|s| {
                jlines[b]
                    .windows(2)
                    .any(|t| seg_intersection(&s[0], &s[1], &t[0], &t[1]).is_some())
            });
            if crosses {
                ends.union(jkey(jl[a]), jkey(jl[b]));
            }
        }
    }

    // one OSM node per set; a set with junction lanelets sits at their centre
    let mut set_points: HashMap<usize, (Vec<P3>, bool)> = HashMap::new();
    for (k, w) in ways.iter().enumerate() {
        for (side, p) in [(0, w.line[0]), (1, w.line[w.line.len() - 1])] {
            set_points
                .entry(ends.find(2 * k + side))
                .or_insert_with(|| (Vec::new(), false))
                .0
                .push(p);
        }
    }
    let mut junction_pts: HashMap<usize, Vec<P3>> = HashMap::new();
    for (a, &i) in jl.iter().enumerate() {
        junction_pts
            .entry(ends.find(jkey(i)))
            .or_default()
            .extend(jlines[a].iter().copied());
    }
    for (root, pts) in &junction_pts {
        let e = set_points
            .entry(*root)
            .or_insert_with(|| (Vec::new(), false));
        e.0 = pts.clone();
        e.1 = true;
    }

    let mut out = Document::default();
    let mut next_id: Id = 1;
    let mut new_node = |out: &mut Document, p: P3, tags: Tags| -> Id {
        let g = proj
            .reverse([p[0], p[1], 0.0])
            .expect("local cartesian reverse");
        let id = next_id;
        next_id += 1;
        out.nodes.insert(
            id,
            Node {
                id,
                lat: g.lat,
                lon: g.lon,
                ele: p[2],
                tags,
            },
        );
        id
    };
    let mut set_node: HashMap<usize, (Id, P3, bool)> = HashMap::new();
    for (root, (pts, is_junction)) in &set_points {
        let n = pts.len() as f64;
        let p = [
            pts.iter().map(|q| q[0]).sum::<f64>() / n,
            pts.iter().map(|q| q[1]).sum::<f64>() / n,
            pts.iter().map(|q| q[2]).sum::<f64>() / n,
        ];
        let id = new_node(&mut out, p, Tags::new());
        set_node.insert(*root, (id, p, *is_junction));
    }
    report.junctions = set_node.values().filter(|v| v.2).count();

    // a carriageway whose two ends meet in one intersection, with its middle inside the
    // intersection's turn lanelets, is a stretch a turn lane runs alongside (a right-turn
    // lane that leaves before a short through stretch and rejoins after it). Written, it
    // would be a way from the junction node back to itself; the turn lanelets already
    // stand for the traffic through it. A loop road that leaves a junction and comes back
    // to it keeps its middle outside the junction and is written.
    let hulls: HashMap<usize, Vec<[f64; 2]>> = junction_pts
        .iter()
        .map(|(root, pts)| (*root, convex_hull(pts)))
        .collect();
    let inside: Vec<bool> = ways
        .iter()
        .enumerate()
        .map(|(k, w)| {
            let root = ends.find(2 * k);
            root == ends.find(2 * k + 1)
                && hulls
                    .get(&root)
                    .is_some_and(|h| in_convex(h, &w.line[w.line.len() / 2]))
        })
        .collect();
    report.inside_junctions = inside.iter().filter(|&&b| b).count();

    // -- levels -----------------------------------------------------------------------------
    let level = levels(&ways);
    // -- the road ways ------------------------------------------------------------------------
    let mut road_nodes: Vec<Vec<(Id, P3)>> = Vec::new();
    let mut approach: Vec<[Id; 2]> = Vec::new(); // the node just before each end (stop signs)
    let mut way_ids: Vec<Id> = Vec::new();
    let mut set_degree: HashMap<usize, usize> = HashMap::new();
    for k in (0..ways.len()).filter(|&k| !inside[k]) {
        for side in 0..2 {
            *set_degree.entry(ends.find(2 * k + side)).or_default() += 1;
        }
    }
    for (k, w) in ways.iter().enumerate() {
        let (s, e) = (set_node[&ends.find(2 * k)], set_node[&ends.find(2 * k + 1)]);
        let mut nodes: Vec<(Id, P3)> = Vec::new();
        let interior = &w.line[1..w.line.len() - 1];
        let mut appr = [0, 0];
        if s.2 {
            nodes.push((s.0, s.1));
            let id = new_node(&mut out, w.line[0], Tags::new());
            nodes.push((id, w.line[0]));
            appr[0] = id;
        } else {
            nodes.push((s.0, s.1));
            appr[0] = s.0;
        }
        for p in interior {
            nodes.push((new_node(&mut out, *p, Tags::new()), *p));
        }
        let last = w.line[w.line.len() - 1];
        if e.2 {
            let id = new_node(&mut out, last, Tags::new());
            nodes.push((id, last));
            appr[1] = id;
        }
        nodes.push((e.0, e.1));
        if !e.2 {
            appr[1] = e.0;
        }
        for side in (0..2).filter(|_| !inside[k]) {
            let root = ends.find(2 * k + side);
            if set_degree[&root] == 1 && !set_node[&root].2 {
                report.dead_ends += 1;
            }
        }
        road_nodes.push(nodes);
        approach.push(appr);
    }

    // -- crossings and footways ---------------------------------------------------------------
    let crosswalks = lanelets(doc, &local, &["crosswalk"]);
    let walkways = lanelets(doc, &local, &["walkway"]);
    let tl_regelems: BTreeSet<Id> = doc
        .relations
        .values()
        .filter(|r| r.tags.get("subtype").map(String::as_str) == Some("traffic_light"))
        .map(|r| r.id)
        .collect();
    let mut foot_ways: Vec<(Vec<(Id, P3)>, Tags)> = Vec::new();
    for cw in &crosswalks {
        let signalised = cw.regelems.iter().any(|r| tl_regelems.contains(r));
        let kind = if signalised {
            "traffic_signals"
        } else {
            "marked"
        };
        let line = cw.centerline();
        let mut nodes: Vec<(f64, Id, P3)> = Vec::new();
        for (a, s) in line.windows(2).enumerate() {
            for rn in road_nodes.iter_mut() {
                let mut b = 0;
                while b + 1 < rn.len() {
                    if let Some((t, u)) = seg_intersection(&s[0], &s[1], &rn[b].1, &rn[b + 1].1) {
                        let p = lerp(&rn[b].1, &rn[b + 1].1, u);
                        let mut tags = Tags::new();
                        tags.insert("highway".into(), "crossing".into());
                        tags.insert("crossing".into(), kind.into());
                        let id = new_node(&mut out, p, tags);
                        rn.insert(b + 1, (id, p));
                        nodes.push((a as f64 + t, id, p));
                        report.crossings += 1;
                        b += 1;
                    }
                    b += 1;
                }
            }
        }
        let mut path: Vec<(f64, Id, P3)> = vec![
            (0.0, 0, line[0]),
            ((line.len() - 1) as f64, 0, line[line.len() - 1]),
        ];
        path.extend(nodes);
        path.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut ids = Vec::new();
        for (_, id, p) in path {
            let id = if id == 0 {
                new_node(&mut out, p, Tags::new())
            } else {
                id
            };
            ids.push((id, p));
        }
        let mut tags = Tags::new();
        tags.insert("highway".into(), "footway".into());
        tags.insert("footway".into(), "crossing".into());
        tags.insert("crossing".into(), kind.into());
        foot_ways.push((ids, tags));
    }
    for wk in &walkways {
        let ids: Vec<(Id, P3)> = wk
            .centerline()
            .into_iter()
            .map(|p| (new_node(&mut out, p, Tags::new()), p))
            .collect();
        let mut tags = Tags::new();
        tags.insert("highway".into(), "footway".into());
        foot_ways.push((ids, tags));
    }

    // -- traffic signals and stop signs --------------------------------------------------------
    let stop_regelems: BTreeSet<Id> = doc
        .relations
        .values()
        .filter(|r| {
            r.tags.get("subtype").map(String::as_str) == Some("traffic_sign")
                && r.members.iter().any(|m| {
                    m.role == "refers"
                        && doc
                            .ways
                            .get(&m.reference)
                            .and_then(|w| w.tags.get("subtype"))
                            .map(String::as_str)
                            == Some("stop_sign")
                })
        })
        .map(|r| r.id)
        .collect();
    let mut signal_nodes: BTreeSet<Id> = BTreeSet::new();
    let mut stop_nodes: BTreeMap<Id, &'static str> = BTreeMap::new();
    for (i, l) in ll.iter().enumerate() {
        let tl = l.regelems.iter().any(|r| tl_regelems.contains(r));
        let stop = l.regelems.iter().any(|r| stop_regelems.contains(r));
        if !tl && !stop {
            continue;
        }
        let key = ends.find(exit_key(i, &ways, &way_of));
        if tl {
            signal_nodes.insert(set_node[&key].0);
        }
        if stop {
            if let Some(&k) = way_of.get(&i) {
                let pos = ways[k].members.iter().position(|&m| m == i).unwrap();
                let fwd = ways[k].forward[pos];
                let node = approach[k][usize::from(fwd)];
                stop_nodes.insert(node, if fwd { "forward" } else { "backward" });
            }
        }
    }
    for id in &signal_nodes {
        out.nodes
            .get_mut(id)
            .unwrap()
            .tags
            .insert("highway".into(), "traffic_signals".into());
    }
    for (id, dir) in &stop_nodes {
        let tags = &mut out.nodes.get_mut(id).unwrap().tags;
        if !tags.contains_key("highway") {
            tags.insert("highway".into(), "stop".into());
            tags.insert("direction".into(), (*dir).into());
        }
    }
    report.traffic_signals = signal_nodes.len();
    report.stops = stop_nodes.len();

    // -- chain carriageways that simply continue into one way ----------------------------------
    // OSM splits a way only where something changes; a road cut at every lanelet boundary
    // would be short pieces that a junction's pull-back swallows whole.
    let signature = |k: usize, reversed: bool| {
        let w = &ways[k];
        let (f, b) = if reversed {
            (w.lanes_bwd, w.lanes_fwd)
        } else {
            (w.lanes_fwd, w.lanes_bwd)
        };
        (
            f,
            b,
            w.speed.map(|v| v.round() as i64),
            level[k],
            w.highway_subtype,
        )
    };
    let roots: Vec<usize> = (0..2 * ways.len()).map(|x| ends.find(x)).collect();
    let mut at_root: HashMap<usize, Vec<(usize, usize)>> = HashMap::new(); // root -> (way, side)
    for k in (0..ways.len()).filter(|&k| !inside[k]) {
        for side in 0..2 {
            at_root
                .entry(roots[2 * k + side])
                .or_default()
                .push((k, side));
        }
    }
    // the other carriageway end at a plain degree-2 node, if the road continues unchanged
    let continuation = |k: usize, side: usize, reversed: bool| -> Option<(usize, bool)> {
        let root = roots[2 * k + side];
        if set_node[&root].2 {
            return None;
        }
        let here = &at_root[&root];
        if here.len() != 2 {
            return None;
        }
        let &(k2, side2) = here.iter().find(|&&(kk, ss)| (kk, ss) != (k, side))?;
        if k2 == k {
            return None;
        }
        // leaving k through its end (side 1, not reversed) we enter k2 at its start
        let leaving_end = (side == 1) != reversed;
        let entering_start = side2 == 0;
        let rev2 = leaving_end != entering_start;
        (signature(k, reversed) == signature(k2, rev2)).then_some((k2, rev2))
    };
    let mut used = inside.clone();
    let mut chains: Vec<Vec<(usize, bool)>> = Vec::new();
    for k0 in 0..ways.len() {
        if used[k0] {
            continue;
        }
        used[k0] = true;
        // walk back from the start (travelling against k0), then forward from the end
        let mut back: Vec<(usize, bool)> = Vec::new();
        let (mut k, mut rev) = (k0, true);
        while let Some((k2, rev2)) =
            continuation(k, usize::from(!rev), rev).filter(|(k2, _)| !used[*k2])
        {
            used[k2] = true;
            back.push((k2, !rev2));
            (k, rev) = (k2, rev2);
        }
        let mut chain: Vec<(usize, bool)> = back.into_iter().rev().collect();
        chain.push((k0, false));
        let (mut k, mut rev) = (k0, false);
        while let Some((k2, rev2)) =
            continuation(k, usize::from(!rev), rev).filter(|(k2, _)| !used[*k2])
        {
            used[k2] = true;
            chain.push((k2, rev2));
            (k, rev) = (k2, rev2);
        }
        chains.push(chain);
    }

    // -- write the ways -------------------------------------------------------------------------
    let mut wid: Id = 1;
    for chain in &chains {
        let (k, rev0) = chain[0];
        let w = &ways[k];
        let (lanes_fwd, lanes_bwd) = if rev0 {
            (w.lanes_bwd, w.lanes_fwd)
        } else {
            (w.lanes_fwd, w.lanes_bwd)
        };
        let width = chain.iter().map(|&(k, _)| ways[k].width).sum::<f64>() / chain.len() as f64;
        let lanes = lanes_fwd + lanes_bwd;
        let per_dir = lanes_fwd.max(lanes_bwd);
        let slow = w.speed.is_some_and(|v| v <= SERVICE_MAX_KMH);
        let highway = if w.highway_subtype {
            "motorway"
        } else if lanes_bwd == 0 && slow {
            report.service += 1;
            "service"
        } else if per_dir >= 3 {
            "primary"
        } else if per_dir >= 2 {
            "secondary"
        } else if lanes_bwd > 0 {
            "tertiary"
        } else {
            "unclassified"
        };
        let mut tags = Tags::new();
        tags.insert("highway".into(), highway.into());
        tags.insert("lanes".into(), lanes.to_string());
        if lanes_bwd == 0 {
            tags.insert("oneway".into(), "yes".into());
            if let Some(r) = &options.oneway_ref {
                tags.insert("ref".into(), r.clone());
            }
        } else {
            tags.insert("lanes:forward".into(), lanes_fwd.to_string());
            tags.insert("lanes:backward".into(), lanes_bwd.to_string());
        }
        tags.insert("width".into(), format!("{width:.1}"));
        if let Some(v) = w.speed {
            tags.insert("maxspeed".into(), format!("{}", v.round() as i64));
        }
        match level[k] {
            l if l > 0 => {
                tags.insert("bridge".into(), "yes".into());
                tags.insert("layer".into(), l.to_string());
                report.bridges += 1;
            }
            l if l < 0 => {
                tags.insert("tunnel".into(), "yes".into());
                tags.insert("layer".into(), l.to_string());
                report.tunnels += 1;
            }
            _ => {}
        }
        let ids: Vec<String> = chain
            .iter()
            .flat_map(|&(k, _)| ways[k].members.iter().map(|&i| ll[i].id.to_string()))
            .collect();
        tags.insert("lanelet2:ids".into(), ids.join(";"));
        let mut nodes: Vec<Id> = Vec::new();
        for &(k, rev) in chain {
            let mut part: Vec<Id> = road_nodes[k].iter().map(|n| n.0).collect();
            if rev {
                part.reverse();
            }
            nodes.extend(part);
        }
        nodes.dedup();
        out.ways.insert(
            wid,
            Way {
                id: wid,
                nodes,
                tags,
            },
        );
        way_ids.push(wid);
        wid += 1;
    }
    for (nodes, tags) in foot_ways {
        let mut ids: Vec<Id> = nodes.iter().map(|n| n.0).collect();
        ids.dedup();
        if ids.len() < 2 {
            continue;
        }
        out.ways.insert(
            wid,
            Way {
                id: wid,
                nodes: ids,
                tags,
            },
        );
        wid += 1;
        report.footways += 1;
    }
    Ok((out, report))
}

/// `+n` for a carriageway over another one, `-n` for one under the local ground, else 0.
fn levels(ways: &[Carriageway]) -> Vec<i32> {
    let n = ways.len();
    let mut level = vec![0i32; n];
    let z_at = |w: &Carriageway, seg: usize, t: f64| lerp(&w.line[seg], &w.line[seg + 1], t)[2];
    let mut pairs: Vec<(usize, usize, f64, P3)> = Vec::new(); // (upper, lower, z of the lower, where)
    for a in 0..n {
        for b in a + 1..n {
            'found: for (sa, s) in ways[a].line.windows(2).enumerate() {
                for (sb, t) in ways[b].line.windows(2).enumerate() {
                    if let Some((ta, tb)) = seg_intersection(&s[0], &s[1], &t[0], &t[1]) {
                        let (za, zb) = (z_at(&ways[a], sa, ta), z_at(&ways[b], sb, tb));
                        let at = lerp(&s[0], &s[1], ta);
                        if (za - zb).abs() > LEVEL_DZ {
                            pairs.push(if za > zb {
                                (a, b, zb, at)
                            } else {
                                (b, a, za, at)
                            });
                        }
                        break 'found;
                    }
                }
            }
        }
    }
    if pairs.is_empty() {
        return level;
    }
    // the ground: the median z of the carriageways that cross nothing (decks and underpasses
    // would pull it towards themselves), near the crossing
    let crossing: BTreeSet<usize> = pairs.iter().flat_map(|&(u, l, _, _)| [u, l]).collect();
    let flat: Vec<&Carriageway> = ways
        .iter()
        .enumerate()
        .filter(|(k, _)| !crossing.contains(k))
        .map(|(_, w)| w)
        .collect();
    let flat: Vec<&Carriageway> = if flat.is_empty() {
        ways.iter().collect()
    } else {
        flat
    };
    let mut zs: Vec<f64> = flat
        .iter()
        .flat_map(|w| w.line.iter().map(|p| p[2]))
        .collect();
    zs.sort_by(f64::total_cmp);
    let ground = |at: &P3| -> f64 {
        let mut near: Vec<f64> = flat
            .iter()
            .flat_map(|w| w.line.iter())
            .filter(|p| dist2(p, at) < 150.0)
            .map(|p| p[2])
            .collect();
        if near.is_empty() {
            return zs[zs.len() / 2];
        }
        near.sort_by(f64::total_cmp);
        near[near.len() / 2]
    };
    for (up, lo, z_lo, at) in pairs {
        // the lower one runs below the ground there: an underpass; otherwise the upper one is
        // a deck over a street at ground level
        if z_lo < ground(&at) - LEVEL_DZ / 2.0 {
            level[lo] = level[lo].min(-1);
        } else {
            level[up] = level[up].max(1);
        }
    }
    level
}
