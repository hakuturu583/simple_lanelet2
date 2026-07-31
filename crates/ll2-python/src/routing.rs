//! `lanelet2.routing` — the routing graph and the queries over it.
//!
//! Ground truth: `lanelet2_python/python_api/routing.cpp`.
//!
//! `RelationType` is installed by the Python shim rather than here, because
//! Boost's enum derives from `int` and a PyO3 enum cannot.

use std::sync::Arc;

use ll2_core::lanelet::Lanelet;
use ll2_routing::{
    RelationType, RoutingCost, RoutingCostDistance, RoutingCostTravelTime, RoutingGraph,
    default_costs,
};
use pyo3::prelude::*;
use pyo3::types::{PyList, PyModule};

use crate::core::lanelet::{PyConstLanelet, lanelet_of};
use crate::core::map::{PyLaneletMap, PyLaneletSubmap};
use crate::err::{argument_error, runtime};
use crate::traffic_rules::traffic_rules_of;

/// Looks up a `RelationType` member from the Python shim.
fn relation_type(py: Python<'_>, relation: RelationType) -> PyResult<Py<PyAny>> {
    Ok(py
        .import("lanelet2.routing")?
        .getattr("RelationType")?
        .getattr(relation.name())?
        .unbind())
}

/// `lanelet2.routing.LaneletRelation`.
#[pyclass(name = "LaneletRelation", module = "lanelet2.routing")]
pub struct PyLaneletRelation {
    lanelet: Lanelet,
    relation: RelationType,
}

#[pymethods]
impl PyLaneletRelation {
    #[getter]
    fn lanelet(&self) -> PyConstLanelet {
        PyConstLanelet::wrap(self.lanelet.clone())
    }

    #[getter]
    #[pyo3(name = "relationType")]
    fn relation_type(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        relation_type(py, self.relation)
    }
}

macro_rules! cost_class {
    ($py_name:literal, $rust:ident, $default_change:expr, $build:expr) => {
        #[doc = concat!("`lanelet2.routing.", $py_name, "`.")]
        #[pyclass(name = $py_name, module = "lanelet2.routing", frozen)]
        pub struct $rust {
            model: Arc<dyn RoutingCost>,
        }

        #[pymethods]
        impl $rust {
            #[new]
            #[pyo3(signature = (laneChangeCost = $default_change, minLaneChangeLength = 0.0))]
            #[allow(non_snake_case)]
            fn new(laneChangeCost: f64, minLaneChangeLength: f64) -> Self {
                let build: fn(f64, f64) -> Arc<dyn RoutingCost> = $build;
                $rust {
                    model: build(laneChangeCost, minLaneChangeLength),
                }
            }

            /// Exposed for shape only: the cost is evaluated during graph
            /// construction, and Python cannot subclass these.
            #[pyo3(name = "getCostSucceeding")]
            fn get_cost_succeeding(&self) -> PyResult<f64> {
                Err(runtime(
                    "Routing costs are evaluated during graph construction",
                ))
            }

            #[pyo3(name = "getCostLaneChange")]
            fn get_cost_lane_change(&self) -> PyResult<f64> {
                Err(runtime(
                    "Routing costs are evaluated during graph construction",
                ))
            }
        }
    };
}

/// `lanelet2.routing.RoutingCost`, the abstract base.
#[pyclass(name = "RoutingCost", module = "lanelet2.routing", frozen)]
pub struct PyRoutingCost;

#[pymethods]
impl PyRoutingCost {
    #[new]
    fn new() -> PyResult<Self> {
        Err(crate::err::not_instantiable())
    }

    #[pyo3(name = "getCostSucceeding")]
    fn get_cost_succeeding(&self) -> PyResult<f64> {
        Err(crate::err::not_instantiable())
    }

    #[pyo3(name = "getCostLaneChange")]
    fn get_cost_lane_change(&self) -> PyResult<f64> {
        Err(crate::err::not_instantiable())
    }
}

cost_class!(
    "RoutingCostDistance",
    PyRoutingCostDistance,
    10.0,
    |change, min| { Arc::new(RoutingCostDistance::new(change, min)) }
);
cost_class!(
    "RoutingCostTravelTime",
    PyRoutingCostTravelTime,
    5.0,
    |change, min| { Arc::new(RoutingCostTravelTime::new(change, min)) }
);

fn costs_from_any(obj: Option<&Bound<'_, PyAny>>) -> PyResult<Vec<Box<dyn RoutingCost>>> {
    let Some(obj) = obj else {
        return Ok(default_costs());
    };
    if obj.is_none() {
        return Ok(default_costs());
    }
    let mut out: Vec<Box<dyn RoutingCost>> = Vec::new();
    for item in obj.try_iter()? {
        let item = item?;
        if let Ok(model) = item.cast::<PyRoutingCostDistance>() {
            out.push(Box::new(SharedCost(model.borrow().model.clone())));
        } else if let Ok(model) = item.cast::<PyRoutingCostTravelTime>() {
            out.push(Box::new(SharedCost(model.borrow().model.clone())));
        } else {
            return Err(argument_error("RoutingGraph", "__init__"));
        }
    }
    if out.is_empty() {
        return Ok(default_costs());
    }
    Ok(out)
}

/// Adapts a shared cost model to the owned trait object the builder wants.
struct SharedCost(Arc<dyn RoutingCost>);

impl RoutingCost for SharedCost {
    fn cost_succeeding(
        &self,
        rules: &ll2_traffic_rules::TrafficRules,
        from: &Lanelet,
        to: &Lanelet,
    ) -> f64 {
        self.0.cost_succeeding(rules, from, to)
    }

    fn cost_lane_change(
        &self,
        rules: &ll2_traffic_rules::TrafficRules,
        from: &[Lanelet],
        to: &[Lanelet],
    ) -> f64 {
        self.0.cost_lane_change(rules, from, to)
    }

    fn name(&self) -> &'static str {
        self.0.name()
    }
}

/// `lanelet2.routing.PossiblePathsParams`.
#[pyclass(name = "PossiblePathsParams", module = "lanelet2.routing")]
#[derive(Clone, Default)]
pub struct PyPossiblePathsParams {
    #[pyo3(get, set, name = "routingCostLimit")]
    routing_cost_limit: Option<f64>,
    #[pyo3(get, set, name = "elementLimit")]
    element_limit: Option<usize>,
    #[pyo3(get, set, name = "routingCostId")]
    routing_cost_id: usize,
    #[pyo3(get, set, name = "includeLaneChanges")]
    include_lane_changes: bool,
    #[pyo3(get, set, name = "includeShorterPaths")]
    include_shorter_paths: bool,
}

#[pymethods]
impl PyPossiblePathsParams {
    #[new]
    #[pyo3(signature = (routingCostLimit = None, elementLimit = None, routingCostId = 0,
                        includeLaneChanges = false, includeShorterPaths = false))]
    #[allow(non_snake_case)]
    fn new(
        routingCostLimit: Option<f64>,
        elementLimit: Option<usize>,
        routingCostId: usize,
        includeLaneChanges: bool,
        includeShorterPaths: bool,
    ) -> Self {
        PyPossiblePathsParams {
            routing_cost_limit: routingCostLimit,
            element_limit: elementLimit,
            routing_cost_id: routingCostId,
            include_lane_changes: includeLaneChanges,
            include_shorter_paths: includeShorterPaths,
        }
    }
}

/// `lanelet2.routing.LaneletPath`.
#[pyclass(name = "LaneletPath", module = "lanelet2.routing")]
#[derive(Clone)]
pub struct PyLaneletPath {
    lanelets: Vec<Lanelet>,
}

impl PyLaneletPath {
    pub fn wrap(lanelets: Vec<Lanelet>) -> Self {
        PyLaneletPath { lanelets }
    }
}

#[pymethods]
impl PyLaneletPath {
    #[new]
    #[pyo3(signature = (lanelets = None))]
    fn new(lanelets: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let mut out = Vec::new();
        if let Some(lanelets) = lanelets {
            for item in lanelets.try_iter()? {
                let item = item?;
                let (lanelet, _) =
                    lanelet_of(&item).ok_or_else(|| argument_error("LaneletPath", "__init__"))?;
                out.push(lanelet);
            }
        }
        Ok(PyLaneletPath { lanelets: out })
    }

    fn __len__(&self) -> usize {
        self.lanelets.len()
    }

    fn __getitem__(&self, index: isize) -> PyResult<PyConstLanelet> {
        let index = crate::err::resolve_index(index, self.lanelets.len())?;
        Ok(PyConstLanelet::wrap(self.lanelets[index].clone()))
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let items: Vec<PyConstLanelet> = slf
            .lanelets
            .iter()
            .cloned()
            .map(PyConstLanelet::wrap)
            .collect();
        Ok(PyList::new(py, items)?.try_iter()?.into())
    }

    /// The stretch from `lanelet` onwards that continues without a lane change.
    #[pyo3(name = "getRemainingLane")]
    fn get_remaining_lane(&self, lanelet: &Bound<'_, PyAny>) -> PyResult<Vec<PyConstLanelet>> {
        let (lanelet, _) =
            lanelet_of(lanelet).ok_or_else(|| argument_error("LaneletPath", "getRemainingLane"))?;
        let Some(start) = self
            .lanelets
            .iter()
            .position(|candidate| candidate.is_same_view(&lanelet))
        else {
            // Upstream's std::find yields end(), and the loop then produces nothing.
            return Ok(Vec::new());
        };
        let mut out = vec![PyConstLanelet::wrap(self.lanelets[start].clone())];
        for pair in self.lanelets[start..].windows(2) {
            if !ll2_core::geometry::lanelet::follows(&pair[0], &pair[1]) {
                break;
            }
            out.push(PyConstLanelet::wrap(pair[1].clone()));
        }
        Ok(out)
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let rendered: Vec<String> = self
            .lanelets
            .iter()
            .cloned()
            .map(|lanelet| PyConstLanelet::wrap(lanelet).__repr__(py))
            .collect::<PyResult<_>>()?;
        Ok(format!("LaneletPath([{}])", rendered.join(", ")))
    }
}

fn to_path_list<'py>(py: Python<'py>, paths: Vec<Vec<Lanelet>>) -> PyResult<Bound<'py, PyList>> {
    PyList::new(
        py,
        paths
            .into_iter()
            .map(PyLaneletPath::wrap)
            .collect::<Vec<_>>(),
    )
}

fn to_lanelet_list<'py>(py: Python<'py>, lanelets: Vec<Lanelet>) -> PyResult<Bound<'py, PyList>> {
    PyList::new(
        py,
        lanelets
            .into_iter()
            .map(PyConstLanelet::wrap)
            .collect::<Vec<_>>(),
    )
}

/// `lanelet2.routing.RoutingGraph`.
#[pyclass(name = "RoutingGraph", module = "lanelet2.routing", frozen)]
pub struct PyRoutingGraph {
    graph: Arc<RoutingGraph>,
}

#[pymethods]
impl PyRoutingGraph {
    #[new]
    #[pyo3(signature = (laneletMap, trafficRules, routingCost = None))]
    #[allow(non_snake_case)]
    fn new(
        laneletMap: &Bound<'_, PyAny>,
        trafficRules: &Bound<'_, PyAny>,
        routingCost: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let rules = traffic_rules_of(trafficRules)
            .ok_or_else(|| argument_error("RoutingGraph", "__init__"))?;
        let costs = costs_from_any(routingCost)?;

        let map = if let Ok(map) = laneletMap.cast::<PyLaneletMap>() {
            map.borrow().inner_arc()
        } else if let Ok(submap) = laneletMap.cast::<PyLaneletSubmap>() {
            submap.borrow().inner_arc()
        } else {
            return Err(argument_error("RoutingGraph", "__init__"));
        };
        Ok(PyRoutingGraph {
            graph: Arc::new(RoutingGraph::build(&map, rules, &costs)),
        })
    }

    #[pyo3(signature = (lanelet, withLaneChanges = false))]
    #[allow(non_snake_case)]
    fn following<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
        withLaneChanges: bool,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "following")?;
        to_lanelet_list(py, self.graph.following(&lanelet, withLaneChanges))
    }

    #[pyo3(name = "followingRelations", signature = (lanelet, withLaneChanges = false))]
    #[allow(non_snake_case)]
    fn following_relations<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
        withLaneChanges: bool,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "followingRelations")?;
        self.relations(
            py,
            self.graph.following_relations(&lanelet, withLaneChanges),
        )
    }

    #[pyo3(signature = (lanelet, withLaneChanges = false))]
    #[allow(non_snake_case)]
    fn previous<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
        withLaneChanges: bool,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "previous")?;
        to_lanelet_list(py, self.graph.previous(&lanelet, withLaneChanges))
    }

    #[pyo3(name = "previousRelations", signature = (lanelet, withLaneChanges = false))]
    #[allow(non_snake_case)]
    fn previous_relations<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
        withLaneChanges: bool,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "previousRelations")?;
        self.relations(py, self.graph.previous_relations(&lanelet, withLaneChanges))
    }

    #[pyo3(signature = (lanelet, routingCostId = 0))]
    #[allow(non_snake_case)]
    fn besides<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
        routingCostId: usize,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "besides")?;
        to_lanelet_list(py, self.graph.besides(&lanelet, routingCostId))
    }

    #[pyo3(signature = (lanelet, routingCostId = 0))]
    #[allow(non_snake_case)]
    fn conflicting<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
        routingCostId: usize,
    ) -> PyResult<Bound<'py, PyList>> {
        let _ = routingCostId;
        let lanelet = self.lanelet(lanelet, "conflicting")?;
        to_lanelet_list(py, self.graph.conflicting(&lanelet))
    }

    #[pyo3(name = "routingRelation", signature = (from, to, includeConflicting = false))]
    #[allow(non_snake_case)]
    fn routing_relation(
        &self,
        py: Python<'_>,
        from: &Bound<'_, PyAny>,
        to: &Bound<'_, PyAny>,
        includeConflicting: bool,
    ) -> PyResult<Py<PyAny>> {
        let from = self.lanelet(from, "routingRelation")?;
        let to = self.lanelet(to, "routingRelation")?;
        match self.graph.routing_relation(&from, &to, includeConflicting) {
            None => Ok(py.None()),
            Some(relation) => relation_type(py, relation),
        }
    }

    #[pyo3(name = "shortestPath", signature = (fromLanelet, toLanelet, routingCostId = 0, withLaneChanges = true))]
    #[allow(non_snake_case)]
    fn shortest_path(
        &self,
        py: Python<'_>,
        fromLanelet: &Bound<'_, PyAny>,
        toLanelet: &Bound<'_, PyAny>,
        routingCostId: usize,
        withLaneChanges: bool,
    ) -> PyResult<Py<PyAny>> {
        let from = self.lanelet(fromLanelet, "shortestPath")?;
        let to = self.lanelet(toLanelet, "shortestPath")?;
        match self
            .graph
            .shortest_path(&from, &to, routingCostId, withLaneChanges)
        {
            None => Ok(py.None()),
            Some(path) => Ok(Py::new(py, PyLaneletPath::wrap(path))?.into_any()),
        }
    }

    #[pyo3(name = "shortestPathWithVia",
           signature = (start, via, end, routingCostId = 0, withLaneChanges = true))]
    #[allow(non_snake_case)]
    fn shortest_path_with_via(
        &self,
        py: Python<'_>,
        start: &Bound<'_, PyAny>,
        via: &Bound<'_, PyAny>,
        end: &Bound<'_, PyAny>,
        routingCostId: usize,
        withLaneChanges: bool,
    ) -> PyResult<Py<PyAny>> {
        let start = self.lanelet(start, "shortestPathWithVia")?;
        let end = self.lanelet(end, "shortestPathWithVia")?;
        let mut waypoints = Vec::new();
        for item in via.try_iter()? {
            waypoints.push(self.lanelet(&item?, "shortestPathWithVia")?);
        }
        match self
            .graph
            .shortest_path_via(&start, &waypoints, &end, routingCostId, withLaneChanges)
        {
            None => Ok(py.None()),
            Some(path) => Ok(Py::new(py, PyLaneletPath::wrap(path))?.into_any()),
        }
    }

    #[pyo3(name = "reachableSet",
           signature = (lanelet, maxRoutingCost, routingCostId = 0, allowLaneChanges = true))]
    #[allow(non_snake_case)]
    fn reachable_set<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
        maxRoutingCost: f64,
        routingCostId: usize,
        allowLaneChanges: bool,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "reachableSet")?;
        to_lanelet_list(
            py,
            self.graph
                .reachable_set(&lanelet, maxRoutingCost, routingCostId, allowLaneChanges),
        )
    }

    #[pyo3(name = "reachableSetTowards",
           signature = (lanelet, maxRoutingCost, routingCostId = 0, allowLaneChanges = true))]
    #[allow(non_snake_case)]
    fn reachable_set_towards<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
        maxRoutingCost: f64,
        routingCostId: usize,
        allowLaneChanges: bool,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "reachableSetTowards")?;
        to_lanelet_list(
            py,
            self.graph.reachable_set_towards(
                &lanelet,
                maxRoutingCost,
                routingCostId,
                allowLaneChanges,
            ),
        )
    }

    #[pyo3(name = "possiblePaths",
           signature = (lanelet, minRoutingCost, routingCostId = 0, allowLaneChanges = false))]
    #[allow(non_snake_case)]
    fn possible_paths<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
        minRoutingCost: f64,
        routingCostId: usize,
        allowLaneChanges: bool,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "possiblePaths")?;
        to_path_list(
            py,
            self.graph.possible_paths(
                &lanelet,
                Some(minRoutingCost),
                None,
                routingCostId,
                allowLaneChanges,
                false,
            ),
        )
    }

    #[pyo3(name = "possiblePathsMinLen",
           signature = (lanelet, minLanelets, allowLaneChanges = false, routingCostId = 0))]
    #[allow(non_snake_case)]
    fn possible_paths_min_len<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
        minLanelets: usize,
        allowLaneChanges: bool,
        routingCostId: usize,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "possiblePathsMinLen")?;
        to_path_list(
            py,
            self.graph.possible_paths(
                &lanelet,
                None,
                Some(minLanelets),
                routingCostId,
                allowLaneChanges,
                false,
            ),
        )
    }

    #[pyo3(name = "possiblePathsTowards",
           signature = (lanelet, minRoutingCost, routingCostId = 0, allowLaneChanges = false))]
    #[allow(non_snake_case)]
    fn possible_paths_towards<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
        minRoutingCost: f64,
        routingCostId: usize,
        allowLaneChanges: bool,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "possiblePathsTowards")?;
        to_path_list(
            py,
            self.graph.possible_paths_towards(
                &lanelet,
                Some(minRoutingCost),
                None,
                routingCostId,
                allowLaneChanges,
                false,
            ),
        )
    }

    #[pyo3(name = "possiblePathsTowardsMinLen",
           signature = (lanelet, minLanelets, allowLaneChanges = false, routingCostId = 0))]
    #[allow(non_snake_case)]
    fn possible_paths_towards_min_len<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
        minLanelets: usize,
        allowLaneChanges: bool,
        routingCostId: usize,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "possiblePathsTowardsMinLen")?;
        to_path_list(
            py,
            self.graph.possible_paths_towards(
                &lanelet,
                None,
                Some(minLanelets),
                routingCostId,
                allowLaneChanges,
                false,
            ),
        )
    }

    #[pyo3(name = "getRoute",
           signature = (fromLanelet, toLanelet, routingCostId = 0, withLaneChanges = true))]
    #[allow(non_snake_case)]
    fn get_route(
        &self,
        py: Python<'_>,
        fromLanelet: &Bound<'_, PyAny>,
        toLanelet: &Bound<'_, PyAny>,
        routingCostId: usize,
        withLaneChanges: bool,
    ) -> PyResult<Py<PyAny>> {
        let from = self.lanelet(fromLanelet, "getRoute")?;
        let to = self.lanelet(toLanelet, "getRoute")?;
        match self.graph.route(&from, &to, routingCostId, withLaneChanges) {
            None => Ok(py.None()),
            Some(route) => Ok(Py::new(
                py,
                PyRoute {
                    route: Arc::new(route),
                    graph: self.graph.clone(),
                },
            )?
            .into_any()),
        }
    }

    #[pyo3(name = "getRouteVia",
           signature = (fromLanelet, via, toLanelet, routingCostId = 0, withLaneChanges = true))]
    #[allow(non_snake_case)]
    fn get_route_via(
        &self,
        py: Python<'_>,
        fromLanelet: &Bound<'_, PyAny>,
        via: &Bound<'_, PyAny>,
        toLanelet: &Bound<'_, PyAny>,
        routingCostId: usize,
        withLaneChanges: bool,
    ) -> PyResult<Py<PyAny>> {
        let from = self.lanelet(fromLanelet, "getRouteVia")?;
        let to = self.lanelet(toLanelet, "getRouteVia")?;
        let mut waypoints = Vec::new();
        for item in via.try_iter()? {
            waypoints.push(self.lanelet(&item?, "getRouteVia")?);
        }
        match self
            .graph
            .route_via(&from, &waypoints, &to, routingCostId, withLaneChanges)
        {
            None => Ok(py.None()),
            Some(route) => Ok(Py::new(
                py,
                PyRoute {
                    route: Arc::new(route),
                    graph: self.graph.clone(),
                },
            )?
            .into_any()),
        }
    }

    #[pyo3(name = "forEachSuccessor", signature = (lanelet, func, allowLaneChanges = true, routingCostId = 0))]
    #[allow(non_snake_case)]
    fn for_each_successor(
        &self,
        py: Python<'_>,
        lanelet: &Bound<'_, PyAny>,
        func: &Bound<'_, PyAny>,
        allowLaneChanges: bool,
        routingCostId: usize,
    ) -> PyResult<()> {
        let _ = (allowLaneChanges, routingCostId);
        let lanelet = self.lanelet(lanelet, "forEachSuccessor")?;
        visit(py, &self.graph, &lanelet, func, true, None)
    }

    #[pyo3(name = "forEachPredecessor", signature = (lanelet, func, allowLaneChanges = true, routingCostId = 0))]
    #[allow(non_snake_case)]
    fn for_each_predecessor(
        &self,
        py: Python<'_>,
        lanelet: &Bound<'_, PyAny>,
        func: &Bound<'_, PyAny>,
        allowLaneChanges: bool,
        routingCostId: usize,
    ) -> PyResult<()> {
        let _ = (allowLaneChanges, routingCostId);
        let lanelet = self.lanelet(lanelet, "forEachPredecessor")?;
        visit(py, &self.graph, &lanelet, func, false, None)
    }

    #[pyo3(name = "forEachSuccessorIncludingAreas", signature = (lanelet, func, allowLaneChanges = true, routingCostId = 0))]
    #[allow(non_snake_case)]
    fn for_each_successor_including_areas(
        &self,
        py: Python<'_>,
        lanelet: &Bound<'_, PyAny>,
        func: &Bound<'_, PyAny>,
        allowLaneChanges: bool,
        routingCostId: usize,
    ) -> PyResult<()> {
        self.for_each_successor(py, lanelet, func, allowLaneChanges, routingCostId)
    }

    #[pyo3(name = "forEachPredecessorIncludingAreas", signature = (lanelet, func, allowLaneChanges = true, routingCostId = 0))]
    #[allow(non_snake_case)]
    fn for_each_predecessor_including_areas(
        &self,
        py: Python<'_>,
        lanelet: &Bound<'_, PyAny>,
        func: &Bound<'_, PyAny>,
        allowLaneChanges: bool,
        routingCostId: usize,
    ) -> PyResult<()> {
        self.for_each_predecessor(py, lanelet, func, allowLaneChanges, routingCostId)
    }

    #[pyo3(name = "getDebugLaneletMap", signature = (routingCostId = 0, includeAdjacent = false, includeConflicting = false))]
    #[allow(non_snake_case)]
    fn get_debug_lanelet_map(
        &self,
        py: Python<'_>,
        routingCostId: usize,
        includeAdjacent: bool,
        includeConflicting: bool,
    ) -> PyResult<Py<PyLaneletMap>> {
        let _ = (routingCostId, includeAdjacent, includeConflicting);
        debug_map(py, self.graph.vertices())
    }

    /// Not implemented: GraphML and GraphViz export are out of scope.
    #[pyo3(name = "exportGraphML", signature = (filename, edgeTypesToExclude = None, routingCostId = 0))]
    #[allow(non_snake_case)]
    fn export_graph_ml(
        &self,
        filename: &str,
        edgeTypesToExclude: Option<&Bound<'_, PyAny>>,
        routingCostId: usize,
    ) -> PyResult<()> {
        let _ = (filename, edgeTypesToExclude, routingCostId);
        Err(runtime("exportGraphML is not implemented"))
    }

    #[pyo3(name = "exportGraphViz", signature = (filename, edgeTypesToExclude = None, routingCostId = 0))]
    #[allow(non_snake_case)]
    fn export_graph_viz(
        &self,
        filename: &str,
        edgeTypesToExclude: Option<&Bound<'_, PyAny>>,
        routingCostId: usize,
    ) -> PyResult<()> {
        let _ = (filename, edgeTypesToExclude, routingCostId);
        Err(runtime("exportGraphViz is not implemented"))
    }

    #[pyo3(name = "passableLaneletSubmap")]
    fn passable_lanelet_submap(&self, py: Python<'_>) -> PyResult<Py<PyLaneletSubmap>> {
        Py::new(py, PyLaneletSubmap::wrap(self.graph.passable_submap()))
    }

    #[pyo3(name = "checkValidity", signature = (throwOnError = true))]
    #[allow(non_snake_case)]
    fn check_validity(&self, throwOnError: bool) -> PyResult<Vec<String>> {
        let errors = self.graph.check_validity();
        if throwOnError && !errors.is_empty() {
            return Err(runtime(format!(
                "Errors found in routing graph:\n\t- {}",
                errors.join("\n\t- ")
            )));
        }
        Ok(errors)
    }

    /// Every edge, as `(fromId, fromInverted, toId, toInverted, relation, costId,
    /// cost)`. Not part of upstream's surface, but it is what makes the builder
    /// itself testable.
    fn edges<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let rows: Vec<(i64, bool, i64, bool, String, usize, f64)> = self
            .graph
            .edges()
            .into_iter()
            .map(|(from, from_inv, to, to_inv, relation, cost_id, cost)| {
                (
                    from,
                    from_inv,
                    to,
                    to_inv,
                    relation.name().to_owned(),
                    cost_id,
                    cost,
                )
            })
            .collect();
        PyList::new(py, rows)
    }
}

macro_rules! side_query {
    ($py_name:literal, $rust:ident, $call:ident, $single:tt) => {
        #[pymethods]
        impl PyRoutingGraph {
            #[pyo3(name = $py_name, signature = (lanelet, routingCostId = 0))]
            #[allow(non_snake_case)]
            fn $rust(
                &self,
                py: Python<'_>,
                lanelet: &Bound<'_, PyAny>,
                routingCostId: usize,
            ) -> PyResult<Py<PyAny>> {
                let lanelet = self.lanelet(lanelet, $py_name)?;
                side_query!(@body $single, self, py, lanelet, routingCostId, $call)
            }
        }
    };

    (@body true, $self:ident, $py:ident, $lanelet:ident, $cost:ident, $call:ident) => {
        match $self.graph.$call(&$lanelet, $cost) {
            None => Ok($py.None()),
            Some(found) => Ok(Py::new($py, PyConstLanelet::wrap(found))?.into_any()),
        }
    };
    (@body false, $self:ident, $py:ident, $lanelet:ident, $cost:ident, $call:ident) => {
        Ok(to_lanelet_list($py, $self.graph.$call(&$lanelet, $cost))?.into_any().unbind())
    };
    (@body relations, $self:ident, $py:ident, $lanelet:ident, $cost:ident, $call:ident) => {
        Ok($self.relations($py, $self.graph.$call(&$lanelet, $cost))?.into_any().unbind())
    };
}

side_query!("left", left, left, true);
side_query!("right", right, right, true);
side_query!("adjacentLeft", adjacent_left, adjacent_left, true);
side_query!("adjacentRight", adjacent_right, adjacent_right, true);
side_query!("lefts", lefts, lefts, false);
side_query!("rights", rights, rights, false);
side_query!("adjacentLefts", adjacent_lefts, adjacent_lefts, false);
side_query!("adjacentRights", adjacent_rights, adjacent_rights, false);
side_query!("leftRelations", left_relations, left_relations, relations);
side_query!(
    "rightRelations",
    right_relations,
    right_relations,
    relations
);

impl PyRoutingGraph {
    fn lanelet(&self, obj: &Bound<'_, PyAny>, method: &str) -> PyResult<Lanelet> {
        lanelet_of(obj)
            .map(|(lanelet, _)| lanelet)
            .ok_or_else(|| argument_error("RoutingGraph", method))
    }

    fn relations<'py>(
        &self,
        py: Python<'py>,
        relations: Vec<ll2_routing::LaneletRelation>,
    ) -> PyResult<Bound<'py, PyList>> {
        PyList::new(
            py,
            relations
                .into_iter()
                .map(|found| PyLaneletRelation {
                    lanelet: found.lanelet,
                    relation: found.relation,
                })
                .collect::<Vec<_>>(),
        )
    }
}

/// `lanelet2.routing.LaneletVisitInformation`, handed to the `forEach*` callbacks.
#[pyclass(name = "LaneletVisitInformation", module = "lanelet2.routing")]
pub struct PyLaneletVisitInformation {
    lanelet: Lanelet,
    predecessor: Lanelet,
    cost: f64,
    length: usize,
    num_lane_changes: usize,
}

#[pymethods]
impl PyLaneletVisitInformation {
    #[getter]
    fn lanelet(&self) -> PyConstLanelet {
        PyConstLanelet::wrap(self.lanelet.clone())
    }

    #[getter]
    fn predecessor(&self) -> PyConstLanelet {
        PyConstLanelet::wrap(self.predecessor.clone())
    }

    #[getter]
    fn cost(&self) -> f64 {
        self.cost
    }

    /// How many lanelets have been visited on the way here, the start included.
    #[getter]
    fn length(&self) -> usize {
        self.length
    }

    #[getter]
    #[pyo3(name = "numLaneChanges")]
    fn num_lane_changes(&self) -> usize {
        self.num_lane_changes
    }
}

/// The same, for the traversals that also walk areas.
#[pyclass(name = "LaneletOrAreaVisitInformation", module = "lanelet2.routing")]
pub struct PyLaneletOrAreaVisitInformation {
    lanelet: Lanelet,
    predecessor: Lanelet,
    cost: f64,
    length: usize,
    num_lane_changes: usize,
}

#[pymethods]
impl PyLaneletOrAreaVisitInformation {
    #[getter]
    #[pyo3(name = "laneletOrArea")]
    fn lanelet_or_area(&self) -> PyConstLanelet {
        PyConstLanelet::wrap(self.lanelet.clone())
    }

    #[getter]
    fn predecessor(&self) -> PyConstLanelet {
        PyConstLanelet::wrap(self.predecessor.clone())
    }

    #[getter]
    fn cost(&self) -> f64 {
        self.cost
    }

    #[getter]
    fn length(&self) -> usize {
        self.length
    }

    #[getter]
    #[pyo3(name = "numLaneChanges")]
    fn num_lane_changes(&self) -> usize {
        self.num_lane_changes
    }
}

/// `lanelet2.routing.Route`.
#[pyclass(name = "Route", module = "lanelet2.routing", frozen)]
pub struct PyRoute {
    route: Arc<ll2_routing::Route>,
    graph: Arc<RoutingGraph>,
}

#[pymethods]
impl PyRoute {
    #[pyo3(name = "shortestPath")]
    fn shortest_path(&self) -> PyLaneletPath {
        PyLaneletPath::wrap(self.route.shortest_path().to_vec())
    }

    #[pyo3(name = "remainingShortestPath")]
    fn remaining_shortest_path(&self, lanelet: &Bound<'_, PyAny>) -> PyResult<PyLaneletPath> {
        let lanelet = self.lanelet(lanelet, "remainingShortestPath")?;
        Ok(PyLaneletPath::wrap(
            self.route.remaining_shortest_path(&lanelet),
        ))
    }

    #[pyo3(name = "remainingLane")]
    fn remaining_lane<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "remainingLane")?;
        to_lanelet_list(py, self.route.remaining_lane(&self.graph, &lanelet))
    }

    #[pyo3(name = "fullLane")]
    fn full_lane<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "fullLane")?;
        to_lanelet_list(py, self.route.full_lane(&self.graph, &lanelet))
    }

    /// The length of the *shortest path*, not of the whole corridor.
    #[pyo3(name = "length2d")]
    fn length_2d(&self) -> f64 {
        self.route.length_2d()
    }

    #[pyo3(name = "numLanes")]
    fn num_lanes(&self) -> usize {
        self.route.num_lanes()
    }

    #[pyo3(name = "laneletSubmap")]
    fn lanelet_submap(&self, py: Python<'_>) -> PyResult<Py<PyLaneletSubmap>> {
        Py::new(py, PyLaneletSubmap::wrap(self.route.lanelet_submap()))
    }

    fn size(&self) -> usize {
        self.route.size()
    }

    fn __contains__(&self, lanelet: &Bound<'_, PyAny>) -> bool {
        match lanelet_of(lanelet) {
            Some((lanelet, _)) => self.route.contains(&lanelet),
            None => false,
        }
    }

    #[pyo3(name = "followingRelations")]
    fn following_relations<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "followingRelations")?;
        self.wrap_relations(
            py,
            self.route
                .relations(&self.graph, &lanelet, RelationType::SUCCESSOR, true),
        )
    }

    #[pyo3(name = "previousRelations")]
    fn previous_relations<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "previousRelations")?;
        self.wrap_relations(
            py,
            self.route
                .relations(&self.graph, &lanelet, RelationType::SUCCESSOR, false),
        )
    }

    #[pyo3(name = "leftRelation")]
    fn left_relation(&self, py: Python<'_>, lanelet: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        self.side_relation(py, lanelet, true, "leftRelation")
    }

    #[pyo3(name = "rightRelation")]
    fn right_relation(&self, py: Python<'_>, lanelet: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        self.side_relation(py, lanelet, false, "rightRelation")
    }

    #[pyo3(name = "leftRelations")]
    fn left_relations<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "leftRelations")?;
        let found: Vec<ll2_routing::LaneletRelation> = self
            .graph
            .left_relations(&lanelet, self.route.cost_id())
            .into_iter()
            .filter(|relation| self.route.contains(&relation.lanelet))
            .collect();
        self.wrap_relations(py, found)
    }

    #[pyo3(name = "rightRelations")]
    fn right_relations<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "rightRelations")?;
        let found: Vec<ll2_routing::LaneletRelation> = self
            .graph
            .right_relations(&lanelet, self.route.cost_id())
            .into_iter()
            .filter(|relation| self.route.contains(&relation.lanelet))
            .collect();
        self.wrap_relations(py, found)
    }

    #[pyo3(name = "conflictingInRoute")]
    fn conflicting_in_route<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "conflictingInRoute")?;
        to_lanelet_list(py, self.route.conflicting_in_route(&self.graph, &lanelet))
    }

    #[pyo3(name = "conflictingInMap")]
    fn conflicting_in_map<'py>(
        &self,
        py: Python<'py>,
        lanelet: &Bound<'_, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let lanelet = self.lanelet(lanelet, "conflictingInMap")?;
        to_lanelet_list(py, self.route.conflicting_in_map(&self.graph, &lanelet))
    }

    #[pyo3(name = "allConflictingInMap")]
    fn all_conflicting_in_map<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        to_lanelet_list(py, self.route.all_conflicting_in_map(&self.graph))
    }

    #[pyo3(name = "forEachSuccessor")]
    fn for_each_successor(
        &self,
        py: Python<'_>,
        lanelet: &Bound<'_, PyAny>,
        func: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let lanelet = self.lanelet(lanelet, "forEachSuccessor")?;
        visit(py, &self.graph, &lanelet, func, true, Some(&self.route))
    }

    #[pyo3(name = "forEachPredecessor")]
    fn for_each_predecessor(
        &self,
        py: Python<'_>,
        lanelet: &Bound<'_, PyAny>,
        func: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let lanelet = self.lanelet(lanelet, "forEachPredecessor")?;
        visit(py, &self.graph, &lanelet, func, false, Some(&self.route))
    }

    #[pyo3(name = "getDebugLaneletMap", signature = (routingCostId = 0, includeAdjacent = false, includeConflicting = false))]
    #[allow(non_snake_case)]
    fn get_debug_lanelet_map(
        &self,
        py: Python<'_>,
        routingCostId: usize,
        includeAdjacent: bool,
        includeConflicting: bool,
    ) -> PyResult<Py<PyLaneletMap>> {
        let _ = (routingCostId, includeAdjacent, includeConflicting);
        debug_map(py, self.route.lanelets())
    }

    #[pyo3(name = "checkValidity", signature = (throwOnError = false))]
    #[allow(non_snake_case)]
    fn check_validity(&self, throwOnError: bool) -> PyResult<Vec<String>> {
        let errors = self.route.check_validity();
        if throwOnError && !errors.is_empty() {
            return Err(runtime(errors.join("; ")));
        }
        Ok(errors)
    }
}

impl PyRoute {
    fn lanelet(&self, obj: &Bound<'_, PyAny>, method: &str) -> PyResult<Lanelet> {
        lanelet_of(obj)
            .map(|(lanelet, _)| lanelet)
            .ok_or_else(|| argument_error("Route", method))
    }

    fn wrap_relations<'py>(
        &self,
        py: Python<'py>,
        relations: Vec<ll2_routing::LaneletRelation>,
    ) -> PyResult<Bound<'py, PyList>> {
        PyList::new(
            py,
            relations
                .into_iter()
                .map(|found| PyLaneletRelation {
                    lanelet: found.lanelet,
                    relation: found.relation,
                })
                .collect::<Vec<_>>(),
        )
    }

    fn side_relation(
        &self,
        py: Python<'_>,
        lanelet: &Bound<'_, PyAny>,
        left: bool,
        method: &str,
    ) -> PyResult<Py<PyAny>> {
        let lanelet = self.lanelet(lanelet, method)?;
        let found = if left {
            self.graph.left_relations(&lanelet, self.route.cost_id())
        } else {
            self.graph.right_relations(&lanelet, self.route.cost_id())
        };
        match found
            .into_iter()
            .find(|relation| self.route.contains(&relation.lanelet))
        {
            None => Ok(py.None()),
            Some(found) => Ok(Py::new(
                py,
                PyLaneletRelation {
                    lanelet: found.lanelet,
                    relation: found.relation,
                },
            )?
            .into_any()),
        }
    }
}

/// Walks the graph breadth-first, calling `func` with visit information.
///
/// Returning `False` from the callback stops the traversal descending past that
/// lanelet, which is how upstream lets a caller prune.
fn visit(
    py: Python<'_>,
    graph: &RoutingGraph,
    start: &Lanelet,
    func: &Bound<'_, PyAny>,
    forwards: bool,
    route: Option<&ll2_routing::Route>,
) -> PyResult<()> {
    let mut queue: Vec<(Lanelet, Lanelet, f64, usize, usize)> =
        vec![(start.clone(), start.clone(), 0.0, 1, 0)];
    let mut seen: Vec<Lanelet> = Vec::new();

    while !queue.is_empty() {
        let (lanelet, predecessor, cost, length, changes) = queue.remove(0);
        if seen.iter().any(|held| held.is_same_view(&lanelet)) {
            continue;
        }
        seen.push(lanelet.clone());

        let information = PyLaneletVisitInformation {
            lanelet: lanelet.clone(),
            predecessor,
            cost,
            length,
            num_lane_changes: changes,
        };
        let verdict = func.call1((Py::new(py, information)?,))?;
        if !verdict.is_truthy()? {
            continue;
        }

        let neighbours = if forwards {
            graph.following_relations(&lanelet, true)
        } else {
            graph.previous_relations(&lanelet, true)
        };
        for relation in neighbours {
            if route.is_some_and(|route| !route.contains(&relation.lanelet)) {
                continue;
            }
            let extra = usize::from(relation.relation != RelationType::SUCCESSOR);
            queue.push((
                relation.lanelet,
                lanelet.clone(),
                cost + relation.cost,
                length + 1,
                changes + extra,
            ));
        }
    }
    Ok(())
}

/// A map with one point per lanelet, at its first centerline point.
fn debug_map(py: Python<'_>, lanelets: &[Lanelet]) -> PyResult<Py<PyLaneletMap>> {
    use ll2_core::attribute::{Attribute, AttributeMap};
    use ll2_core::map::{LaneletMap, Primitive};
    use ll2_core::point::Point;

    let map = LaneletMap::new_map();
    for lanelet in lanelets {
        let Some(first) = lanelet.centerline().front() else {
            continue;
        };
        let [x, y, z] = first.xyz();
        let mut attributes = AttributeMap::new();
        attributes.insert("id".into(), Attribute::new(lanelet.id().to_string()));
        map.add(Primitive::Point(Point::new(
            lanelet.id(),
            x,
            y,
            z,
            attributes,
        )));
    }
    Py::new(py, PyLaneletMap::wrap(map))
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // RelationType is installed by the Python shim, because Boost's enum derives
    // from int and a PyO3 enum cannot.
    m.add("_needs_RelationType", true)?;
    m.add_class::<PyLaneletRelation>()?;
    m.add_class::<PyRoutingCost>()?;
    m.add_class::<PyRoutingCostDistance>()?;
    m.add_class::<PyRoutingCostTravelTime>()?;
    m.add_class::<PyPossiblePathsParams>()?;
    m.add_class::<PyLaneletPath>()?;
    m.add_class::<PyRoutingGraph>()?;
    m.add_class::<PyRoute>()?;
    m.add_class::<PyLaneletVisitInformation>()?;
    m.add_class::<PyLaneletOrAreaVisitInformation>()?;
    Ok(())
}
