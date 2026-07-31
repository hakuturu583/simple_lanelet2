//! `lanelet2.geometry` — the free functions.
//!
//! Upstream registers these through Boost.Python's overload chain: `distance`
//! alone has 55 registrations. PyO3 has no overloading, so each function takes
//! untyped arguments and dispatches on what it was actually handed, in the same
//! order the overload chain would have tried.
//!
//! Most functions only need coordinates, so the dispatch flattens whatever
//! primitive it was given into a plain array first and shares one implementation
//! between all the accepted types.
//!
//! Ground truth: `lanelet2_python/python_api/geometry.cpp`.

use ll2_core::fmt::to_string_double;
use ll2_core::geometry::{bbox, distance as dist, lanelet as llgeom, linestring as lsgeom};
use ll2_core::geometry::linestring::{ArcCoordinates, Point2, Point3};
use ll2_core::map::Primitive;
use pyo3::prelude::*;
use pyo3::types::{PyList, PyModule, PyTuple};

use crate::core::area::area_of;
use crate::core::basic::{PyBasicPoint2d, PyBasicPoint3d, PyBoundingBox2d, PyBoundingBox3d};
use crate::core::compound::compound_of;
use crate::core::lanelet::lanelet_of;
use crate::core::linestring::linestring_of;
use crate::core::map::primitive_from_any;
use crate::core::point::point_of;
use crate::accept;
use crate::err::{argument_error, runtime};

// ---------------------------------------------------------------------------
// argument extraction
// ---------------------------------------------------------------------------

/// Reads a 3D coordinate from any point-shaped object.
fn coords_3d(obj: &Bound<'_, PyAny>) -> Option<Point3> {
    if let Ok(value) = obj.cast::<PyBasicPoint3d>() {
        return Some(value.borrow().view().xyz());
    }
    if let Ok(value) = obj.cast::<PyBasicPoint2d>() {
        let [x, y, _] = value.borrow().view().xyz();
        return Some([x, y, 0.0]);
    }
    point_of(obj).map(|(point, _)| point.xyz())
}

fn coords_2d(obj: &Bound<'_, PyAny>) -> Option<Point2> {
    coords_3d(obj).map(|[x, y, _]| [x, y])
}

/// What kind of shape a polyline argument came from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Open,
    Ring,
}

/// Reads a 3D polyline from any linestring-, polygon-, compound- or list-shaped
/// object, along with whether it is closed.
fn polyline_3d(obj: &Bound<'_, PyAny>) -> Option<(Vec<Point3>, Shape)> {
    if let Some((line, kind)) = linestring_of(obj) {
        let points = line.points().iter().map(ll2_core::point::Point::xyz).collect();
        return Some((points, if kind.polygon { Shape::Ring } else { Shape::Open }));
    }
    if let Some(compound) = compound_of(obj) {
        let points = compound.points().iter().map(ll2_core::point::Point::xyz).collect();
        // A compound polygon and a compound linestring are separate Python classes
        // but share this core type, so the class name is what tells them apart.
        let name = obj.get_type().name().ok()?.to_string();
        let shape = if name.contains("Polygon") { Shape::Ring } else { Shape::Open };
        return Some((points, shape));
    }
    // A plain list of basic points is accepted wherever a linestring is.
    let items = obj.try_iter().ok()?;
    let mut points = Vec::new();
    for item in items {
        points.push(coords_3d(&item.ok()?)?);
    }
    (!points.is_empty()).then_some((points, Shape::Open))
}

fn polyline_2d(obj: &Bound<'_, PyAny>) -> Option<(Vec<Point2>, Shape)> {
    polyline_3d(obj).map(|(points, shape)| {
        (points.into_iter().map(|[x, y, _]| [x, y]).collect(), shape)
    })
}

/// A polyline in whichever form the caller supplied, plus its shape.
fn any_line_2d(obj: &Bound<'_, PyAny>) -> Option<(Vec<Point2>, Shape)> {
    if let Some((lanelet, _)) = lanelet_of(obj) {
        return Some((llgeom::outline_2d(&lanelet), Shape::Ring));
    }
    if let Some(area) = area_of(obj) {
        let ring = area
            .outer_bound_polygon()
            .points()
            .iter()
            .map(|point| {
                let [x, y, _] = point.xyz();
                [x, y]
            })
            .collect();
        return Some((ring, Shape::Ring));
    }
    polyline_2d(obj)
}

/// Distance from a point to a shape, honouring whether the shape is closed.
fn point_to_shape_2d(point: Point2, line: &[Point2], shape: Shape) -> f64 {
    match shape {
        Shape::Open => dist::distance_2d_point_polyline(point, line),
        Shape::Ring => dist::distance_2d_point_ring(point, line),
    }
}

// ---------------------------------------------------------------------------
// ArcCoordinates
// ---------------------------------------------------------------------------

/// `lanelet2.geometry.ArcCoordinates`.
#[pyclass(name = "ArcCoordinates", module = "lanelet2.geometry")]
#[derive(Clone, Copy)]
pub struct PyArcCoordinates {
    pub(crate) arc: ArcCoordinates,
}

#[pymethods]
impl PyArcCoordinates {
    #[new]
    #[pyo3(signature = (length = 0.0, distance = 0.0))]
    fn new(length: f64, distance: f64) -> Self {
        PyArcCoordinates {
            arc: ArcCoordinates { length, distance },
        }
    }

    #[getter]
    fn length(&self) -> f64 {
        self.arc.length
    }

    #[setter(length)]
    fn set_length(&mut self, value: f64) {
        self.arc.length = value;
    }

    /// Signed, and **left of the direction of travel is positive**.
    #[getter]
    fn distance(&self) -> f64 {
        self.arc.distance
    }

    #[setter(distance)]
    fn set_distance(&mut self, value: f64) {
        self.arc.distance = value;
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match other.cast::<PyArcCoordinates>() {
            Ok(other) => self.arc == other.borrow().arc,
            Err(_) => false,
        }
    }

    fn __ne__(&self, other: &Bound<'_, PyAny>) -> bool {
        !self.__eq__(other)
    }

    /// Rendered with `std::to_string`, i.e. six *decimal places* rather than the
    /// six significant digits every other repr in the library uses.
    fn __repr__(&self) -> String {
        format!(
            "ArcCoordinates({}, {})",
            to_string_double(self.arc.length),
            to_string_double(self.arc.distance)
        )
    }

    fn __str__(&self) -> String {
        self.__repr__()
    }
}

// ---------------------------------------------------------------------------
// conversions
// ---------------------------------------------------------------------------

/// `to2D(x)` — the 2D counterpart of whatever it is handed.
///
/// Anything without a 2D counterpart is returned unchanged, which is what upstream
/// does rather than raising.
#[pyfunction]
#[pyo3(name = "to2D")]
fn to_2d<'py>(obj: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    crate::core::convert::to_2d(obj)
}

#[pyfunction]
#[pyo3(name = "to3D")]
fn to_3d<'py>(obj: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    crate::core::convert::to_3d(obj)
}

// ---------------------------------------------------------------------------
// distance and friends
// ---------------------------------------------------------------------------

/// `distance(a, b)` — between any two of point, linestring, polygon, lanelet, area.
#[pyfunction]
fn distance(a: &Bound<'_, PyAny>, b: &Bound<'_, PyAny>) -> PyResult<f64> {
    require_binary(a, b, accept::DISTANCE, "distance")?;
    // Both points: pick 3D only when both sides really are 3D.
    if let (Some(pa), Some(pb)) = (coords_3d(a), coords_3d(b)) {
        return Ok(if is_3d(a) && is_3d(b) {
            lsgeom::distance_3d(pa, pb)
        } else {
            dist::distance_2d_point_point([pa[0], pa[1]], [pb[0], pb[1]])
        });
    }
    // A point and a shape.
    if let Some(point) = coords_3d(a) {
        if let Some((line, shape)) = any_line_2d(b) {
            if is_3d(a) && shape == Shape::Open {
                if let Some((line3, _)) = polyline_3d(b) {
                    if is_3d(b) {
                        return Ok(distance_point_polyline_3d(point, &line3));
                    }
                }
            }
            return Ok(point_to_shape_2d([point[0], point[1]], &line, shape));
        }
    }
    if let Some(point) = coords_3d(b) {
        if let Some((line, shape)) = any_line_2d(a) {
            if is_3d(b) && shape == Shape::Open {
                if let Some((line3, _)) = polyline_3d(a) {
                    if is_3d(a) {
                        return Ok(distance_point_polyline_3d(point, &line3));
                    }
                }
            }
            return Ok(point_to_shape_2d([point[0], point[1]], &line, shape));
        }
    }
    // Two shapes.
    if let (Some((la, sa)), Some((lb, sb))) = (any_line_2d(a), any_line_2d(b)) {
        if is_3d(a) && is_3d(b) && sa == Shape::Open && sb == Shape::Open {
            if let (Some((a3, _)), Some((b3, _))) = (polyline_3d(a), polyline_3d(b)) {
                return Ok(match lsgeom::projected_point_3d(&a3, &b3) {
                    None => f64::INFINITY,
                    Some((pa, pb)) => lsgeom::distance_3d(pa, pb),
                });
            }
        }
        return Ok(shape_distance_2d(&la, sa, &lb, sb));
    }
    Err(argument_error("geometry", "distance"))
}

fn distance_point_polyline_3d(point: Point3, line: &[Point3]) -> f64 {
    match line.len() {
        0 => f64::INFINITY,
        1 => lsgeom::distance_3d(point, line[0]),
        _ => line
            .windows(2)
            .map(|segment| {
                lsgeom::distance_3d(
                    point,
                    lsgeom::project_onto_segment_3d(point, segment[0], segment[1]),
                )
            })
            .fold(f64::INFINITY, f64::min),
    }
}

/// Distance between two 2D shapes: zero when they touch or one contains the other.
fn shape_distance_2d(a: &[Point2], sa: Shape, b: &[Point2], sb: Shape) -> f64 {
    let edges = |line: &[Point2], shape: Shape| -> Vec<(Point2, Point2)> {
        let count = match shape {
            Shape::Open => line.len().saturating_sub(1),
            Shape::Ring => line.len(),
        };
        (0..count)
            .map(|index| (line[index], line[(index + 1) % line.len()]))
            .collect()
    };
    let (ea, eb) = (edges(a, sa), edges(b, sb));
    for (a0, a1) in &ea {
        for (b0, b1) in &eb {
            if lsgeom::segments_intersect_2d(*a0, *a1, *b0, *b1) {
                return 0.0;
            }
        }
    }
    if sb == Shape::Ring && a.first().is_some_and(|&p| dist::covered_by_ring(p, b)) {
        return 0.0;
    }
    if sa == Shape::Ring && b.first().is_some_and(|&p| dist::covered_by_ring(p, a)) {
        return 0.0;
    }
    let mut best = f64::INFINITY;
    for &p in a {
        best = best.min(point_to_shape_2d(p, b, sb));
    }
    for &p in b {
        best = best.min(point_to_shape_2d(p, a, sa));
    }
    best
}

/// The Python class name of an argument, or the empty string for a plain list.
fn class_name(obj: &Bound<'_, PyAny>) -> String {
    obj.get_type()
        .name()
        .map(|name| name.to_string())
        .unwrap_or_default()
}

/// Rejects an argument type upstream has no overload for.
fn require_unary(obj: &Bound<'_, PyAny>, allowed: &[&str], method: &str) -> PyResult<()> {
    if accept::accepts_unary(allowed, &class_name(obj)) {
        return Ok(());
    }
    Err(argument_error("geometry", method))
}

/// Rejects an argument *pair* upstream has no overload for.
fn require_binary(
    a: &Bound<'_, PyAny>,
    b: &Bound<'_, PyAny>,
    allowed: &[(&str, &str)],
    method: &str,
) -> PyResult<()> {
    if accept::accepts_binary(allowed, &class_name(a), &class_name(b)) {
        return Ok(());
    }
    Err(argument_error("geometry", method))
}

/// Whether an object carries a meaningful third coordinate.
fn is_3d(obj: &Bound<'_, PyAny>) -> bool {
    let name = class_name(obj);
    name.ends_with("3d") || matches!(name.as_str(), "Lanelet" | "ConstLanelet" | "Area" | "ConstArea")
}

/// `distanceToLines(point, lines)` — the smallest distance to any of them.
#[pyfunction]
#[pyo3(name = "distanceToLines")]
fn distance_to_lines(point: &Bound<'_, PyAny>, lines: &Bound<'_, PyAny>) -> PyResult<f64> {
    let point = coords_2d(point).ok_or_else(|| argument_error("geometry", "distanceToLines"))?;
    let mut best = f64::INFINITY;
    for item in lines.try_iter()? {
        let item = item?;
        let (line, shape) =
            any_line_2d(&item).ok_or_else(|| argument_error("geometry", "distanceToLines"))?;
        best = best.min(point_to_shape_2d(point, &line, shape));
    }
    Ok(best)
}

/// `equals(a, b)` — exact coordinate equality, *not* identity.
#[pyfunction]
fn equals(a: &Bound<'_, PyAny>, b: &Bound<'_, PyAny>) -> PyResult<bool> {
    require_binary(a, b, accept::EQUALS, "equals")?;
    let (Some(pa), Some(pb)) = (coords_3d(a), coords_3d(b)) else {
        return Err(argument_error("geometry", "equals"));
    };
    Ok(if is_3d(a) && is_3d(b) {
        pa == pb
    } else {
        pa[0] == pb[0] && pa[1] == pb[1]
    })
}

// ---------------------------------------------------------------------------
// bounding boxes
// ---------------------------------------------------------------------------

#[pyfunction]
#[pyo3(name = "boundingBox2d")]
fn bounding_box_2d(obj: &Bound<'_, PyAny>) -> PyResult<PyBoundingBox2d> {
    require_unary(obj, accept::BOUNDING_BOX_2D, "boundingBox2d")?;
    let primitive = primitive_from_any(obj);
    let box2d = match primitive {
        Some(primitive) => primitive.bounding_box_2d(),
        None => {
            let (line, _) = polyline_2d(obj)
                .ok_or_else(|| argument_error("geometry", "boundingBox2d"))?;
            let mut box2d = bbox::BoundingBox2d::empty();
            for point in line {
                box2d.extend_point(point);
            }
            box2d
        }
    };
    Ok(PyBoundingBox2d::from_corners(
        [box2d.min[0], box2d.min[1], 0.0],
        [box2d.max[0], box2d.max[1], 0.0],
    ))
}

#[pyfunction]
#[pyo3(name = "boundingBox3d")]
fn bounding_box_3d(obj: &Bound<'_, PyAny>) -> PyResult<PyBoundingBox3d> {
    require_unary(obj, accept::BOUNDING_BOX_3D, "boundingBox3d")?;
    let box3d = match primitive_from_any(obj) {
        Some(Primitive::Point(point)) => bbox::of_point_3d(&point),
        Some(Primitive::LineString(line)) | Some(Primitive::Polygon(line)) => {
            bbox::of_linestring_3d(&line)
        }
        Some(Primitive::Lanelet(lanelet)) => bbox::of_lanelet_3d(&lanelet),
        Some(Primitive::Area(area)) => bbox::of_area_3d(&area),
        _ => {
            let (line, _) = polyline_3d(obj)
                .ok_or_else(|| argument_error("geometry", "boundingBox3d"))?;
            let mut box3d = bbox::BoundingBox3d::empty();
            for point in line {
                box3d.extend_point(point);
            }
            box3d
        }
    };
    Ok(PyBoundingBox3d::from_corners(box3d.min, box3d.max))
}

// ---------------------------------------------------------------------------
// lengths, interpolation, projection
// ---------------------------------------------------------------------------

#[pyfunction]
fn length(obj: &Bound<'_, PyAny>) -> PyResult<f64> {
    require_unary(obj, accept::LENGTH, "length")?;
    let (line, _) = polyline_3d(obj).ok_or_else(|| argument_error("geometry", "length"))?;
    Ok(if is_3d(obj) {
        lsgeom::length_3d(&line)
    } else {
        lsgeom::length_2d(&line.into_iter().map(|[x, y, _]| [x, y]).collect::<Vec<_>>())
    })
}

#[pyfunction]
#[pyo3(name = "interpolatedPointAtDistance")]
fn interpolated_point_at_distance<'py>(
    py: Python<'py>,
    obj: &Bound<'py, PyAny>,
    distance: f64,
) -> PyResult<Bound<'py, PyAny>> {
    require_unary(obj, accept::INTERPOLATED_POINT_AT_DISTANCE, "interpolatedPointAtDistance")?;
    let (line, _) = polyline_3d(obj)
        .ok_or_else(|| argument_error("geometry", "interpolatedPointAtDistance"))?;
    if is_3d(obj) {
        let point = lsgeom::interpolated_point_at_distance_3d(&line, distance)
            .ok_or_else(|| runtime("Empty line string"))?;
        return Ok(Bound::new(py, PyBasicPoint3d::free(point))?.into_any());
    }
    let flat: Vec<Point2> = line.into_iter().map(|[x, y, _]| [x, y]).collect();
    let point = lsgeom::interpolated_point_at_distance_2d(&flat, distance)
        .ok_or_else(|| runtime("Empty line string"))?;
    Ok(Bound::new(py, PyBasicPoint2d::free([point[0], point[1], 0.0]))?.into_any())
}

#[pyfunction]
#[pyo3(name = "nearestPointAtDistance")]
fn nearest_point_at_distance<'py>(
    obj: &Bound<'py, PyAny>,
    distance: f64,
) -> PyResult<Bound<'py, PyAny>> {
    require_unary(obj, accept::NEAREST_POINT_AT_DISTANCE, "nearestPointAtDistance")?;
    let (flat, _) = polyline_2d(obj)
        .ok_or_else(|| argument_error("geometry", "nearestPointAtDistance"))?;
    let index = lsgeom::nearest_index_at_distance_2d(&flat, distance)
        .ok_or_else(|| runtime("Empty line string"))?;
    // The result is an existing vertex, so hand back the primitive itself.
    obj.get_item(index)
}

#[pyfunction]
fn project<'py>(
    py: Python<'py>,
    obj: &Bound<'py, PyAny>,
    point: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    require_binary(obj, point, accept::PROJECT, "project")?;
    let (line, _) = polyline_3d(obj).ok_or_else(|| argument_error("geometry", "project"))?;
    let target = coords_3d(point).ok_or_else(|| argument_error("geometry", "project"))?;
    if is_3d(obj) {
        let projected =
            lsgeom::project_3d(&line, target).ok_or_else(|| runtime("Empty line string"))?;
        return Ok(Bound::new(py, PyBasicPoint3d::free(projected))?.into_any());
    }
    let flat: Vec<Point2> = line.into_iter().map(|[x, y, _]| [x, y]).collect();
    let projected = lsgeom::project_2d(&flat, [target[0], target[1]])
        .ok_or_else(|| runtime("Empty line string"))?;
    Ok(Bound::new(py, PyBasicPoint2d::free([projected[0], projected[1], 0.0]))?.into_any())
}

#[pyfunction]
#[pyo3(name = "projectedPoint3d")]
fn projected_point_3d<'py>(
    py: Python<'py>,
    a: &Bound<'py, PyAny>,
    b: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyTuple>> {
    require_binary(a, b, accept::PROJECTED_POINT_3D, "projectedPoint3d")?;
    let (la, _) = polyline_3d(a).ok_or_else(|| argument_error("geometry", "projectedPoint3d"))?;
    let (lb, _) = polyline_3d(b).ok_or_else(|| argument_error("geometry", "projectedPoint3d"))?;
    let (pa, pb) =
        lsgeom::projected_point_3d(&la, &lb).ok_or_else(|| runtime("Empty line string"))?;
    PyTuple::new(
        py,
        [PyBasicPoint3d::free(pa), PyBasicPoint3d::free(pb)],
    )
}

// ---------------------------------------------------------------------------
// arc coordinates and curvature
// ---------------------------------------------------------------------------

#[pyfunction]
#[pyo3(name = "toArcCoordinates")]
fn to_arc_coordinates(
    obj: &Bound<'_, PyAny>,
    point: &Bound<'_, PyAny>,
) -> PyResult<PyArcCoordinates> {
    require_binary(obj, point, accept::TO_ARC_COORDINATES, "toArcCoordinates")?;
    let (line, _) =
        polyline_2d(obj).ok_or_else(|| argument_error("geometry", "toArcCoordinates"))?;
    let target = coords_2d(point).ok_or_else(|| argument_error("geometry", "toArcCoordinates"))?;
    Ok(PyArcCoordinates {
        arc: lsgeom::to_arc_coordinates(&line, target),
    })
}

#[pyfunction]
#[pyo3(name = "fromArcCoordinates")]
fn from_arc_coordinates(
    obj: &Bound<'_, PyAny>,
    arc: &PyArcCoordinates,
) -> PyResult<PyBasicPoint2d> {
    require_unary(obj, &["LineString2d", "CompoundLineString2d"], "fromArcCoordinates")?;
    let (line, _) =
        polyline_2d(obj).ok_or_else(|| argument_error("geometry", "fromArcCoordinates"))?;
    let point = lsgeom::from_arc_coordinates(&line, arc.arc).map_err(|e| runtime(e.0))?;
    Ok(PyBasicPoint2d::free([point[0], point[1], 0.0]))
}

#[pyfunction]
#[pyo3(name = "curvature2d")]
fn curvature_2d(
    p1: &Bound<'_, PyAny>,
    p2: &Bound<'_, PyAny>,
    p3: &Bound<'_, PyAny>,
) -> PyResult<f64> {
    let points = three_points(p1, p2, p3, "curvature2d")?;
    Ok(lsgeom::curvature_2d(points[0], points[1], points[2]))
}

#[pyfunction]
#[pyo3(name = "signedCurvature2d")]
fn signed_curvature_2d(
    p1: &Bound<'_, PyAny>,
    p2: &Bound<'_, PyAny>,
    p3: &Bound<'_, PyAny>,
) -> PyResult<f64> {
    let points = three_points(p1, p2, p3, "signedCurvature2d")?;
    Ok(lsgeom::signed_curvature_2d(points[0], points[1], points[2]))
}

fn three_points(
    p1: &Bound<'_, PyAny>,
    p2: &Bound<'_, PyAny>,
    p3: &Bound<'_, PyAny>,
    method: &str,
) -> PyResult<[Point2; 3]> {
    let read = |obj: &Bound<'_, PyAny>| {
        coords_2d(obj).ok_or_else(|| argument_error("geometry", method))
    };
    Ok([read(p1)?, read(p2)?, read(p3)?])
}

/// `area(ring)` — the signed area of a closed ring.
#[pyfunction]
fn area(obj: &Bound<'_, PyAny>) -> PyResult<f64> {
    require_unary(obj, accept::AREA, "area")?;
    let (ring, _) = polyline_2d(obj).ok_or_else(|| argument_error("geometry", "area"))?;
    Ok(lsgeom::ring_area(&ring))
}

// ---------------------------------------------------------------------------
// predicates
// ---------------------------------------------------------------------------

#[pyfunction]
#[pyo3(name = "intersects2d")]
fn intersects_2d(a: &Bound<'_, PyAny>, b: &Bound<'_, PyAny>) -> PyResult<bool> {
    require_binary(a, b, accept::INTERSECTS_2D, "intersects2d")?;
    if let (Ok(ba), Ok(bb)) = (a.cast::<PyBoundingBox3d>(), b.cast::<PyBoundingBox3d>()) {
        let (ba, bb) = (ba.borrow(), bb.borrow());
        let to_box = |min: [f64; 3], max: [f64; 3]| bbox::BoundingBox3d { min, max };
        return Ok(to_box(ba.min_values(), ba.max_values())
            .intersects(&to_box(bb.min_values(), bb.max_values())));
    }
    if let (Some((la, _)), Some((lb, _))) = (lanelet_of(a), lanelet_of(b)) {
        return Ok(llgeom::intersects_2d(&la, &lb));
    }
    let (Some((la, sa)), Some((lb, sb))) = (any_line_2d(a), any_line_2d(b)) else {
        return Err(argument_error("geometry", "intersects2d"));
    };
    Ok(shape_distance_2d(&la, sa, &lb, sb) == 0.0)
}

#[pyfunction]
#[pyo3(name = "intersects3d", signature = (a, b, heightTolerance = None))]
#[allow(non_snake_case)]
fn intersects_3d(
    a: &Bound<'_, PyAny>,
    b: &Bound<'_, PyAny>,
    heightTolerance: Option<f64>,
) -> PyResult<bool> {
    // Only the lanelet and bounding-box overloads give the tolerance a default, so
    // the linestring forms are unreachable from a two-argument call. Reproducing
    // that means the accepted set depends on the argument *count*.
    let allowed: &[(&str, &str)] = if heightTolerance.is_some() {
        accept::INTERSECTS_3D_WITH_TOLERANCE
    } else {
        accept::INTERSECTS_3D
    };
    require_binary(a, b, allowed, "intersects3d")?;
    let heightTolerance = heightTolerance.unwrap_or(3.0);
    if let (Ok(ba), Ok(bb)) = (a.cast::<PyBoundingBox3d>(), b.cast::<PyBoundingBox3d>()) {
        let (ba, bb) = (ba.borrow(), bb.borrow());
        let to_box = |min: [f64; 3], max: [f64; 3]| bbox::BoundingBox3d { min, max };
        return Ok(to_box(ba.min_values(), ba.max_values())
            .intersects(&to_box(bb.min_values(), bb.max_values())));
    }
    if let (Some((la, _)), Some((lb, _))) = (lanelet_of(a), lanelet_of(b)) {
        return Ok(llgeom::intersects_3d(&la, &lb, heightTolerance));
    }
    // For linestrings: they must meet in plan, and be within tolerance in height
    // where they do.
    let (Some((a3, _)), Some((b3, _))) = (polyline_3d(a), polyline_3d(b)) else {
        return Err(argument_error("geometry", "intersects3d"));
    };
    let flat = |line: &[Point3]| -> Vec<Point2> { line.iter().map(|&[x, y, _]| [x, y]).collect() };
    if !lsgeom::polylines_intersect_2d(&flat(&a3), &flat(&b3)) {
        return Ok(false);
    }
    Ok(match lsgeom::projected_point_3d(&a3, &b3) {
        None => false,
        Some((pa, pb)) => (pa[2] - pb[2]).abs() < heightTolerance,
    })
}

#[pyfunction]
fn inside(lanelet: &Bound<'_, PyAny>, point: &Bound<'_, PyAny>) -> PyResult<bool> {
    require_binary(lanelet, point, accept::INSIDE, "inside")?;
    let (lanelet, _) = lanelet_of(lanelet).ok_or_else(|| argument_error("geometry", "inside"))?;
    let point = coords_2d(point).ok_or_else(|| argument_error("geometry", "inside"))?;
    Ok(llgeom::inside(&lanelet, point))
}

macro_rules! lanelet_metric {
    ($py_name:literal, $rust:ident, $call:expr) => {
        #[pyfunction]
        #[pyo3(name = $py_name)]
        fn $rust(lanelet: &Bound<'_, PyAny>) -> PyResult<f64> {
            let (lanelet, _) =
                lanelet_of(lanelet).ok_or_else(|| argument_error("geometry", $py_name))?;
            let call: fn(&ll2_core::lanelet::Lanelet) -> f64 = $call;
            Ok(call(&lanelet))
        }
    };
}

lanelet_metric!("length2d", length_2d_fn, llgeom::length_2d);
lanelet_metric!("length3d", length_3d_fn, llgeom::length_3d);
lanelet_metric!("approximatedLength2d", approximated_length_2d_fn, llgeom::approximated_length_2d);

#[pyfunction]
#[pyo3(name = "distanceToCenterline2d")]
fn distance_to_centerline_2d(lanelet: &Bound<'_, PyAny>, point: &Bound<'_, PyAny>) -> PyResult<f64> {
    require_binary(lanelet, point, accept::DISTANCE_TO_CENTERLINE_2D, "distanceToCenterline2d")?;
    let (lanelet, _) =
        lanelet_of(lanelet).ok_or_else(|| argument_error("geometry", "distanceToCenterline2d"))?;
    let point =
        coords_2d(point).ok_or_else(|| argument_error("geometry", "distanceToCenterline2d"))?;
    Ok(llgeom::distance_to_centerline_2d(&lanelet, point))
}

#[pyfunction]
#[pyo3(name = "distanceToCenterline3d")]
fn distance_to_centerline_3d(lanelet: &Bound<'_, PyAny>, point: &Bound<'_, PyAny>) -> PyResult<f64> {
    require_binary(lanelet, point, accept::DISTANCE_TO_CENTERLINE_3D, "distanceToCenterline3d")?;
    let (lanelet, _) =
        lanelet_of(lanelet).ok_or_else(|| argument_error("geometry", "distanceToCenterline3d"))?;
    let point =
        coords_3d(point).ok_or_else(|| argument_error("geometry", "distanceToCenterline3d"))?;
    Ok(llgeom::distance_to_centerline_3d(&lanelet, point))
}

#[pyfunction]
#[pyo3(name = "overlaps2d")]
fn overlaps_2d(a: &Bound<'_, PyAny>, b: &Bound<'_, PyAny>) -> PyResult<bool> {
    require_binary(a, b, accept::OVERLAPS_2D, "overlaps2d")?;
    let (Some((la, _)), Some((lb, _))) = (lanelet_of(a), lanelet_of(b)) else {
        return Err(argument_error("geometry", "overlaps2d"));
    };
    Ok(llgeom::overlaps_2d(&la, &lb))
}

#[pyfunction]
#[pyo3(name = "overlaps3d", signature = (lanelet1, lanelet2, heightTolerance = 3.0))]
#[allow(non_snake_case)]
fn overlaps_3d(
    lanelet1: &Bound<'_, PyAny>,
    lanelet2: &Bound<'_, PyAny>,
    heightTolerance: f64,
) -> PyResult<bool> {
    let (Some((la, _)), Some((lb, _))) = (lanelet_of(lanelet1), lanelet_of(lanelet2)) else {
        return Err(argument_error("geometry", "overlaps3d"));
    };
    Ok(llgeom::overlaps_3d(&la, &lb, heightTolerance))
}

#[pyfunction]
#[pyo3(name = "intersectCenterlines2d")]
fn intersect_centerlines_2d<'py>(
    py: Python<'py>,
    a: &Bound<'_, PyAny>,
    b: &Bound<'_, PyAny>,
) -> PyResult<Bound<'py, PyList>> {
    let (Some((la, _)), Some((lb, _))) = (lanelet_of(a), lanelet_of(b)) else {
        return Err(argument_error("geometry", "intersectCenterlines2d"));
    };
    let points: Vec<PyBasicPoint2d> = llgeom::intersect_centerlines_2d(&la, &lb)
        .into_iter()
        .map(|[x, y]| PyBasicPoint2d::free([x, y, 0.0]))
        .collect();
    PyList::new(py, points)
}

macro_rules! lanelet_predicate {
    ($py_name:literal, $rust:ident, $call:expr) => {
        #[pyfunction]
        #[pyo3(name = $py_name)]
        fn $rust(a: &Bound<'_, PyAny>, b: &Bound<'_, PyAny>) -> PyResult<bool> {
            let (Some((la, _)), Some((lb, _))) = (lanelet_of(a), lanelet_of(b)) else {
                return Err(argument_error("geometry", $py_name));
            };
            let call: fn(&ll2_core::lanelet::Lanelet, &ll2_core::lanelet::Lanelet) -> bool = $call;
            Ok(call(&la, &lb))
        }
    };
}

lanelet_predicate!("leftOf", left_of, llgeom::left_of);
lanelet_predicate!("rightOf", right_of, llgeom::right_of);
lanelet_predicate!("follows", follows, llgeom::follows);

/// `intersection(a, b)` — where two polylines cross.
#[pyfunction]
fn intersection<'py>(
    py: Python<'py>,
    a: &Bound<'_, PyAny>,
    b: &Bound<'_, PyAny>,
) -> PyResult<Bound<'py, PyList>> {
    require_binary(a, b, accept::INTERSECTION, "intersection")?;
    let (Some((la, _)), Some((lb, _))) = (polyline_2d(a), polyline_2d(b)) else {
        return Err(argument_error("geometry", "intersection"));
    };
    let points: Vec<PyBasicPoint2d> = lsgeom::polyline_intersections_2d(&la, &lb)
        .into_iter()
        .map(|[x, y]| PyBasicPoint2d::free([x, y, 0.0]))
        .collect();
    PyList::new(py, points)
}

// ---------------------------------------------------------------------------
// map queries
// ---------------------------------------------------------------------------

/// `findNearest(layer, point, n)` — the `n` closest primitives with their distances.
#[pyfunction]
#[pyo3(name = "findNearest")]
fn find_nearest<'py>(
    py: Python<'py>,
    layer: &Bound<'py, PyAny>,
    point: &Bound<'py, PyAny>,
    n: usize,
) -> PyResult<Bound<'py, PyList>> {
    let target = coords_2d(point).ok_or_else(|| argument_error("geometry", "findNearest"))?;
    let candidates = layer.call_method1("nearest", (point.clone(), n))?;
    let mut scored: Vec<(f64, Py<PyAny>)> = Vec::new();
    for item in candidates.try_iter()? {
        let item = item?;
        let primitive =
            primitive_from_any(&item).ok_or_else(|| argument_error("geometry", "findNearest"))?;
        scored.push((primitive.distance_2d(target), item.unbind()));
    }
    scored.sort_by(|a, b| a.0.total_cmp(&b.0));
    let pairs: Vec<Bound<'py, PyTuple>> = scored
        .into_iter()
        .map(|(distance, object)| PyTuple::new(py, [distance.into_pyobject(py).unwrap().into_any().unbind(), object]))
        .collect::<PyResult<_>>()?;
    PyList::new(py, pairs)
}

macro_rules! find_within {
    ($py_name:literal, $rust:ident, $three_d:tt) => {
        /// Everything in the layer within `maxDist` of the geometry, closest first.
        ///
        /// The bound is inclusive, so a `maxDist` of zero returns only what actually
        /// touches the geometry.
        #[pyfunction]
        #[pyo3(name = $py_name, signature = (layer, geometry, maxDist = 0.0))]
        #[allow(non_snake_case)]
        fn $rust<'py>(
            py: Python<'py>,
            layer: &Bound<'py, PyAny>,
            geometry: &Bound<'py, PyAny>,
            maxDist: f64,
        ) -> PyResult<Bound<'py, PyList>> {
            let mut scored: Vec<(f64, Py<PyAny>)> = Vec::new();
            for item in layer.try_iter()? {
                let item = item?;
                let distance = distance(&item, geometry)?;
                if distance <= maxDist {
                    scored.push((distance, item.unbind()));
                }
            }
            scored.sort_by(|a, b| a.0.total_cmp(&b.0));
            let pairs: Vec<Bound<'py, PyTuple>> = scored
                .into_iter()
                .map(|(distance, object)| {
                    PyTuple::new(py, [distance.into_pyobject(py).unwrap().into_any().unbind(), object])
                })
                .collect::<PyResult<_>>()?;
            PyList::new(py, pairs)
        }
    };
}

find_within!("findWithin2d", find_within_2d, false);
find_within!("findWithin3d", find_within_3d, true);

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyArcCoordinates>()?;
    m.add_function(wrap_pyfunction!(to_2d, m)?)?;
    m.add_function(wrap_pyfunction!(to_3d, m)?)?;
    m.add_function(wrap_pyfunction!(distance, m)?)?;
    m.add_function(wrap_pyfunction!(distance_to_lines, m)?)?;
    m.add_function(wrap_pyfunction!(equals, m)?)?;
    m.add_function(wrap_pyfunction!(bounding_box_2d, m)?)?;
    m.add_function(wrap_pyfunction!(bounding_box_3d, m)?)?;
    m.add_function(wrap_pyfunction!(length, m)?)?;
    m.add_function(wrap_pyfunction!(interpolated_point_at_distance, m)?)?;
    m.add_function(wrap_pyfunction!(nearest_point_at_distance, m)?)?;
    m.add_function(wrap_pyfunction!(project, m)?)?;
    m.add_function(wrap_pyfunction!(projected_point_3d, m)?)?;
    m.add_function(wrap_pyfunction!(to_arc_coordinates, m)?)?;
    m.add_function(wrap_pyfunction!(from_arc_coordinates, m)?)?;
    m.add_function(wrap_pyfunction!(curvature_2d, m)?)?;
    m.add_function(wrap_pyfunction!(signed_curvature_2d, m)?)?;
    m.add_function(wrap_pyfunction!(area, m)?)?;
    m.add_function(wrap_pyfunction!(intersects_2d, m)?)?;
    m.add_function(wrap_pyfunction!(intersects_3d, m)?)?;
    m.add_function(wrap_pyfunction!(inside, m)?)?;
    m.add_function(wrap_pyfunction!(length_2d_fn, m)?)?;
    m.add_function(wrap_pyfunction!(length_3d_fn, m)?)?;
    m.add_function(wrap_pyfunction!(approximated_length_2d_fn, m)?)?;
    m.add_function(wrap_pyfunction!(distance_to_centerline_2d, m)?)?;
    m.add_function(wrap_pyfunction!(distance_to_centerline_3d, m)?)?;
    m.add_function(wrap_pyfunction!(overlaps_2d, m)?)?;
    m.add_function(wrap_pyfunction!(overlaps_3d, m)?)?;
    m.add_function(wrap_pyfunction!(intersect_centerlines_2d, m)?)?;
    m.add_function(wrap_pyfunction!(left_of, m)?)?;
    m.add_function(wrap_pyfunction!(right_of, m)?)?;
    m.add_function(wrap_pyfunction!(follows, m)?)?;
    m.add_function(wrap_pyfunction!(intersection, m)?)?;
    m.add_function(wrap_pyfunction!(find_nearest, m)?)?;
    m.add_function(wrap_pyfunction!(find_within_2d, m)?)?;
    m.add_function(wrap_pyfunction!(find_within_3d, m)?)?;
    Ok(())
}
