//! Turning a `LaneletMap` into the eight batches a Spatial3D view draws.
//!
//! One entity per layer, not one per primitive. A city map has hundreds of thousands
//! of linestrings; logged individually that is hundreds of thousands of entities for
//! the viewer's blueprint tree to hold, and a tree nobody can scroll. Rerun batches
//! natively — a `LineStrips3D` carries a colour, a radius and a label *per strip* —
//! so a whole layer goes over as one archetype and every strip in it stays
//! individually selectable, hoverable and labelled.
//!
//! The layers are [`ll2_viz::VizLayer`]'s, and so is their order. That vocabulary is
//! already the one the SVG writer's `<g>` ids and the web demo's toggles use; making
//! it the entity path too means a map looks the same and is called the same thing
//! whichever of the three you are looking at.

use ll2_core::area::Area;
use ll2_core::geometry::lanelet::{centerline_3d, mean_width_2d};
use ll2_core::lanelet::Lanelet;
use ll2_core::linestring::LineString;
use ll2_core::map::{LaneletMap, as_area, as_lanelet, as_linestring, as_point};
use ll2_viz::style::{self, Color, Palette, Style};
use ll2_viz::{MapStats, VizLayer, VizOptions, attribute, describe, describe_lanelet};
use rerun::{
    Arrows3D, AsComponents, LineStrips3D, Mesh3D, Points3D, Radius, RecordingStream,
    RecordingStreamResult,
};

use crate::geometry::{self, Point3};

/// What to build, and how to space it out.
#[derive(Clone, Debug)]
pub struct MapOptions {
    /// Which layers to build and how far apart the direction arrows sit — the same
    /// knobs the SVG writer and the web demo take, so a map can be described once
    /// and drawn by any of them.
    pub viz: VizOptions,

    /// Metres of separation between one layer and the next, along z.
    ///
    /// A road surface and the lines painted on it are the same surface in a Lanelet2
    /// map: identical elevations, and therefore z-fighting the moment a depth buffer
    /// gets hold of them. Two centimetres per layer is below what anyone can see at
    /// map scale and far above what `f32` depth precision needs to separate them.
    /// Set it to zero for coordinates that must stay exactly as the file had them.
    pub layer_lift: f64,

    /// Multiplies every line width.
    ///
    /// Widths are in UI points rather than metres, for the reason the SVG writer
    /// gives: a lane marking is 12 cm wide, which is invisible at city scale, and a
    /// hairline is what every map viewer actually draws. Rerun keeps a UI-point
    /// radius the same size on screen at any zoom, which is the behaviour that makes
    /// a whole city legible and a single junction still look right.
    pub line_scale: f32,
}

impl Default for MapOptions {
    fn default() -> Self {
        MapOptions {
            viz: VizOptions::default(),
            layer_lift: 0.02,
            line_scale: 1.0,
        }
    }
}

/// How much geometry came out, for a caller that wants to say so.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayerCounts {
    pub triangles: usize,
    pub strips: usize,
    pub arrows: usize,
    pub points: usize,
}

/// What a map held and what was drawn from it — everything worth keeping once the
/// geometry itself has been handed to Rerun.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MapSummary {
    pub stats: MapStats,
    pub counts: LayerCounts,
}

/// A map as Rerun archetypes: one per layer, each `None` when its layer is switched
/// off or the map has nothing in it.
///
/// Public fields, because the whole point of stopping here rather than logging
/// straight through is that a caller can put these somewhere else — a different
/// entity path, a recording that already has a robot in it, a timeline where the map
/// changes.
pub struct MapLayers {
    pub lanelet_fill: Option<Mesh3D>,
    pub areas: Option<Mesh3D>,
    pub polygons: Option<Mesh3D>,
    pub bounds: Option<LineStrips3D>,
    pub regulatory: Option<LineStrips3D>,
    pub centerlines: Option<LineStrips3D>,
    pub direction: Option<Arrows3D>,
    pub points: Option<Points3D>,
    /// What the source map held.
    pub stats: MapStats,
    /// What was built from it.
    pub counts: LayerCounts,
}

impl MapLayers {
    /// Builds every enabled layer. Touches no recording and can fail at nothing.
    pub fn from_map(map: &LaneletMap, options: &MapOptions) -> MapLayers {
        let mut builder = Builder::new(options);
        builder.add_lanelets(map);
        builder.add_areas(map);
        builder.add_polygons(map);
        builder.add_linestrings(map);
        builder.add_points(map);
        builder.finish(MapStats::of(map))
    }

    /// Logs the layers under `root`, as static data.
    ///
    /// Static, because a map is not an observation: it has no time at which it
    /// became true, and logging it on a timeline would make it disappear whenever a
    /// recording is scrubbed back before the frame it arrived on.
    ///
    /// `root` also gets the view coordinates. Lanelet2 is x east, y north, z up, and
    /// saying so is what tells Rerun which way "up" is when it frames the map.
    pub fn log_to(&self, rec: &RecordingStream, root: &str) -> RecordingStreamResult<()> {
        rec.log_static(root.to_owned(), &rerun::ViewCoordinates::RIGHT_HAND_Z_UP())?;
        log_layer(rec, root, VizLayer::LaneletFill, self.lanelet_fill.as_ref())?;
        log_layer(rec, root, VizLayer::Area, self.areas.as_ref())?;
        log_layer(rec, root, VizLayer::Polygon, self.polygons.as_ref())?;
        log_layer(rec, root, VizLayer::Bound, self.bounds.as_ref())?;
        log_layer(rec, root, VizLayer::Regulatory, self.regulatory.as_ref())?;
        log_layer(rec, root, VizLayer::Centerline, self.centerlines.as_ref())?;
        log_layer(rec, root, VizLayer::Direction, self.direction.as_ref())?;
        log_layer(rec, root, VizLayer::Point, self.points.as_ref())?;
        Ok(())
    }

    /// [`log_to`](Self::log_to), plus the blueprint that makes a viewer open this as
    /// a map rather than as eight unrelated entities.
    pub fn log_with_blueprint(
        &self,
        rec: &RecordingStream,
        root: &str,
    ) -> RecordingStreamResult<()> {
        self.log_to(rec, root)?;
        crate::blueprint::spatial3d(root)
            .send(rec, rerun::blueprint::BlueprintActivation::default())
    }

    /// What the map held and what was drawn from it.
    pub fn summary(&self) -> MapSummary {
        MapSummary {
            stats: self.stats,
            counts: self.counts,
        }
    }

    /// Whether anything at all was built.
    pub fn is_empty(&self) -> bool {
        self.counts == LayerCounts::default()
    }
}

/// The entity path a layer is logged at: `<root>/<layer key>`.
pub fn entity_path(root: &str, layer: VizLayer) -> String {
    format!("{root}/{}", layer.key())
}

fn log_layer<A: AsComponents>(
    rec: &RecordingStream,
    root: &str,
    layer: VizLayer,
    archetype: Option<&A>,
) -> RecordingStreamResult<()> {
    match archetype {
        Some(archetype) => rec.log_static(entity_path(root, layer), archetype),
        None => Ok(()),
    }
}

// --- building ----------------------------------------------------------------

struct Builder<'a> {
    options: &'a MapOptions,
    palette: Palette,
    lanelet_fill: MeshParts,
    areas: MeshParts,
    polygons: MeshParts,
    bounds: StripParts,
    regulatory: StripParts,
    centerlines: StripParts,
    direction: ArrowParts,
    points: PointParts,
}

impl<'a> Builder<'a> {
    fn new(options: &'a MapOptions) -> Builder<'a> {
        Builder {
            options,
            palette: options.viz.theme.palette(),
            lanelet_fill: MeshParts::default(),
            areas: MeshParts::default(),
            polygons: MeshParts::default(),
            bounds: StripParts::default(),
            regulatory: StripParts::default(),
            centerlines: StripParts::default(),
            direction: ArrowParts::default(),
            points: PointParts::default(),
        }
    }

    /// How far above the ground a layer sits. The layer's own declaration order is
    /// the painter's order the 2D renderers sort by, so reusing it here puts the
    /// same things on top.
    fn lift(&self, layer: VizLayer) -> f64 {
        layer.index() as f64 * self.options.layer_lift
    }

    /// A stroke's colour, keeping its opacity: Rerun blends lines and points.
    fn stroke_color(&self, style: &Style) -> rerun::Color {
        rgba(
            style.stroke.unwrap_or(self.palette.text),
            style.stroke_opacity,
        )
    }

    /// A fill's colour, with its opacity already resolved against the theme
    /// background.
    ///
    /// A translucent fill is a 2D idea: it exists so a crosswalk reads as painted
    /// *on* the road under it. In 3D the layers are already held apart by
    /// `layer_lift`, and a translucent mesh needs the renderer to depth-sort
    /// overlapping road surfaces correctly — which is not something to depend on for
    /// a map that is mostly overlapping road surfaces. So the blend is done here,
    /// once, and the mesh goes over opaque and looking like the SVG does.
    fn fill_color(&self, style: &Style) -> rerun::Color {
        let color = style.fill.unwrap_or(self.palette.area_other);
        rgba(
            blend(color, self.palette.background, style.fill_opacity),
            1.0,
        )
    }

    fn radius(&self, style: &Style) -> Radius {
        // A style's width is a diameter, as every 2D stroke is.
        Radius::new_ui_points((style.stroke_width * 0.5 * self.options.line_scale).max(0.1))
    }

    fn add_lanelets(&mut self, map: &LaneletMap) {
        let wants_fill = self.options.viz.wants_layer(VizLayer::LaneletFill);
        let wants_centerline = self.options.viz.wants_layer(VizLayer::Centerline);
        let wants_arrows = self.options.viz.wants_layer(VizLayer::Direction);
        if !(wants_fill || wants_centerline || wants_arrows) {
            return;
        }

        let (fill_lift, centerline_lift) = (
            self.lift(VizLayer::LaneletFill),
            self.lift(VizLayer::Centerline),
        );
        let centerline_style = style::centerline_style(&self.palette);
        let (centerline_color, centerline_radius) = (
            self.stroke_color(&centerline_style),
            self.radius(&centerline_style),
        );
        if wants_arrows {
            self.direction.reserve(map.lanelets.len());
        }

        for primitive in map.lanelets.all() {
            let Some(lanelet) = as_lanelet(&primitive) else {
                continue;
            };
            let subtype = attribute(lanelet.attributes(), "subtype");

            if wants_fill {
                let left = points_3d(&lanelet.left_bound());
                let right = points_3d(&lanelet.right_bound());
                let (positions, triangles) = geometry::ribbon(&left, &right);
                let fill = style::lanelet_style(&subtype, &self.palette);
                self.lanelet_fill
                    .push(&positions, &triangles, self.fill_color(&fill), fill_lift);
            }

            if wants_centerline || wants_arrows {
                // One description, shared by the centerline and every arrow: whichever
                // the cursor lands on has to be able to answer "what lanelet is this?".
                // The surface cannot use it — a `Mesh3D` is one merged blob with no
                // per-lanelet label — so it is not built unless something wants it.
                let description = describe_lanelet(lanelet, &subtype);
                let centerline = centerline_3d(lanelet);
                if wants_centerline {
                    self.centerlines.push(
                        &centerline,
                        centerline_color,
                        centerline_radius,
                        format!("centerline · {description}"),
                        centerline_lift,
                    );
                }
                if wants_arrows {
                    self.add_arrows(lanelet, &centerline, &description);
                }
            }
        }
    }

    /// One lanelet's chevrons, sampled along its centerline and sized from its own
    /// width so a footpath does not get a motorway-sized arrow.
    fn add_arrows(&mut self, lanelet: &Lanelet, centerline: &[Point3], description: &str) {
        let size = (mean_width_2d(lanelet).clamp(1.0, 6.0) * 0.9).clamp(1.2, 5.0);
        let label = format!("{description} ▸");
        let lift = self.lift(VizLayer::Direction);
        for (position, heading) in
            geometry::sample_along(centerline, self.options.viz.arrow_spacing)
        {
            self.direction
                .push(position, heading, size, label.clone(), lift);
        }
    }

    fn add_areas(&mut self, map: &LaneletMap) {
        if !self.options.viz.wants_layer(VizLayer::Area) {
            return;
        }
        let lift = self.lift(VizLayer::Area);
        for primitive in map.areas.all() {
            let Some(area) = as_area(&primitive) else {
                continue;
            };
            let ring = area_ring(area);
            if ring.len() < 3 {
                continue;
            }
            let subtype = attribute(area.attributes(), "subtype");
            let style = style::area_style(&subtype, &self.palette);
            let triangles = geometry::triangulate_ring(&ring);
            self.areas
                .push(&ring, &triangles, self.fill_color(&style), lift);
        }
    }

    fn add_polygons(&mut self, map: &LaneletMap) {
        if !self.options.viz.wants_layer(VizLayer::Polygon) {
            return;
        }
        let lift = self.lift(VizLayer::Polygon);
        for primitive in map.polygons.all() {
            let Some(line) = as_linestring(&primitive) else {
                continue;
            };
            let ring = open_ring(points_3d(line));
            if ring.len() < 3 {
                continue;
            }
            let attributes = line.attributes();
            let kind = attribute(attributes, "type");
            let subtype = attribute(attributes, "subtype");
            let style = style::polygon_style(style::polygon_key(&kind, &subtype), &self.palette);
            let triangles = geometry::triangulate_ring(&ring);
            self.polygons
                .push(&ring, &triangles, self.fill_color(&style), lift);
        }
    }

    fn add_linestrings(&mut self, map: &LaneletMap) {
        let wants_bounds = self.options.viz.wants_layer(VizLayer::Bound);
        let wants_regulatory = self.options.viz.wants_layer(VizLayer::Regulatory);
        if !(wants_bounds || wants_regulatory) {
            return;
        }
        let (bound_lift, regulatory_lift) =
            (self.lift(VizLayer::Bound), self.lift(VizLayer::Regulatory));
        // Nearly every linestring in a map is a boundary; the regulatory ones are a
        // handful, so only the bulk is worth reserving for.
        if wants_bounds {
            self.bounds.reserve(map.line_strings.len());
        }

        for primitive in map.line_strings.all() {
            let Some(line) = as_linestring(&primitive) else {
                continue;
            };
            let attributes = line.attributes();
            let kind = attribute(attributes, "type");
            let subtype = attribute(attributes, "subtype");
            let layer = style::linestring_layer(&kind);
            if !self.options.viz.wants_layer(layer) {
                continue;
            }
            let points = points_3d(line);
            if points.len() < 2 {
                continue;
            }
            let style = style::linestring_style(&kind, &subtype, &self.palette);
            let label = describe(line.id(), "linestring", &kind, &subtype);
            let (color, radius) = (self.stroke_color(&style), self.radius(&style));
            let (strips, lift) = if layer == VizLayer::Regulatory {
                (&mut self.regulatory, regulatory_lift)
            } else {
                (&mut self.bounds, bound_lift)
            };
            strips.push(&points, color, radius, label, lift);
        }
    }

    fn add_points(&mut self, map: &LaneletMap) {
        if !self.options.viz.wants_layer(VizLayer::Point) {
            return;
        }
        let lift = self.lift(VizLayer::Point);
        self.points.reserve(map.points.len());
        for primitive in map.points.all() {
            let Some(point) = as_point(&primitive) else {
                continue;
            };
            self.points
                .push(point.xyz(), format!("point {}", point.id()), lift);
        }
    }

    fn finish(self, stats: MapStats) -> MapLayers {
        // Every arrow in the map is one colour and one thickness, and so is every
        // point; the look is decided here, once, rather than copied onto each of the
        // hundreds of thousands of them.
        let arrows = style::direction_style(&self.palette);
        let direction_color = rgba(
            arrows.fill.unwrap_or(self.palette.direction),
            // Not `fill_color`: an arrow is drawn over the road rather than being
            // part of it, so it keeps its opacity instead of being blended away.
            arrows.fill_opacity,
        );
        let direction_radius = self.radius(&arrows);
        let dots = style::point_style(&self.palette);
        let (point_color, point_radius) = (self.stroke_color(&dots), self.radius(&dots));

        let counts = LayerCounts {
            triangles: self.lanelet_fill.triangles.len()
                + self.areas.triangles.len()
                + self.polygons.triangles.len(),
            strips: self.bounds.strips.len()
                + self.regulatory.strips.len()
                + self.centerlines.strips.len(),
            arrows: self.direction.vectors.len(),
            points: self.points.positions.len(),
        };
        MapLayers {
            lanelet_fill: self.lanelet_fill.finish(),
            areas: self.areas.finish(),
            polygons: self.polygons.finish(),
            bounds: self.bounds.finish(),
            regulatory: self.regulatory.finish(),
            centerlines: self.centerlines.finish(),
            direction: self.direction.finish(direction_color, direction_radius),
            points: self.points.finish(point_color, point_radius),
            stats,
            counts,
        }
    }
}

// --- per-layer accumulators ---------------------------------------------------
//
// Each collects one layer's worth of geometry into the parallel arrays its
// archetype wants, and hands back `None` when nothing landed in it — an empty
// archetype is not the same as an absent one to Rerun, and logging one would put an
// empty entity in the blueprint tree for every layer a map does not use.

#[derive(Default)]
struct MeshParts {
    positions: Vec<[f32; 3]>,
    colors: Vec<rerun::Color>,
    triangles: Vec<[u32; 3]>,
}

impl MeshParts {
    fn push(
        &mut self,
        positions: &[Point3],
        triangles: &[[u32; 3]],
        color: rerun::Color,
        lift: f64,
    ) {
        if triangles.is_empty() {
            return;
        }
        let base = self.positions.len() as u32;
        for point in positions {
            self.positions.push(geometry::lifted(*point, lift));
            self.colors.push(color);
        }
        self.triangles.extend(
            triangles
                .iter()
                .map(|[a, b, c]| [base + a, base + b, base + c]),
        );
    }

    fn finish(self) -> Option<Mesh3D> {
        if self.triangles.is_empty() {
            return None;
        }
        Some(
            Mesh3D::new(self.positions)
                .with_vertex_colors(self.colors)
                .with_triangle_indices(self.triangles),
        )
    }
}

#[derive(Default)]
struct StripParts {
    strips: Vec<Vec<[f32; 3]>>,
    colors: Vec<rerun::Color>,
    radii: Vec<Radius>,
    labels: Vec<String>,
}

impl StripParts {
    fn reserve(&mut self, count: usize) {
        self.strips.reserve(count);
        self.colors.reserve(count);
        self.radii.reserve(count);
        self.labels.reserve(count);
    }

    fn push(
        &mut self,
        points: &[Point3],
        color: rerun::Color,
        radius: Radius,
        label: String,
        lift: f64,
    ) {
        if points.len() < 2 {
            return;
        }
        self.strips
            .push(points.iter().map(|p| geometry::lifted(*p, lift)).collect());
        self.colors.push(color);
        self.radii.push(radius);
        self.labels.push(label);
    }

    fn finish(self) -> Option<LineStrips3D> {
        if self.strips.is_empty() {
            return None;
        }
        Some(
            LineStrips3D::new(self.strips)
                .with_colors(self.colors)
                .with_radii(self.radii)
                .with_labels(self.labels)
                // Labels are for the cursor, not for the picture. Drawn, they would
                // put one caption per linestring over the map.
                .with_show_labels(false),
        )
    }
}

#[derive(Default)]
struct ArrowParts {
    origins: Vec<[f32; 3]>,
    vectors: Vec<[f32; 3]>,
    labels: Vec<String>,
}

impl ArrowParts {
    /// A lanelet yields at least one arrow, so its count is the floor.
    fn reserve(&mut self, lanelets: usize) {
        self.origins.reserve(lanelets);
        self.vectors.reserve(lanelets);
        self.labels.reserve(lanelets);
    }

    fn push(&mut self, position: Point3, heading: Point3, size: f64, label: String, lift: f64) {
        // The arrow is centred on the sample, so a chevron sits across the point the
        // sampler chose rather than starting at it.
        let half = [
            heading[0] * size * 0.5,
            heading[1] * size * 0.5,
            heading[2] * size * 0.5,
        ];
        self.origins.push(geometry::lifted(
            [
                position[0] - half[0],
                position[1] - half[1],
                position[2] - half[2],
            ],
            lift,
        ));
        // A vector is a displacement, not a position: it does not get the lift.
        self.vectors.push(geometry::to_f32([
            half[0] * 2.0,
            half[1] * 2.0,
            half[2] * 2.0,
        ]));
        self.labels.push(label);
    }

    /// The colour and the radius go over as one-element batches: Rerun splats those
    /// across the whole archetype, so they cost one component each rather than one
    /// per arrowhead — and there are a great many arrowheads.
    fn finish(self, color: rerun::Color, radius: Radius) -> Option<Arrows3D> {
        if self.vectors.is_empty() {
            return None;
        }
        Some(
            Arrows3D::from_vectors(self.vectors)
                .with_origins(self.origins)
                .with_colors([color])
                .with_radii([radius])
                .with_labels(self.labels)
                .with_show_labels(false),
        )
    }
}

#[derive(Default)]
struct PointParts {
    positions: Vec<[f32; 3]>,
    labels: Vec<String>,
}

impl PointParts {
    /// The one layer that reaches millions on a city map, and so the one where
    /// doubling a `Vec` up from empty is worth not doing.
    fn reserve(&mut self, count: usize) {
        self.positions.reserve(count);
        self.labels.reserve(count);
    }

    fn push(&mut self, position: Point3, label: String, lift: f64) {
        self.positions.push(geometry::lifted(position, lift));
        self.labels.push(label);
    }

    /// One colour and one radius for every point in the map — see
    /// [`ArrowParts::finish`], for the same reason and rather more of them.
    fn finish(self, color: rerun::Color, radius: Radius) -> Option<Points3D> {
        if self.positions.is_empty() {
            return None;
        }
        Some(
            Points3D::new(self.positions)
                .with_colors([color])
                .with_radii([radius])
                .with_labels(self.labels)
                .with_show_labels(false),
        )
    }
}

// --- small helpers ------------------------------------------------------------

fn points_3d(line: &LineString) -> Vec<Point3> {
    line.points()
        .iter()
        .map(ll2_core::point::Point::xyz)
        .collect()
}

/// A ring with its closing vertex removed, if it has one.
///
/// Ways in an OSM file repeat their first node to close themselves; a triangulator
/// treats that repeat as a zero-length edge and a vertex that can never be an ear.
fn open_ring(mut ring: Vec<Point3>) -> Vec<Point3> {
    if ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    ring
}

/// An area's outer boundary, walked into one ring.
fn area_ring(area: &Area) -> Vec<Point3> {
    let mut ring: Vec<Point3> = Vec::new();
    for line in area.outer_bound() {
        for point in points_3d(&line) {
            // Consecutive members of a ring share their end points.
            if ring.last() != Some(&point) {
                ring.push(point);
            }
        }
    }
    open_ring(ring)
}

/// A palette colour at an opacity, as Rerun wants it.
fn rgba(color: Color, opacity: f32) -> rerun::Color {
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    rerun::Color::from_unmultiplied_rgba(color.0, color.1, color.2, alpha)
}

/// `color` laid over `under` at `opacity`, in straight sRGB.
///
/// Not gamma-correct, deliberately: it has to match what a browser does to the same
/// two colours in the SVG and the canvas, and a browser composites in sRGB too.
fn blend(color: Color, under: Color, opacity: f32) -> Color {
    let opacity = opacity.clamp(0.0, 1.0);
    let mix = |over: u8, under: u8| {
        (over as f32 * opacity + under as f32 * (1.0 - opacity)).round() as u8
    };
    Color(
        mix(color.0, under.0),
        mix(color.1, under.1),
        mix(color.2, under.2),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ll2_core::attribute::{Attribute, AttributeMap};
    use ll2_core::id::Id;
    use ll2_core::map::Primitive;
    use ll2_core::point::Point;
    use ll2_viz::Theme;
    use std::sync::Arc;

    fn line(id: Id, points: &[[f64; 3]], tags: &[(&str, &str)]) -> LineString {
        let attributes: AttributeMap = tags
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Attribute::new(*value)))
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

    /// One 30 m lanelet climbing two metres, with a solid right bound and a dashed
    /// left one — enough to exercise every layer that is on by default.
    fn one_lanelet_map() -> Arc<LaneletMap> {
        let map = LaneletMap::new_map();
        let left = line(
            1,
            &[[0.0, 3.0, 0.0], [30.0, 3.0, 2.0]],
            &[("type", "line_thin"), ("subtype", "dashed")],
        );
        let right = line(
            2,
            &[[0.0, 0.0, 0.0], [15.0, 0.0, 1.0], [30.0, 0.0, 2.0]],
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
    fn the_default_layers_are_the_ones_that_come_out() {
        let layers = MapLayers::from_map(&one_lanelet_map(), &MapOptions::default());
        assert!(layers.lanelet_fill.is_some());
        assert!(layers.bounds.is_some());
        assert!(layers.direction.is_some());
        // Off by default, and absent rather than empty.
        assert!(layers.centerlines.is_none());
        assert!(layers.points.is_none());
        assert!(layers.areas.is_none());

        assert_eq!(layers.stats.lanelets, 1);
        assert_eq!(layers.stats.line_strings, 2);
        assert_eq!(layers.counts.strips, 2);
        assert_eq!(layers.counts.arrows, 1);
        assert!(layers.counts.triangles > 0);
        assert!(!layers.is_empty());
    }

    #[test]
    fn turning_a_layer_on_produces_it() {
        let options = MapOptions {
            viz: VizOptions {
                centerlines: true,
                points: true,
                ..VizOptions::default()
            },
            ..MapOptions::default()
        };
        let layers = MapLayers::from_map(&one_lanelet_map(), &options);
        assert!(layers.centerlines.is_some());
        assert!(layers.points.is_some());
        assert_eq!(layers.counts.points, 5);
    }

    #[test]
    fn turning_every_layer_off_leaves_nothing_to_log() {
        let options = MapOptions {
            viz: VizOptions {
                lanelet_fill: false,
                areas: false,
                polygons: false,
                bounds: false,
                regulatory: false,
                centerlines: false,
                direction_arrows: false,
                points: false,
                ..VizOptions::default()
            },
            ..MapOptions::default()
        };
        let layers = MapLayers::from_map(&one_lanelet_map(), &options);
        assert!(layers.is_empty());
        assert!(layers.lanelet_fill.is_none());
        assert!(layers.bounds.is_none());
    }

    #[test]
    fn an_empty_map_builds_nothing_rather_than_empty_archetypes() {
        let layers = MapLayers::from_map(&LaneletMap::new_map(), &MapOptions::default());
        assert!(layers.is_empty());
        assert!(layers.lanelet_fill.is_none());
    }

    #[test]
    fn layers_land_at_the_layer_key_under_the_root() {
        assert_eq!(entity_path("map", VizLayer::Bound), "map/bound");
        assert_eq!(
            entity_path("world/map", VizLayer::LaneletFill),
            "world/map/lanelet_fill"
        );
    }

    /// Boundaries have to sit above the surface they bound, or the depth buffer
    /// decides which of two coincident things wins and the road markings flicker.
    #[test]
    fn the_lift_stacks_the_layers_in_painter_order() {
        let options = MapOptions::default();
        let builder = Builder::new(&options);
        let fill = builder.lift(VizLayer::LaneletFill);
        let bound = builder.lift(VizLayer::Bound);
        let direction = builder.lift(VizLayer::Direction);
        assert!(fill < bound && bound < direction);
        assert!(direction < 0.25, "{direction} m is enough to see");
    }

    #[test]
    fn a_zero_lift_leaves_the_files_own_elevations_alone() {
        let options = MapOptions {
            layer_lift: 0.0,
            ..MapOptions::default()
        };
        let builder = Builder::new(&options);
        for layer in VizLayer::ALL {
            assert_eq!(builder.lift(layer), 0.0);
        }
    }

    #[test]
    fn a_fill_is_opaque_and_a_stroke_keeps_its_opacity() {
        let options = MapOptions::default();
        let builder = Builder::new(&options);
        let palette = Theme::Dark.palette();

        let road = builder.fill_color(&style::lanelet_style("road", &palette));
        assert_eq!(road.to_array()[3], 255);

        let virtual_line = builder.stroke_color(&style::linestring_style("virtual", "", &palette));
        assert!(virtual_line.to_array()[3] < 255);
    }

    #[test]
    fn blending_walks_from_the_background_to_the_colour() {
        let (over, under) = (Color(200, 100, 0), Color(0, 0, 100));
        assert_eq!(blend(over, under, 1.0), over);
        assert_eq!(blend(over, under, 0.0), under);
        assert_eq!(blend(over, under, 0.5), Color(100, 50, 50));
        // Out-of-range opacities are clamped rather than wrapped into a wrong colour.
        assert_eq!(blend(over, under, 2.0), over);
    }

    #[test]
    fn a_way_that_closes_itself_is_not_triangulated_with_a_repeated_vertex() {
        let square = vec![
            [0.0, 0.0, 0.0],
            [4.0, 0.0, 0.0],
            [4.0, 4.0, 0.0],
            [0.0, 4.0, 0.0],
            [0.0, 0.0, 0.0],
        ];
        assert_eq!(open_ring(square).len(), 4);
        assert_eq!(open_ring(vec![[0.0, 0.0, 0.0]]).len(), 1);
        assert!(open_ring(Vec::new()).is_empty());
    }

    #[test]
    fn a_polygon_way_becomes_a_mesh() {
        let map = LaneletMap::new_map();
        map.add(Primitive::Polygon(line(
            7,
            &[
                [0.0, 0.0, 0.0],
                [4.0, 0.0, 0.0],
                [4.0, 4.0, 0.0],
                [0.0, 4.0, 0.0],
                [0.0, 0.0, 0.0],
            ],
            &[("type", "crosswalk_polygon"), ("area", "yes")],
        )));
        let layers = MapLayers::from_map(&map, &MapOptions::default());
        assert!(layers.polygons.is_some(), "a closed way should fill");
        assert_eq!(layers.counts.triangles, 2);
    }
}
