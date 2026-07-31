//! `lanelet2.traffic_rules` — who may use a lanelet, how fast, and where a lane
//! change is allowed.
//!
//! Ground truth: `lanelet2_python/python_api/traffic_rules.cpp`.
//!
//! `location()` and `participant()` are *methods* here, not properties. The `.pyi`
//! stubs upstream ships declare them as properties and are simply wrong.

use std::sync::Arc;

use ll2_traffic_rules::{SpeedLimit, TrafficRules, locations, participants};
use pyo3::prelude::*;
use pyo3::types::PyModule;

use crate::core::lanelet::lanelet_of;
use crate::core::linestring::linestring_of;
use crate::err::{argument_error, runtime};

/// `lanelet2.traffic_rules.Locations`.
#[pyclass(name = "Locations", module = "lanelet2.traffic_rules", frozen)]
pub struct PyLocations;

#[pymethods]
impl PyLocations {
    #[classattr]
    #[allow(non_snake_case)]
    fn Germany() -> &'static str {
        locations::GERMANY
    }
}

/// `lanelet2.traffic_rules.Participants`.
///
/// The strings are hierarchical and colon-separated, and the hierarchy is
/// load-bearing: a rule written for `vehicle` applies to `vehicle:car`.
#[pyclass(name = "Participants", module = "lanelet2.traffic_rules", frozen)]
pub struct PyParticipants;

macro_rules! participant_constants {
    ($($py_name:ident => $value:expr),+ $(,)?) => {
        #[pymethods]
        impl PyParticipants {
            $(
                #[classattr]
                #[allow(non_snake_case)]
                fn $py_name() -> &'static str {
                    $value
                }
            )+
        }
    };
}

participant_constants! {
    Vehicle => participants::VEHICLE,
    VehicleBus => participants::VEHICLE_BUS,
    VehicleCar => participants::VEHICLE_CAR,
    VehicleCarElectric => participants::VEHICLE_CAR_ELECTRIC,
    VehicleCarCombustion => participants::VEHICLE_CAR_COMBUSTION,
    VehicleTruck => participants::VEHICLE_TRUCK,
    VehicleMotorcycle => participants::VEHICLE_MOTORCYCLE,
    VehicleTaxi => participants::VEHICLE_TAXI,
    VehicleEmergency => participants::VEHICLE_EMERGENCY,
    Pedestrian => participants::PEDESTRIAN,
    Bicycle => participants::BICYCLE,
    Train => participants::TRAIN,
}

/// `lanelet2.traffic_rules.SpeedLimitInformation`.
///
/// Only default construction works upstream: the two-argument form is registered
/// but unreachable, so this is effectively a read-only result type.
#[pyclass(name = "SpeedLimitInformation", module = "lanelet2.traffic_rules")]
#[derive(Clone, Copy)]
pub struct PySpeedLimitInformation {
    limit: SpeedLimit,
}

impl PySpeedLimitInformation {
    pub fn wrap(limit: SpeedLimit) -> Self {
        PySpeedLimitInformation { limit }
    }
}

#[pymethods]
impl PySpeedLimitInformation {
    /// Only the no-argument form works, matching upstream: the two-argument
    /// constructor is registered there but unreachable, so passing anything gives
    /// the same overload error it does.
    #[new]
    #[pyo3(signature = (*args))]
    fn new(args: &Bound<'_, pyo3::types::PyTuple>) -> PyResult<Self> {
        if !args.is_empty() {
            return Err(argument_error("SpeedLimitInformation", "__init__"));
        }
        Ok(PySpeedLimitInformation {
            limit: SpeedLimit::default(),
        })
    }

    /// Kilometres per hour, despite what the upstream parameter name suggests.
    #[getter]
    #[pyo3(name = "speedLimit")]
    fn speed_limit(&self) -> f64 {
        self.limit.as_kmh()
    }

    #[getter]
    #[pyo3(name = "speedLimitKmH")]
    fn speed_limit_kmh(&self) -> f64 {
        self.limit.as_kmh()
    }

    #[getter]
    #[pyo3(name = "speedLimitMPS")]
    fn speed_limit_mps(&self) -> f64 {
        self.limit.speed
    }

    #[getter]
    #[pyo3(name = "isMandatory")]
    fn is_mandatory(&self) -> bool {
        self.limit.is_mandatory
    }

    fn __str__(&self) -> String {
        self.limit.to_string()
    }
}

/// `lanelet2.traffic_rules.TrafficRules`.
#[pyclass(name = "TrafficRules", module = "lanelet2.traffic_rules", frozen)]
pub struct PyTrafficRules {
    rules: Arc<TrafficRules>,
}

impl PyTrafficRules {
    pub fn shared(&self) -> Arc<TrafficRules> {
        self.rules.clone()
    }
}

#[pymethods]
impl PyTrafficRules {
    /// A method, not a property, matching upstream.
    fn location(&self) -> &str {
        self.rules.location()
    }

    fn participant(&self) -> &str {
        self.rules.participant()
    }

    /// `canPass(lanelet)` or `canPass(from, to)`.
    #[pyo3(signature = (lanelet, other = None))]
    #[pyo3(name = "canPass")]
    fn can_pass(
        &self,
        lanelet: &Bound<'_, PyAny>,
        other: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        let (first, _) =
            lanelet_of(lanelet).ok_or_else(|| argument_error("TrafficRules", "canPass"))?;
        match other {
            None => Ok(self.rules.can_pass(&first)),
            Some(other) => {
                let (second, _) =
                    lanelet_of(other).ok_or_else(|| argument_error("TrafficRules", "canPass"))?;
                Ok(self.rules.can_pass_between(&first, &second))
            }
        }
    }

    #[pyo3(name = "canChangeLane")]
    fn can_change_lane(&self, from: &Bound<'_, PyAny>, to: &Bound<'_, PyAny>) -> PyResult<bool> {
        let (Some((from, _)), Some((to, _))) = (lanelet_of(from), lanelet_of(to)) else {
            return Err(argument_error("TrafficRules", "canChangeLane"));
        };
        Ok(self.rules.can_change_lane(&from, &to))
    }

    #[pyo3(name = "speedLimit")]
    fn speed_limit(&self, lanelet: &Bound<'_, PyAny>) -> PyResult<PySpeedLimitInformation> {
        let (lanelet, _) =
            lanelet_of(lanelet).ok_or_else(|| argument_error("TrafficRules", "speedLimit"))?;
        Ok(PySpeedLimitInformation::wrap(
            self.rules.speed_limit(&lanelet),
        ))
    }

    #[pyo3(name = "isOneWay")]
    fn is_one_way(&self, lanelet: &Bound<'_, PyAny>) -> PyResult<bool> {
        let (lanelet, _) =
            lanelet_of(lanelet).ok_or_else(|| argument_error("TrafficRules", "isOneWay"))?;
        Ok(self.rules.is_one_way(&lanelet))
    }

    #[pyo3(name = "hasDynamicRules")]
    fn has_dynamic_rules(&self, lanelet: &Bound<'_, PyAny>) -> PyResult<bool> {
        let (lanelet, _) =
            lanelet_of(lanelet).ok_or_else(|| argument_error("TrafficRules", "hasDynamicRules"))?;
        Ok(self.rules.has_dynamic_rules(&lanelet))
    }

    /// Not part of upstream's Python surface, but the routing graph needs it and
    /// exposing it costs nothing.
    #[pyo3(name = "laneChangeType", signature = (boundary, virtualIsPassable = false))]
    #[allow(non_snake_case)]
    fn lane_change_type(
        &self,
        boundary: &Bound<'_, PyAny>,
        virtualIsPassable: bool,
    ) -> PyResult<String> {
        let (boundary, _) = linestring_of(boundary)
            .ok_or_else(|| argument_error("TrafficRules", "laneChangeType"))?;
        Ok(format!(
            "{:?}",
            self.rules.lane_change_type(&boundary, virtualIsPassable)
        ))
    }

    fn __str__(&self) -> String {
        self.rules.to_string()
    }
}

/// `create(location, participant)`.
#[pyfunction]
#[pyo3(signature = (location, participant))]
fn create(location: &str, participant: &str) -> PyResult<PyTrafficRules> {
    let rules = TrafficRules::create(location, participant).map_err(|error| runtime(error.0))?;
    Ok(PyTrafficRules {
        rules: Arc::new(rules),
    })
}

/// Extracts the shared rules behind a `TrafficRules` object.
pub fn traffic_rules_of(obj: &Bound<'_, PyAny>) -> Option<Arc<TrafficRules>> {
    obj.cast::<PyTrafficRules>()
        .ok()
        .map(|value| value.borrow().shared())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyLocations>()?;
    m.add_class::<PyParticipants>()?;
    m.add_class::<PySpeedLimitInformation>()?;
    m.add_class::<PyTrafficRules>()?;
    m.add_function(wrap_pyfunction!(create, m)?)?;
    Ok(())
}
