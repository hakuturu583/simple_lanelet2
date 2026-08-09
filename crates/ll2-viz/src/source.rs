//! Getting a drawable map out of a `.osm` file that nobody told you anything about.
//!
//! `lanelet2.io.load` asks for a projector and an origin, because a library that
//! guesses coordinates silently is a library that hands you a map a kilometre from
//! where you thought it was. A viewer is the one place where guessing is the right
//! answer: it has one file, no configuration, and the only thing it does with the
//! coordinates is draw them relative to each other.
//!
//! So this reads the origin *out of the file* — the centre of its own nodes — and
//! projects through UTM from there. Two things then remain to notice:
//!
//! * Maps written by Autoware's toolchain carry `local_x`/`local_y` tags holding the
//!   MGRS-grid coordinates, and some of them carry a placeholder `lat`/`lon` of zero
//!   alongside. Projecting those gives a map that is one point wide, so when the
//!   projected extent collapses and the local one does not, the local tags win.
//! * A file with no usable coordinates at all is an error, not an empty canvas.

use std::sync::Arc;

use ll2_core::map::{LaneletMap, as_lanelet, as_point};
use ll2_io::osm::Document;
use ll2_projection::{GpsPoint, LocalCartesian, Origin, Projector, Utm};

/// Where a point's x and y should come from.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CoordinateSource {
    /// Project lat/lon, unless that collapses and `local_x`/`local_y` does not.
    #[default]
    Auto,
    /// Always project lat/lon.
    Projected,
    /// Always use the `local_x`/`local_y` tags.
    LocalXy,
}

impl CoordinateSource {
    pub fn from_key(key: &str) -> Option<CoordinateSource> {
        match key {
            "auto" => Some(CoordinateSource::Auto),
            "projected" => Some(CoordinateSource::Projected),
            "local" | "local_xy" => Some(CoordinateSource::LocalXy),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            CoordinateSource::Auto => "auto",
            CoordinateSource::Projected => "projected",
            CoordinateSource::LocalXy => "local_xy",
        }
    }
}

/// How a caller wants the file read.
#[derive(Clone, Copy, Debug, Default)]
pub struct LoadOptions {
    pub coordinates: CoordinateSource,
    /// Overrides the origin taken from the file. Only affects `Projected`.
    pub origin: Option<GpsPoint>,
}

/// A loaded map, plus what had to be decided to load it.
pub struct LoadedMap {
    pub map: Arc<LaneletMap>,
    /// Non-fatal problems, in the shape `loadRobust` reports them — a header line
    /// followed by one bulleted line each.
    pub errors: Vec<String>,
    /// How many problems that is. Reported here rather than left for a caller to
    /// derive by subtracting one for the header it cannot see.
    pub problems: usize,
    /// The origin the projection was anchored at.
    pub origin: GpsPoint,
    /// What `Auto` settled on, or what was asked for.
    pub coordinates: CoordinateSource,
    /// Whether the file carried a UTM zone the origin could be placed in.
    pub projection: &'static str,
}

/// Reads OSM XML into a map, deciding the projection from the file itself.
pub fn load_osm_str(text: &str, options: &LoadOptions) -> Result<LoadedMap, String> {
    let (document, mut errors) = ll2_io::osm::parse(text).map_err(|error| error.to_string())?;
    if document.nodes.is_empty() {
        return Err("The file contains no nodes — is it a Lanelet2 OSM map?".to_owned());
    }

    let origin = options.origin.unwrap_or_else(|| origin_of(&document));
    let (projector, projection): (Box<dyn Projector>, &'static str) =
        match Utm::new(Origin::new(origin), true, false) {
            Ok(utm) => (Box::new(utm), "utm"),
            // UTM is undefined near the poles; ENU is defined everywhere and is the
            // same thing to within a rounding error over a map-sized area.
            Err(_) => (
                Box::new(LocalCartesian::new(Origin::new(origin))),
                "local_cartesian",
            ),
        };

    let (map, load_errors) = ll2_io::load::to_map(&document, projector.as_ref());
    errors.extend(load_errors);

    // Each of these walks the whole point layer, cloning an `Arc` per point, and
    // `local_xy_extent` parses two attributes on top — so they are asked for only
    // when the answer can change the outcome.
    let projected_extent = extent(&map);
    let coordinates = match options.coordinates {
        CoordinateSource::Auto => resolve_auto(projected_extent, local_xy_extent(&map)),
        explicit => explicit,
    };
    let final_extent = if coordinates == CoordinateSource::LocalXy {
        apply_local_xy(&map);
        extent(&map)
    } else {
        projected_extent
    };

    if final_extent <= 0.0 {
        return Err(
            "Every node in the file projects to the same place — the map has no extent to draw"
                .to_owned(),
        );
    }

    let problems = errors.len();
    Ok(LoadedMap {
        map,
        errors: ll2_io::wrap_errors(ll2_io::PARSE_ERROR_HEADER, errors),
        problems,
        origin,
        coordinates,
        projection,
    })
}

/// Picks the coordinate source, given how big the map turns out to be either way.
///
/// The threshold is a metre: no real map is smaller than that, and a placeholder
/// `lat`/`lon` of zero on every node projects to an extent of exactly zero.
fn resolve_auto(projected_extent: f64, local_extent: Option<f64>) -> CoordinateSource {
    match local_extent {
        Some(local) if projected_extent < 1.0 && local >= 1.0 => CoordinateSource::LocalXy,
        _ => CoordinateSource::Projected,
    }
}

/// The median of the file's own latitudes and longitudes.
///
/// The median rather than the mean or the box centre, because both of those are
/// moved by a single node: OSM editors leave nodes at (0, 0) behind, and an origin
/// dragged out to sea takes the UTM zone with it and skews the whole map. Half the
/// nodes have to be wrong before the median cares.
fn origin_of(document: &Document) -> GpsPoint {
    let mut lats: Vec<f64> = Vec::with_capacity(document.nodes.len());
    let mut lons: Vec<f64> = Vec::with_capacity(document.nodes.len());
    for node in document.nodes.values() {
        if node.lat.is_finite() && node.lon.is_finite() {
            lats.push(node.lat);
            lons.push(node.lon);
        }
    }
    GpsPoint::new(
        median(&mut lats).clamp(-90.0, 90.0),
        median(&mut lons).clamp(-180.0, 180.0),
        0.0,
    )
}

/// The middle element, by value. Reorders the slice; the caller owns it anyway.
fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let middle = values.len() / 2;
    values.select_nth_unstable_by(middle, |a, b| a.total_cmp(b));
    values[middle]
}

/// The larger side of the map's bounding box, in metres.
fn extent(map: &LaneletMap) -> f64 {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for primitive in map.points.all() {
        let Some(point) = as_point(&primitive) else {
            continue;
        };
        let [x, y, _] = point.xyz();
        for (axis, value) in [x, y].into_iter().enumerate() {
            if !value.is_finite() {
                continue;
            }
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    }
    if min[0] > max[0] {
        return 0.0;
    }
    (max[0] - min[0]).max(max[1] - min[1])
}

/// The same measure over the `local_x`/`local_y` tags, or `None` when the file does
/// not carry them on enough of its nodes to be worth trusting.
fn local_xy_extent(map: &LaneletMap) -> Option<f64> {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    let mut tagged = 0usize;
    let total = map.points.len();

    for primitive in map.points.all() {
        let Some(point) = as_point(&primitive) else {
            continue;
        };
        let Some((x, y)) = local_xy_of(point) else {
            continue;
        };
        tagged += 1;
        for (axis, value) in [x, y].into_iter().enumerate() {
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    }

    // A handful of tagged nodes in an otherwise untagged file is a leftover, not a
    // coordinate system.
    if total == 0 || tagged * 2 < total {
        return None;
    }
    Some((max[0] - min[0]).max(max[1] - min[1]))
}

fn local_xy_of(point: &ll2_core::point::Point) -> Option<(f64, f64)> {
    let attributes = point.attributes().read();
    let x = attributes.get("local_x")?.as_double()?;
    let y = attributes.get("local_y")?.as_double()?;
    if x.is_finite() && y.is_finite() {
        Some((x, y))
    } else {
        None
    }
}

/// Overwrites projected coordinates with the file's own local ones.
///
/// Points that carry no tags keep what the projection gave them; mixing the two
/// would be wrong, but the alternative — dropping the point — breaks every
/// linestring that references it, and `local_xy_extent` has already established that
/// such points are a minority.
fn apply_local_xy(map: &LaneletMap) {
    for primitive in map.points.all() {
        let Some(point) = as_point(&primitive) else {
            continue;
        };
        if let Some((x, y)) = local_xy_of(point) {
            point.set_x(x);
            point.set_y(y);
        }
    }
    // Centerlines are cached on first use and are derived from point coordinates,
    // so anything computed during loading now describes the old frame.
    for primitive in map.lanelets.all() {
        if let Some(lanelet) = as_lanelet(&primitive) {
            lanelet.reset_cache();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEOREFERENCED: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<osm version='0.6'>
  <node id='1' lat='49.0000000' lon='8.4000000'/>
  <node id='2' lat='49.0000000' lon='8.4010000'/>
  <node id='3' lat='49.0001000' lon='8.4000000'/>
  <node id='4' lat='49.0001000' lon='8.4010000'/>
  <way id='10'>
    <nd ref='1'/><nd ref='2'/>
    <tag k='type' v='line_thin'/><tag k='subtype' v='solid'/>
  </way>
  <way id='11'>
    <nd ref='3'/><nd ref='4'/>
    <tag k='type' v='line_thin'/><tag k='subtype' v='dashed'/>
  </way>
  <relation id='20'>
    <member type='way' ref='11' role='left'/>
    <member type='way' ref='10' role='right'/>
    <tag k='type' v='lanelet'/><tag k='subtype' v='road'/>
  </relation>
</osm>"#;

    /// The Autoware shape: real geometry in `local_x`/`local_y`, placeholder lat/lon.
    const LOCAL_ONLY: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<osm version='0.6'>
  <node id='1' lat='0' lon='0'>
    <tag k='local_x' v='100.0'/><tag k='local_y' v='200.0'/>
  </node>
  <node id='2' lat='0' lon='0'>
    <tag k='local_x' v='140.0'/><tag k='local_y' v='200.0'/>
  </node>
  <node id='3' lat='0' lon='0'>
    <tag k='local_x' v='100.0'/><tag k='local_y' v='203.5'/>
  </node>
  <node id='4' lat='0' lon='0'>
    <tag k='local_x' v='140.0'/><tag k='local_y' v='203.5'/>
  </node>
  <way id='10'><nd ref='1'/><nd ref='2'/><tag k='type' v='line_thin'/></way>
  <way id='11'><nd ref='3'/><nd ref='4'/><tag k='type' v='line_thin'/></way>
  <relation id='20'>
    <member type='way' ref='11' role='left'/>
    <member type='way' ref='10' role='right'/>
    <tag k='type' v='lanelet'/><tag k='subtype' v='road'/>
  </relation>
</osm>"#;

    #[test]
    fn a_georeferenced_file_projects_and_lands_near_its_own_origin() {
        let loaded = load_osm_str(GEOREFERENCED, &LoadOptions::default()).unwrap();
        assert_eq!(loaded.coordinates, CoordinateSource::Projected);
        assert_eq!(loaded.projection, "utm");
        assert!(
            (loaded.origin.lat - 49.0001).abs() < 1e-6,
            "{}",
            loaded.origin.lat
        );
        // The origin's own easting and northing are subtracted, so the map straddles
        // zero rather than sitting at six-digit UTM values.
        assert!(extent(&loaded.map) > 10.0 && extent(&loaded.map) < 200.0);
        assert_eq!(loaded.map.lanelets.len(), 1);
        assert!(loaded.errors.is_empty());
    }

    #[test]
    fn placeholder_latlon_falls_back_to_the_local_tags() {
        let loaded = load_osm_str(LOCAL_ONLY, &LoadOptions::default()).unwrap();
        assert_eq!(loaded.coordinates, CoordinateSource::LocalXy);
        let point = loaded.map.points.get(1).unwrap();
        let [x, y, _] = as_point(&point).unwrap().xyz();
        assert_eq!([x, y], [100.0, 200.0]);
    }

    #[test]
    fn a_real_projection_is_not_overridden_by_local_tags() {
        // Both coordinate systems present and both usable: the georeferenced one is
        // authoritative, because that is what `lanelet2.io.load` would have used.
        let text = GEOREFERENCED.replace(
            "<node id='1' lat='49.0000000' lon='8.4000000'/>",
            "<node id='1' lat='49.0000000' lon='8.4000000'>\
             <tag k='local_x' v='0'/><tag k='local_y' v='0'/></node>",
        );
        let loaded = load_osm_str(&text, &LoadOptions::default()).unwrap();
        assert_eq!(loaded.coordinates, CoordinateSource::Projected);
    }

    #[test]
    fn the_source_can_be_forced_either_way() {
        let forced = LoadOptions {
            coordinates: CoordinateSource::Projected,
            ..LoadOptions::default()
        };
        // Forcing projection on a placeholder file leaves nothing to draw, and that
        // is reported rather than rendered as a blank page.
        assert!(load_osm_str(LOCAL_ONLY, &forced).is_err());

        let forced = LoadOptions {
            coordinates: CoordinateSource::LocalXy,
            ..LoadOptions::default()
        };
        let loaded = load_osm_str(LOCAL_ONLY, &forced).unwrap();
        assert_eq!(loaded.coordinates, CoordinateSource::LocalXy);
    }

    #[test]
    fn an_explicit_origin_wins_over_the_file() {
        let options = LoadOptions {
            origin: Some(GpsPoint::new(49.0, 8.4, 0.0)),
            ..LoadOptions::default()
        };
        let loaded = load_osm_str(GEOREFERENCED, &options).unwrap();
        assert_eq!(loaded.origin.lat, 49.0);
    }

    #[test]
    fn a_file_with_no_nodes_is_an_error_rather_than_an_empty_map() {
        // `LoadedMap` holds an `Arc<LaneletMap>`, which is not `Debug`, so the error
        // has to be matched out rather than unwrapped.
        let Err(error) = load_osm_str(
            "<?xml version='1.0'?><osm version='0.6'/>",
            &Default::default(),
        ) else {
            panic!("a node-less document loaded");
        };
        assert!(error.contains("no nodes"), "{error}");
    }

    #[test]
    fn malformed_xml_is_reported_not_panicked_on() {
        assert!(load_osm_str("not xml at all <<<", &Default::default()).is_err());
    }

    #[test]
    fn a_stray_node_at_null_island_does_not_move_the_origin() {
        let text = GEOREFERENCED.replace(
            "<node id='4'",
            "<node id='99' lat='0' lon='0'/>\n  <node id='4'",
        );
        let (document, _) = ll2_io::osm::parse(&text).unwrap();
        let origin = origin_of(&document);
        // The box centre would be 24.5 here and the mean 39.2; the median is 49.
        assert!((origin.lat - 49.0).abs() < 1e-3, "{}", origin.lat);
        assert!((origin.lon - 8.4).abs() < 1e-3, "{}", origin.lon);
    }

    #[test]
    fn the_real_example_map_loads_and_has_lanelets() {
        let text = std::fs::read_to_string("../../tests/data/mapping_example.osm").unwrap();
        let loaded = load_osm_str(&text, &LoadOptions::default()).unwrap();
        assert!(loaded.map.lanelets.len() > 50);
        assert_eq!(loaded.coordinates, CoordinateSource::Projected);
    }
}
