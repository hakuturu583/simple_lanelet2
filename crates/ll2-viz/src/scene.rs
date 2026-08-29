//! Turning a `LaneletMap` into a flat list of styled shapes.
//!
//! A `Scene` is deliberately dumb: polylines and polygons in map coordinates, each
//! pointing at an entry in a style table. It knows nothing about SVG, canvas or
//! pixels, which is what lets the same visualisation drive the SVG writer here and
//! a `<canvas>` on the other side of a WebAssembly boundary.
//!
//! Map coordinates are metres, x east, y north and **z up**: a scene keeps the
//! elevation every Lanelet2 node carries. A plan view throws it away again at the
//! last moment — see [`crate::view`] — but it is kept this far because dropping it
//! here is what makes an overpass indistinguishable from the road beneath it, and no
//! renderer can put back what the scene never had.

use ll2_core::area::Area;
use ll2_core::geometry::bbox::{BoundingBox2d, BoundingBox3d};
use ll2_core::geometry::lanelet::{centerline_3d, mean_width_2d, outline_3d};
use ll2_core::id::Id;
use ll2_core::lanelet::Lanelet;
use ll2_core::linestring::LineString;
use ll2_core::map::{LaneletMap, as_area, as_lanelet, as_linestring, as_point};

use crate::polyline::{Point3, sample_along};
use crate::style::{self, Palette, Style, StyleTable, Theme, VizLayer};
use crate::view::View;

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
    /// Vertices in map coordinates: metres, x east, y north, z up.
    pub points: Vec<Point3>,
    pub closed: bool,
}

impl Shape {
    pub fn bounding_box(&self) -> BoundingBox3d {
        let mut box3d = BoundingBox3d::empty();
        for point in &self.points {
            box3d.extend_point(*point);
        }
        box3d
    }

    /// How far the shape is from the viewer, for painter's ordering.
    ///
    /// The mean of its vertices rather than its nearest or furthest point: a road
    /// surface that dips at one end should not sort as though all of it were down
    /// there, and the mean is the only one of the three that moves smoothly as the
    /// camera turns. Shapes only ever compete with others in their own layer — see
    /// [`Scene::draw_order`] — where they are small and comparable.
    pub fn depth(&self, view: &View) -> f64 {
        if self.points.is_empty() {
            return 0.0;
        }
        let total: f64 = self.points.iter().map(|point| view.depth(*point)).sum();
        total / self.points.len() as f64
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
    /// The map's extent in three dimensions. Its z span is the map's relief.
    pub bounds: BoundingBox3d,
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

        let mut bounds = BoundingBox3d::empty();
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
    ///
    /// A flat map is the ordinary case rather than a degenerate one, so its zero z
    /// span is left alone; only the axes a page is fitted to are widened.
    pub fn safe_bounds(&self) -> BoundingBox3d {
        if self.bounds.is_empty() {
            return BoundingBox3d {
                min: [-1.0, -1.0, 0.0],
                max: [1.0, 1.0, 0.0],
            };
        }
        let mut bounds = self.bounds;
        widen_flat_axes(&mut bounds.min[..2], &mut bounds.max[..2]);
        bounds
    }

    /// Metres between the map's lowest point and its highest — how much relief there
    /// is for a tilted view to show, and zero for the many files whose nodes carry no
    /// `ele` at all. See [`crate::view::worth_tilting`].
    pub fn relief(&self) -> f64 {
        if self.bounds.is_empty() {
            return 0.0;
        }
        (self.bounds.max[2] - self.bounds.min[2]).max(0.0)
    }

    /// The extent of the map *as `view` draws it*, on the page, never degenerate.
    ///
    /// What a renderer fits its page or its canvas to. Tilting the camera shortens
    /// the map along screen y, so the framing has to come from the projection rather
    /// than from the map — a scene fitted to its plan extent and then drawn obliquely
    /// sits in the middle of the page with a band of nothing above and below it.
    pub fn view_bounds(&self, view: &View) -> BoundingBox2d {
        let mut bounds = view.project_bounds(&self.safe_bounds());
        widen_flat_axes(&mut bounds.min, &mut bounds.max);
        bounds
    }

    /// The shapes in the order they must be drawn, as indices into [`Scene::shapes`].
    ///
    /// Under a plan view that is the order they are already in: shapes are sorted
    /// into style z bands when the scene is built, so a lane marking is drawn over
    /// the road it is painted on and nothing else can matter, since two shapes seen
    /// from directly above never hide one another.
    ///
    /// Tilt the camera and depth starts to matter — but not more than the bands do.
    /// The band is what says "this is painted on that", which is true from every
    /// angle; depth only decides between shapes that make the same claim. So an
    /// overpass is drawn over the road it crosses, because both are lanelet fills
    /// and the overpass is nearer, while the markings of the road below are still
    /// drawn over both. That last part is wrong, and is the price of a painter's
    /// algorithm with no depth buffer: a renderer that has one should sort by
    /// [`Shape::depth`] alone and let the buffer settle the rest.
    pub fn draw_order(&self, view: &View) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.shapes.len()).collect();
        if view.is_plan() {
            return order;
        }
        // Both keys in one linear pass. Deriving the band inside the comparator
        // instead would chase a style index through the shape table twice per
        // comparison — n log n scattered reads over tens of megabytes, for a value
        // that does not change.
        let keys: Vec<(i32, f64)> = self
            .shapes
            .iter()
            .map(|shape| {
                let band = self
                    .styles
                    .get(shape.style)
                    .map(|style| style.z)
                    .unwrap_or(0);
                (band, shape.depth(view))
            })
            .collect();
        order.sort_unstable_by(|a, b| {
            keys[*a]
                .0
                .cmp(&keys[*b].0)
                // Furthest first, so the nearest shape in a band is painted last.
                .then_with(|| keys[*b].1.total_cmp(&keys[*a].1))
        });
        order
    }
}

/// Widens any axis with no extent, so a caller fitting a view to a box never
/// divides by zero. The threshold and the margin live here rather than at each of
/// the two call sites that would otherwise state them.
fn widen_flat_axes(min: &mut [f64], max: &mut [f64]) {
    for (low, high) in min.iter_mut().zip(max) {
        if (*high - *low).abs() < 1e-9 {
            *low -= 1.0;
            *high += 1.0;
        }
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
        points: Vec<Point3>,
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
        points: Vec<Point3>,
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
                let outline = outline_3d(lanelet);
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
                let centerline = centerline_3d(lanelet);
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
    fn add_arrows(&mut self, lanelet: &Lanelet, centerline: &[Point3], description: &str) {
        if centerline.len() < 2 {
            return;
        }
        let width = mean_width_2d(lanelet).clamp(1.0, 6.0);
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
            let kind = attribute(attributes, "type");
            let subtype = attribute(attributes, "subtype");
            let style = style::polygon_style(style::polygon_key(&kind, &subtype), &self.palette);
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
            let layer = style::linestring_layer(&kind);
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
            let label = format!("point {}", point.id());
            // A one-point "polyline"; renderers draw these as dots.
            self.push(
                VizLayer::Point,
                style.clone(),
                point.id(),
                label,
                vec![point.xyz()],
                false,
            );
        }
    }
}

/// One attribute's value, or the empty string when it is absent.
///
/// Public because every renderer built on this crate has to read the same handful
/// of tags to label what it draws, and reading them twice is how two viewers end up
/// disagreeing about what a primitive is called.
pub fn attribute(attributes: &ll2_core::refs::Attrs, key: &str) -> String {
    attributes
        .read()
        .get(key)
        .map(|value| value.value().to_owned())
        .unwrap_or_default()
}

/// A linestring's vertices, elevation and all.
///
/// Public for the same reason [`attribute`] is: it is the one statement of how a
/// Lanelet2 primitive becomes coordinates, and a renderer that writes its own
/// gets to disagree about it.
pub fn points_of(line: &LineString) -> Vec<Point3> {
    line.points()
        .iter()
        .map(ll2_core::point::Point::xyz)
        .collect()
}

/// An area's outer bound, stitched into one ring.
///
/// The members of a ring share their end points, so the joins are dropped as they
/// are met; the ring is left closed — its last point repeats its first — for a
/// caller that wants it that way.
pub fn area_outline(area: &Area) -> Vec<Point3> {
    let mut outline: Vec<Point3> = Vec::new();
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

/// A triangle of the given size pointing along `heading`.
///
/// It lies in the road's own surface rather than flat on the ground: the tip runs up
/// the slope with the heading, and the base is spread across it. Drawn flat, an arrow
/// on a ramp would cut through the surface it belongs to as soon as anything tilted
/// the camera.
fn arrowhead(position: Point3, heading: Point3, size: f64) -> Vec<Point3> {
    let [dx, dy, dz] = heading;
    // Across the road, horizontally: a banked arrow would be a claim about camber
    // that a Lanelet2 map does not make. A heading with no horizontal part at all is
    // a vertical road, which no map has; north keeps the triangle from collapsing.
    let flat = f64::hypot(dx, dy);
    let (nx, ny) = if flat > 1e-9 {
        (-dy / flat, dx / flat)
    } else {
        (0.0, 1.0)
    };
    let half = size * 0.45;
    let back = |across: f64| {
        [
            position[0] - dx * size * 0.4 + nx * across,
            position[1] - dy * size * 0.4 + ny * across,
            position[2] - dz * size * 0.4,
        ]
    };
    vec![
        [
            position[0] + dx * size * 0.6,
            position[1] + dy * size * 0.6,
            position[2] + dz * size * 0.6,
        ],
        back(half),
        back(-half),
    ]
}

/// The one-line description a tooltip, a status bar or a Rerun label shows for a
/// primitive: what it is, which id it has, and the tags that classified it.
pub fn describe(id: Id, kind: &str, type_tag: &str, subtype: &str) -> String {
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

/// [`describe`] for a lanelet, which has more worth saying: its subtype, its speed
/// limit and how many regulatory elements apply to it.
pub fn describe_lanelet(lanelet: &Lanelet, subtype: &str) -> String {
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

    fn line(id: Id, points: &[Point3], tags: &[(&str, &str)]) -> LineString {
        let attributes: AttributeMap = tags
            .iter()
            .map(|(k, v)| ((*k).to_owned(), Attribute::new(*v)))
            .collect();
        let points = points
            .iter()
            .enumerate()
            .map(|(index, [x, y, z])| {
                Point::new(id * 100 + index as Id + 1, *x, *y, *z, AttributeMap::new())
            })
            .collect();
        LineString::new(id, points, attributes)
    }

    fn lanelet_map(left_z: [f64; 2], right_z: [f64; 2]) -> std::sync::Arc<LaneletMap> {
        let map = LaneletMap::new_map();
        let left = line(
            1,
            &[[0.0, 3.0, left_z[0]], [30.0, 3.0, left_z[1]]],
            &[("type", "line_thin"), ("subtype", "dashed")],
        );
        let right = line(
            2,
            &[[0.0, 0.0, right_z[0]], [30.0, 0.0, right_z[1]]],
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

    fn one_lanelet_map() -> std::sync::Arc<LaneletMap> {
        lanelet_map([0.0, 0.0], [0.0, 0.0])
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
            vec![
                [0.0, 3.0, 0.0],
                [30.0, 3.0, 0.0],
                [30.0, 0.0, 0.0],
                [0.0, 0.0, 0.0]
            ]
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
        let samples = sample_along(&[[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]], 25.0);
        assert_eq!(samples.len(), 1);
        assert!((samples[0].0[0] - 1.0).abs() < 1e-9);
    }

    #[test]
    fn sampling_a_degenerate_line_yields_nothing_rather_than_dividing_by_zero() {
        assert!(sample_along(&[[1.0, 1.0, 0.0], [1.0, 1.0, 0.0]], 5.0).is_empty());
        assert!(sample_along(&[[1.0, 1.0, 0.0]], 5.0).is_empty());
        // A purely vertical "road" is not a map, but it must not be a NaN either.
        assert!(sample_along(&[[0.0, 0.0, 0.0], [0.0, 0.0, 4.0]], 25.0).len() == 1);
    }

    /// A road that climbs is longer than its shadow, and the spacing is measured
    /// along the road — so it earns the arrow its length deserves.
    #[test]
    fn sampling_measures_the_slope_rather_than_its_shadow() {
        let flat = sample_along(&[[0.0, 0.0, 0.0], [30.0, 0.0, 0.0]], 20.0);
        let steep = sample_along(&[[0.0, 0.0, 0.0], [30.0, 0.0, 40.0]], 20.0);
        assert_eq!(flat.len(), 1);
        assert_eq!(steep.len(), 2, "50 m of road, not 30");
        assert_eq!(steep[0].1, [0.6, 0.0, 0.8], "a unit heading up the slope");
    }

    /// An arrow drawn flat on a ramp cuts through the road it belongs to the moment
    /// the camera tilts, so it is built in the surface instead.
    #[test]
    fn an_arrowhead_lies_in_the_slope_it_marks() {
        let size = 2.0;
        let triangle = arrowhead([0.0, 0.0, 10.0], [0.6, 0.0, 0.8], size);
        assert_eq!(triangle.len(), 3);
        // The tip is up the slope and the base is below it, all of it on the plane
        // through the sample with the heading's own gradient.
        assert!((triangle[0][2] - (10.0 + 0.8 * size * 0.6)).abs() < 1e-9);
        assert!((triangle[1][2] - (10.0 - 0.8 * size * 0.4)).abs() < 1e-9);
        assert_eq!(triangle[1][2], triangle[2][2], "the base is level across");
        // The base spreads horizontally: a Lanelet2 map says nothing about camber.
        assert!((triangle[1][1] - triangle[2][1]).abs() > 1.0);
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
        assert_eq!(scene.bounds.min, [0.0, 0.0, 0.0]);
        assert_eq!(scene.bounds.max, [30.0, 3.0, 0.0]);

        let empty = Scene::from_map(&LaneletMap::new_map(), &VizOptions::default());
        assert!(empty.is_empty());
        assert!(!empty.safe_bounds().is_empty());
        assert!(!empty.view_bounds(&View::three_quarter()).is_empty());
    }

    /// The whole point: a map on a hill is a hill in the scene. Nothing here has to
    /// be asked for it — the elevation rides along with every vertex, and a plan
    /// view is what discards it, not the builder.
    #[test]
    fn every_layer_carries_the_elevation_its_nodes_had() {
        let map = lanelet_map([4.0, 9.0], [4.0, 9.0]);
        let options = VizOptions {
            centerlines: true,
            points: true,
            ..VizOptions::default()
        };
        let scene = Scene::from_map(&map, &options);
        for layer in [
            VizLayer::LaneletFill,
            VizLayer::Bound,
            VizLayer::Centerline,
            VizLayer::Direction,
            VizLayer::Point,
        ] {
            let shape = scene
                .shapes
                .iter()
                .find(|shape| shape.layer == layer)
                .unwrap_or_else(|| panic!("no {layer:?} shape"));
            assert!(
                shape.points.iter().any(|point| point[2] > 3.9),
                "{layer:?} was flattened: {:?}",
                shape.points
            );
        }
        assert_eq!(scene.bounds.min[2], 4.0);
        assert_eq!(scene.bounds.max[2], 9.0);
        assert_eq!(scene.relief(), 5.0);
        // A flat map has no relief to show, and neither has an empty one.
        assert_eq!(Scene::from_map(&one_lanelet_map(), &options).relief(), 0.0);
        assert_eq!(
            Scene::from_map(&LaneletMap::new_map(), &options).relief(),
            0.0
        );
    }

    /// Seen from above, two shapes cannot hide each other and the style's z band is
    /// the whole story. Tilt the camera and the nearer of two shapes in one band
    /// has to be drawn second — which is what makes a bridge look like a bridge.
    #[test]
    fn a_tilted_view_draws_the_nearer_of_two_lanelets_last() {
        let map = LaneletMap::new_map();
        let attributes: AttributeMap = [("subtype".to_owned(), Attribute::new("road"))]
            .into_iter()
            .collect();
        // Two lanelets over the same ground, one four metres above the other.
        for (id, z) in [(3, 0.0), (6, 4.0)] {
            map.add(Primitive::Lanelet(Lanelet::new(
                id,
                line(id * 10, &[[0.0, 3.0, z], [30.0, 3.0, z]], &[]),
                line(id * 10 + 1, &[[0.0, 0.0, z], [30.0, 0.0, z]], &[]),
                attributes.clone(),
            )));
        }
        let scene = Scene::from_map(&map, &VizOptions::default());
        let fill_ids = |view: &View| -> Vec<Id> {
            scene
                .draw_order(view)
                .into_iter()
                .map(|index| &scene.shapes[index])
                .filter(|shape| shape.layer == VizLayer::LaneletFill)
                .map(|shape| shape.id)
                .collect()
        };
        // Looking down or from any angle, the deck is nearer, so it is painted over
        // the road it crosses rather than under it.
        assert_eq!(fill_ids(&View::three_quarter()).last(), Some(&6));
        assert_eq!(fill_ids(&View::oblique(210.0, 20.0, 1.0)).last(), Some(&6));

        // A plan view leaves the scene's own order alone: there is nothing to sort.
        assert_eq!(
            scene.draw_order(&View::plan()),
            (0..scene.shapes.len()).collect::<Vec<_>>()
        );
    }

    /// The SVG writer emits one `<g>` per layer by starting a new group whenever
    /// the layer changes along the draw order, which only produces one group per
    /// layer while every style in a layer shares one z band. That holds today by
    /// construction rather than by rule, and depth sorting inside a band is what
    /// would expose it: interleave two layers and the export silently grows a
    /// second `<g>` for each. So it is checked on a real map, tilted.
    #[test]
    fn a_layer_stays_in_one_run_of_the_draw_order() {
        let text = std::fs::read_to_string("../../tests/data/mapping_example.osm").unwrap();
        let loaded = crate::load_osm_str(&text, &crate::LoadOptions::default()).unwrap();
        let options = VizOptions {
            centerlines: true,
            points: true,
            ..VizOptions::default()
        };
        let scene = Scene::from_map(&loaded.map, &options);
        let mut seen: Vec<VizLayer> = Vec::new();
        let mut previous: Option<VizLayer> = None;
        for index in scene.draw_order(&View::three_quarter()) {
            let layer = scene.shapes[index].layer;
            if previous == Some(layer) {
                continue;
            }
            assert!(!seen.contains(&layer), "{layer:?} comes back after a gap");
            seen.push(layer);
            previous = Some(layer);
        }
        assert!(seen.len() > 4, "only saw {seen:?}");
    }

    /// Depth decides *within* a band, never across one. A lane marking belongs to
    /// the road it is painted on from every angle, and a viewer that let a distant
    /// road surface bury a nearby kerb would be unreadable.
    #[test]
    fn depth_never_reorders_one_layer_past_another() {
        let map = lanelet_map([0.0, 12.0], [0.0, 12.0]);
        let scene = Scene::from_map(&map, &VizOptions::default());
        let view = View::three_quarter();
        let bands: Vec<i32> = scene
            .draw_order(&view)
            .into_iter()
            .map(|index| scene.styles.get(scene.shapes[index].style).unwrap().z)
            .collect();
        assert!(bands.windows(2).all(|pair| pair[0] <= pair[1]), "{bands:?}");
    }

    #[test]
    fn the_page_is_fitted_to_what_the_camera_sees_not_to_the_map() {
        let map = one_lanelet_map();
        let scene = Scene::from_map(&map, &VizOptions::default());
        let plan = scene.view_bounds(&View::plan());
        assert_eq!(plan.min, [0.0, 0.0]);
        assert_eq!(plan.max, [30.0, 3.0]);
        // Tilted, the flat map is foreshortened along screen y and untouched along
        // screen x — so a page fitted to it is not fitted to the plan extent.
        let tilted = scene.view_bounds(&View::oblique(0.0, 30.0, 1.0));
        assert_eq!(tilted.max[0] - tilted.min[0], 30.0);
        assert!((tilted.max[1] - tilted.min[1] - 1.5).abs() < 1e-9);
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
