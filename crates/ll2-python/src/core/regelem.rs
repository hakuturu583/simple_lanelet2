//! Regulatory elements: the base class, the five typed subclasses, and the
//! parameter maps they are built from.
//!
//! The typed classes are conveniences over one parameter map. `TrafficLight`'s
//! lights are the `refers` role and its stop line is the single entry under
//! `ref_line`; `RightOfWay` uses `right_of_way` and `yield`. Which roles a
//! constructor creates — including ones it leaves empty — is observable through
//! `.roles`, so it is reproduced per type.
//!
//! Upstream: `lanelet2_python/python_api/core.cpp:1206-1400`,
//! `lanelet2_core/src/BasicRegulatoryElements.cpp`

use ll2_core::attribute::{Attribute, AttributeMap};
use ll2_core::compat;
use ll2_core::id::Id;
use ll2_core::lanelet::Lanelet;
use ll2_core::linestring::LineString;
use ll2_core::regelem::{RegElemKind, RegulatoryElement, RuleParameter, RuleParameterMap, roles};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::conv::{attributes_repr_arg, optional_attribute_map};
use crate::core::area::{PyArea, PyConstArea, area_of};
use crate::core::attribute::PyAttributeMap;
use crate::core::lanelet::{PyConstLanelet, PyLanelet, lanelet_of};
use crate::core::linestring::{
    Kind, PyConstLineString3d, PyConstPolygon3d, PyLineString3d, PyPolygon3d, linestring_of,
};
use crate::err::{argument_error, not_instantiable, runtime};

// ---------------------------------------------------------------------------
// rule parameters
// ---------------------------------------------------------------------------

/// Reads one rule parameter from any primitive Python object.
pub fn rule_parameter_from_any(obj: &Bound<'_, PyAny>) -> Option<RuleParameter> {
    if let Some((line, kind)) = linestring_of(obj) {
        return Some(if kind.polygon {
            RuleParameter::Polygon(line)
        } else {
            RuleParameter::LineString(line)
        });
    }
    if let Some((lanelet, _)) = lanelet_of(obj) {
        return Some(RuleParameter::Lanelet(lanelet.downgrade()));
    }
    if let Some(area) = area_of(obj) {
        return Some(RuleParameter::Area(area.downgrade()));
    }
    crate::core::point::point_of(obj).map(|(point, _)| RuleParameter::Point(point))
}

/// Converts a rule parameter to Python.
///
/// `const` selects between the read-only and writable class families: the
/// `parameters` property hands out `Const` types, while the typed accessors like
/// `trafficLights` hand out writable ones.
pub fn rule_parameter_to_py(
    py: Python<'_>,
    parameter: &RuleParameter,
    is_const: bool,
) -> PyResult<Py<PyAny>> {
    Ok(match parameter {
        RuleParameter::Point(point) => {
            if is_const {
                Py::new(py, crate::core::point::PyConstPoint3d::wrap(point.clone()))?.into_any()
            } else {
                Py::new(py, crate::core::point::PyPoint3d::wrap(point.clone()))?.into_any()
            }
        }
        RuleParameter::LineString(line) => {
            if is_const {
                Py::new(py, PyConstLineString3d::wrap(line.clone()))?.into_any()
            } else {
                Py::new(py, PyLineString3d::wrap(line.clone()))?.into_any()
            }
        }
        RuleParameter::Polygon(polygon) => {
            if is_const {
                Py::new(py, PyConstPolygon3d::wrap(polygon.clone()))?.into_any()
            } else {
                Py::new(py, PyPolygon3d::wrap(polygon.clone()))?.into_any()
            }
        }
        RuleParameter::Lanelet(weak) => match weak.upgrade() {
            None => py.None(),
            Some(lanelet) => {
                if is_const {
                    Py::new(py, PyConstLanelet::wrap(lanelet))?.into_any()
                } else {
                    Py::new(py, PyLanelet::wrap(lanelet))?.into_any()
                }
            }
        },
        RuleParameter::Area(weak) => match weak.upgrade() {
            None => py.None(),
            Some(area) => {
                if is_const {
                    Py::new(py, PyConstArea::wrap(area))?.into_any()
                } else {
                    Py::new(py, PyArea::wrap(area))?.into_any()
                }
            }
        },
    })
}

/// Renders a parameter map as a plain Python dict, the way a regulatory element's
/// `repr` embeds it.
///
/// Lanelets and areas are replaced with a `_ReprWrapper` holding their repr rendered
/// *without* regulatory elements. Without that, a lanelet's repr would recurse
/// through its regulatory element straight back into itself.
fn parameters_dict_repr(py: Python<'_>, parameters: &RuleParameterMap) -> PyResult<String> {
    let dict = PyDict::new(py);
    for (role, values) in parameters {
        let rendered = PyList::empty(py);
        for value in values {
            match value {
                RuleParameter::Lanelet(weak) => {
                    let text = match weak.upgrade() {
                        None => "None".to_owned(),
                        Some(lanelet) => lanelet_repr_without_regelems(py, &lanelet)?,
                    };
                    rendered.append(Py::new(py, ReprWrapper { text })?)?;
                }
                RuleParameter::Area(weak) => {
                    let text = match weak.upgrade() {
                        None => "None".to_owned(),
                        Some(area) => crate::core::area::area_repr_without_regelems(py, &area)?,
                    };
                    rendered.append(Py::new(py, ReprWrapper { text })?)?;
                }
                other => rendered.append(rule_parameter_to_py(py, other, false)?)?,
            }
        }
        dict.set_item(role, rendered)?;
    }
    dict.repr()?.extract()
}

/// Renders a lanelet held in a parameter map.
///
/// The bounds come out as `ConstLineString3d` because the map holds the lanelet
/// through a const handle, but the class name is hardcoded to `Lanelet` — upstream's
/// `makeLaneletRepr` takes a display name and then ignores it.
fn lanelet_repr_without_regelems(py: Python<'_>, lanelet: &Lanelet) -> PyResult<String> {
    let attributes = attributes_repr_arg(py, &lanelet.attributes().read())?;
    let left = PyConstLineString3d::wrap(lanelet.left_bound()).__repr__(py)?;
    let right = PyConstLineString3d::wrap(lanelet.right_bound()).__repr__(py)?;
    Ok(lanelet.repr("Lanelet", &left, &right, &attributes, ""))
}

/// Carries a pre-rendered repr, so that a cycle can be broken by substitution.
///
/// Upstream exports this under exactly this name.
#[pyclass(name = "_ReprWrapper", module = "lanelet2.core", frozen)]
pub struct ReprWrapper {
    text: String,
}

#[pymethods]
impl ReprWrapper {
    fn __repr__(&self) -> &str {
        &self.text
    }

    fn __str__(&self) -> &str {
        &self.text
    }
}

// ---------------------------------------------------------------------------
// parameter maps
// ---------------------------------------------------------------------------

macro_rules! parameter_map {
    ($py_name:literal, $rust:ident, $is_const:tt, $entry:ident, $entry_name:literal) => {
        #[doc = concat!("One entry of a `", $py_name, "`.")]
        #[pyclass(name = $entry_name, module = "lanelet2.core")]
        pub struct $entry {
            key: String,
            data: Py<PyAny>,
        }

        #[pymethods]
        impl $entry {
            fn key(&self) -> &str {
                &self.key
            }

            fn data(&self, py: Python<'_>) -> Py<PyAny> {
                self.data.clone_ref(py)
            }
        }

        #[doc = concat!("`lanelet2.core.", $py_name, "`.")]
        #[pyclass(name = $py_name, module = "lanelet2.core")]
        pub struct $rust {
            parameters: RuleParameterMap,
        }

        impl $rust {
            pub fn wrap(parameters: RuleParameterMap) -> Self {
                $rust { parameters }
            }
        }

        #[pymethods]
        impl $rust {
            fn __len__(&self) -> usize {
                self.parameters.len()
            }

            fn __contains__(&self, role: &str) -> bool {
                self.parameters.contains_key(role)
            }

            fn __getitem__(&self, py: Python<'_>, role: &str) -> PyResult<Py<PyAny>> {
                let values = self
                    .parameters
                    .get(role)
                    .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err("Invalid key"))?;
                let list = PyList::empty(py);
                for value in values {
                    list.append(rule_parameter_to_py(py, value, $is_const)?)?;
                }
                Ok(list.into_any().unbind())
            }

            fn keys(&self) -> Vec<String> {
                self.parameters.keys().cloned().collect()
            }

            fn values(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                let outer = PyList::empty(py);
                for values in self.parameters.values() {
                    let inner = PyList::empty(py);
                    for value in values {
                        inner.append(rule_parameter_to_py(py, value, $is_const)?)?;
                    }
                    outer.append(inner)?;
                }
                Ok(outer.into_any().unbind())
            }

            /// Iterating yields entry objects, a `map_indexing_suite` convention.
            fn __iter__(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
                let py = slf.py();
                let entries = PyList::empty(py);
                for (role, values) in &slf.parameters {
                    let list = PyList::empty(py);
                    for value in values {
                        list.append(rule_parameter_to_py(py, value, $is_const)?)?;
                    }
                    entries.append(Py::new(
                        py,
                        $entry {
                            key: role.clone(),
                            data: list.into_any().unbind(),
                        },
                    )?)?;
                }
                Ok(entries.try_iter()?.into())
            }

            fn __setitem__(&mut self, role: String, values: &Bound<'_, PyAny>) -> PyResult<()> {
                let mut parsed = Vec::new();
                for item in values.try_iter()? {
                    let item = item?;
                    parsed.push(
                        rule_parameter_from_any(&item)
                            .ok_or_else(|| argument_error($py_name, "__setitem__"))?,
                    );
                }
                self.parameters.insert(role, parsed);
                Ok(())
            }

            fn __delitem__(&mut self, role: &str) -> PyResult<()> {
                self.parameters
                    .remove(role)
                    .map(|_| ())
                    .ok_or_else(|| pyo3::exceptions::PyKeyError::new_err("Invalid key"))
            }

            fn items(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
                let outer = PyList::empty(py);
                for (role, values) in &self.parameters {
                    let inner = PyList::empty(py);
                    for value in values {
                        inner.append(rule_parameter_to_py(py, value, $is_const)?)?;
                    }
                    outer.append((role, inner))?;
                }
                Ok(outer.into_any().unbind())
            }

            fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
                let dict = PyDict::new(py);
                for (role, values) in &self.parameters {
                    let list = PyList::empty(py);
                    for value in values {
                        list.append(rule_parameter_to_py(py, value, $is_const)?)?;
                    }
                    dict.set_item(role, list)?;
                }
                Ok(format!(
                    "{}({})",
                    $py_name,
                    dict.repr()?.extract::<String>()?
                ))
            }
        }
    };
}

parameter_map!(
    "RuleParameterMap",
    PyRuleParameterMap,
    false,
    PyRuleParameterMapEntry,
    "map_indexing_suite_RuleParameterMap_entry"
);
parameter_map!(
    "ConstRuleParameterMap",
    PyConstRuleParameterMap,
    true,
    PyConstRuleParameterMapEntry,
    "map_indexing_suite_ConstRuleParameterMap_entry"
);

// ---------------------------------------------------------------------------
// the base class
// ---------------------------------------------------------------------------

/// `lanelet2.core.RegulatoryElement`, the abstract base of every typed element.
#[pyclass(name = "RegulatoryElement", module = "lanelet2.core", subclass)]
pub struct PyRegulatoryElement {
    pub(crate) regelem: RegulatoryElement,
}

impl PyRegulatoryElement {
    pub fn wrap(regelem: RegulatoryElement) -> Self {
        PyRegulatoryElement { regelem }
    }

    pub fn inner(&self) -> &RegulatoryElement {
        &self.regelem
    }
}

#[pymethods]
impl PyRegulatoryElement {
    #[new]
    fn new() -> PyResult<Self> {
        Err(not_instantiable())
    }

    #[getter]
    fn id(&self) -> Id {
        self.regelem.id()
    }

    #[setter(id)]
    fn set_id(&self, value: Id) {
        self.regelem.set_id(value);
    }

    #[getter]
    fn attributes(&self) -> PyAttributeMap {
        PyAttributeMap::proxy(self.regelem.attributes().clone())
    }

    /// A snapshot, not a live view: upstream returns the map by value.
    #[getter]
    fn parameters(&self) -> PyConstRuleParameterMap {
        PyConstRuleParameterMap::wrap(self.regelem.parameters())
    }

    #[getter]
    fn roles(&self) -> Vec<String> {
        self.regelem.roles()
    }

    /// The first parameter with this id, across every role.
    fn find(&self, py: Python<'_>, id: Id) -> PyResult<Py<PyAny>> {
        match self.regelem.find(id) {
            None => Ok(py.None()),
            Some(parameter) => rule_parameter_to_py(py, &parameter, true),
        }
    }

    /// The number of *roles*, not of referenced primitives.
    fn __len__(&self) -> usize {
        self.regelem.len()
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        match regelem_of(other) {
            Some(regelem) => self.regelem.is_same_data(&regelem),
            None => false,
        }
    }

    fn __ne__(&self, other: &Bound<'_, PyAny>) -> bool {
        !self.__eq__(other)
    }

    fn __hash__(&self) -> i64 {
        if compat::hash_by_id_only() {
            self.regelem.id()
        } else {
            self.regelem.identity() as i64
        }
    }

    fn __str__(&self) -> String {
        self.regelem.to_display_string()
    }

    pub fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        let attributes = attributes_repr_arg(py, &self.regelem.attributes().read())?;
        let parameters = parameters_dict_repr(py, &self.regelem.parameters())?;
        Ok(ll2_core::fmt::make_repr(
            self.regelem.kind().class_name(),
            &[self.regelem.id().to_string(), parameters, attributes],
        ))
    }
}

/// Extracts the shared element behind any regulatory element object.
pub fn regelem_of(obj: &Bound<'_, PyAny>) -> Option<RegulatoryElement> {
    obj.cast::<PyRegulatoryElement>()
        .ok()
        .map(|value| value.borrow().regelem.clone())
}

// ---------------------------------------------------------------------------
// typed subclasses
// ---------------------------------------------------------------------------

/// The attributes every typed element carries, so that a written map round-trips.
fn typed_attributes(mut attributes: AttributeMap, subtype: &str) -> AttributeMap {
    attributes.insert("type".into(), Attribute::new("regulatory_element"));
    attributes.insert("subtype".into(), Attribute::new(subtype));
    attributes
}

fn linestrings_or_polygons(obj: &Bound<'_, PyAny>, class: &str) -> PyResult<Vec<RuleParameter>> {
    let mut out = Vec::new();
    for item in obj.try_iter()? {
        let item = item?;
        let (line, kind) = linestring_of(&item).ok_or_else(|| argument_error(class, "__init__"))?;
        out.push(if kind.polygon {
            RuleParameter::Polygon(line)
        } else {
            RuleParameter::LineString(line)
        });
    }
    Ok(out)
}

fn lanelets(obj: &Bound<'_, PyAny>, class: &str) -> PyResult<Vec<RuleParameter>> {
    let mut out = Vec::new();
    for item in obj.try_iter()? {
        let item = item?;
        let (lanelet, _) = lanelet_of(&item).ok_or_else(|| argument_error(class, "__init__"))?;
        out.push(RuleParameter::Lanelet(lanelet.downgrade()));
    }
    Ok(out)
}

fn optional_linestring(obj: Option<&Bound<'_, PyAny>>) -> Option<LineString> {
    let obj = obj?;
    if obj.is_none() {
        return None;
    }
    linestring_of(obj).map(|(line, _)| line)
}

/// Reads the writable primitives stored under a role.
fn role_as_py(py: Python<'_>, regelem: &RegulatoryElement, role: &str) -> PyResult<Py<PyAny>> {
    let list = PyList::empty(py);
    for value in regelem.parameters_for(role) {
        // A weak reference that no longer resolves is skipped, not reported as
        // None: upstream's typed accessors filter expired entries out.
        if matches!(value, RuleParameter::Lanelet(ref w) if w.upgrade().is_none()) {
            continue;
        }
        if matches!(value, RuleParameter::Area(ref w) if w.upgrade().is_none()) {
            continue;
        }
        list.append(rule_parameter_to_py(py, &value, false)?)?;
    }
    Ok(list.into_any().unbind())
}

fn first_of_role(py: Python<'_>, regelem: &RegulatoryElement, role: &str) -> PyResult<Py<PyAny>> {
    match regelem.parameters_for(role).first() {
        None => Ok(py.None()),
        Some(value) => rule_parameter_to_py(py, value, false),
    }
}

/// `lanelet2.core.TrafficLight`.
#[pyclass(name = "TrafficLight", module = "lanelet2.core", extends = PyRegulatoryElement)]
#[derive(Default)]
pub struct PyTrafficLight;

#[pymethods]
impl PyTrafficLight {
    #[new]
    #[pyo3(signature = (id, attributes, trafficLights, stopLine = None))]
    #[allow(non_snake_case)]
    fn new(
        id: Id,
        attributes: &Bound<'_, PyAny>,
        trafficLights: &Bound<'_, PyAny>,
        stopLine: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<(Self, PyRegulatoryElement)> {
        let lights = linestrings_or_polygons(trafficLights, "TrafficLight")?;
        if lights.is_empty() {
            return Err(runtime("No traffic light defined!"));
        }
        let mut parameters = RuleParameterMap::new();
        parameters.insert(roles::REFERS.into(), lights);
        if let Some(line) = optional_linestring(stopLine) {
            parameters.insert(
                roles::REF_LINE.into(),
                vec![RuleParameter::LineString(line)],
            );
        }
        let attributes =
            typed_attributes(optional_attribute_map(Some(attributes))?, "traffic_light");
        Ok((
            PyTrafficLight,
            PyRegulatoryElement::wrap(RegulatoryElement::new(
                RegElemKind::TrafficLight,
                id,
                attributes,
                parameters,
            )),
        ))
    }

    #[getter]
    #[pyo3(name = "trafficLights")]
    fn traffic_lights(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        role_as_py(py, &slf.as_super().regelem, roles::REFERS)
    }

    #[getter]
    #[pyo3(name = "stopLine")]
    fn stop_line(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        first_of_role(py, &slf.as_super().regelem, roles::REF_LINE)
    }

    #[setter(stopLine)]
    fn set_stop_line(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let regelem = &slf.as_super().regelem;
        match optional_linestring(Some(value)) {
            None => regelem.set_parameters_for(roles::REF_LINE, Vec::new()),
            Some(line) => {
                regelem.set_parameters_for(roles::REF_LINE, vec![RuleParameter::LineString(line)])
            }
        }
        Ok(())
    }

    #[pyo3(name = "removeStopLine")]
    fn remove_stop_line(slf: PyRef<'_, Self>) {
        slf.as_super()
            .regelem
            .set_parameters_for(roles::REF_LINE, Vec::new());
    }

    #[pyo3(name = "addTrafficLight")]
    fn add_traffic_light(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let parameter = rule_parameter_from_any(value)
            .ok_or_else(|| argument_error("TrafficLight", "addTrafficLight"))?;
        slf.as_super()
            .regelem
            .push_parameter(roles::REFERS, parameter);
        Ok(())
    }

    #[pyo3(name = "removeTrafficLight")]
    fn remove_traffic_light(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        let parameter = rule_parameter_from_any(value)
            .ok_or_else(|| argument_error("TrafficLight", "removeTrafficLight"))?;
        Ok(slf
            .as_super()
            .regelem
            .remove_parameter(roles::REFERS, &parameter))
    }
}

/// Looks up a `ManeuverType` member.
///
/// The enum is built in the Python shim rather than here, because Boost's derives
/// from `int` and a PyO3 enum cannot.
fn maneuver_type(py: Python<'_>, member: &str) -> PyResult<Py<PyAny>> {
    Ok(py
        .import("lanelet2.core")?
        .getattr("ManeuverType")?
        .getattr(member)?
        .unbind())
}

/// `lanelet2.core.RightOfWay`.
#[pyclass(name = "RightOfWay", module = "lanelet2.core", extends = PyRegulatoryElement)]
#[derive(Default)]
pub struct PyRightOfWay;

#[pymethods]
impl PyRightOfWay {
    #[new]
    #[pyo3(signature = (id, attributes, rightOfWayLanelets, yieldLanelets, stopLine = None))]
    #[allow(non_snake_case)]
    fn new(
        id: Id,
        attributes: &Bound<'_, PyAny>,
        rightOfWayLanelets: &Bound<'_, PyAny>,
        yieldLanelets: &Bound<'_, PyAny>,
        stopLine: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<(Self, PyRegulatoryElement)> {
        let mut parameters = RuleParameterMap::new();
        parameters.insert(
            roles::RIGHT_OF_WAY.into(),
            lanelets(rightOfWayLanelets, "RightOfWay")?,
        );
        parameters.insert(roles::YIELD.into(), lanelets(yieldLanelets, "RightOfWay")?);
        if let Some(line) = optional_linestring(stopLine) {
            parameters.insert(
                roles::REF_LINE.into(),
                vec![RuleParameter::LineString(line)],
            );
        }
        let attributes =
            typed_attributes(optional_attribute_map(Some(attributes))?, "right_of_way");
        Ok((
            PyRightOfWay,
            PyRegulatoryElement::wrap(RegulatoryElement::new(
                RegElemKind::RightOfWay,
                id,
                attributes,
                parameters,
            )),
        ))
    }

    #[pyo3(name = "rightOfWayLanelets")]
    fn right_of_way_lanelets(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        role_as_py(py, &slf.as_super().regelem, roles::RIGHT_OF_WAY)
    }

    #[pyo3(name = "yieldLanelets")]
    fn yield_lanelets(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        role_as_py(py, &slf.as_super().regelem, roles::YIELD)
    }

    #[getter]
    #[pyo3(name = "stopLine")]
    fn stop_line(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        first_of_role(py, &slf.as_super().regelem, roles::REF_LINE)
    }

    #[setter(stopLine)]
    fn set_stop_line(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) {
        let regelem = &slf.as_super().regelem;
        match optional_linestring(Some(value)) {
            None => regelem.set_parameters_for(roles::REF_LINE, Vec::new()),
            Some(line) => {
                regelem.set_parameters_for(roles::REF_LINE, vec![RuleParameter::LineString(line)])
            }
        }
    }

    #[pyo3(name = "removeStopLine")]
    fn remove_stop_line(slf: PyRef<'_, Self>) {
        slf.as_super()
            .regelem
            .set_parameters_for(roles::REF_LINE, Vec::new());
    }

    #[pyo3(name = "addRightOfWayLanelet")]
    fn add_right_of_way(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let (lanelet, _) = lanelet_of(value)
            .ok_or_else(|| argument_error("RightOfWay", "addRightOfWayLanelet"))?;
        slf.as_super().regelem.push_parameter(
            roles::RIGHT_OF_WAY,
            RuleParameter::Lanelet(lanelet.downgrade()),
        );
        Ok(())
    }

    #[pyo3(name = "removeRightOfWayLanelet")]
    fn remove_right_of_way(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        let (lanelet, _) = lanelet_of(value)
            .ok_or_else(|| argument_error("RightOfWay", "removeRightOfWayLanelet"))?;
        Ok(slf.as_super().regelem.remove_parameter(
            roles::RIGHT_OF_WAY,
            &RuleParameter::Lanelet(lanelet.downgrade()),
        ))
    }

    #[pyo3(name = "addYieldLanelet")]
    fn add_yield(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let (lanelet, _) =
            lanelet_of(value).ok_or_else(|| argument_error("RightOfWay", "addYieldLanelet"))?;
        slf.as_super()
            .regelem
            .push_parameter(roles::YIELD, RuleParameter::Lanelet(lanelet.downgrade()));
        Ok(())
    }

    #[pyo3(name = "removeYieldLanelet")]
    fn remove_yield(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        let (lanelet, _) =
            lanelet_of(value).ok_or_else(|| argument_error("RightOfWay", "removeYieldLanelet"))?;
        Ok(slf
            .as_super()
            .regelem
            .remove_parameter(roles::YIELD, &RuleParameter::Lanelet(lanelet.downgrade())))
    }

    /// Which manoeuvre this element prescribes for a lanelet: right of way, yield,
    /// or unknown when the lanelet is not one of its own.
    #[pyo3(name = "getManeuver")]
    fn get_maneuver(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let (lanelet, _) =
            lanelet_of(value).ok_or_else(|| argument_error("RightOfWay", "getManeuver"))?;
        let regelem = &slf.as_super().regelem;
        let holds = |role: &str| {
            regelem.parameters_for(role).iter().any(|parameter| {
                matches!(parameter, RuleParameter::Lanelet(weak)
                    if weak.upgrade().is_some_and(|held| held.is_same_data(&lanelet)))
            })
        };
        maneuver_type(
            py,
            if holds(roles::RIGHT_OF_WAY) {
                "RightOfWay"
            } else if holds(roles::YIELD) {
                "Yield"
            } else {
                "Unknown"
            },
        )
    }
}

/// `lanelet2.core.TrafficSignsWithType` — signs plus the sign type they share.
#[pyclass(name = "TrafficSignsWithType", module = "lanelet2.core")]
pub struct PyTrafficSignsWithType {
    pub(crate) signs: Vec<RuleParameter>,
    pub(crate) sign_type: String,
}

#[pymethods]
impl PyTrafficSignsWithType {
    #[new]
    #[pyo3(signature = (trafficSigns, sign_type = String::new()))]
    #[allow(non_snake_case)]
    fn new(trafficSigns: &Bound<'_, PyAny>, sign_type: String) -> PyResult<Self> {
        Ok(PyTrafficSignsWithType {
            signs: linestrings_or_polygons(trafficSigns, "TrafficSignsWithType")?,
            sign_type,
        })
    }

    #[getter]
    #[pyo3(name = "trafficSigns")]
    fn traffic_signs(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        if compat::traffic_signs_property_unreadable() {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "No Python class registered for C++ class \
                 std::vector<lanelet::LineStringOrPolygon3d, \
                 std::allocator<lanelet::LineStringOrPolygon3d> >",
            ));
        }
        let list = PyList::empty(py);
        for sign in &self.signs {
            list.append(rule_parameter_to_py(py, sign, false)?)?;
        }
        Ok(list.into_any().unbind())
    }

    #[getter]
    #[pyo3(name = "type")]
    fn get_type(&self) -> &str {
        &self.sign_type
    }

    #[setter]
    #[pyo3(name = "type")]
    fn set_type(&mut self, value: String) {
        self.sign_type = value;
    }
}

fn push_role(
    base: &PyRegulatoryElement,
    role: &str,
    value: &Bound<'_, PyAny>,
    method: &str,
) -> PyResult<()> {
    let parameter =
        rule_parameter_from_any(value).ok_or_else(|| argument_error("TrafficSign", method))?;
    base.regelem.push_parameter(role, parameter);
    Ok(())
}

fn drop_role(
    base: &PyRegulatoryElement,
    role: &str,
    value: &Bound<'_, PyAny>,
    method: &str,
) -> PyResult<bool> {
    let parameter =
        rule_parameter_from_any(value).ok_or_else(|| argument_error("TrafficSign", method))?;
    Ok(base.regelem.remove_parameter(role, &parameter))
}

/// `lanelet2.core.TrafficSign`.
#[pyclass(name = "TrafficSign", module = "lanelet2.core", extends = PyRegulatoryElement, subclass)]
#[derive(Default)]
pub struct PyTrafficSign;

/// Builds the four-role parameter map shared by `TrafficSign` and `SpeedLimit`.
#[allow(non_snake_case)]
fn traffic_sign_parameters(
    trafficSigns: &Bound<'_, PyAny>,
    cancellingTrafficSigns: Option<&Bound<'_, PyAny>>,
    refLines: Option<&Bound<'_, PyAny>>,
    cancelLines: Option<&Bound<'_, PyAny>>,
) -> PyResult<(RuleParameterMap, String)> {
    let signs = trafficSigns.cast::<PyTrafficSignsWithType>()?.borrow();
    let cancelling = match cancellingTrafficSigns {
        None => Vec::new(),
        Some(value) if value.is_none() => Vec::new(),
        Some(value) => value
            .cast::<PyTrafficSignsWithType>()?
            .borrow()
            .signs
            .clone(),
    };
    let lines = |obj: Option<&Bound<'_, PyAny>>| -> PyResult<Vec<RuleParameter>> {
        match obj {
            None => Ok(Vec::new()),
            Some(value) if value.is_none() => Ok(Vec::new()),
            Some(value) => linestrings_or_polygons(value, "TrafficSign"),
        }
    };

    let mut parameters = RuleParameterMap::new();
    parameters.insert(roles::REFERS.into(), signs.signs.clone());
    parameters.insert(roles::CANCELS.into(), cancelling);
    parameters.insert(roles::REF_LINE.into(), lines(refLines)?);
    parameters.insert(roles::CANCEL_LINE.into(), lines(cancelLines)?);
    Ok((parameters, signs.sign_type.clone()))
}

#[pymethods]
impl PyTrafficSign {
    #[new]
    #[pyo3(signature = (id, attributes, trafficSigns, cancellingTrafficSigns = None,
                        refLines = None, cancelLines = None))]
    #[allow(non_snake_case)]
    fn new(
        id: Id,
        attributes: &Bound<'_, PyAny>,
        trafficSigns: &Bound<'_, PyAny>,
        cancellingTrafficSigns: Option<&Bound<'_, PyAny>>,
        refLines: Option<&Bound<'_, PyAny>>,
        cancelLines: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<(Self, PyRegulatoryElement)> {
        let (parameters, _) =
            traffic_sign_parameters(trafficSigns, cancellingTrafficSigns, refLines, cancelLines)?;
        let attributes =
            typed_attributes(optional_attribute_map(Some(attributes))?, "traffic_sign");
        Ok((
            PyTrafficSign,
            PyRegulatoryElement::wrap(RegulatoryElement::new(
                RegElemKind::TrafficSign,
                id,
                attributes,
                parameters,
            )),
        ))
    }

    #[pyo3(name = "trafficSigns")]
    fn traffic_signs(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        role_as_py(py, &slf.as_super().regelem, roles::REFERS)
    }

    #[pyo3(name = "cancellingTrafficSigns")]
    fn cancelling_traffic_signs(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        role_as_py(py, &slf.as_super().regelem, roles::CANCELS)
    }

    #[pyo3(name = "refLines")]
    fn ref_lines(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        role_as_py(py, &slf.as_super().regelem, roles::REF_LINE)
    }

    #[pyo3(name = "cancelLines")]
    fn cancel_lines(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        role_as_py(py, &slf.as_super().regelem, roles::CANCEL_LINE)
    }

    /// The `subtype` of the first sign, falling back to the element's own
    /// `sign_type` attribute when there are no signs.
    #[pyo3(name = "type")]
    fn sign_type(slf: PyRef<'_, Self>) -> PyResult<String> {
        let regelem = &slf.as_super().regelem;
        if let Some(first) = regelem.parameters_for(roles::REFERS).first() {
            let subtype = match first {
                RuleParameter::LineString(line) | RuleParameter::Polygon(line) => line
                    .attributes()
                    .read()
                    .get("subtype")
                    .map(|a| a.value().to_owned()),
                _ => None,
            };
            return subtype.ok_or_else(|| runtime("Traffic sign has no 'subtype' attribute!"));
        }
        regelem
            .attributes()
            .read()
            .get("sign_type")
            .map(|value| value.value().to_owned())
            .ok_or_else(|| runtime("Regulatory element has no traffic sign and no 'sign_type'!"))
    }

    #[pyo3(name = "addTrafficSign")]
    fn add_traffic_sign(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        push_role(slf.as_super(), roles::REFERS, value, "addTrafficSign")
    }

    #[pyo3(name = "removeTrafficSign")]
    fn remove_traffic_sign(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        drop_role(slf.as_super(), roles::REFERS, value, "removeTrafficSign")
    }

    #[pyo3(name = "addCancellingTrafficSign")]
    fn add_cancelling(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        push_role(
            slf.as_super(),
            roles::CANCELS,
            value,
            "addCancellingTrafficSign",
        )
    }

    #[pyo3(name = "removeCancellingTrafficSign")]
    fn remove_cancelling(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        drop_role(
            slf.as_super(),
            roles::CANCELS,
            value,
            "removeCancellingTrafficSign",
        )
    }

    #[pyo3(name = "addRefLine")]
    fn add_ref_line(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        push_role(slf.as_super(), roles::REF_LINE, value, "addRefLine")
    }

    #[pyo3(name = "removeRefLine")]
    fn remove_ref_line(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        drop_role(slf.as_super(), roles::REF_LINE, value, "removeRefLine")
    }

    #[pyo3(name = "addCancellingRefLine")]
    fn add_cancel_line(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        push_role(
            slf.as_super(),
            roles::CANCEL_LINE,
            value,
            "addCancellingRefLine",
        )
    }

    #[pyo3(name = "removeCancellingRefLine")]
    fn remove_cancel_line(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        drop_role(
            slf.as_super(),
            roles::CANCEL_LINE,
            value,
            "removeCancellingRefLine",
        )
    }

    /// The distinct `subtype`s of the cancelling signs, sorted.
    #[pyo3(name = "cancelTypes")]
    fn cancel_types(slf: PyRef<'_, Self>) -> Vec<String> {
        let mut types: Vec<String> = slf
            .as_super()
            .regelem
            .parameters_for(roles::CANCELS)
            .iter()
            .filter_map(|parameter| match parameter {
                RuleParameter::LineString(line) | RuleParameter::Polygon(line) => line
                    .attributes()
                    .read()
                    .get("subtype")
                    .map(|a| a.value().to_owned()),
                _ => None,
            })
            .collect();
        types.sort();
        types.dedup();
        types
    }
}

/// `lanelet2.core.SpeedLimit`, which derives from `TrafficSign` upstream too.
#[pyclass(name = "SpeedLimit", module = "lanelet2.core", extends = PyTrafficSign)]
pub struct PySpeedLimit;

/// The hybrid upstream's `SpeedLimit(...)` constructor produces.
///
/// Its Python class is `SpeedLimit`, but the C++ object it holds is a `TrafficSign`,
/// so `trafficSigns()` returns it — displayed as a `SpeedLimit` — while
/// `speedLimits()` does not. One `RegElemKind` cannot say both unless the two are
/// separate kinds sharing a class name.
fn compat_speed_limit_kind() -> &'static RegElemKind {
    static KIND: std::sync::OnceLock<RegElemKind> = std::sync::OnceLock::new();
    KIND.get_or_init(|| {
        ll2_core::regelem::registry::register(
            None,
            "SpeedLimit",
            Some(RegElemKind::TrafficSign),
            None,
        )
    })
}

#[pymethods]
impl PySpeedLimit {
    #[new]
    #[pyo3(signature = (id, attributes, trafficSigns, cancellingTrafficSigns = None,
                        refLines = None, cancelLines = None))]
    #[allow(non_snake_case)]
    fn new(
        id: Id,
        attributes: &Bound<'_, PyAny>,
        trafficSigns: &Bound<'_, PyAny>,
        cancellingTrafficSigns: Option<&Bound<'_, PyAny>>,
        refLines: Option<&Bound<'_, PyAny>>,
        cancelLines: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<PyClassInitializer<Self>> {
        let (parameters, _) =
            traffic_sign_parameters(trafficSigns, cancellingTrafficSigns, refLines, cancelLines)?;
        // Upstream binds SpeedLimit's constructor to TrafficSign::make, so what it
        // builds really *is* a TrafficSign: it is tagged `traffic_sign`, and because
        // the typed accessors are dynamic casts it turns up in `trafficSigns()` but
        // not in `speedLimits()`. The Python wrapper class is `SpeedLimit` either
        // way, which is why the reference's own `SpeedLimit.__repr__` rejects it.
        let (subtype, kind) = if compat::speed_limit_ctor_makes_traffic_sign() {
            ("traffic_sign", *compat_speed_limit_kind())
        } else {
            ("speed_limit", RegElemKind::SpeedLimit)
        };
        let attributes = typed_attributes(optional_attribute_map(Some(attributes))?, subtype);
        let base = PyRegulatoryElement::wrap(RegulatoryElement::new(
            kind, id, attributes, parameters,
        ));
        Ok(PyClassInitializer::from(base)
            .add_subclass(PyTrafficSign)
            .add_subclass(PySpeedLimit))
    }
}

/// `lanelet2.core.LaneletWithStopLine` — a lanelet paired with its stop line.
#[pyclass(name = "LaneletWithStopLine", module = "lanelet2.core")]
pub struct PyLaneletWithStopLine {
    pub(crate) lanelet: Lanelet,
    pub(crate) stop_line: Option<LineString>,
}

#[pymethods]
impl PyLaneletWithStopLine {
    #[new]
    #[pyo3(signature = (lanelet, stopLine = None))]
    #[allow(non_snake_case)]
    fn new(lanelet: &Bound<'_, PyAny>, stopLine: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let (lanelet, _) =
            lanelet_of(lanelet).ok_or_else(|| argument_error("LaneletWithStopLine", "__init__"))?;
        Ok(PyLaneletWithStopLine {
            lanelet,
            stop_line: optional_linestring(stopLine),
        })
    }

    #[getter]
    fn lanelet(&self) -> PyLanelet {
        PyLanelet::wrap(self.lanelet.clone())
    }

    #[setter(lanelet)]
    fn set_lanelet(&mut self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let (lanelet, _) =
            lanelet_of(value).ok_or_else(|| argument_error("LaneletWithStopLine", "lanelet"))?;
        self.lanelet = lanelet;
        Ok(())
    }

    #[getter]
    #[pyo3(name = "stopLine")]
    fn stop_line(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match &self.stop_line {
            None => Ok(py.None()),
            Some(line) => Ok(Py::new(py, PyLineString3d::wrap(line.clone()))?.into_any()),
        }
    }

    #[setter(stopLine)]
    fn set_stop_line(&mut self, value: &Bound<'_, PyAny>) {
        self.stop_line = optional_linestring(Some(value));
    }
}

/// `lanelet2.core.ConstLaneletWithStopLine`.
///
/// Despite the name, upstream's constructor takes a mutable `Lanelet` and its
/// `stopLine` setter builds a `LineString3d`; only the accessors are const.
#[pyclass(name = "ConstLaneletWithStopLine", module = "lanelet2.core")]
pub struct PyConstLaneletWithStopLine {
    lanelet: Lanelet,
    stop_line: Option<LineString>,
}

#[pymethods]
impl PyConstLaneletWithStopLine {
    #[new]
    #[pyo3(signature = (lanelet, stopLine = None))]
    #[allow(non_snake_case)]
    fn new(lanelet: &Bound<'_, PyAny>, stopLine: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let (lanelet, _) = lanelet_of(lanelet)
            .ok_or_else(|| argument_error("ConstLaneletWithStopLine", "__init__"))?;
        Ok(PyConstLaneletWithStopLine {
            lanelet,
            stop_line: optional_linestring(stopLine),
        })
    }

    #[getter]
    fn lanelet(&self) -> PyConstLanelet {
        PyConstLanelet::wrap(self.lanelet.clone())
    }

    #[setter(lanelet)]
    fn set_lanelet(&mut self, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let (lanelet, _) = lanelet_of(value)
            .ok_or_else(|| argument_error("ConstLaneletWithStopLine", "lanelet"))?;
        self.lanelet = lanelet;
        Ok(())
    }

    #[getter]
    #[pyo3(name = "stopLine")]
    fn stop_line(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match &self.stop_line {
            None => Ok(py.None()),
            Some(line) => Ok(Py::new(py, PyConstLineString3d::wrap(line.clone()))?.into_any()),
        }
    }

    #[setter(stopLine)]
    fn set_stop_line(&mut self, value: &Bound<'_, PyAny>) {
        self.stop_line = optional_linestring(Some(value));
    }
}

/// `lanelet2.core.AllWayStop`.
#[pyclass(name = "AllWayStop", module = "lanelet2.core", extends = PyRegulatoryElement)]
#[derive(Default)]
pub struct PyAllWayStop;

#[pymethods]
impl PyAllWayStop {
    #[new]
    #[pyo3(signature = (id, attributes, lltsWithStop, signs = None))]
    #[allow(non_snake_case)]
    fn new(
        id: Id,
        attributes: &Bound<'_, PyAny>,
        lltsWithStop: &Bound<'_, PyAny>,
        signs: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<(Self, PyRegulatoryElement)> {
        let mut lanelet_params = Vec::new();
        let mut stop_lines = Vec::new();
        for item in lltsWithStop.try_iter()? {
            let item = item?;
            let pair = item.cast::<PyLaneletWithStopLine>()?.borrow();
            lanelet_params.push(RuleParameter::Lanelet(pair.lanelet.downgrade()));
            // Only lanelets that actually have a stop line contribute one, which is
            // why stopLines() is not index-aligned with lanelets().
            if let Some(line) = &pair.stop_line {
                stop_lines.push(RuleParameter::LineString(line.clone()));
            }
        }
        if !stop_lines.is_empty() && stop_lines.len() != lanelet_params.len() {
            return Err(runtime(
                "Inconsistent number of lanelets and stop lines found! \
                 Either one stop line per lanelet or no stop lines!",
            ));
        }
        let signs = match signs {
            None => Vec::new(),
            Some(value) if value.is_none() => Vec::new(),
            Some(value) => linestrings_or_polygons(value, "AllWayStop")?,
        };

        let mut parameters = RuleParameterMap::new();
        parameters.insert(roles::YIELD.into(), lanelet_params);
        parameters.insert(roles::REF_LINE.into(), stop_lines);
        parameters.insert(roles::REFERS.into(), signs);
        let attributes =
            typed_attributes(optional_attribute_map(Some(attributes))?, "all_way_stop");
        Ok((
            PyAllWayStop,
            PyRegulatoryElement::wrap(RegulatoryElement::new(
                RegElemKind::AllWayStop,
                id,
                attributes,
                parameters,
            )),
        ))
    }

    fn lanelets(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        role_as_py(py, &slf.as_super().regelem, roles::YIELD)
    }

    /// Not index-aligned with `lanelets()`: only lanelets that have a stop line
    /// contribute an entry.
    #[pyo3(name = "stopLines")]
    fn stop_lines(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        role_as_py(py, &slf.as_super().regelem, roles::REF_LINE)
    }

    #[pyo3(name = "trafficSigns")]
    fn traffic_signs(slf: PyRef<'_, Self>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        role_as_py(py, &slf.as_super().regelem, roles::REFERS)
    }

    #[pyo3(name = "addTrafficSign")]
    fn add_traffic_sign(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let parameter = rule_parameter_from_any(value)
            .ok_or_else(|| argument_error("AllWayStop", "addTrafficSign"))?;
        slf.as_super()
            .regelem
            .push_parameter(roles::REFERS, parameter);
        Ok(())
    }

    #[pyo3(name = "removeTrafficSign")]
    fn remove_traffic_sign(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        let parameter = rule_parameter_from_any(value)
            .ok_or_else(|| argument_error("AllWayStop", "removeTrafficSign"))?;
        Ok(slf
            .as_super()
            .regelem
            .remove_parameter(roles::REFERS, &parameter))
    }

    #[pyo3(name = "addLanelet")]
    fn add_lanelet(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<()> {
        let pair = value.cast::<PyLaneletWithStopLine>()?.borrow();
        let regelem = &slf.as_super().regelem;
        regelem.push_parameter(
            roles::YIELD,
            RuleParameter::Lanelet(pair.lanelet.downgrade()),
        );
        if let Some(line) = &pair.stop_line {
            regelem.push_parameter(roles::REF_LINE, RuleParameter::LineString(line.clone()));
        }
        Ok(())
    }

    #[pyo3(name = "removeLanelet")]
    fn remove_lanelet(slf: PyRef<'_, Self>, value: &Bound<'_, PyAny>) -> PyResult<bool> {
        let (lanelet, _) =
            lanelet_of(value).ok_or_else(|| argument_error("AllWayStop", "removeLanelet"))?;
        Ok(slf
            .as_super()
            .regelem
            .remove_parameter(roles::YIELD, &RuleParameter::Lanelet(lanelet.downgrade())))
    }
}

/// Builds the Python object for one kind, given its base.
///
/// Each arm needs a differently shaped `PyClassInitializer`, so instead of one match
/// that every new class has to be threaded through, each class contributes its own
/// constructor to a table indexed by kind. An extension registering a kind at import
/// time drops its entry in here at the same moment.
pub type WrapFn = fn(Python<'_>, RegulatoryElement) -> PyResult<Py<PyAny>>;

static WRAPPERS: std::sync::RwLock<Vec<Option<WrapFn>>> = std::sync::RwLock::new(Vec::new());

pub fn set_wrapper(kind: RegElemKind, wrap: WrapFn) {
    let mut wrappers = WRAPPERS.write().expect("wrapper table poisoned");
    if wrappers.len() <= kind.index() {
        wrappers.resize(kind.index() + 1, None);
    }
    wrappers[kind.index()] = Some(wrap);
}

/// Wraps an element in whatever Python class its kind reports.
///
/// A kind with no entry — the generic one, or an extension kind that registered a
/// rule name but no class — surfaces as the plain base.
pub fn wrap_typed(py: Python<'_>, regelem: RegulatoryElement) -> PyResult<Py<PyAny>> {
    let wrap = WRAPPERS.read().expect("wrapper table poisoned").get(regelem.kind().index()).copied().flatten();
    match wrap {
        Some(wrap) => wrap(py, regelem),
        None => Ok(Py::new(py, PyRegulatoryElement::wrap(regelem))?.into_any()),
    }
}

fn wrap_plain<T: Default + Send + pyo3::PyClass<BaseType = PyRegulatoryElement>>(
    py: Python<'_>,
    regelem: RegulatoryElement,
) -> PyResult<Py<PyAny>>
where
    PyClassInitializer<T>: From<(T, PyRegulatoryElement)>,
{
    Ok(Py::new(py, (T::default(), PyRegulatoryElement::wrap(regelem)))?.into_any())
}

fn wrap_speed_limit(py: Python<'_>, regelem: RegulatoryElement) -> PyResult<Py<PyAny>> {
    Ok(Py::new(
        py,
        PyClassInitializer::from(PyRegulatoryElement::wrap(regelem))
            .add_subclass(PyTrafficSign)
            .add_subclass(PySpeedLimit),
    )?
    .into_any())
}

pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    set_wrapper(RegElemKind::TrafficLight, wrap_plain::<PyTrafficLight>);
    set_wrapper(RegElemKind::RightOfWay, wrap_plain::<PyRightOfWay>);
    set_wrapper(RegElemKind::TrafficSign, wrap_plain::<PyTrafficSign>);
    set_wrapper(RegElemKind::AllWayStop, wrap_plain::<PyAllWayStop>);
    set_wrapper(RegElemKind::SpeedLimit, wrap_speed_limit);
    // What upstream's `SpeedLimit(...)` really builds in bug-compat mode: a Python
    // `SpeedLimit` that a cast to `SpeedLimit` nevertheless rejects. No subtype
    // resolves to it, so it is registered without a rule name.
    set_wrapper(*compat_speed_limit_kind(), wrap_speed_limit);

    m.add_class::<ReprWrapper>()?;
    m.add_class::<PyRuleParameterMap>()?;
    m.add_class::<PyConstRuleParameterMap>()?;
    m.add_class::<PyRegulatoryElement>()?;
    m.add_class::<PyTrafficLight>()?;
    // ManeuverType is installed by the Python shim; flag that it is wanted.
    m.add("_needs_ManeuverType", true)?;
    m.add_class::<PyRightOfWay>()?;
    m.add_class::<PyTrafficSignsWithType>()?;
    m.add_class::<PyTrafficSign>()?;
    m.add_class::<PySpeedLimit>()?;
    m.add_class::<PyLaneletWithStopLine>()?;
    m.add_class::<PyConstLaneletWithStopLine>()?;
    m.add_class::<PyRuleParameterMapEntry>()?;
    m.add_class::<PyConstRuleParameterMapEntry>()?;
    m.add_class::<PyAllWayStop>()?;
    Ok(())
}

// Referenced by the generated conversions for polygon-valued rule parameters.
const _: Option<Kind> = None;
