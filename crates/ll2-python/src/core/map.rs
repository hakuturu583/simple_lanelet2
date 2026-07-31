//! Map layers, `LaneletMap`, `LaneletSubmap` and the `createMapFrom*` helpers.
//!
//! Adding to a `LaneletMap` pulls in everything the primitive references and gives
//! ids to anything that has none. A `LaneletSubmap` records only what it is handed,
//! though it still assigns that object an id.
//!
//! Upstream: `lanelet2_python/python_api/core.cpp:430-453, 1425-1516`

use std::sync::Arc;

use ll2_core::id::Id;
use ll2_core::map::{LaneletMap, LayerKind, Primitive};
use pyo3::prelude::*;
use pyo3::types::PyList;

use crate::core::area::{PyArea, area_of};
use crate::core::basic::{PyBasicPoint2d, PyBoundingBox2d};
use crate::core::lanelet::{PyLanelet, lanelet_of};
use crate::core::linestring::{PyLineString3d, PyPolygon3d, linestring_of};
use crate::core::point::{PyPoint3d, point_of};
use crate::core::regelem::regelem_of;
use crate::err::{argument_error, runtime};

/// Wraps a stored primitive in the Python class its layer hands out.
fn primitive_to_py(py: Python<'_>, primitive: &Primitive) -> PyResult<Py<PyAny>> {
    Ok(match primitive {
        Primitive::Point(point) => Py::new(py, PyPoint3d::wrap(point.clone()))?.into_any(),
        Primitive::LineString(line) => Py::new(py, PyLineString3d::wrap(line.clone()))?.into_any(),
        Primitive::Polygon(polygon) => Py::new(py, PyPolygon3d::wrap(polygon.clone()))?.into_any(),
        Primitive::Lanelet(lanelet) => Py::new(py, PyLanelet::wrap(lanelet.clone()))?.into_any(),
        Primitive::Area(area) => Py::new(py, PyArea::wrap(area.clone()))?.into_any(),
        Primitive::RegulatoryElement(regelem) => {
            let list = crate::core::lanelet::regelems_to_py(py, std::slice::from_ref(regelem))?;
            list.bind(py).get_item(0)?.unbind()
        }
    })
}

/// Reads any primitive Python object into a storable primitive.
pub fn primitive_from_any(obj: &Bound<'_, PyAny>) -> Option<Primitive> {
    if let Some((line, kind)) = linestring_of(obj) {
        return Some(if kind.polygon {
            Primitive::Polygon(line)
        } else {
            Primitive::LineString(line)
        });
    }
    if let Some((lanelet, _)) = lanelet_of(obj) {
        return Some(Primitive::Lanelet(lanelet));
    }
    if let Some(area) = area_of(obj) {
        return Some(Primitive::Area(area));
    }
    if let Some(regelem) = regelem_of(obj) {
        return Some(Primitive::RegulatoryElement(regelem));
    }
    point_of(obj).map(|(point, _)| Primitive::Point(point))
}

/// Empty base classes upstream gives the lanelet and area layers.
///
/// They carry no members of their own, but they are part of the class hierarchy
/// and show up in `__bases__`.
#[pyclass(
    name = "PrimitiveLayerLanelet",
    module = "lanelet2.core",
    subclass,
    frozen
)]
pub struct PyPrimitiveLayerLanelet;

#[pyclass(
    name = "PrimitiveLayerArea",
    module = "lanelet2.core",
    subclass,
    frozen
)]
pub struct PyPrimitiveLayerArea;

macro_rules! layer_class {
    ($py_name:literal, $rust:ident, $kind:expr, $usages:tt) => {
        layer_class!($py_name, $rust, $kind, $usages, );
    };

    ($py_name:literal, $rust:ident, $kind:expr, $usages:tt, $($base:tt)*) => {
        #[doc = concat!("`lanelet2.core.", $py_name, "`.")]
        #[pyclass(name = $py_name, module = "lanelet2.core", frozen $($base)*)]
        pub struct $rust {
            map: Arc<LaneletMap>,
        }

        impl $rust {
            pub fn wrap(map: Arc<LaneletMap>) -> Self {
                $rust { map }
            }

            fn layer(&self) -> &ll2_core::map::Layer {
                self.map.layer($kind)
            }
        }

        #[pymethods]
        impl $rust {
            fn exists(&self, id: Id) -> bool {
                self.layer().exists(id)
            }

            fn get(&self, py: Python<'_>, id: Id) -> PyResult<Py<PyAny>> {
                let primitive = self.layer().get(id).map_err(|e| runtime(e.0))?;
                primitive_to_py(py, &primitive)
            }

            /// Accepts an id, or a primitive whose id is looked up.
            fn __contains__(&self, value: &Bound<'_, PyAny>) -> bool {
                if let Ok(id) = value.extract::<Id>() {
                    return self.layer().exists(id);
                }
                primitive_from_any(value).is_some_and(|p| self.layer().exists(p.id()))
            }

            fn __getitem__(&self, py: Python<'_>, id: Id) -> PyResult<Py<PyAny>> {
                self.get(py, id)
            }

            fn __len__(&self) -> usize {
                self.layer().len()
            }

            fn __iter__(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
                let py = slf.py();
                let list = PyList::empty(py);
                for primitive in slf.layer().all() {
                    list.append(primitive_to_py(py, &primitive)?)?;
                }
                Ok(list.try_iter()?.into())
            }

            /// Everything whose bounding box meets the given one.
            fn search(&self, py: Python<'_>, area: &PyBoundingBox2d) -> PyResult<Py<PyAny>> {
                let min = area.min_values();
                let max = area.max_values();
                let query = ll2_core::geometry::BoundingBox2d {
                    min: [min[0], min[1]],
                    max: [max[0], max[1]],
                };
                let list = PyList::empty(py);
                for primitive in self.layer().search(&query) {
                    list.append(primitive_to_py(py, &primitive)?)?;
                }
                Ok(list.into_any().unbind())
            }

            /// The `n` nearest primitives, closest first.
            #[pyo3(signature = (point, n = 1))]
            fn nearest(
                &self,
                py: Python<'_>,
                point: &PyBasicPoint2d,
                n: usize,
            ) -> PyResult<Py<PyAny>> {
                let values = point.values();
                let list = PyList::empty(py);
                for primitive in self.layer().nearest([values[0], values[1]], n) {
                    list.append(primitive_to_py(py, &primitive)?)?;
                }
                Ok(list.into_any().unbind())
            }

            /// A fresh id, drawn from the same global counter as `getId`.
            #[pyo3(name = "uniqueId")]
            fn unique_id(&self) -> Id {
                self.layer().unique_id()
            }

        }

        layer_class!(@usages $usages, $rust);
    };

    (@usages false, $rust:ident) => {};
    (@usages true, $rust:ident) => {
        #[pymethods]
        impl $rust {
            /// The primitives in this layer that reference the given one.
            #[pyo3(name = "findUsages")]
            fn find_usages(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
                let target = primitive_from_any(value)
                    .ok_or_else(|| argument_error("Layer", "findUsages"))?;
                let list = PyList::empty(py);
                for primitive in self.layer().find_usages(target.id()) {
                    list.append(primitive_to_py(py, &primitive)?)?;
                }
                Ok(list.into_any().unbind())
            }
        }
    };
}

layer_class!("PointLayer", PyPointLayer, LayerKind::Point, false);
layer_class!(
    "LineStringLayer",
    PyLineStringLayer,
    LayerKind::LineString,
    true
);
layer_class!("PolygonLayer", PyPolygonLayer, LayerKind::Polygon, true);
layer_class!("LaneletLayer", PyLaneletLayer, LayerKind::Lanelet, true,
             , extends = PyPrimitiveLayerLanelet);
layer_class!("AreaLayer", PyAreaLayer, LayerKind::Area, true,
             , extends = PyPrimitiveLayerArea);
layer_class!(
    "RegulatoryElementLayer",
    PyRegulatoryElementLayer,
    LayerKind::RegulatoryElement,
    false
);

/// The six layers, shared by `LaneletMap` and `LaneletSubmap`.
#[pyclass(name = "LaneletMapLayers", module = "lanelet2.core", subclass, frozen)]
pub struct PyLaneletMapLayers {
    map: Arc<LaneletMap>,
}

impl PyLaneletMapLayers {
    fn wrap(map: Arc<LaneletMap>) -> Self {
        PyLaneletMapLayers { map }
    }
}

macro_rules! map_class {
    ($py_name:literal, $rust:ident, $recursive:tt) => {
        #[doc = concat!("`lanelet2.core.", $py_name, "`.")]
        #[pyclass(name = $py_name, module = "lanelet2.core", extends = PyLaneletMapLayers, frozen)]
        pub struct $rust {
            map: Arc<LaneletMap>,
        }

        impl $rust {
            pub fn wrap(map: Arc<LaneletMap>) -> (Self, PyLaneletMapLayers) {
                ($rust { map: map.clone() }, PyLaneletMapLayers::wrap(map))
            }

            pub fn inner(&self) -> &LaneletMap {
                &self.map
            }

            pub fn inner_arc(&self) -> Arc<LaneletMap> {
                self.map.clone()
            }
        }

        #[pymethods]
        impl $rust {
            #[new]
            fn new() -> (Self, PyLaneletMapLayers) {
                let map = if $recursive {
                    LaneletMap::new_map()
                } else {
                    LaneletMap::new_submap()
                };
                $rust::wrap(map)
            }

            #[getter]
            #[pyo3(name = "pointLayer")]
            fn point_layer(&self) -> PyPointLayer {
                PyPointLayer::wrap(self.map.clone())
            }

            #[getter]
            #[pyo3(name = "lineStringLayer")]
            fn line_string_layer(&self) -> PyLineStringLayer {
                PyLineStringLayer::wrap(self.map.clone())
            }

            #[getter]
            #[pyo3(name = "polygonLayer")]
            fn polygon_layer(&self) -> PyPolygonLayer {
                PyPolygonLayer::wrap(self.map.clone())
            }

            #[getter]
            #[pyo3(name = "laneletLayer")]
            fn lanelet_layer(&self, py: Python<'_>) -> PyResult<Py<PyLaneletLayer>> {
                Py::new(
                    py,
                    (
                        PyLaneletLayer::wrap(self.map.clone()),
                        PyPrimitiveLayerLanelet,
                    ),
                )
            }

            #[getter]
            #[pyo3(name = "areaLayer")]
            fn area_layer(&self, py: Python<'_>) -> PyResult<Py<PyAreaLayer>> {
                Py::new(
                    py,
                    (PyAreaLayer::wrap(self.map.clone()), PyPrimitiveLayerArea),
                )
            }

            #[getter]
            #[pyo3(name = "regulatoryElementLayer")]
            fn regulatory_element_layer(&self) -> PyRegulatoryElementLayer {
                PyRegulatoryElementLayer::wrap(self.map.clone())
            }

            fn add(&self, value: &Bound<'_, PyAny>) -> PyResult<()> {
                let primitive =
                    primitive_from_any(value).ok_or_else(|| argument_error($py_name, "add"))?;
                self.map.add(primitive);
                Ok(())
            }
        }
    };
}

#[pymethods]
impl PyLaneletMapLayers {
    #[getter]
    #[pyo3(name = "pointLayer")]
    fn point_layer(&self) -> PyPointLayer {
        PyPointLayer::wrap(self.map.clone())
    }

    #[getter]
    #[pyo3(name = "lineStringLayer")]
    fn line_string_layer(&self) -> PyLineStringLayer {
        PyLineStringLayer::wrap(self.map.clone())
    }

    #[getter]
    #[pyo3(name = "polygonLayer")]
    fn polygon_layer(&self) -> PyPolygonLayer {
        PyPolygonLayer::wrap(self.map.clone())
    }

    #[getter]
    #[pyo3(name = "laneletLayer")]
    fn lanelet_layer(&self, py: Python<'_>) -> PyResult<Py<PyLaneletLayer>> {
        Py::new(
            py,
            (
                PyLaneletLayer::wrap(self.map.clone()),
                PyPrimitiveLayerLanelet,
            ),
        )
    }

    #[getter]
    #[pyo3(name = "areaLayer")]
    fn area_layer(&self, py: Python<'_>) -> PyResult<Py<PyAreaLayer>> {
        Py::new(
            py,
            (PyAreaLayer::wrap(self.map.clone()), PyPrimitiveLayerArea),
        )
    }

    #[getter]
    #[pyo3(name = "regulatoryElementLayer")]
    fn regulatory_element_layer(&self) -> PyRegulatoryElementLayer {
        PyRegulatoryElementLayer::wrap(self.map.clone())
    }
}

map_class!("LaneletMap", PyLaneletMap, true);
map_class!("LaneletSubmap", PyLaneletSubmap, false);

/// Materialises a full map from a submap, pulling in every referenced object.
#[pymethods]
impl PyLaneletSubmap {
    #[pyo3(name = "laneletMap")]
    fn lanelet_map(&self, py: Python<'_>) -> PyResult<Py<PyLaneletMap>> {
        let full = LaneletMap::new_map();
        for kind in [
            LayerKind::Point,
            LayerKind::LineString,
            LayerKind::Polygon,
            LayerKind::Lanelet,
            LayerKind::Area,
            LayerKind::RegulatoryElement,
        ] {
            for primitive in self.map.layer(kind).all() {
                full.add(primitive);
            }
        }
        Py::new(py, PyLaneletMap::wrap(full))
    }
}

/// Builds the `createMapFrom*` / `createSubmapFrom*` factory pair for one kind.
macro_rules! create_from {
    ($map_fn:ident, $submap_fn:ident, $map_name:literal, $submap_name:literal) => {
        #[pyfunction]
        #[pyo3(name = $map_name)]
        fn $map_fn(py: Python<'_>, values: &Bound<'_, PyAny>) -> PyResult<Py<PyLaneletMap>> {
            let map = LaneletMap::new_map();
            for item in values.try_iter()? {
                let item = item?;
                let primitive = primitive_from_any(&item)
                    .ok_or_else(|| argument_error($map_name, "argument"))?;
                map.add(primitive);
            }
            Py::new(py, PyLaneletMap::wrap(map))
        }

        #[pyfunction]
        #[pyo3(name = $submap_name)]
        fn $submap_fn(py: Python<'_>, values: &Bound<'_, PyAny>) -> PyResult<Py<PyLaneletSubmap>> {
            let map = LaneletMap::new_submap();
            for item in values.try_iter()? {
                let item = item?;
                let primitive = primitive_from_any(&item)
                    .ok_or_else(|| argument_error($submap_name, "argument"))?;
                map.add(primitive);
            }
            Py::new(py, PyLaneletSubmap::wrap(map))
        }
    };
}

create_from!(
    create_map_from_points,
    create_submap_from_points,
    "createMapFromPoints",
    "createSubmapFromPoints"
);
create_from!(
    create_map_from_line_strings,
    create_submap_from_line_strings,
    "createMapFromLineStrings",
    "createSubmapFromLineStrings"
);
create_from!(
    create_map_from_polygons,
    create_submap_from_polygons,
    "createMapFromPolygons",
    "createSubmapFromPolygons"
);
create_from!(
    create_map_from_lanelets,
    create_submap_from_lanelets,
    "createMapFromLanelets",
    "createSubmapFromLanelets"
);
create_from!(
    create_map_from_areas,
    create_submap_from_areas,
    "createMapFromAreas",
    "createSubmapFromAreas"
);

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyPointLayer>()?;
    m.add_class::<PyLineStringLayer>()?;
    m.add_class::<PyPolygonLayer>()?;
    m.add_class::<PyLaneletLayer>()?;
    m.add_class::<PyAreaLayer>()?;
    m.add_class::<PyRegulatoryElementLayer>()?;
    m.add_class::<PyPrimitiveLayerLanelet>()?;
    m.add_class::<PyPrimitiveLayerArea>()?;
    m.add_class::<PyLaneletMapLayers>()?;
    m.add_class::<PyLaneletMap>()?;
    m.add_class::<PyLaneletSubmap>()?;

    m.add_function(wrap_pyfunction!(create_map_from_points, m)?)?;
    m.add_function(wrap_pyfunction!(create_map_from_line_strings, m)?)?;
    m.add_function(wrap_pyfunction!(create_map_from_polygons, m)?)?;
    m.add_function(wrap_pyfunction!(create_map_from_lanelets, m)?)?;
    m.add_function(wrap_pyfunction!(create_map_from_areas, m)?)?;
    m.add_function(wrap_pyfunction!(create_submap_from_points, m)?)?;
    m.add_function(wrap_pyfunction!(create_submap_from_line_strings, m)?)?;
    m.add_function(wrap_pyfunction!(create_submap_from_polygons, m)?)?;
    m.add_function(wrap_pyfunction!(create_submap_from_lanelets, m)?)?;
    m.add_function(wrap_pyfunction!(create_submap_from_areas, m)?)?;
    Ok(())
}
