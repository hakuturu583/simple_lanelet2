//! The `ll2-viz` visualiser, on the other side of a WebAssembly boundary.
//!
//! The interesting decision here is what crosses that boundary. A city-scale
//! Lanelet2 map is a few hundred thousand shapes; handing them over as JavaScript
//! objects — even through `serde` — costs more than parsing the file did, and
//! leaves the renderer walking a linked structure once per frame.
//!
//! So a scene crosses as **flat typed arrays** instead:
//!
//! ```text
//! coords    Float32Array   x, y, x, y, ...  every shape's vertices, end to end
//! offsets   Uint32Array    where each shape starts in `coords`, plus a final end
//! styles    Uint32Array    one index into the style table per shape
//! layers    Uint32Array    one layer index per shape
//! closed    Uint8Array     whether each shape is a polygon
//! ids       Float64Array   the source primitive's id per shape
//! ```
//!
//! which is four copies and no allocation per shape, and lets the canvas renderer
//! build one `Path2D` per style and then do nothing per frame but set a transform.
//!
//! Coordinates are `f32` *relative to the scene's centre*, which is handed over
//! separately as `f64`. That matters: MGRS-local coordinates run to 100 000 metres,
//! where an `f32` resolves about 8 mm — visible. Recentred, the exponent tracks the
//! map's own radius and a kilometre-wide map resolves to well under a millimetre.

use ll2_viz::style::VizLayer;
use ll2_viz::{
    CoordinateSource, LoadOptions, LoadedMap, Scene, SvgOptions, Theme, VizOptions, load_osm_str,
    render_svg,
};
use wasm_bindgen::prelude::*;

/// Installs the panic hook, so a Rust panic reaches the browser console as a stack
/// trace rather than as `unreachable executed`. Safe to call more than once.
#[wasm_bindgen(start)]
pub fn start() {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();
}

/// The version of `simple_lanelet2` this module was built from.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// What to draw. Construct with `new SceneOptions()` and assign to the fields.
#[wasm_bindgen(getter_with_clone)]
#[derive(Clone, Debug)]
pub struct SceneOptions {
    /// `"dark"` or `"light"`; anything else falls back to `"dark"`.
    pub theme: String,
    pub lanelet_fill: bool,
    pub areas: bool,
    pub polygons: bool,
    pub bounds: bool,
    pub regulatory: bool,
    pub centerlines: bool,
    pub direction_arrows: bool,
    pub points: bool,
    /// Metres between driving-direction arrowheads.
    pub arrow_spacing: f64,
}

impl Default for SceneOptions {
    fn default() -> Self {
        let defaults = VizOptions::default();
        SceneOptions {
            theme: defaults.theme.key().to_owned(),
            lanelet_fill: defaults.lanelet_fill,
            areas: defaults.areas,
            polygons: defaults.polygons,
            bounds: defaults.bounds,
            regulatory: defaults.regulatory,
            centerlines: defaults.centerlines,
            direction_arrows: defaults.direction_arrows,
            points: defaults.points,
            arrow_spacing: defaults.arrow_spacing,
        }
    }
}

#[wasm_bindgen]
impl SceneOptions {
    #[wasm_bindgen(constructor)]
    pub fn new() -> SceneOptions {
        SceneOptions::default()
    }
}

impl SceneOptions {
    fn to_viz(&self) -> VizOptions {
        VizOptions {
            theme: Theme::from_key(&self.theme).unwrap_or_default(),
            lanelet_fill: self.lanelet_fill,
            areas: self.areas,
            polygons: self.polygons,
            bounds: self.bounds,
            regulatory: self.regulatory,
            centerlines: self.centerlines,
            direction_arrows: self.direction_arrows,
            points: self.points,
            arrow_spacing: self.arrow_spacing,
        }
    }
}

/// A parsed map. Cheap to keep around; build as many scenes from it as you like.
#[wasm_bindgen]
pub struct LaneletMapHandle {
    loaded: LoadedMap,
}

impl LaneletMapHandle {
    /// The whole of `load_osm`, minus the `JsError`.
    ///
    /// Split out because `JsError::new` traps on a non-wasm target, which would put
    /// every failure path beyond the reach of `cargo test` — and the failure paths
    /// are the ones a drag-and-drop viewer hits, since users drop the wrong file.
    pub fn parse(text: &str, coordinates: &str) -> Result<LaneletMapHandle, String> {
        let options = LoadOptions {
            coordinates: CoordinateSource::from_key(coordinates).unwrap_or_default(),
            origin: None,
        };
        Ok(LaneletMapHandle {
            loaded: load_osm_str(text, &options)?,
        })
    }
}

/// Parses OSM XML into a map.
///
/// `coordinates` is `"auto"`, `"projected"` or `"local"`. `"auto"` projects lat/lon
/// unless doing so collapses the map to a point while the file's `local_x`/`local_y`
/// tags do not — which is how Autoware writes maps whose lat/lon is a placeholder.
#[wasm_bindgen]
pub fn load_osm(text: &str, coordinates: &str) -> Result<LaneletMapHandle, JsError> {
    LaneletMapHandle::parse(text, coordinates).map_err(|error| JsError::new(&error))
}

#[wasm_bindgen]
impl LaneletMapHandle {
    /// Non-fatal parse problems, in the shape `lanelet2.io.loadRobust` reports them.
    pub fn errors(&self) -> Vec<String> {
        self.loaded.errors.clone()
    }

    /// Which coordinate source was actually used: `"projected"` or `"local_xy"`.
    pub fn coordinate_source(&self) -> String {
        self.loaded.coordinates.key().to_owned()
    }

    /// `"utm"`, or `"local_cartesian"` when the origin was too near a pole for UTM.
    pub fn projection(&self) -> String {
        self.loaded.projection.to_owned()
    }

    pub fn origin_lat(&self) -> f64 {
        self.loaded.origin.lat
    }

    pub fn origin_lon(&self) -> f64 {
        self.loaded.origin.lon
    }

    /// Primitive counts, as `{"points":…,"lineStrings":…,…}`.
    pub fn stats_json(&self) -> String {
        let stats = ll2_viz::MapStats::of(&self.loaded.map);
        format!(
            "{{\"points\":{},\"lineStrings\":{},\"polygons\":{},\"lanelets\":{},\
             \"areas\":{},\"regulatoryElements\":{}}}",
            stats.points,
            stats.line_strings,
            stats.polygons,
            stats.lanelets,
            stats.areas,
            stats.regulatory_elements
        )
    }

    /// Builds a drawable scene.
    pub fn build_scene(&self, options: &SceneOptions) -> SceneData {
        SceneData::build(Scene::from_map(&self.loaded.map, &options.to_viz()))
    }

    /// Renders the map as a standalone SVG document.
    pub fn to_svg(&self, options: &SceneOptions, width: f64, height: f64) -> String {
        let scene = Scene::from_map(&self.loaded.map, &options.to_viz());
        let page = SvgOptions {
            width,
            height,
            ..SvgOptions::default()
        };
        render_svg(&scene, &page)
    }
}

/// A scene, flattened for transfer.
///
/// The array accessors copy into fresh JavaScript memory on each call, because a
/// view into the wasm heap is invalidated the moment the module grows its memory.
/// Call each one once and keep the result.
#[wasm_bindgen]
pub struct SceneData {
    coords: Vec<f32>,
    offsets: Vec<u32>,
    styles: Vec<u32>,
    layers: Vec<u32>,
    closed: Vec<u8>,
    ids: Vec<f64>,
    labels: Vec<String>,
    centre: [f64; 2],
    min: [f64; 2],
    max: [f64; 2],
    styles_json: String,
    layers_json: String,
    background: String,
}

impl SceneData {
    fn build(scene: Scene) -> SceneData {
        let bounds = scene.safe_bounds();
        let centre = [
            (bounds.min[0] + bounds.max[0]) * 0.5,
            (bounds.min[1] + bounds.max[1]) * 0.5,
        ];

        let total: usize = scene.shapes.iter().map(|shape| shape.points.len()).sum();
        let mut coords = Vec::with_capacity(total * 2);
        let mut offsets = Vec::with_capacity(scene.shapes.len() + 1);
        let mut styles = Vec::with_capacity(scene.shapes.len());
        let mut layers = Vec::with_capacity(scene.shapes.len());
        let mut closed = Vec::with_capacity(scene.shapes.len());
        let mut ids = Vec::with_capacity(scene.shapes.len());
        let mut labels = Vec::with_capacity(scene.shapes.len());

        for shape in &scene.shapes {
            // In vertices, not floats: the renderer indexes points, and halving the
            // number here is one fewer place to get the factor of two wrong.
            offsets.push((coords.len() / 2) as u32);
            for [x, y] in &shape.points {
                coords.push((x - centre[0]) as f32);
                coords.push((y - centre[1]) as f32);
            }
            styles.push(shape.style as u32);
            layers.push(shape.layer.index());
            closed.push(u8::from(shape.closed));
            ids.push(shape.id as f64);
            labels.push(shape.label.clone());
        }
        offsets.push((coords.len() / 2) as u32);

        SceneData {
            coords,
            offsets,
            styles,
            layers,
            closed,
            ids,
            labels,
            centre,
            min: bounds.min,
            max: bounds.max,
            styles_json: styles_json(&scene),
            layers_json: layers_json(),
            background: scene.theme.palette().background.to_hex(),
        }
    }
}

#[wasm_bindgen]
impl SceneData {
    /// Vertices as `x, y, x, y, …`, relative to [`SceneData::centre_x`]/`centre_y`.
    pub fn coords(&self) -> Vec<f32> {
        self.coords.clone()
    }

    /// Vertex index where each shape starts, with a final entry marking the end —
    /// so shape `i` spans `offsets[i]..offsets[i + 1]`.
    pub fn offsets(&self) -> Vec<u32> {
        self.offsets.clone()
    }

    /// One index into the style table per shape.
    pub fn styles(&self) -> Vec<u32> {
        self.styles.clone()
    }

    /// One layer index per shape; see [`SceneData::layers_json`].
    pub fn layers(&self) -> Vec<u32> {
        self.layers.clone()
    }

    /// `1` where the shape is a closed polygon.
    pub fn closed(&self) -> Vec<u8> {
        self.closed.clone()
    }

    /// The id of the primitive each shape came from.
    pub fn ids(&self) -> Vec<f64> {
        self.ids.clone()
    }

    pub fn shape_count(&self) -> usize {
        self.styles.len()
    }

    /// A one-line description of a shape, for a tooltip. Empty when out of range.
    pub fn label(&self, index: usize) -> String {
        self.labels.get(index).cloned().unwrap_or_default()
    }

    pub fn centre_x(&self) -> f64 {
        self.centre[0]
    }

    pub fn centre_y(&self) -> f64 {
        self.centre[1]
    }

    /// The map's extent in map coordinates, as `[minX, minY, maxX, maxY]`.
    pub fn bounds(&self) -> Vec<f64> {
        vec![self.min[0], self.min[1], self.max[0], self.max[1]]
    }

    /// The style table: `[{"name":…,"label":…,"stroke":…,"fill":…,…}, …]`, indexed
    /// by the values in [`SceneData::styles`].
    pub fn styles_json(&self) -> String {
        self.styles_json.clone()
    }

    /// The layer table: `[{"key":…,"label":…}, …]`, indexed by [`SceneData::layers`].
    pub fn layers_json(&self) -> String {
        self.layers_json.clone()
    }

    /// The theme's page colour, as `#rrggbb`.
    pub fn background(&self) -> String {
        self.background.clone()
    }
}

fn styles_json(scene: &Scene) -> String {
    let mut out = String::from("[");
    for (index, style) in scene.styles.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":\"");
        push_escaped(&mut out, &style.name);
        out.push_str("\",\"label\":\"");
        push_escaped(&mut out, &style.label);
        out.push_str("\",\"z\":");
        out.push_str(&style.z.to_string());
        out.push_str(",\"inLegend\":");
        out.push_str(if style.in_legend { "true" } else { "false" });

        match style.stroke {
            Some(color) => out.push_str(&format!(
                ",\"stroke\":\"{}\",\"strokeOpacity\":{},\"strokeWidth\":{}",
                color.to_hex(),
                style.stroke_opacity,
                style.stroke_width
            )),
            None => out.push_str(",\"stroke\":null"),
        }
        match &style.dash {
            Some(dash) => {
                let pattern: Vec<String> = dash.iter().map(|value| value.to_string()).collect();
                out.push_str(&format!(",\"dash\":[{}]", pattern.join(",")));
            }
            None => out.push_str(",\"dash\":null"),
        }
        match style.fill {
            Some(color) => out.push_str(&format!(
                ",\"fill\":\"{}\",\"fillOpacity\":{}",
                color.to_hex(),
                style.fill_opacity
            )),
            None => out.push_str(",\"fill\":null"),
        }
        out.push('}');
    }
    out.push(']');
    out
}

fn layers_json() -> String {
    let mut out = String::from("[");
    for (index, layer) in VizLayer::ALL.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str(&format!(
            "{{\"key\":\"{}\",\"label\":\"{}\"}}",
            layer.key(),
            layer.label()
        ));
    }
    out.push(']');
    out
}

/// The five escapes JSON strings need. Style and layer names come from a fixed
/// vocabulary, but labels carry map tags, and a map tag is whatever someone typed.
fn push_escaped(out: &mut String, text: &str) {
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<osm version='0.6'>
  <node id='1' lat='49.0000000' lon='8.4000000'/>
  <node id='2' lat='49.0000000' lon='8.4010000'/>
  <node id='3' lat='49.0001000' lon='8.4000000'/>
  <node id='4' lat='49.0001000' lon='8.4010000'/>
  <way id='10'><nd ref='1'/><nd ref='2'/><tag k='type' v='line_thin'/><tag k='subtype' v='solid'/></way>
  <way id='11'><nd ref='3'/><nd ref='4'/><tag k='type' v='line_thin'/><tag k='subtype' v='dashed'/></way>
  <relation id='20'>
    <member type='way' ref='11' role='left'/>
    <member type='way' ref='10' role='right'/>
    <tag k='type' v='lanelet'/><tag k='subtype' v='road'/>
  </relation>
</osm>"#;

    fn scene() -> SceneData {
        LaneletMapHandle::parse(MAP, "auto")
            .unwrap()
            .build_scene(&SceneOptions::new())
    }

    #[test]
    fn offsets_partition_the_coordinate_buffer_exactly() {
        let data = scene();
        let offsets = data.offsets();
        assert_eq!(offsets.len(), data.shape_count() + 1);
        assert_eq!(offsets[0], 0);
        assert_eq!(*offsets.last().unwrap() as usize * 2, data.coords().len());
        // Monotonic, and every shape has at least two vertices.
        for pair in offsets.windows(2) {
            assert!(pair[1] >= pair[0] + 2, "{pair:?}");
        }
    }

    #[test]
    fn the_per_shape_arrays_are_all_the_same_length() {
        let data = scene();
        let count = data.shape_count();
        assert!(count > 0);
        assert_eq!(data.styles().len(), count);
        assert_eq!(data.layers().len(), count);
        assert_eq!(data.closed().len(), count);
        assert_eq!(data.ids().len(), count);
    }

    #[test]
    fn coordinates_are_recentred_and_the_centre_puts_them_back() {
        let data = scene();
        let coords = data.coords();
        let bounds = data.bounds();
        let restored_x = coords[0] as f64 + data.centre_x();
        assert!(restored_x >= bounds[0] - 1e-3 && restored_x <= bounds[2] + 1e-3);
        // Recentring is the point: nothing should be far from zero.
        let extreme = coords.iter().fold(0.0f32, |acc, v| acc.max(v.abs()));
        assert!(extreme < 200.0, "{extreme}");
    }

    #[test]
    fn style_indices_all_resolve_and_the_table_is_valid_json() {
        let data = scene();
        let json = data.styles_json();
        assert!(json.starts_with('[') && json.ends_with(']'));
        let entries = json.matches("{\"name\":").count();
        assert!(entries > 0);
        for index in data.styles() {
            assert!((index as usize) < entries, "style {index} of {entries}");
        }
        assert!(json.contains("\"dash\":[4,4]"), "{json}");
    }

    #[test]
    fn layer_indices_all_resolve() {
        let data = scene();
        let count = data.layers_json().matches("{\"key\":").count();
        assert_eq!(count, VizLayer::ALL.len());
        for index in data.layers() {
            assert!((index as usize) < count);
        }
    }

    #[test]
    fn labels_are_addressable_and_out_of_range_is_empty_not_a_panic() {
        let data = scene();
        assert!(data.label(0).contains(' '));
        assert_eq!(data.label(data.shape_count()), "");
        assert_eq!(data.label(usize::MAX), "");
    }

    #[test]
    fn turning_a_layer_off_removes_its_shapes() {
        let handle = LaneletMapHandle::parse(MAP, "auto").unwrap();
        let mut options = SceneOptions::new();
        options.direction_arrows = false;
        options.bounds = false;
        let data = handle.build_scene(&options);
        let bound = VizLayer::Bound.index();
        let direction = VizLayer::Direction.index();
        assert!(!data.layers().iter().any(|l| *l == bound || *l == direction));
        assert!(data.shape_count() > 0);
    }

    #[test]
    fn the_handle_reports_what_it_decided() {
        let handle = LaneletMapHandle::parse(MAP, "auto").unwrap();
        assert_eq!(handle.coordinate_source(), "projected");
        assert_eq!(handle.projection(), "utm");
        assert!(handle.errors().is_empty());
        assert_eq!(
            handle.stats_json(),
            "{\"points\":4,\"lineStrings\":2,\"polygons\":0,\"lanelets\":1,\
             \"areas\":0,\"regulatoryElements\":0}"
        );
    }

    #[test]
    fn svg_export_goes_through_the_same_scene() {
        let handle = LaneletMapHandle::parse(MAP, "auto").unwrap();
        let svg = handle.to_svg(&SceneOptions::new(), 800.0, 600.0);
        assert!(svg.contains("width=\"800\""));
        assert!(svg.contains("data-layer=\"lanelet_fill\""));
    }

    #[test]
    fn a_bad_file_returns_an_error_rather_than_panicking() {
        assert!(LaneletMapHandle::parse("definitely not xml", "auto").is_err());
        assert!(LaneletMapHandle::parse("", "auto").is_err());
    }

    #[test]
    fn labels_with_json_metacharacters_survive_the_table() {
        let mut out = String::new();
        push_escaped(&mut out, "a\"b\\c\nd\te");
        assert_eq!(out, "a\\\"b\\\\c\\nd\\te");
        let mut control = String::new();
        push_escaped(&mut control, "\u{1}");
        assert_eq!(control, "\\u0001");
    }
}
