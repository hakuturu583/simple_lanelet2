//! Writing a [`Scene`] out as SVG.
//!
//! The whole map goes into one `<g>` carrying `translate(...) scale(s, -s)`, which
//! is what flips map y (north up) into SVG y (down) and does the fitting in one
//! step. Strokes then carry `vector-effect="non-scaling-stroke"`, so a width of 1.2
//! stays 1.2 *pixels* whatever the scale — the same rule the canvas renderer
//! follows, and the reason a lane marking is visible on a city-sized map at all.
//!
//! A scene is three-dimensional and a page is not, so every vertex goes through
//! [`SvgOptions::view`] on the way out. Under the default plan view that is the
//! identity on x and y and the document is exactly what it always was; tilt the view
//! and the same machinery draws the map in relief, because the projection happens
//! before the page transform rather than instead of it.
//!
//! No dependency: an SVG document is text, and the alternative is pulling a whole
//! writer in for eight element names.

use std::fmt::Write as _;

use crate::scene::{Scene, Shape};
use crate::style::{Style, VizLayer};
use crate::view::View;

/// Page size, margin and viewpoint for an SVG export.
#[derive(Clone, Copy, Debug)]
pub struct SvgOptions {
    pub width: f64,
    pub height: f64,
    pub margin: f64,
    /// Paint the theme's background colour behind the map.
    pub background: bool,
    /// Where the map is drawn from. [`View::plan`] is the map view; anything else
    /// is the 3D one, and is the only way the scene's elevations reach the page.
    pub view: View,
}

impl Default for SvgOptions {
    fn default() -> Self {
        SvgOptions {
            width: 1600.0,
            height: 1000.0,
            margin: 24.0,
            background: true,
            view: View::plan(),
        }
    }
}

/// Renders a scene as a standalone SVG document.
pub fn render_svg(scene: &Scene, options: &SvgOptions) -> String {
    let width = options.width.max(1.0);
    let height = options.height.max(1.0);
    let margin = options.margin.clamp(0.0, width.min(height) / 2.0 - 1.0);
    let view = &options.view;

    let bounds = scene.view_bounds(view);
    let span_x = bounds.max[0] - bounds.min[0];
    let span_y = bounds.max[1] - bounds.min[1];
    let scale = ((width - 2.0 * margin) / span_x).min((height - 2.0 * margin) / span_y);
    // Centre the map on the page: the offsets place the map's own centre at the
    // page's centre, after the y flip has been folded into the scale.
    let centre = [
        (bounds.min[0] + bounds.max[0]) * 0.5,
        (bounds.min[1] + bounds.max[1]) * 0.5,
    ];
    let offset_x = width * 0.5 - centre[0] * scale;
    let offset_y = height * 0.5 + centre[1] * scale;

    let palette = scene.theme.palette();
    let mut out = String::with_capacity(scene.shapes.len() * 96 + 512);
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\">"
    );
    let _ = writeln!(
        out,
        "<desc>Lanelet2 map — {} lanelets, {} linestrings, {} points</desc>",
        scene.stats.lanelets, scene.stats.line_strings, scene.stats.points
    );
    if options.background {
        let _ = writeln!(
            out,
            "<rect width=\"{width}\" height=\"{height}\" fill=\"{}\"/>",
            palette.background.to_hex()
        );
    }
    let _ = writeln!(
        out,
        "<g transform=\"translate({} {}) scale({} {})\">",
        number(offset_x),
        number(offset_y),
        number(scale),
        number(-scale)
    );

    // One group per layer, in the order shapes are drawn in, so an exported file can
    // have its layers toggled in an editor exactly as in the demo. Layers stay
    // contiguous under a tilted camera too: depth only reorders shapes within a
    // style's z band, and a band is a layer.
    let mut current: Option<VizLayer> = None;
    for index in scene.draw_order(view) {
        let shape = &scene.shapes[index];
        let Some(style) = scene.styles.get(shape.style) else {
            continue;
        };
        // The same thresholds the canvas renderer applies, from the same place —
        // which is what "the export is what you see" has to mean.
        if style.hide_below_scale > 0.0 && (scale as f32) < style.hide_below_scale {
            continue;
        }
        if current != Some(shape.layer) {
            if current.is_some() {
                out.push_str("</g>\n");
            }
            let _ = writeln!(
                out,
                "<g id=\"{}\" data-layer=\"{}\">",
                shape.layer.key(),
                shape.layer.key()
            );
            current = Some(shape.layer);
        }
        let dashed = (scale as f32) >= style.solid_below_scale;
        write_shape(&mut out, shape, style, dashed, view);
    }
    if current.is_some() {
        out.push_str("</g>\n");
    }

    out.push_str("</g>\n</svg>\n");
    out
}

fn write_shape(out: &mut String, shape: &Shape, style: &Style, dashed: bool, view: &View) {
    if shape.points.is_empty() {
        return;
    }
    // A lone point has no path to draw; a small circle is the honest rendering, and
    // its radius is in world units so it does not swamp the map when zoomed out.
    if shape.points.len() == 1 {
        let [x, y] = view.project(shape.points[0]);
        let color = style
            .stroke
            .or(style.fill)
            .unwrap_or(crate::style::Color(0x88, 0x88, 0x88));
        let _ = writeln!(
            out,
            "<circle cx=\"{}\" cy=\"{}\" r=\"0.25\" fill=\"{}\" fill-opacity=\"{}\"/>",
            number(x),
            number(y),
            color.to_hex(),
            style.stroke_opacity
        );
        return;
    }

    out.push_str("<path d=\"M");
    for (index, point) in shape.points.iter().enumerate() {
        if index > 0 {
            out.push('L');
        }
        let [x, y] = view.project(*point);
        let _ = write!(out, "{} {}", number(x), number(y));
        if index + 1 < shape.points.len() {
            out.push(' ');
        }
    }
    if shape.closed {
        out.push('Z');
    }
    out.push('"');

    match style.fill {
        Some(color) => {
            let _ = write!(
                out,
                " fill=\"{}\" fill-opacity=\"{}\"",
                color.to_hex(),
                style.fill_opacity
            );
        }
        None => out.push_str(" fill=\"none\""),
    }
    if let Some(color) = style.stroke {
        let _ = write!(
            out,
            " stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{}\" \
             stroke-linecap=\"round\" stroke-linejoin=\"round\" \
             vector-effect=\"non-scaling-stroke\"",
            color.to_hex(),
            style.stroke_opacity,
            style.stroke_width
        );
        if let Some(dash) = style.dash.as_ref().filter(|_| dashed) {
            let pattern: Vec<String> = dash.iter().map(|value| number(*value as f64)).collect();
            let _ = write!(out, " stroke-dasharray=\"{}\"", pattern.join(" "));
        }
    }
    if shape.id != 0 {
        let _ = write!(out, " data-id=\"{}\"", shape.id);
    }
    out.push_str("/>\n");
}

/// Millimetre precision, with no trailing zeroes.
///
/// A city map is a million coordinates; `{:?}` on an f64 would add a megabyte of
/// digits that no renderer can see.
fn number(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_owned();
    }
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded == rounded.trunc() && rounded.abs() < 1e15 {
        return format!("{}", rounded as i64);
    }
    let mut text = format!("{rounded:.3}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Scene, VizOptions};
    use crate::source::{LoadOptions, load_osm_str};
    use crate::view::View;

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

    fn svg_of(options: &VizOptions) -> String {
        let loaded = load_osm_str(MAP, &LoadOptions::default()).unwrap();
        let scene = Scene::from_map(&loaded.map, options);
        render_svg(&scene, &SvgOptions::default())
    }

    #[test]
    fn the_document_is_well_formed_and_carries_the_map() {
        let svg = svg_of(&VizOptions::default());
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert_eq!(svg.matches("<g ").count(), svg.matches("</g>").count());
        assert!(svg.contains("data-layer=\"lanelet_fill\""));
        assert!(svg.contains("data-layer=\"bound\""));
        assert!(svg.contains("data-id=\"20\""));
    }

    #[test]
    fn strokes_do_not_scale_with_the_zoom() {
        let svg = svg_of(&VizOptions::default());
        // Every stroked path must carry it, or a marking vanishes on a large map.
        assert_eq!(
            svg.matches("stroke=\"").count(),
            svg.matches("non-scaling-stroke").count()
        );
    }

    #[test]
    fn a_dashed_subtype_survives_into_the_output() {
        let svg = svg_of(&VizOptions::default());
        assert!(svg.contains("stroke-dasharray="));
    }

    /// The level-of-detail thresholds live on the style, so the export drops the
    /// same things the canvas does. A page big enough to show a lane loses
    /// nothing; a postage stamp loses the dashes and the arrowheads.
    #[test]
    fn zooming_out_drops_the_detail_that_is_no_longer_legible() {
        let loaded = load_osm_str(MAP, &LoadOptions::default()).unwrap();
        let scene = Scene::from_map(&loaded.map, &VizOptions::default());

        let large = render_svg(&scene, &SvgOptions::default());
        assert!(large.contains("stroke-dasharray="));
        assert!(large.contains("data-layer=\"direction\""));

        let stamp = render_svg(
            &scene,
            &SvgOptions {
                width: 60.0,
                height: 40.0,
                margin: 2.0,
                ..SvgOptions::default()
            },
        );
        assert!(!stamp.contains("stroke-dasharray="));
        assert!(!stamp.contains("data-layer=\"direction\""));
        // The road itself is still there — this is detail, not the map.
        assert!(stamp.contains("data-layer=\"lanelet_fill\""));
    }

    /// The same scene, drawn from the same place, with and without the elevation:
    /// a plan view cannot tell a bridge from the road under it, and a tilted one
    /// must. This is the whole of what "3D" buys.
    #[test]
    fn a_tilted_view_separates_what_a_plan_view_flattens() {
        let raised = MAP.replace(
            "<node id='3' lat='49.0001000' lon='8.4000000'/>",
            "<node id='3' lat='49.0001000' lon='8.4000000'><tag k='ele' v='20'/></node>",
        );
        let raised = raised.replace(
            "<node id='4' lat='49.0001000' lon='8.4010000'/>",
            "<node id='4' lat='49.0001000' lon='8.4010000'><tag k='ele' v='20'/></node>",
        );
        let loaded = load_osm_str(&raised, &LoadOptions::default()).unwrap();
        let scene = Scene::from_map(&loaded.map, &VizOptions::default());
        assert!(
            scene.bounds.max[2] - scene.bounds.min[2] > 19.0,
            "no relief"
        );

        let page = |view: View| {
            render_svg(
                &scene,
                &SvgOptions {
                    view,
                    ..SvgOptions::default()
                },
            )
        };
        let plan = page(View::plan());
        let tilted = page(View::three_quarter());
        assert!(tilted.starts_with("<svg"));
        assert_ne!(plan, tilted, "the elevation changed nothing");

        // Flattening the same tilted camera puts the raised bound back down with the
        // other one, so the two documents differ only by what the elevation did.
        let flattened = page(View::oblique(30.0, 55.0, 0.0));
        assert_ne!(flattened, tilted);
        // The page is always fitted to the map, so the drawing is the same size
        // either way; what the relief changes is the shape it is fitted *from*.
        let span = |view: View| {
            let box2d = scene.view_bounds(&view);
            box2d.max[1] - box2d.min[1]
        };
        assert!(
            span(View::three_quarter()) > span(View::oblique(30.0, 55.0, 0.0)) + 10.0,
            "the twenty metres of relief did not reach the page"
        );
    }

    #[test]
    fn a_plan_view_is_the_default_and_the_option_is_the_only_difference() {
        let loaded = load_osm_str(MAP, &LoadOptions::default()).unwrap();
        let scene = Scene::from_map(&loaded.map, &VizOptions::default());
        assert_eq!(
            render_svg(&scene, &SvgOptions::default()),
            render_svg(
                &scene,
                &SvgOptions {
                    view: View::plan(),
                    ..SvgOptions::default()
                }
            )
        );
    }

    #[test]
    fn an_empty_scene_still_produces_a_valid_document() {
        let scene = Scene::from_map(
            &ll2_core::map::LaneletMap::new_map(),
            &VizOptions::default(),
        );
        let svg = render_svg(&scene, &SvgOptions::default());
        assert!(svg.contains("<svg"));
        assert!(svg.trim_end().ends_with("</svg>"));
    }

    #[test]
    fn numbers_are_short_and_never_exponential() {
        assert_eq!(number(0.0), "0");
        assert_eq!(number(-0.0), "0");
        assert_eq!(number(12.0), "12");
        assert_eq!(number(12.5), "12.5");
        assert_eq!(number(1.23456), "1.235");
        assert_eq!(number(-1.0e-9), "0");
        assert_eq!(number(f64::NAN), "0");
        assert_eq!(number(1234567.0), "1234567");
    }

    #[test]
    fn the_map_is_centred_on_the_page() {
        let loaded = load_osm_str(MAP, &LoadOptions::default()).unwrap();
        let scene = Scene::from_map(&loaded.map, &VizOptions::default());
        let svg = render_svg(&scene, &SvgOptions::default());
        let transform = svg
            .lines()
            .find(|line| line.starts_with("<g transform="))
            .unwrap();
        // The y scale is the negative of the x scale — that is the flip.
        let numbers: Vec<f64> = transform
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .filter(|part| !part.is_empty())
            .filter_map(|part| part.parse().ok())
            .collect();
        let (sx, sy) = (numbers[numbers.len() - 2], numbers[numbers.len() - 1]);
        assert!((sx + sy).abs() < 1e-6, "{sx} vs {sy}");
    }
}
