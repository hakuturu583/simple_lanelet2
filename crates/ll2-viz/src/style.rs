//! What a primitive should look like, decided from its tags.
//!
//! Lanelet2 carries no styling of its own — a map says `type=line_thin`,
//! `subtype=dashed` and leaves the rest to the viewer. Everything here is that
//! translation, kept in one place so a renderer never has to read a tag.
//!
//! The tag vocabulary is the union of the Lanelet2 primitive tagging document and
//! what Autoware's `lanelet2_extension` adds on top (`stop_line`, `road_border`,
//! `pedestrian_marking`, `road_shoulder`, the `crosswalk_polygon` areas). Anything
//! unrecognised still renders, in a neutral colour, rather than disappearing.

/// A traffic lane, in metres. Only ever used to turn a zoom into a statement
/// about how much of a map a viewer can actually make out.
pub const LANE_WIDTH_M: f32 = 3.5;

/// An sRGB colour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Color(pub u8, pub u8, pub u8);

impl Color {
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

/// Which toggleable group a shape belongs to.
///
/// The demo turns these on and off, and the SVG writer emits one `<g>` per layer,
/// so the discriminants are part of the wire format and must stay stable. `repr`
/// makes that literally true: the number a renderer indexes by is the declaration
/// order, rather than a second list that has to be kept in step with it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum VizLayer {
    LaneletFill,
    Area,
    Polygon,
    Bound,
    Regulatory,
    Centerline,
    Direction,
    Point,
}

impl VizLayer {
    pub const ALL: [VizLayer; 8] = [
        VizLayer::LaneletFill,
        VizLayer::Area,
        VizLayer::Polygon,
        VizLayer::Bound,
        VizLayer::Regulatory,
        VizLayer::Centerline,
        VizLayer::Direction,
        VizLayer::Point,
    ];

    pub fn index(self) -> u32 {
        self as u32
    }

    /// The identifier a caller toggles by, also used as the SVG group id.
    pub fn key(self) -> &'static str {
        match self {
            VizLayer::LaneletFill => "lanelet_fill",
            VizLayer::Area => "area",
            VizLayer::Polygon => "polygon",
            VizLayer::Bound => "bound",
            VizLayer::Regulatory => "regulatory",
            VizLayer::Centerline => "centerline",
            VizLayer::Direction => "direction",
            VizLayer::Point => "point",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            VizLayer::LaneletFill => "Lanelets",
            VizLayer::Area => "Areas",
            VizLayer::Polygon => "Polygons",
            VizLayer::Bound => "Boundaries",
            VizLayer::Regulatory => "Regulatory elements",
            VizLayer::Centerline => "Centerlines",
            VizLayer::Direction => "Driving direction",
            VizLayer::Point => "Points",
        }
    }
}

/// How one shape is painted.
///
/// Widths and dash lengths are in *screen pixels*, not metres: a lane marking that
/// is 12 cm wide is invisible at city scale and a hairline is what every map viewer
/// actually draws. Fills are in world coordinates and so scale with the zoom, which
/// is what you want for the road surface itself.
#[derive(Clone, PartialEq, Debug)]
pub struct Style {
    /// Stable identifier, unique within a table.
    pub name: String,
    /// Shown in the legend. Always a literal, so a scene with a million shapes
    /// does not allocate a million copies of "Lane marking".
    pub label: &'static str,
    pub stroke: Option<Color>,
    pub stroke_opacity: f32,
    pub stroke_width: f32,
    pub dash: Option<Vec<f32>>,
    pub fill: Option<Color>,
    pub fill_opacity: f32,
    /// Painter's order; lower is drawn first.
    pub z: i32,
    /// Whether the legend should list it.
    pub in_legend: bool,
    /// Pixels per metre below which this style is not worth drawing at all.
    /// Zero means always.
    pub hide_below_scale: f32,
    /// Pixels per metre below which the dash pattern is dropped for a plain
    /// stroke. Zero means never.
    pub solid_below_scale: f32,
}

impl Style {
    fn new(name: &str, label: &'static str, z: i32) -> Self {
        Style {
            name: name.to_owned(),
            label,
            stroke: None,
            stroke_opacity: 1.0,
            stroke_width: 1.0,
            dash: None,
            fill: None,
            fill_opacity: 1.0,
            z,
            in_legend: true,
            hide_below_scale: 0.0,
            solid_below_scale: 0.0,
        }
    }

    /// Below `pixels` on-screen pixels per `LANE_WIDTH_M`, stop drawing this.
    fn hidden_below(mut self, pixels: f32) -> Self {
        self.hide_below_scale = pixels / LANE_WIDTH_M;
        self
    }

    fn stroke(mut self, color: Color, width: f32) -> Self {
        self.stroke = Some(color);
        self.stroke_width = width;
        self
    }

    fn stroke_opacity(mut self, opacity: f32) -> Self {
        self.stroke_opacity = opacity;
        self
    }

    /// A width with no stroke colour.
    ///
    /// Both 2D renderers emit a width only alongside a colour, so this is invisible
    /// to them. It exists for a shape that is a filled triangle on a page and a line
    /// with thickness in a 3D viewer — so that viewer reads its dimension from this
    /// table like every other, instead of inventing a constant of its own.
    fn stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = width;
        self
    }

    /// A dash pattern, and the zoom below which it stops being worth drawing.
    ///
    /// Dashing a city's worth of lane markings costs more than every fill and
    /// solid stroke in the map put together, and at a zoom where a whole lane is
    /// five pixels across it buys a grey smear over lines that already overlap.
    /// Which is a fact about *this dash pattern*, so it is recorded next to it
    /// rather than left for each renderer to invent — the reason the canvas and
    /// the SVG export agree about what a map looks like.
    fn dashed(mut self, on: f32, off: f32) -> Self {
        self.dash = Some(vec![on, off]);
        self.solid_below_scale = 5.0 / LANE_WIDTH_M;
        self
    }

    fn fill(mut self, color: Color, opacity: f32) -> Self {
        self.fill = Some(color);
        self.fill_opacity = opacity;
        self
    }

    fn hidden_from_legend(mut self) -> Self {
        self.in_legend = false;
        self
    }
}

/// Every colour a theme decides, named by role rather than by hue so the two
/// themes stay in step.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub background: Color,
    pub road: Color,
    pub shoulder: Color,
    pub crosswalk: Color,
    pub walkway: Color,
    pub bicycle: Color,
    pub bus_lane: Color,
    pub lanelet_other: Color,
    pub marking: Color,
    pub curbstone: Color,
    pub road_border: Color,
    pub barrier: Color,
    pub stop_line: Color,
    pub traffic_light: Color,
    pub traffic_sign: Color,
    pub virtual_line: Color,
    pub centerline: Color,
    pub direction: Color,
    pub parking: Color,
    pub keepout: Color,
    pub area_other: Color,
    pub point: Color,
    pub text: Color,
    pub highlight: Color,
}

/// Which of the two built-in palettes to use.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

impl Theme {
    pub fn from_key(key: &str) -> Option<Theme> {
        match key {
            "dark" => Some(Theme::Dark),
            "light" => Some(Theme::Light),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
        }
    }

    pub fn palette(self) -> Palette {
        match self {
            Theme::Dark => Palette {
                background: Color(0x11, 0x14, 0x1a),
                road: Color(0x3a, 0x43, 0x55),
                shoulder: Color(0x2e, 0x35, 0x44),
                crosswalk: Color(0x2f, 0x86, 0x8f),
                walkway: Color(0x3d, 0x74, 0x4a),
                bicycle: Color(0x7a, 0x5c, 0x2e),
                bus_lane: Color(0x63, 0x3f, 0x6e),
                lanelet_other: Color(0x44, 0x44, 0x4c),
                marking: Color(0xe8, 0xec, 0xf2),
                curbstone: Color(0x9a, 0xa4, 0xb4),
                road_border: Color(0xc2, 0xc9, 0xd4),
                barrier: Color(0x8a, 0x93, 0xa3),
                stop_line: Color(0xff, 0x4d, 0x4d),
                traffic_light: Color(0xff, 0xc4, 0x3d),
                traffic_sign: Color(0xff, 0x8c, 0x42),
                virtual_line: Color(0x6a, 0x74, 0x88),
                centerline: Color(0x4d, 0xd0, 0xe1),
                direction: Color(0x7f, 0xe3, 0xf0),
                parking: Color(0x35, 0x53, 0x7a),
                keepout: Color(0x7a, 0x35, 0x40),
                area_other: Color(0x3b, 0x42, 0x4e),
                point: Color(0x8e, 0x99, 0xab),
                text: Color(0xe6, 0xea, 0xf0),
                highlight: Color(0xff, 0xd7, 0x4a),
            },
            Theme::Light => Palette {
                background: Color(0xf7, 0xf8, 0xfa),
                road: Color(0xd6, 0xdb, 0xe3),
                shoulder: Color(0xe3, 0xe7, 0xed),
                crosswalk: Color(0x8f, 0xd4, 0xdb),
                walkway: Color(0xa8, 0xd8, 0xb4),
                bicycle: Color(0xe6, 0xcf, 0x9c),
                bus_lane: Color(0xd8, 0xbc, 0xe2),
                lanelet_other: Color(0xdd, 0xdd, 0xe2),
                marking: Color(0x33, 0x38, 0x42),
                curbstone: Color(0x6b, 0x74, 0x84),
                road_border: Color(0x2f, 0x35, 0x40),
                barrier: Color(0x5a, 0x63, 0x73),
                stop_line: Color(0xd4, 0x2f, 0x2f),
                traffic_light: Color(0xc9, 0x8a, 0x00),
                traffic_sign: Color(0xd4, 0x66, 0x14),
                virtual_line: Color(0x9a, 0xa2, 0xb0),
                centerline: Color(0x0b, 0x84, 0x99),
                direction: Color(0x0b, 0x6d, 0x80),
                parking: Color(0xbe, 0xcf, 0xe8),
                keepout: Color(0xe8, 0xbe, 0xc4),
                area_other: Color(0xe2, 0xe6, 0xec),
                point: Color(0x7a, 0x83, 0x93),
                text: Color(0x1b, 0x1f, 0x27),
                highlight: Color(0xd9, 0x8b, 0x00),
            },
        }
    }
}

/// Interns styles so a scene carries each one once and shapes carry an index.
///
/// A city-scale map has hundreds of thousands of shapes and perhaps thirty distinct
/// looks; storing the look per shape would dominate the transfer to a renderer.
#[derive(Default)]
pub struct StyleTable {
    styles: Vec<Style>,
    /// Name to index. A linear scan would be ~30 string comparisons per shape,
    /// which on a city map is tens of millions of them for a table that settles
    /// at a few dozen entries within the first hundred shapes.
    index: std::collections::HashMap<String, usize>,
}

impl StyleTable {
    pub fn new() -> Self {
        StyleTable::default()
    }

    /// Returns the index of `style`, adding it if it is new.
    pub fn intern(&mut self, style: Style) -> usize {
        if let Some(index) = self.index.get(&style.name) {
            return *index;
        }
        let index = self.styles.len();
        self.index.insert(style.name.clone(), index);
        self.styles.push(style);
        index
    }

    /// The index of an already-interned style, without building one to ask with.
    ///
    /// What lets a caller with many shapes of one kind — every arrowhead in the
    /// map — pay for the `Style` once and then look it up by name.
    pub fn lookup(&self, name: &str) -> Option<usize> {
        self.index.get(name).copied()
    }

    pub fn get(&self, index: usize) -> Option<&Style> {
        self.styles.get(index)
    }

    pub fn len(&self) -> usize {
        self.styles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Style> {
        self.styles.iter()
    }
}

// --- z bands -----------------------------------------------------------------
// Kept as named constants because the *relative* order is the whole point: fills
// under polygons under boundaries under regulatory lines under annotations.
const Z_LANELET: i32 = 10;
const Z_AREA: i32 = 20;
const Z_POLYGON: i32 = 30;
const Z_BOUND: i32 = 40;
const Z_REGULATORY: i32 = 60;
const Z_CENTERLINE: i32 = 70;
const Z_DIRECTION: i32 = 80;
const Z_POINT: i32 = 90;

/// The fill for a lanelet, chosen by its `subtype`.
pub fn lanelet_style(subtype: &str, palette: &Palette) -> Style {
    let (color, opacity, label) = match subtype {
        "road" | "highway" => (palette.road, 0.85, "Road"),
        "road_shoulder" | "shoulder" => (palette.shoulder, 0.85, "Road shoulder"),
        "crosswalk" => (palette.crosswalk, 0.55, "Crosswalk"),
        "walkway" | "pedestrian_lane" => (palette.walkway, 0.55, "Walkway"),
        "bicycle_lane" => (palette.bicycle, 0.6, "Bicycle lane"),
        "bus_lane" => (palette.bus_lane, 0.7, "Bus lane"),
        _ => (palette.lanelet_other, 0.7, "Other lanelet"),
    };
    let name = format!(
        "lanelet:{}",
        if subtype.is_empty() { "none" } else { subtype }
    );
    Style::new(&name, label, Z_LANELET).fill(color, opacity)
}

/// The fill for an area, chosen by its `subtype`.
pub fn area_style(subtype: &str, palette: &Palette) -> Style {
    let (color, opacity, label) = match subtype {
        "parking_lot" | "parking_spot" | "parking_access" => {
            (palette.parking, 0.55, "Parking area")
        }
        "keepout" | "no_parking_area" | "no_stopping_area" => {
            (palette.keepout, 0.5, "Keepout area")
        }
        _ => (palette.area_other, 0.6, "Other area"),
    };
    let name = format!("area:{}", if subtype.is_empty() { "none" } else { subtype });
    Style::new(&name, label, Z_AREA).fill(color, opacity)
}

/// Which of a standalone `area=yes` way's tags names it.
///
/// `type` is the usual carrier, but Autoware's polygons sometimes name themselves
/// through `subtype` instead. Decided here rather than in each renderer, so the next
/// carrier tag is a one-line change in one file.
pub fn polygon_key<'a>(kind: &'a str, subtype: &'a str) -> &'a str {
    if kind.is_empty() || kind == "polygon" {
        subtype
    } else {
        kind
    }
}

/// The fill for a standalone `area=yes` way — Autoware's `crosswalk_polygon`,
/// `hatched_road_markings` and friends. `kind` is [`polygon_key`]'s answer.
pub fn polygon_style(kind: &str, palette: &Palette) -> Style {
    let (color, opacity, label) = match kind {
        "crosswalk_polygon" => (palette.crosswalk, 0.35, "Crosswalk polygon"),
        "hatched_road_markings" => (palette.marking, 0.2, "Hatched marking"),
        "no_parking_area" | "no_stopping_area" | "keepout" => {
            (palette.keepout, 0.4, "Keepout polygon")
        }
        "parking_lot" | "parking_spot" => (palette.parking, 0.45, "Parking polygon"),
        "intersection_area" => (palette.road, 0.35, "Intersection area"),
        _ => (palette.area_other, 0.45, "Other polygon"),
    };
    let name = format!("polygon:{}", if kind.is_empty() { "none" } else { kind });
    Style::new(&name, label, Z_POLYGON).fill(color, opacity)
}

/// Whether a linestring belongs to the regulatory layer rather than the boundary
/// layer — stop lines and the physical extent of lights and signs.
pub fn is_regulatory_linestring(kind: &str) -> bool {
    matches!(
        kind,
        "stop_line" | "traffic_light" | "traffic_sign" | "light_bulbs" | "arrow"
    )
}

/// Which layer a linestring is drawn in, from its `type`.
///
/// The predicate above says *what* a linestring is; this says where it goes. Both
/// live here so that adding a regulatory type does not mean editing every renderer's
/// traversal to agree about which layer it landed in.
pub fn linestring_layer(kind: &str) -> VizLayer {
    if is_regulatory_linestring(kind) {
        VizLayer::Regulatory
    } else {
        VizLayer::Bound
    }
}

/// The stroke for a linestring, chosen by `type` and refined by `subtype`.
///
/// `type` carries the physical thing (a curb, a guard rail, a painted line) and
/// `subtype` its style (`solid`, `dashed`, `solid_dashed`). Both matter: a dashed
/// `line_thin` and a solid one are the difference between a lane you may change out
/// of and one you may not.
pub fn linestring_style(kind: &str, subtype: &str, palette: &Palette) -> Style {
    let z = if is_regulatory_linestring(kind) {
        Z_REGULATORY
    } else {
        Z_BOUND
    };
    let name = format!(
        "line:{}:{}",
        if kind.is_empty() { "none" } else { kind },
        if subtype.is_empty() { "none" } else { subtype }
    );

    let base = match kind {
        "line_thin" => Style::new(&name, marking_label("Lane marking", subtype), z)
            .stroke(palette.marking, 1.2),
        "line_thick" => Style::new(&name, marking_label("Thick marking", subtype), z)
            .stroke(palette.marking, 2.2),
        "pedestrian_marking" | "zebra_marking" => {
            Style::new(&name, "Pedestrian marking", z).stroke(palette.marking, 1.6)
        }
        "curbstone" => Style::new(&name, "Curbstone", z).stroke(palette.curbstone, 1.6),
        "road_border" => Style::new(&name, "Road border", z).stroke(palette.road_border, 1.8),
        "guard_rail" | "fence" | "wall" | "bump" | "door" | "gate" => {
            Style::new(&name, "Barrier", z).stroke(palette.barrier, 1.6)
        }
        "stop_line" => Style::new(&name, "Stop line", z).stroke(palette.stop_line, 2.6),
        "traffic_light" | "light_bulbs" => {
            Style::new(&name, "Traffic light", z).stroke(palette.traffic_light, 3.0)
        }
        "traffic_sign" | "arrow" => {
            Style::new(&name, "Traffic sign", z).stroke(palette.traffic_sign, 2.6)
        }
        "virtual" => Style::new(&name, "Virtual boundary", z)
            .stroke(palette.virtual_line, 1.0)
            .stroke_opacity(0.7)
            .dashed(3.0, 4.0),
        _ => Style::new(&name, "Other boundary", z)
            .stroke(palette.barrier, 1.2)
            .stroke_opacity(0.8),
    };

    apply_marking_subtype(base, kind, subtype)
}

/// `line_thin`/`line_thick` subtypes describe the paint, so they change the dash
/// pattern rather than the colour. The composite forms (`solid_dashed`) cannot be
/// drawn as one stroke without doubling the geometry, so they take the stricter
/// half — solid — which is the conservative reading for a viewer.
fn apply_marking_subtype(style: Style, kind: &str, subtype: &str) -> Style {
    if !matches!(
        kind,
        "line_thin" | "line_thick" | "pedestrian_marking" | "zebra_marking"
    ) {
        return style;
    }
    match subtype {
        "dashed" => style.dashed(4.0, 4.0),
        "dashed_solid" | "solid_dashed" => style.dashed(9.0, 3.0),
        _ => style,
    }
}

fn marking_label(base: &'static str, subtype: &str) -> &'static str {
    match subtype {
        "dashed" => "Lane marking (dashed)",
        "dashed_solid" | "solid_dashed" => "Lane marking (mixed)",
        _ => base,
    }
}

/// The centerline annotation — one style for every lanelet.
pub fn centerline_style(palette: &Palette) -> Style {
    Style::new("centerline", "Centerline", Z_CENTERLINE)
        .stroke(palette.centerline, 1.0)
        .stroke_opacity(0.85)
        .dashed(6.0, 5.0)
}

/// The driving-direction arrowheads, drawn as filled triangles — or, where a
/// renderer has real arrows, as shafts of [`Style::stroke_width`].
pub fn direction_style(palette: &Palette) -> Style {
    Style::new("direction", "Driving direction", Z_DIRECTION)
        .fill(palette.direction, 0.9)
        .stroke_width(3.0)
        .hidden_from_legend()
        // An arrowhead is a speck once a lane is four pixels wide.
        .hidden_below(4.0)
}

/// Individual map points, off by default because a city map has millions.
pub fn point_style(palette: &Palette) -> Style {
    Style::new("point", "Points", Z_POINT)
        .stroke(palette.point, 2.5)
        .stroke_opacity(0.8)
        .hidden_from_legend()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interning_is_by_name_and_stable() {
        let palette = Theme::Dark.palette();
        let mut table = StyleTable::new();
        let first = table.intern(lanelet_style("road", &palette));
        let again = table.intern(lanelet_style("road", &palette));
        let other = table.intern(lanelet_style("crosswalk", &palette));
        assert_eq!(first, again);
        assert_ne!(first, other);
        assert_eq!(table.len(), 2);
    }

    #[test]
    fn a_dashed_marking_gets_a_dash_pattern_and_a_solid_one_does_not() {
        let palette = Theme::Dark.palette();
        assert!(
            linestring_style("line_thin", "dashed", &palette)
                .dash
                .is_some()
        );
        assert!(
            linestring_style("line_thin", "solid", &palette)
                .dash
                .is_none()
        );
        // A curbstone is a physical thing; its subtype (`low`/`high`) is not paint.
        assert!(
            linestring_style("curbstone", "dashed", &palette)
                .dash
                .is_none()
        );
    }

    #[test]
    fn regulatory_linestrings_sort_above_boundaries() {
        let palette = Theme::Dark.palette();
        let stop = linestring_style("stop_line", "", &palette);
        let curb = linestring_style("curbstone", "low", &palette);
        assert!(stop.z > curb.z);
    }

    #[test]
    fn an_unknown_type_still_gets_a_stroke() {
        let palette = Theme::Dark.palette();
        let style = linestring_style("something_new", "", &palette);
        assert!(style.stroke.is_some());
        assert_eq!(style.name, "line:something_new:none");
    }

    #[test]
    fn colours_render_as_six_digit_hex() {
        assert_eq!(Color(0x00, 0x0a, 0xff).to_hex(), "#000aff");
    }

    /// There are two statements of the stacking order in this project: `Style::z`,
    /// which the 2D renderers sort by, and `VizLayer`'s declaration order, which the
    /// Rerun renderer turns into real separation along z. They agree today by
    /// coincidence, and a lane marking sinking into the road it is painted on is
    /// what disagreement looks like — so the coincidence is checked here.
    #[test]
    fn the_z_bands_run_in_vizlayer_declaration_order() {
        let palette = Theme::Dark.palette();
        let z_of = |layer: VizLayer| match layer {
            VizLayer::LaneletFill => lanelet_style("road", &palette).z,
            VizLayer::Area => area_style("parking_lot", &palette).z,
            VizLayer::Polygon => polygon_style("crosswalk_polygon", &palette).z,
            VizLayer::Bound => linestring_style("curbstone", "", &palette).z,
            VizLayer::Regulatory => linestring_style("stop_line", "", &palette).z,
            VizLayer::Centerline => centerline_style(&palette).z,
            VizLayer::Direction => direction_style(&palette).z,
            VizLayer::Point => point_style(&palette).z,
        };
        for pair in VizLayer::ALL.windows(2) {
            assert!(
                z_of(pair[0]) < z_of(pair[1]),
                "{:?} does not sort below {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn a_polygons_name_comes_from_whichever_tag_carries_it() {
        assert_eq!(polygon_key("crosswalk_polygon", ""), "crosswalk_polygon");
        // Autoware writes the generic `type` and names the thing in `subtype`.
        assert_eq!(polygon_key("polygon", "parking_lot"), "parking_lot");
        assert_eq!(polygon_key("", "parking_lot"), "parking_lot");
    }

    #[test]
    fn a_linestrings_layer_follows_its_type() {
        assert_eq!(linestring_layer("stop_line"), VizLayer::Regulatory);
        assert_eq!(linestring_layer("curbstone"), VizLayer::Bound);
        assert_eq!(linestring_layer("something_new"), VizLayer::Bound);
    }
}
