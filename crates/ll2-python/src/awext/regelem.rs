//! The regulatory elements `autoware_lanelet2_extension` adds.
//!
//! These are the same shape as the stock typed classes — an id, an attribute map and
//! roles — so almost all of them are generated from a table by [`awext_regelem!`].
//! What differs from the stock ones, and is easy to get wrong:
//!
//! * their accessors are **methods**, not properties. Only the `trafficLights` and
//!   `stopLine` that `AutowareTrafficLight` inherits from the stock `TrafficLight`
//!   are properties.
//! * they create **only the roles they were given**. The stock `TrafficSign` always
//!   creates all four of its roles, possibly empty; these do not, and `.roles` shows
//!   the difference.
//! * whether an accessor hands back a writable or a `Const` primitive follows no
//!   rule — `detectionAreas()` is writable, `crosswalkAreas()` is not — so it is
//!   copied from the reference rather than derived.
//!
//! Registration happens when the Python shim imports us, never at module init:
//! upstream registers on shared-library load, and until that happens a map holding
//! these subtypes must fail exactly as stock Lanelet2 fails.
//!
//! Upstream: `autoware_lanelet2_extension/lib/*.cpp`,
//! `autoware_lanelet2_extension_python/src/regulatory_elements.cpp`

use ll2_core::attribute::AttributeMap;
use ll2_core::id::Id;
use ll2_core::regelem::{
    RegElemKind, RegulatoryElement, RuleParameter, RuleParameterMap, registry, roles,
};
use pyo3::prelude::*;

use crate::conv::optional_attribute_map;
use crate::core::regelem::{
    PyRegulatoryElement, PyTrafficLight, drop_role, first_of_role, linestrings_or_polygons,
    optional_linestring, push_role, role_as_py, role_as_py_const, set_wrapper,
    typed_attributes,
};
use crate::err::argument_error;

// Boost.Python raises this — a TypeError subclass — when an argument does not match
// any registered C++ signature. `NoStoppingArea.__repr__` is bound to a
// `NoParkingArea&`, so calling it is exactly that kind of mismatch, and the class
// name is observable through `type(exc).__name__`.
pyo3::create_exception!(
    autoware_lanelet2_extension_python,
    ArgumentError,
    pyo3::exceptions::PyTypeError
);

/// Roles the extension introduces beyond the stock set.
mod awroles {
    pub const LIGHT_BULBS: &str = "light_bulbs";
    pub const CROSSWALK_POLYGON: &str = "crosswalk_polygon";
    /// What `addCrosswalkArea` writes to. Deliberately *not* what `crosswalkAreas()`
    /// reads — reproducing that mismatch is the point.
    pub const CROSSWALK: &str = "crosswalk";
    pub const START_LINE: &str = "start_line";
    pub const END_LINE: &str = "end_line";
}

/// Reads one linestring or polygon argument, or fails the way the reference does.
fn one_of(obj: &Bound<'_, PyAny>, class: &str) -> PyResult<RuleParameter> {
    let list = pyo3::types::PyList::new(obj.py(), [obj])?;
    linestrings_or_polygons(list.as_any(), class)?
        .pop()
        .ok_or_else(|| argument_error(class, "__init__"))
}

/// Generates one extension regulatory element.
///
/// `repr` is spelled separately from `class` because upstream's `NoStoppingArea`
/// binds `__repr__` to a `NoParkingArea&` and so cannot render itself at all — that
/// is expressed by giving it a repr name it does not answer to.
macro_rules! awext_regelem {
    (
        class: $class:ident, rule: $rule:literal, py: $py:literal,
        $( parent: $parent:expr, )?
        roles: [ $( $role:expr ),* $(,)? ],
    ) => {
        #[pyclass(name = $py, module = "autoware_lanelet2_extension_python.regulatory_elements",
                  extends = PyRegulatoryElement)]
        #[derive(Default)]
        pub struct $class;

        impl $class {
            pub fn kind() -> RegElemKind {
                static KIND: std::sync::OnceLock<RegElemKind> = std::sync::OnceLock::new();
                *KIND.get_or_init(|| {
                    registry::register(Some($rule), $py, awext_regelem!(@parent $($parent)?), None)
                })
            }

            fn wrap(py: Python<'_>, regelem: RegulatoryElement) -> PyResult<Py<PyAny>> {
                Ok(Py::new(py, ($class, PyRegulatoryElement::wrap(regelem)))?.into_any())
            }

            fn build(
                id: Id,
                attributes: &Bound<'_, PyAny>,
                parameters: RuleParameterMap,
            ) -> PyResult<(Self, PyRegulatoryElement)> {
                let attributes =
                    typed_attributes(optional_attribute_map(Some(attributes))?, $rule);
                Ok((
                    $class,
                    PyRegulatoryElement::wrap(RegulatoryElement::new(
                        Self::kind(), id, attributes, parameters,
                    )),
                ))
            }
        }

        // Roles are only mentioned so the compiler checks the table is used.
        const _: &[&str] = &[ $( $role ),* ];
    };
    (@parent) => { None };
    (@parent $parent:expr) => { Some($parent) };
}

// ---------------------------------------------------------------------------
// the table-driven six
// ---------------------------------------------------------------------------

awext_regelem! { class: PyRoadMarking, rule: "road_marking", py: "RoadMarking",
                 roles: [roles::REFERS], }
awext_regelem! { class: PySpeedBump, rule: "speed_bump", py: "SpeedBump",
                 roles: [roles::REFERS], }
awext_regelem! { class: PyNoParkingArea, rule: "no_parking_area", py: "NoParkingArea",
                 roles: [roles::REFERS], }
awext_regelem! { class: PyNoStoppingArea, rule: "no_stopping_area", py: "NoStoppingArea",
                 roles: [roles::REFERS, roles::REF_LINE], }
awext_regelem! { class: PyDetectionArea, rule: "detection_area", py: "DetectionArea",
                 roles: [roles::REFERS, roles::REF_LINE], }
awext_regelem! { class: PyVirtualTrafficLight, rule: "virtual_traffic_light",
                 py: "VirtualTrafficLight",
                 roles: [roles::REFERS, roles::REF_LINE,
                         awroles::START_LINE, awroles::END_LINE], }
awext_regelem! { class: PyCrosswalk, rule: "crosswalk", py: "Crosswalk",
                 roles: [roles::REFERS, awroles::CROSSWALK_POLYGON, roles::REF_LINE], }
/// The one class the macro cannot generate: it derives from the *stock*
/// `TrafficLight` rather than from `RegulatoryElement`, which is what makes
/// `lanelet.trafficLights()` return it, and it takes `traffic_light` over from the
/// stock kind the way upstream's `registry_[name] = factory` assignment does.
#[pyclass(name = "AutowareTrafficLight",
          module = "autoware_lanelet2_extension_python.regulatory_elements",
          extends = PyTrafficLight)]
#[derive(Default)]
pub struct PyAutowareTrafficLight;

impl PyAutowareTrafficLight {
    pub fn kind() -> RegElemKind {
        static KIND: std::sync::OnceLock<RegElemKind> = std::sync::OnceLock::new();
        *KIND.get_or_init(|| {
            registry::register(
                Some("traffic_light"),
                "AutowareTrafficLight",
                Some(RegElemKind::TrafficLight),
                None,
            )
        })
    }

    fn wrap(py: Python<'_>, regelem: RegulatoryElement) -> PyResult<Py<PyAny>> {
        Ok(Py::new(
            py,
            PyClassInitializer::from(PyRegulatoryElement::wrap(regelem))
                .add_subclass(PyTrafficLight)
                .add_subclass(PyAutowareTrafficLight),
        )?
        .into_any())
    }
}

#[pymethods]
impl PyRoadMarking {
    #[new]
    #[pyo3(signature = (id, attributes, road_marking))]
    fn new(
        id: Id,
        attributes: &Bound<'_, PyAny>,
        road_marking: &Bound<'_, PyAny>,
    ) -> PyResult<(Self, PyRegulatoryElement)> {
        let mut parameters = RuleParameterMap::new();
        parameters.insert(
            roles::REFERS.to_owned(),
            vec![one_of(road_marking, "RoadMarking")?],
        );
        Self::build(id, attributes, parameters)
    }

    #[pyo3(name = "roadMarking")]
    fn road_marking(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        first_of_role(slf.py(), &slf.as_super().regelem, roles::REFERS)
    }

    #[pyo3(name = "setRoadMarking")]
    fn set_road_marking(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let parameter = one_of(value, "RoadMarking")?;
        slf.as_super().regelem.set_parameters_for(roles::REFERS, vec![parameter]);
        Ok(())
    }

    #[pyo3(name = "removeRoadMarking")]
    fn remove_road_marking(slf: PyRef<'_, Self>) {
        slf.as_super().regelem.set_parameters_for(roles::REFERS, Vec::new());
    }
}

#[pymethods]
impl PySpeedBump {
    #[new]
    #[pyo3(signature = (id, attributes, speed_bump))]
    fn new(
        id: Id,
        attributes: &Bound<'_, PyAny>,
        speed_bump: &Bound<'_, PyAny>,
    ) -> PyResult<(Self, PyRegulatoryElement)> {
        let mut parameters = RuleParameterMap::new();
        parameters.insert(roles::REFERS.to_owned(), vec![one_of(speed_bump, "SpeedBump")?]);
        Self::build(id, attributes, parameters)
    }

    #[pyo3(name = "speedBump")]
    fn speed_bump(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        first_of_role(slf.py(), &slf.as_super().regelem, roles::REFERS)
    }

    #[pyo3(name = "addSpeedBump")]
    fn add_speed_bump(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        push_role(slf.as_super(), roles::REFERS, value, "SpeedBump", "addSpeedBump")
    }

    #[pyo3(name = "removeSpeedBump")]
    fn remove_speed_bump(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        drop_role(slf.as_super(), roles::REFERS, value, "SpeedBump", "removeSpeedBump")
    }
}

/// The two "a list of polygons under `refers`" classes differ only in their names.
macro_rules! area_class {
    ($class:ident, $py:literal, $getter:literal, $add:literal, $remove:literal) => {
        #[pymethods]
        impl $class {
            #[pyo3(name = $getter)]
            fn areas(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
                role_as_py(slf.py(), &slf.as_super().regelem, roles::REFERS)
            }

            #[pyo3(name = $add)]
            fn add_area(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
                push_role(slf.as_super(), roles::REFERS, value, $py, $add)
            }

            #[pyo3(name = $remove)]
            fn remove_area(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<bool> {
                drop_role(slf.as_super(), roles::REFERS, value, $py, $remove)
            }
        }
    };
}

area_class!(PyNoParkingArea, "NoParkingArea", "noParkingAreas",
            "addNoParkingArea", "removeNoParkingArea");
area_class!(PyNoStoppingArea, "NoStoppingArea", "noStoppingAreas",
            "addNoStoppingArea", "removeNoStoppingArea");
area_class!(PyDetectionArea, "DetectionArea", "detectionAreas",
            "addDetectionArea", "removeDetectionArea");

#[pymethods]
impl PyNoParkingArea {
    #[new]
    #[pyo3(signature = (id, attributes, no_parking_areas))]
    fn new(
        id: Id,
        attributes: &Bound<'_, PyAny>,
        no_parking_areas: &Bound<'_, PyAny>,
    ) -> PyResult<(Self, PyRegulatoryElement)> {
        let mut parameters = RuleParameterMap::new();
        parameters.insert(
            roles::REFERS.to_owned(),
            linestrings_or_polygons(no_parking_areas, "NoParkingArea")?,
        );
        Self::build(id, attributes, parameters)
    }
}

/// The stop-line half that `NoStoppingArea` and `DetectionArea` share.
macro_rules! stop_line_methods {
    ($class:ident, $py:literal) => {
        #[pymethods]
        impl $class {
            #[pyo3(name = "stopLine")]
            fn stop_line(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
                first_of_role(slf.py(), &slf.as_super().regelem, roles::REF_LINE)
            }

            #[pyo3(name = "setStopLine")]
            fn set_stop_line(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
                let parameter = one_of(value, $py)?;
                slf.as_super()
                    .regelem
                    .set_parameters_for(roles::REF_LINE, vec![parameter]);
                Ok(())
            }

            #[pyo3(name = "removeStopLine")]
            fn remove_stop_line(slf: PyRef<'_, Self>) {
                slf.as_super().regelem.set_parameters_for(roles::REF_LINE, Vec::new());
            }
        }
    };
}

stop_line_methods!(PyNoStoppingArea, "NoStoppingArea");

#[pymethods]
impl PyNoStoppingArea {
    /// Upstream binds this to a `NoParkingArea&`, so it does not print the wrong name
    /// — it cannot be called at all. Repaired by default; reproduced under bug-compat.
    fn __repr__(slf: PyRef<'_, Self>) -> PyResult<String> {
        if ll2_core::compat::bug_compat() {
            return Err(ArgumentError::new_err(
                "Python argument types in\n    NoStoppingArea.__repr__(NoStoppingArea)\n\
                 did not match C++ signature:\n    \
                 __repr__(lanelet::autoware::format_v2::NoParkingArea {lvalue})",
            ));
        }
        slf.as_super().__repr__(slf.py())
    }
}
stop_line_methods!(PyDetectionArea, "DetectionArea");

#[pymethods]
impl PyNoStoppingArea {
    #[new]
    #[pyo3(signature = (id, attributes, no_stopping_areas, stopLine = None))]
    #[allow(non_snake_case)]
    fn new(
        id: Id,
        attributes: &Bound<'_, PyAny>,
        no_stopping_areas: &Bound<'_, PyAny>,
        stopLine: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<(Self, PyRegulatoryElement)> {
        let mut parameters = RuleParameterMap::new();
        parameters.insert(
            roles::REFERS.to_owned(),
            linestrings_or_polygons(no_stopping_areas, "NoStoppingArea")?,
        );
        // Measured: with no stop line the role is *absent*, not present and empty.
        if let Some(line) = optional_linestring(stopLine) {
            parameters.insert(
                roles::REF_LINE.to_owned(),
                vec![RuleParameter::LineString(line)],
            );
        }
        Self::build(id, attributes, parameters)
    }
}

#[pymethods]
impl PyDetectionArea {
    #[new]
    #[pyo3(signature = (id, attributes, detectionAreas, stopLine))]
    #[allow(non_snake_case)]
    fn new(
        id: Id,
        attributes: &Bound<'_, PyAny>,
        detectionAreas: &Bound<'_, PyAny>,
        stopLine: &Bound<'_, PyAny>,
    ) -> PyResult<(Self, PyRegulatoryElement)> {
        let mut parameters = RuleParameterMap::new();
        parameters.insert(
            roles::REFERS.to_owned(),
            linestrings_or_polygons(detectionAreas, "DetectionArea")?,
        );
        parameters.insert(
            roles::REF_LINE.to_owned(),
            vec![one_of(stopLine, "DetectionArea")?],
        );
        Self::build(id, attributes, parameters)
    }
}

#[pymethods]
impl PyVirtualTrafficLight {
    /// Upstream's constructor cannot succeed: it validates for exactly one
    /// `start_line`, and its signature gives no way to supply one. The class is
    /// reachable only by loading a map, so the failure is reproduced rather than
    /// repaired — there is no behaviour to be compatible *with* otherwise.
    #[new]
    #[pyo3(signature = (_id, _attributes, _virtual_traffic_light))]
    fn new(
        _id: Id,
        _attributes: &Bound<'_, PyAny>,
        _virtual_traffic_light: &Bound<'_, PyAny>,
    ) -> PyResult<(Self, PyRegulatoryElement)> {
        Err(crate::err::runtime(
            "There must be exactly one start_line defined!",
        ))
    }

    #[pyo3(name = "getVirtualTrafficLight")]
    fn get_virtual_traffic_light(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        first_of_role(slf.py(), &slf.as_super().regelem, roles::REFERS)
    }

    #[pyo3(name = "getStopLine")]
    fn get_stop_line(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        first_of_role(slf.py(), &slf.as_super().regelem, roles::REF_LINE)
    }

    #[pyo3(name = "getStartLine")]
    fn get_start_line(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        first_of_role(slf.py(), &slf.as_super().regelem, awroles::START_LINE)
    }

    #[pyo3(name = "getEndLines")]
    fn get_end_lines(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        role_as_py(slf.py(), &slf.as_super().regelem, awroles::END_LINE)
    }
}

#[pymethods]
impl PyCrosswalk {
    #[new]
    #[pyo3(signature = (id, attributes, crosswalk_lanelet, crosswalk_area, stop_line))]
    fn new(
        id: Id,
        attributes: &Bound<'_, PyAny>,
        crosswalk_lanelet: &Bound<'_, PyAny>,
        crosswalk_area: &Bound<'_, PyAny>,
        stop_line: &Bound<'_, PyAny>,
    ) -> PyResult<(Self, PyRegulatoryElement)> {
        let mut parameters = RuleParameterMap::new();
        // The lanelet is stored weakly, as everywhere else a regulatory element
        // refers back to one. Upstream's `crosswalkLanelet()` dereferences that
        // without checking, so it segfaults once the lanelet is gone rather than
        // reporting anything — our accessor raises instead.
        let (lanelet, _) = crate::core::lanelet::lanelet_of(crosswalk_lanelet)
            .ok_or_else(|| argument_error("Crosswalk", "__init__"))?;
        parameters.insert(
            roles::REFERS.to_owned(),
            vec![RuleParameter::Lanelet(lanelet.downgrade())],
        );
        parameters.insert(
            awroles::CROSSWALK_POLYGON.to_owned(),
            vec![one_of(crosswalk_area, "Crosswalk")?],
        );
        let lines = linestrings_or_polygons(stop_line, "Crosswalk")?;
        if !lines.is_empty() {
            // Upstream inserts per line into a std::map, so only the first survives.
            let kept = if ll2_core::compat::bug_compat() {
                vec![lines[0].clone()]
            } else {
                lines
            };
            parameters.insert(roles::REF_LINE.to_owned(), kept);
        }
        Self::build(id, attributes, parameters)
    }

    #[pyo3(name = "crosswalkAreas")]
    fn crosswalk_areas(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        role_as_py_const(slf.py(), &slf.as_super().regelem, awroles::CROSSWALK_POLYGON)
    }

    #[pyo3(name = "stopLines")]
    fn stop_lines(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        role_as_py_const(slf.py(), &slf.as_super().regelem, roles::REF_LINE)
    }

    /// Upstream takes `.front()` of an empty vector here and segfaults. Raising is
    /// the only reproducible reading of that.
    #[pyo3(name = "crosswalkLanelet")]
    fn crosswalk_lanelet(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let regelem = &slf.as_super().regelem;
        if regelem.parameters_for(roles::REFERS).is_empty() {
            return Err(crate::err::runtime(
                "Crosswalk has no lanelet: the constructor never stores one",
            ));
        }
        first_of_role(slf.py(), regelem, roles::REFERS)
    }

    /// Writes to `crosswalk` while the getter reads `crosswalk_polygon`, so an added
    /// area is invisible — reproduced under bug-compat, repaired by default.
    #[pyo3(name = "addCrosswalkArea")]
    fn add_crosswalk_area(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let role = if ll2_core::compat::bug_compat() {
            awroles::CROSSWALK
        } else {
            awroles::CROSSWALK_POLYGON
        };
        push_role(slf.as_super(), role, value, "Crosswalk", "addCrosswalkArea")
    }

    #[pyo3(name = "removeCrosswalkArea")]
    fn remove_crosswalk_area(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        drop_role(
            slf.as_super(),
            awroles::CROSSWALK_POLYGON,
            value,
            "Crosswalk",
            "removeCrosswalkArea",
        )
    }
}

#[pymethods]
impl PyAutowareTrafficLight {
    #[new]
    #[pyo3(signature = (id, attributes, trafficLights, stopLine = None, lightBulbs = None))]
    #[allow(non_snake_case)]
    fn new(
        id: Id,
        attributes: &Bound<'_, PyAny>,
        trafficLights: &Bound<'_, PyAny>,
        stopLine: Option<&Bound<'_, PyAny>>,
        lightBulbs: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let lights = linestrings_or_polygons(trafficLights, "AutowareTrafficLight")?;
        if lights.is_empty() {
            return Err(crate::err::runtime("No traffic light defined!"));
        }
        let mut parameters = RuleParameterMap::new();
        parameters.insert(roles::REFERS.to_owned(), lights);
        if let Some(line) = optional_linestring(stopLine) {
            parameters.insert(
                roles::REF_LINE.to_owned(),
                vec![RuleParameter::LineString(line)],
            );
        }
        if let Some(bulbs) = lightBulbs
            && !bulbs.is_none()
        {
            let bulbs = linestrings_or_polygons(bulbs, "AutowareTrafficLight")?;
            if !bulbs.is_empty() {
                parameters.insert(awroles::LIGHT_BULBS.to_owned(), bulbs);
            }
        }
        let attributes = typed_attributes(optional_attribute_map(Some(attributes))?, "traffic_light");
        Ok(PyClassInitializer::from(PyRegulatoryElement::wrap(
            RegulatoryElement::new(Self::kind(), id, attributes, parameters),
        ))
        .add_subclass(PyTrafficLight)
        .add_subclass(PyAutowareTrafficLight))
    }

    #[pyo3(name = "lightBulbs")]
    fn light_bulbs(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        role_as_py(
            slf.py(),
            &slf.as_super().as_super().regelem,
            awroles::LIGHT_BULBS,
        )
    }

    #[pyo3(name = "addLightBulbs")]
    fn add_light_bulbs(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        push_role(
            slf.as_super().as_super(),
            awroles::LIGHT_BULBS,
            value,
            "AutowareTrafficLight",
            "addLightBulbs",
        )
    }

    #[pyo3(name = "removeLightBulbs")]
    fn remove_light_bulbs(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        drop_role(
            slf.as_super().as_super(),
            awroles::LIGHT_BULBS,
            value,
            "AutowareTrafficLight",
            "removeLightBulbs",
        )
    }
}

/// The two kinds the C++ factory knows but the Python bindings do not expose.
///
/// They still have to be registered, or a map containing them fails to load with the
/// extension present — which is a difference the reference would not have.
fn register_classless() {
    for (rule, class) in [
        ("bus_stop_area", "BusStopArea"),
        ("roundabout", "Roundabout"),
    ] {
        registry::register(Some(rule), class, None, None);
    }
}

/// Wires the extension into the registry. Idempotent; called by the Python shim.
pub fn install() {
    set_wrapper(PyRoadMarking::kind(), PyRoadMarking::wrap);
    set_wrapper(PySpeedBump::kind(), PySpeedBump::wrap);
    set_wrapper(PyNoParkingArea::kind(), PyNoParkingArea::wrap);
    set_wrapper(PyNoStoppingArea::kind(), PyNoStoppingArea::wrap);
    set_wrapper(PyDetectionArea::kind(), PyDetectionArea::wrap);
    set_wrapper(PyVirtualTrafficLight::kind(), PyVirtualTrafficLight::wrap);
    set_wrapper(PyCrosswalk::kind(), PyCrosswalk::wrap);
    // Claims `traffic_light` from the stock TrafficLight, exactly as upstream's
    // `registry_[name] = factory` assignment does.
    set_wrapper(PyAutowareTrafficLight::kind(), PyAutowareTrafficLight::wrap);
    register_classless();
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyAutowareTrafficLight>()?;
    m.add_class::<PyCrosswalk>()?;
    m.add_class::<PyDetectionArea>()?;
    m.add_class::<PyNoParkingArea>()?;
    m.add_class::<PyNoStoppingArea>()?;
    m.add_class::<PyRoadMarking>()?;
    m.add_class::<PySpeedBump>()?;
    m.add_class::<PyVirtualTrafficLight>()?;
    // Deliberately a dunder: `canon.public_names` filters it, so it cannot show up as
    // an extra name in the API-surface case.
    m.add_function(wrap_pyfunction!(register_extension, m)?)?;
    Ok(())
}

#[pyfunction]
#[pyo3(name = "__register__")]
fn register_extension() {
    install();
}

/// `AttributeMap` is referenced by the generated builders.
const _: Option<AttributeMap> = None;
