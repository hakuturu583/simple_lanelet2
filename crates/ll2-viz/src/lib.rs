//! Visualising Lanelet2 maps.
//!
//! Lanelet2 describes a road network; it says nothing about how to draw one. This
//! crate is that missing half — and because upstream has no viewer, none of it is
//! part of this project's drop-in compatibility claim. There is no counterpart to
//! be byte-identical to, no Python binding, and nothing in the wheel imports it.
//!
//! Four pieces that stack:
//!
//! * [`source`] reads a `.osm` file — from a string, so it works with no filesystem
//!   — and works out for itself how to turn its nodes into metres.
//! * [`scene`] turns the resulting `LaneletMap` into a [`Scene`]: a list of styled
//!   polylines and polygons in map coordinates, and nothing else. Those coordinates
//!   are three-dimensional, because Lanelet2 nodes carry an elevation.
//! * [`view`] says where the map is looked at from, and is what turns those three
//!   coordinates into the two a page has room for. The default is the map view;
//!   [`View::oblique`] is the 3D one, and is where a map's relief becomes visible.
//!   [`polyline`] holds the small amount of 3D arithmetic they share.
//! * [`svg`] draws a scene. So does the `<canvas>` in this repository's web demo,
//!   from the same scene across a WebAssembly boundary — which is the point of the
//!   scene being renderer-agnostic rather than an SVG builder.
//!
//! The shortest path from a file to a picture:
//!
//! ```no_run
//! let text = std::fs::read_to_string("map.osm").unwrap();
//! let svg = ll2_viz::svg_from_osm(&text, &ll2_viz::VizOptions::default()).unwrap();
//! std::fs::write("map.svg", svg).unwrap();
//! ```
//!
//! and the same thing with the steps visible, which is what you want as soon as you
//! care about the errors, the statistics, or drawing the scene yourself:
//!
//! ```no_run
//! use ll2_viz::{LoadOptions, Scene, SvgOptions, VizOptions, load_osm_str, render_svg};
//!
//! let text = std::fs::read_to_string("map.osm").unwrap();
//! let loaded = load_osm_str(&text, &LoadOptions::default()).unwrap();
//! for problem in &loaded.errors {
//!     eprintln!("{problem}");
//! }
//! let scene = Scene::from_map(&loaded.map, &VizOptions::default());
//! println!("{} lanelets, {} shapes", scene.stats.lanelets, scene.shapes.len());
//! let svg = render_svg(&scene, &SvgOptions::default());
//! ```
//!
//! The same scene, drawn in three dimensions — one field, because the elevation was
//! in the scene all along and only the viewpoint was ever missing:
//!
//! ```no_run
//! use ll2_viz::{LoadOptions, Scene, SvgOptions, View, VizOptions, load_osm_str, render_svg};
//!
//! let text = std::fs::read_to_string("map.osm").unwrap();
//! let loaded = load_osm_str(&text, &LoadOptions::default()).unwrap();
//! let scene = Scene::from_map(&loaded.map, &VizOptions::default());
//! let svg = render_svg(
//!     &scene,
//!     &SvgOptions {
//!         view: View::three_quarter(),
//!         ..SvgOptions::default()
//!     },
//! );
//! ```

pub mod polyline;
pub mod scene;
pub mod source;
pub mod style;
pub mod svg;
pub mod view;

pub use polyline::{Point3, sample_along};
pub use scene::{MapStats, Scene, Shape, VizOptions, attribute, describe, describe_lanelet};
pub use source::{CoordinateSource, LoadOptions, LoadedMap, load_osm_str};
pub use style::{Color, Palette, Style, StyleTable, Theme, VizLayer};
pub use svg::{SvgOptions, render_svg};
pub use view::{View, worth_tilting};

/// Reads OSM XML and renders it as an SVG document, at the default page size.
///
/// The one-call form. Everything it decides — the origin, the projection, whether
/// the file's `local_x`/`local_y` tags are the real coordinates — is described on
/// [`source::load_osm_str`], and non-fatal parse errors are swallowed here; use the
/// three-step form when you need to see them.
pub fn svg_from_osm(text: &str, options: &VizOptions) -> Result<String, String> {
    svg_from_osm_with(
        text,
        options,
        &LoadOptions::default(),
        &SvgOptions::default(),
    )
}

/// [`svg_from_osm`], with the loading and page options exposed.
pub fn svg_from_osm_with(
    text: &str,
    options: &VizOptions,
    load: &LoadOptions,
    page: &SvgOptions,
) -> Result<String, String> {
    let loaded = load_osm_str(text, load)?;
    let scene = Scene::from_map(&loaded.map, options);
    Ok(render_svg(&scene, page))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_one_call_form_draws_the_example_map() {
        let text = std::fs::read_to_string("../../tests/data/mapping_example.osm").unwrap();
        let svg = svg_from_osm(&text, &VizOptions::default()).unwrap();
        assert!(svg.starts_with("<svg"));
        assert!(
            svg.len() > 10_000,
            "suspiciously small: {} bytes",
            svg.len()
        );
    }

    #[test]
    fn a_file_that_is_not_a_map_is_an_error_rather_than_a_blank_page() {
        assert!(svg_from_osm("hello", &VizOptions::default()).is_err());
    }
}
