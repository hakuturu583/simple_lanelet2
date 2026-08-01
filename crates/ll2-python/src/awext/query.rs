//! `autoware_lanelet2_extension_python.utility.query`, minus its ROS half.
//!
//! Almost all of these are one filter over one layer, and the filters were read out
//! of `lib/query.cpp` rather than guessed — `getAllPartitions` accepts three type
//! values, and the pedestrian markings are split by *point count* rather than by any
//! attribute, which no amount of reasoning would have produced.
//!
//! Three of upstream's own bindings do not work: `trafficLights`,
//! `autowareTrafficLights` and `detectionAreas` return a C++ vector Boost.Python was
//! never taught to convert, so calling them raises `TypeError`. That is reproduced
//! rather than repaired — a caller who reaches for them today gets an error, and
//! silently starting to work would be a behaviour change nobody asked for.
//!
//! Upstream: `autoware_lanelet2_extension/lib/query.cpp`

use ll2_core::map::LaneletMap;
use pyo3::prelude::*;
use pyo3::types::PyList;

use crate::core::lanelet::{PyConstLanelet, lanelet_of};
use crate::core::linestring::{PyConstLineString3d, PyConstPolygon3d};
use crate::core::map::{PyLaneletMap, PyLaneletSubmap};
use crate::err::argument_error;

/// Reads a map out of either map class.
fn map_of(obj: &Bound<'_, PyAny>) -> Option<std::sync::Arc<LaneletMap>> {
    if let Ok(value) = obj.cast::<PyLaneletMap>() {
        return Some(value.borrow().inner_arc());
    }
    obj.cast::<PyLaneletSubmap>()
        .ok()
        .map(|value| value.borrow().inner_arc())
}

/// Reads a sequence of lanelets, whichever class they arrived as.
fn lanelets_of(obj: &Bound<'_, PyAny>, function: &str) -> PyResult<Vec<ll2_core::lanelet::Lanelet>> {
    let mut out = Vec::new();
    for item in obj.try_iter()? {
        let item = item?;
        let (lanelet, _) =
            lanelet_of(&item).ok_or_else(|| argument_error("query", function))?;
        out.push(lanelet);
    }
    Ok(out)
}

fn lanelet_list(py: Python<'_>, lanelets: Vec<ll2_core::lanelet::Lanelet>) -> PyResult<Py<PyAny>> {
    let list = PyList::empty(py);
    for lanelet in lanelets {
        list.append(Py::new(py, PyConstLanelet::wrap(lanelet))?)?;
    }
    Ok(list.into_any().unbind())
}

/// Every lanelet in the map, as the sequence the other queries take.
#[pyfunction]
#[pyo3(name = "laneletLayer")]
pub fn lanelet_layer(py: Python<'_>, map: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    let map = map_of(map).ok_or_else(|| argument_error("query", "laneletLayer"))?;
    lanelet_list(
        py,
        map.lanelets
            .all()
            .iter()
            .filter_map(ll2_core::map::as_lanelet)
            .cloned()
            .collect(),
    )
}

#[pyfunction]
#[pyo3(name = "subtypeLanelets")]
pub fn subtype_lanelets(
    py: Python<'_>,
    lanelets: &Bound<'_, PyAny>,
    subtype: &str,
) -> PyResult<Py<PyAny>> {
    let lanelets = lanelets_of(lanelets, "subtypeLanelets")?;
    let matching = lanelets
        .into_iter()
        .filter(|lanelet| {
            lanelet
                .attributes()
                .read()
                .get("subtype")
                .is_some_and(|value| value.value() == subtype)
        })
        .collect();
    lanelet_list(py, matching)
}

/// The named lanelet subtypes, each a `subtypeLanelets` with the string filled in.
macro_rules! subtype_query {
    ($rust:ident, $py:literal, $subtype:literal) => {
        #[pyfunction]
        #[pyo3(name = $py)]
        pub fn $rust(py: Python<'_>, lanelets: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
            subtype_lanelets(py, lanelets, $subtype)
        }
    };
}

subtype_query!(road_lanelets, "roadLanelets", "road");
subtype_query!(crosswalk_lanelets, "crosswalkLanelets", "crosswalk");
subtype_query!(walkway_lanelets, "walkwayLanelets", "walkway");
subtype_query!(shoulder_lanelets, "shoulderLanelets", "road_shoulder");

/// Collects the linestrings of a map whose `type` attribute passes a test.
fn linestrings_where(
    py: Python<'_>,
    map: &LaneletMap,
    keep: impl Fn(&str, usize) -> bool,
) -> PyResult<Py<PyAny>> {
    let list = PyList::empty(py);
    let mut matching: Vec<_> = map
        .line_strings
        .all()
        .iter()
        .filter_map(ll2_core::map::as_linestring)
        .cloned()
        .filter(|line| {
            let attributes = line.attributes();
            let attributes = attributes.read();
            let kind = attributes
                .get("type")
                .map(|value| value.value().to_owned())
                .unwrap_or_else(|| "none".to_owned());
            keep(&kind, line.len())
        })
        .collect();
    // Layers are unordered upstream; sorting keeps our own output reproducible.
    matching.sort_by_key(|line| line.id());
    for line in matching {
        list.append(Py::new(py, PyConstLineString3d::wrap(line))?)?;
    }
    Ok(list.into_any().unbind())
}

fn polygons_of_type(py: Python<'_>, map: &LaneletMap, wanted: &str) -> PyResult<Py<PyAny>> {
    let list = PyList::empty(py);
    let mut matching: Vec<_> = map
        .polygons
        .all()
        .iter()
        .filter_map(ll2_core::map::as_linestring)
        .cloned()
        .filter(|polygon| {
            let attributes = polygon.attributes();
            let attributes = attributes.read();
            attributes
                .get("type")
                .is_some_and(|value| value.value() == wanted)
        })
        .collect();
    matching.sort_by_key(|polygon| polygon.id());
    for polygon in matching {
        list.append(Py::new(py, PyConstPolygon3d::wrap(polygon))?)?;
    }
    Ok(list.into_any().unbind())
}

#[pyfunction]
#[pyo3(name = "getAllPolygonsByType")]
pub fn get_all_polygons_by_type(
    py: Python<'_>,
    map: &Bound<'_, PyAny>,
    kind: &str,
) -> PyResult<Py<PyAny>> {
    let map = map_of(map).ok_or_else(|| argument_error("query", "getAllPolygonsByType"))?;
    polygons_of_type(py, &map, kind)
}

macro_rules! polygon_query {
    ($rust:ident, $py:literal, $kind:literal) => {
        #[pyfunction]
        #[pyo3(name = $py)]
        pub fn $rust(py: Python<'_>, map: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
            let map = map_of(map).ok_or_else(|| argument_error("query", $py))?;
            polygons_of_type(py, &map, $kind)
        }
    };
}

polygon_query!(get_all_obstacle_polygons, "getAllObstaclePolygons", "obstacle");
polygon_query!(get_all_parking_lots, "getAllParkingLots", "parking_lot");

macro_rules! linestring_query {
    ($rust:ident, $py:literal, |$kind:ident, $size:ident| $test:expr) => {
        #[pyfunction]
        #[pyo3(name = $py)]
        pub fn $rust(py: Python<'_>, map: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
            let map = map_of(map).ok_or_else(|| argument_error("query", $py))?;
            linestrings_where(py, &map, |$kind, $size| $test)
        }
    };
}

linestring_query!(curbstones, "curbstones", |kind, _size| kind == "curbstone");
linestring_query!(get_all_parking_spaces, "getAllParkingSpaces", |kind, _size| {
    kind == "parking_space"
});
linestring_query!(get_all_fences, "getAllFences", |kind, _size| kind == "fence");
// Three type values, not one -- read from query.cpp rather than inferred from the name.
linestring_query!(get_all_partitions, "getAllPartitions", |kind, _size| {
    kind == "guard_rail" || kind == "fence" || kind == "wall"
});
// The two pedestrian-marking queries split on the *point count*, not on any
// attribute: three or more points is treated as an area, fewer as a line.
linestring_query!(
    get_all_pedestrian_polygon_markings,
    "getAllPedestrianPolygonMarkings",
    |kind, size| kind == "pedestrian_marking" && size >= 3
);
linestring_query!(
    get_all_pedestrian_line_markings,
    "getAllPedestrianLineMarkings",
    |kind, size| kind == "pedestrian_marking" && size < 3
);

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(lanelet_layer, m)?)?;
    m.add_function(wrap_pyfunction!(subtype_lanelets, m)?)?;
    m.add_function(wrap_pyfunction!(road_lanelets, m)?)?;
    m.add_function(wrap_pyfunction!(crosswalk_lanelets, m)?)?;
    m.add_function(wrap_pyfunction!(walkway_lanelets, m)?)?;
    m.add_function(wrap_pyfunction!(shoulder_lanelets, m)?)?;
    m.add_function(wrap_pyfunction!(get_all_polygons_by_type, m)?)?;
    m.add_function(wrap_pyfunction!(get_all_obstacle_polygons, m)?)?;
    m.add_function(wrap_pyfunction!(get_all_parking_lots, m)?)?;
    m.add_function(wrap_pyfunction!(get_all_parking_spaces, m)?)?;
    m.add_function(wrap_pyfunction!(get_all_partitions, m)?)?;
    m.add_function(wrap_pyfunction!(get_all_fences, m)?)?;
    m.add_function(wrap_pyfunction!(get_all_pedestrian_polygon_markings, m)?)?;
    m.add_function(wrap_pyfunction!(get_all_pedestrian_line_markings, m)?)?;
    m.add_function(wrap_pyfunction!(curbstones, m)?)?;
    Ok(())
}
