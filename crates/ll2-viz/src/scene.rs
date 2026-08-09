//! Turning a `LaneletMap` into a flat list of styled shapes.
//!
//! A `Scene` is deliberately dumb: polylines and polygons in map coordinates, each
//! pointing at an entry in a style table. It knows nothing about SVG, canvas or
//! pixels, which is what lets the same visualisation drive the SVG writer here and
//! a `<canvas>` on the other side of a WebAssembly boundary.
//!
//! Map coordinates are metres, x east and y north. Renderers flip y themselves.

use ll2_core::area::Area;
use ll2_core::geometry::bbox::BoundingBox2d;
use ll2_core::geometry::distance::distance_2d_point_point;
use ll2_core::geometry::lanelet::{centerline_2d, outline_2d};
use ll2_core::id::Id;
use ll2_core::lanelet::Lanelet;
use ll2_core::linestring::LineString;
use ll2_core::map::{LaneletMap, as_area, as_lanelet, as_linestring, as_point};

use crate::style::{self, Palette, Style, StyleTable, Theme, VizLayer};

/// What to draw, and how much of it.
#[derive(Clone, Debug)]
pub struct VizOptions {
    pub theme: Theme,
    pub lanelet_fill: bool,
    pub areas: bool,
    pub polygons: bool,
    pub bounds: bool,
    pub regulatory: bool,
    pub centerlines: bool,
    pub direction_arrows: bool,
    pub points: bool,
    /// Metres between driving-direction arrowheads along a centerline.
    pub arrow_spacing: f64,
}

impl Default for VizOptions {
    fn default() -> Self {
        VizOptions {
            theme: Theme::Dark,
            lanelet_fill: true,
            areas: true,
            polygons: true,
            bounds: true,
            regulatory: true,
            centerlines: false,
            direction_arrows: true,
            points: false,
            arrow_spacing: 25.0,
        }
    }
}

impl VizOptions {
    /// Whether a layer is drawn. Public because the layer key, not the field
    /// name, is the vocabulary every renderer and every wire format uses.
    pub fn wants_layer(&self, layer: VizLayer) -> bool {
        match layer {
            VizLayer::LaneletFill => self.lanelet_fill,
            VizLayer::Area => self.areas,
            VizLayer::Polygon => self.polygons,
            VizLayer::Bound => self.bounds,
            VizLayer::Regulatory => self.regulatory,
            VizLayer::Centerline => self.centerlines,
            VizLayer::Direction => self.direction_arrows,
            VizLayer::Point => self.points,
        }
    }
}

/// One drawable thing: a polyline, or a polygon when `closed`.
#[derive(Clone, Debug)]
pub struct Shape {
    /// The id of the primitive this came from, or `0` for derived geometry.
    pub id: Id,
    pub layer: VizLayer,
    /// Index into the scene's [`StyleTable`].
    pub style: usize,
    /// A one-line description, for a tooltip or a status bar.
    pub label: String,
    pub points: Vec<[f64; 2]>,
    pub closed: bool,
}

impl Shape {
    pub fn bounding_box(&self) -> BoundingBox2d {
        let mut box2d = BoundingBox2d::empty();
        for point in &self.points {
            box2d.extend_point(*point);
        }
        box2d
    }
}

/// How many primitives of each kind the map held.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MapStats {
    pub points: usize,
    pub line_strings: usize,
    pub polygons: usize,
    pub lanelets: usize,
    pub areas: usize,
    pub regulatory_elements: usize,
}

impl MapStats {
    pub fn of(map: &LaneletMap) -> Self {
        MapStats {
            points: map.points.len(),
            line_strings: map.line_strings.len(),
            polygons: map.polygons.len(),
            lanelets: map.lanelets.len(),
            areas: map.areas.len(),
            regulatory_elements: map.regulatory_elements.len(),
        }
    }
}

/// A whole map, ready to draw.
pub struct Scene {
    pub shapes: Vec<Shape>,
    pub styles: StyleTable,
    pub bounds: BoundingBox2d,
    pub stats: MapStats,
    pub theme: Theme,
}

impl Scene {
    /// Builds a scene from a map.
    ///
    /// This is the visualisation: everything after it is a matter of which
    /// rasteriser you happen to be holding.
    pub fn from_map(map: &LaneletMap, options: &VizOptions) -> Scene {
        let palette = options.theme.palette();
        let mut builder = Builder {
            shapes: Vec::new(),
            styles: StyleTable::new(),
            palette,
            options,
        };

        // Order of construction does not matter — shapes are sorted by the style's
        // z band at the end — but building fills first keeps the common case, where
        // nothing is sorted out of place, from moving much memory around.
        builder.add_lanelets(map);
        builder.add_areas(map);
        builder.add_polygons(map);
        builder.add_linestrings(map);
        builder.add_points(map);

        let Builder {
            mut shapes, styles, ..
        } = builder;
        shapes.sort_by_key(|shape| styles.get(shape.style).map(|s| s.z).unwrap_or(0));

        let mut bounds = BoundingBox2d::empty();
        for shape in &shapes {
            for point in &shape.points {
                bounds.extend_point(*point);
            }
        }

        Scene {
            shapes,
            styles,
            bounds,
            stats: MapStats::of(map),
            theme: options.theme,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// The map's extent, or a unit box when there is nothing to show — so a caller
    /// fitting the view to the map never divides by zero.
    pub fn safe_bounds(&self) -> BoundingBox2d {
        if self.bounds.is_empty() {
            return BoundingBox2d {
                min: [-1.0, -1.0],
                max: [1.0, 1.0],
            };
        }
        let mut bounds = self.bounds;
        for axis in 0..2 {
            if (bounds.max[axis] - bounds.min[axis]).abs() < 1e-9 {
                bounds.min[axis] -= 1.0;
                bounds.max[axis] += 1.0;
            }
        }
        bounds
    }
}

struct Builder<'a> {
    shapes: Vec<Shape>,
    styles: StyleTable,
    palette: Palette,
    options: &'a VizOptions,
}

impl Builder<'_> {
    fn push(
        &mut self,
        layer: VizLayer,
        style: Style,
        id: Id,
        label: String,
        points: Vec<[f64; 2]>,
        closed: bool,
    ) {
        let style = self.styles.intern(style);
        self.push_interned(layer, style, id, label, points, closed);
    }

    /// The same, for a caller that has already interned its style — every
    /// arrowhead on a lanelet shares one, and a road map is mostly arrowheads.
    fn push_interned(
        &mut self,
        layer: VizLayer,
        style: usize,
        id: Id,
        label: String,
        points: Vec<[f64; 2]>,
        closed: bool,
    ) {
        if points.len() < 2 && !matches!(layer, VizLayer::Point) {
            return;
        }
        self.shapes.push(Shape {
            id,
            layer,
            style,
            label,
            points,
            closed,
        });
    }

    fn add_lanelets(&mut self, map: &LaneletMap) {
        let wants_fill = self.options.wants_layer(VizLayer::LaneletFill);
        let wants_centerline = self.options.wants_layer(VizLayer::Centerline);
        let wants_arrows = self.options.wants_layer(VizLayer::Direction);
        if !(wants_fill || wants_centerline || wants_arrows) {
            return;
        }

        for primitive in map.lanelets.all() {
            let Some(lanelet) = as_lanelet(&primitive) else {
                continue;
            };
            let subtype = attribute(lanelet.attributes(), "subtype");
            // One description, shared by the fill, the centerline and every arrow.
            // A viewer picks the topmost shape under the cursor, which over a road
            // is usually an arrowhead rather than the fill — so the arrow has to be
            // able to answer "what lanelet is this?" as well as the fill does.
            let description = describe_lanelet(lanelet, &subtype);

            if wants_fill {
                let outline = outline_2d(lanelet);
                if outline.len() >= 3 {
                    let style = style::lanelet_style(&subtype, &self.palette);
                    self.push(
                        VizLayer::LaneletFill,
                        style,
                        lanelet.id(),
                        description.clone(),
                        outline,
                        true,
                    );
                }
            }

            if wants_centerline || wants_arrows {
                let centerline = centerline_2d(lanelet);
                if wants_centerline && centerline.len() >= 2 {
                    let style = style::centerline_style(&self.palette);
                    self.push(
                        VizLayer::Centerline,
                        style,
                        lanelet.id(),
                        format!("centerline · {description}"),
                        centerline.clone(),
                        false,
                    );
                }
                if wants_arrows {
                    self.add_arrows(lanelet, &centerline, &description);
                }
            }
        }
    }

    /// Arrowheads along a lanelet's centerline, sized from the lanelet's own width
    /// so a narrow footpath does not get a motorway-sized chevron.
    fn add_arrows(&mut self, lanelet: &Lanelet, centerline: &[[f64; 2]], description: &str) {
        if centerline.len() < 2 {
            return;
        }
        let width = lanelet_width(lanelet).clamp(1.0, 6.0);
        let size = (width * 0.45).clamp(0.6, 2.5);
        let style = self.direction_style();
        let label = format!("{description} ▸");

        for (position, heading) in sample_along(centerline, self.options.arrow_spacing) {
            let triangle = arrowhead(position, heading, size);
            self.push_interned(
                VizLayer::Direction,
                style,
                lanelet.id(),
                label.clone(),
                triangle,
                true,
            );
        }
    }

    /// Every arrowhead in the map shares one style; building it per arrow would
    /// mean four allocations and a hash lookup for a value that never varies.
    fn direction_style(&mut self) -> usize {
        match self.styles.lookup("direction") {
            Some(index) => index,
            None => self.styles.intern(style::direction_style(&self.palette)),
        }
    }

    fn add_areas(&mut self, map: &LaneletMap) {
        if !self.options.wants_layer(VizLayer::Area) {
            return;
        }
        for primitive in map.areas.all() {
            let Some(area) = as_area(&primitive) else {
                continue;
            };
            let subtype = attribute(area.attributes(), "subtype");
            let outline = area_outline(area);
            if outline.len() < 3 {
                continue;
            }
            let style = style::area_style(&subtype, &self.palette);
            let label = describe(
                area.id(),
                "area",
                &attribute(area.attributes(), "type"),
                &subtype,
            );
            self.push(VizLayer::Area, style, area.id(), label, outline, true);
        }
    }

    fn add_polygons(&mut self, map: &LaneletMap) {
        if !self.options.wants_layer(VizLayer::Polygon) {
            return;
        }
        for primitive in map.polygons.all() {
            let Some(line) = as_linestring(&primitive) else {
                continue;
            };
            let attributes = line.attributes();
            // `type` is the usual carrier, but Autoware's polygons sometimes name
            // themselves through `subtype` instead.
            let kind = attribute(attributes, "type");
            let subtype = attribute(attributes, "subtype");
            let key = if kind.is_empty() || kind == "polygon" {
                subtype.clone()
            } else {
                kind.clone()
            };
            let style = style::polygon_style(&key, &self.palette);
            let label = describe(line.id(), "polygon", &kind, &subtype);
            self.push(
                VizLayer::Polygon,
                style,
                line.id(),
                label,
                points_of(line),
                true,
            );
        }
    }

    fn add_linestrings(&mut self, map: &LaneletMap) {
        let wants_bounds = self.options.wants_layer(VizLayer::Bound);
        let wants_regulatory = self.options.wants_layer(VizLayer::Regulatory);
        if !(wants_bounds || wants_regulatory) {
            return;
        }
        for primitive in map.line_strings.all() {
            let Some(line) = as_linestring(&primitive) else {
                continue;
            };
            let attributes = line.attributes();
            let kind = attribute(attributes, "type");
            let subtype = attribute(attributes, "subtype");
            let layer = if style::is_regulatory_linestring(&kind) {
                VizLayer::Regulatory
            } else {
                VizLayer::Bound
            };
            if !self.options.wants_layer(layer) {
                continue;
            }
            let style = style::linestring_style(&kind, &subtype, &self.palette);
            let label = describe(line.id(), "linestring", &kind, &subtype);
            self.push(layer, style, line.id(), label, points_of(line), false);
        }
    }

    fn add_points(&mut self, map: &LaneletMap) {
        if !self.options.wants_layer(VizLayer::Point) {
            return;
        }
        let style = style::point_style(&self.palette);
        for primitive in map.points.all() {
            let Some(point) = as_point(&primitive) else {
                continue;
            };
            let [x, y, _] = point.xyz();
            let label = format!("point {}", point.id());
            // A one-point "polyline"; renderers draw these as dots.
            self.push(
                VizLayer::Point,
                style.clone(),
                point.id(),
                label,
                vec![[x, y]],
                false,
            );
        }
    }
}

fn attribute(attributes: &ll2_core::refs::Attrs, key: &str) -> String {
    attributes
        .read()
        .get(key)
        .map(|value| value.value().to_owned())
        .unwrap_or_default()
}

fn points_of(line: &LineString) -> Vec<[f64; 2]> {
    line.points()
        .iter()
        .map(|point| {
            let [x, y, _] = point.xyz();
            [x, y]
        })
        .collect()
}

fn area_outline(area: &Area) -> Vec<[f64; 2]> {
    let mut outline: Vec<[f64; 2]> = Vec::new();
    for line in area.outer_bound() {
        for point in points_of(&line) {
            // Consecutive members of a ring share their end points.
            if outline.last() != Some(&point) {
                outline.push(point);
            }
        }
    }
    outline
}

/// A lanelet's mean width, from the gap between its bounds at each end.
///
/// Four points, so it asks for four points: materialising both boundaries to read
/// their ends would clone an `Arc` and take a lock per vertex, once per lanelet,
/// for numbers already thrown away.
fn lanelet_width(lanelet: &Lanelet) -> f64 {
    let (left, right) = (lanelet.left_bound(), lanelet.right_bound());
    let ends = [(left.front(), right.front()), (left.back(), right.back())];
    let mut total = 0.0;
    let mut count = 0.0;
    for (a, b) in ends {
        if let (Some(a), Some(b)) = (a, b) {
            let ([ax, ay, _], [bx, by, _]) = (a.xyz(), b.xyz());
            total += distance_2d_point_point([ax, ay], [bx, by]);
            count += 1.0;
        }
    }
    if count == 0.0 { 3.0 } else { total / count }
}

/// Positions and headings at roughly `spacing` metres along a polyline.
///
/// Always yields at least one sample — a five-metre lanelet still gets an arrow,
/// placed at its middle, because a lanelet with no arrow reads as one with no
/// direction rather than as one that was too short to mark.
fn sample_along(points: &[[f64; 2]], spacing: f64) -> Vec<([f64; 2], [f64; 2])> {
    let spacing = if spacing.is_finite() && spacing > 0.1 {
        spacing
    } else {
        25.0
    };
    let lengths: Vec<f64> = points
        .windows(2)
        .map(|pair| distance_2d_point_point(pair[0], pair[1]))
        .collect();
    let total: f64 = lengths.iter().sum();
    if total <= 0.0 {
        return Vec::new();
    }

    let count = (total / spacing).floor().max(1.0) as usize;
    let step = total / (count as f64 + 1.0);
    let mut samples = Vec::with_capacity(count);
    let mut target = step;
    let mut travelled = 0.0;

    for (index, length) in lengths.iter().enumerate() {
        if *length <= 0.0 {
            continue;
        }
        let (start, end) = (points[index], points[index + 1]);
        let heading = [(end[0] - start[0]) / length, (end[1] - start[1]) / length];
        while target <= travelled + length && samples.len() < count {
            let ratio = (target - travelled) / length;
            samples.push((
                [
                    start[0] + (end[0] - start[0]) * ratio,
                    start[1] + (end[1] - start[1]) * ratio,
                ],
                heading,
            ));
            target += step;
        }
        travelled += length;
    }
    samples
}

/// A triangle of the given size pointing along `heading`.
fn arrowhead(position: [f64; 2], heading: [f64; 2], size: f64) -> Vec<[f64; 2]> {
    let (dx, dy) = (heading[0], heading[1]);
    let (nx, ny) = (-dy, dx);
    let half = size * 0.45;
    vec![
        [position[0] + dx * size * 0.6, position[1] + dy * size * 0.6],
        [
            position[0] - dx * size * 0.4 + nx * half,
            position[1] - dy * size * 0.4 + ny * half,
        ],
        [
            position[0] - dx * size * 0.4 - nx * half,
            position[1] - dy * size * 0.4 - ny * half,
        ],
    ]
}

fn describe(id: Id, kind: &str, type_tag: &str, subtype: &str) -> String {
    let mut text = format!("{kind} {id}");
    if !type_tag.is_empty() {
        text.push_str(" · ");
        text.push_str(type_tag);
    }
    if !subtype.is_empty() {
        text.push_str(if type_tag.is_empty() { " · " } else { "/" });
        text.push_str(subtype);
    }
    text
}

fn describe_lanelet(lanelet: &Lanelet, subtype: &str) -> String {
    let mut text = format!("lanelet {}", lanelet.id());
    if !subtype.is_empty() {
        text.push_str(" · ");
        text.push_str(subtype);
    }
    let attributes = lanelet.attributes();
    let speed = attribute(attributes, "speed_limit");
    if !speed.is_empty() {
        text.push_str(" · ");
        text.push_str(&speed);
    }
    let regelems = lanelet.regulatory_elements().len();
    if regelems > 0 {
        text.push_str(&format!(" · {regelems} reg. elem."));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll2_core::attribute::{Attribute, AttributeMap};
    use ll2_core::map::Primitive;
    use ll2_core::point::Point;

    fn line(id: Id, points: &[[f64; 2]], tags: &[(&str, &str)]) -> LineString {
        let attributes: AttributeMap = tags
            .iter()
            .map(|(k, v)| ((*k).to_owned(), Attribute::new(*v)))
            .collect();
        let points = points
            .iter()
            .enumerate()
            .map(|(index, [x, y])| {
                Point::new(id * 100 + index as Id + 1, *x, *y, 0.0, AttributeMap::new())
            })
            .collect();
        LineString::new(id, points, attributes)
    }

    fn one_lanelet_map() -> std::sync::Arc<LaneletMap> {
        let map = LaneletMap::new_map();
        let left = line(
            1,
            &[[0.0, 3.0], [30.0, 3.0]],
            &[("type", "line_thin"), ("subtype", "dashed")],
        );
        let right = line(
            2,
            &[[0.0, 0.0], [30.0, 0.0]],
            &[("type", "line_thin"), ("subtype", "solid")],
        );
        let attributes: AttributeMap = [
            ("type".to_owned(), Attribute::new("lanelet")),
            ("subtype".to_owned(), Attribute::new("road")),
        ]
        .into_iter()
        .collect();
        map.add(Primitive::Lanelet(Lanelet::new(3, left, right, attributes)));
        map
    }

    #[test]
    fn a_lanelet_becomes_a_closed_fill_plus_its_two_bounds() {
        let map = one_lanelet_map();
        let scene = Scene::from_map(&map, &VizOptions::default());

        let fills: Vec<_> = scene
            .shapes
            .iter()
            .filter(|s| s.layer == VizLayer::LaneletFill)
            .collect();
        assert_eq!(fills.len(), 1);
        assert!(fills[0].closed);
        // Left forwards then right backwards: (0,3) (30,3) (30,0) (0,0).
        assert_eq!(
            fills[0].points,
            vec![[0.0, 3.0], [30.0, 3.0], [30.0, 0.0], [0.0, 0.0]]
        );

        let bounds = scene
            .shapes
            .iter()
            .filter(|s| s.layer == VizLayer::Bound)
            .count();
        assert_eq!(bounds, 2);
    }

    #[test]
    fn fills_sort_below_boundaries() {
        let map = one_lanelet_map();
        let scene = Scene::from_map(&map, &VizOptions::default());
        let first_bound = scene
            .shapes
            .iter()
            .position(|s| s.layer == VizLayer::Bound)
            .unwrap();
        let last_fill = scene
            .shapes
            .iter()
            .rposition(|s| s.layer == VizLayer::LaneletFill)
            .unwrap();
        assert!(last_fill < first_bound);
    }

    #[test]
    fn a_thirty_metre_lanelet_gets_one_arrow_at_twenty_five_metre_spacing() {
        let map = one_lanelet_map();
        let scene = Scene::from_map(&map, &VizOptions::default());
        let arrows: Vec<_> = scene
            .shapes
            .iter()
            .filter(|s| s.layer == VizLayer::Direction)
            .collect();
        assert_eq!(arrows.len(), 1);
        assert!(arrows[0].closed);
        assert_eq!(arrows[0].points.len(), 3);
        // Placed at the middle of the run, pointing +x.
        let centroid_x: f64 = arrows[0].points.iter().map(|p| p[0]).sum::<f64>() / 3.0;
        assert!((centroid_x - 15.0).abs() < 1.0, "arrow at {centroid_x}");
    }

    #[test]
    fn a_short_lanelet_still_gets_exactly_one_arrow() {
        let samples = sample_along(&[[0.0, 0.0], [2.0, 0.0]], 25.0);
        assert_eq!(samples.len(), 1);
        assert!((samples[0].0[0] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sampling_a_degenerate_line_yields_nothing_rather_than_dividing_by_zero() {
        assert!(sample_along(&[[1.0, 1.0], [1.0, 1.0]], 5.0).is_empty());
        assert!(sample_along(&[[1.0, 1.0]], 5.0).is_empty());
    }

    #[test]
    fn turning_a_layer_off_removes_exactly_that_layer() {
        let map = one_lanelet_map();
        let options = VizOptions {
            bounds: false,
            ..VizOptions::default()
        };
        let scene = Scene::from_map(&map, &options);
        assert!(!scene.shapes.iter().any(|s| s.layer == VizLayer::Bound));
        assert!(
            scene
                .shapes
                .iter()
                .any(|s| s.layer == VizLayer::LaneletFill)
        );
    }

    #[test]
    fn bounds_cover_the_map_and_an_empty_scene_still_has_a_usable_box() {
        let map = one_lanelet_map();
        let scene = Scene::from_map(&map, &VizOptions::default());
        assert_eq!(scene.bounds.min, [0.0, 0.0]);
        assert_eq!(scene.bounds.max, [30.0, 3.0]);

        let empty = Scene::from_map(&LaneletMap::new_map(), &VizOptions::default());
        assert!(empty.is_empty());
        assert!(!empty.safe_bounds().is_empty());
    }

    #[test]
    fn stats_count_what_the_map_holds() {
        let map = one_lanelet_map();
        let scene = Scene::from_map(&map, &VizOptions::default());
        assert_eq!(scene.stats.lanelets, 1);
        assert_eq!(scene.stats.line_strings, 2);
        assert_eq!(scene.stats.points, 4);
    }

    #[test]
    fn labels_carry_the_tags_a_tooltip_wants() {
        let map = one_lanelet_map();
        let scene = Scene::from_map(&map, &VizOptions::default());
        let fill = scene
            .shapes
            .iter()
            .find(|s| s.layer == VizLayer::LaneletFill)
            .unwrap();
        assert_eq!(fill.label, "lanelet 3 · road");
        let bound = scene
            .shapes
            .iter()
            .find(|s| s.layer == VizLayer::Bound)
            .unwrap();
        assert!(bound.label.contains("line_thin"));
    }

    /// A viewer picks the topmost shape under the cursor, and over a road that is
    /// an arrowhead far more often than it is the fill. Both must therefore say
    /// which lanelet they belong to, or hovering a road tells you nothing.
    #[test]
    fn a_lanelets_arrows_and_centerline_carry_its_description() {
        let map = one_lanelet_map();
        let options = VizOptions {
            centerlines: true,
            ..VizOptions::default()
        };
        let scene = Scene::from_map(&map, &options);
        let label_of = |layer: VizLayer| {
            scene
                .shapes
                .iter()
                .find(|s| s.layer == layer)
                .map(|s| s.label.clone())
                .unwrap()
        };
        let fill = label_of(VizLayer::LaneletFill);
        assert_eq!(fill, "lanelet 3 · road");
        assert!(label_of(VizLayer::Direction).starts_with(&fill));
        assert!(label_of(VizLayer::Centerline).contains(&fill));
    }
}
