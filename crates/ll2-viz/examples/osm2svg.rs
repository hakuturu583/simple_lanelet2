//! Renders a Lanelet2 `.osm` map to an SVG file.
//!
//! ```text
//! cargo run -p ll2-viz --example osm2svg -- map.osm map.svg [--light] [--centerlines]
//! cargo run -p ll2-viz --example osm2svg -- map.osm map.svg --3d [--yaw=30] [--pitch=55] [--exaggerate=3]
//! ```

use std::process::ExitCode;

use ll2_viz::{LoadOptions, Scene, SvgOptions, Theme, View, VizOptions, load_osm_str, render_svg};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let positional: Vec<&String> = arguments.iter().filter(|a| !a.starts_with("--")).collect();
    if positional.len() != 2 {
        eprintln!(
            "usage: osm2svg <map.osm> <out.svg> [--light] [--centerlines] [--points]\n\
             \x20                              [--3d [--yaw=D] [--pitch=D] [--exaggerate=N]]"
        );
        return ExitCode::FAILURE;
    }
    let has = |flag: &str| arguments.iter().any(|a| a == flag);
    // `--pitch=55`. A flag given a value that is not a number is a typo worth
    // failing on rather than a silent fall back to the default.
    let number = |name: &str, fallback: f64| -> Result<f64, String> {
        match arguments.iter().find_map(|a| a.strip_prefix(name)) {
            None => Ok(fallback),
            Some(value) => value.parse().map_err(|_| format!("{name}{value}")),
        }
    };

    // The defaults are the crate's own, so `--3d` here and a viewer's 3D button
    // start from the same camera.
    let default = View::three_quarter();
    let view = if has("--3d") {
        match (
            number("--yaw=", default.yaw()),
            number("--pitch=", default.pitch()),
            number("--exaggerate=", default.exaggeration()),
        ) {
            (Ok(yaw), Ok(pitch), Ok(exaggeration)) => View::oblique(yaw, pitch, exaggeration),
            (Err(bad), _, _) | (_, Err(bad), _) | (_, _, Err(bad)) => {
                eprintln!("not a number: {bad}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        View::plan()
    };

    let options = VizOptions {
        theme: if has("--light") {
            Theme::Light
        } else {
            Theme::Dark
        },
        centerlines: has("--centerlines"),
        points: has("--points"),
        ..VizOptions::default()
    };

    let text = match std::fs::read_to_string(positional[0]) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("{}: {error}", positional[0]);
            return ExitCode::FAILURE;
        }
    };

    let loaded = match load_osm_str(&text, &LoadOptions::default()) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    for problem in &loaded.errors {
        eprintln!("{problem}");
    }

    let scene = Scene::from_map(&loaded.map, &options);
    let relief = loaded.relief;
    eprintln!(
        "{} lanelets, {} linestrings, {} areas, {} points -> {} shapes ({} coordinates, {}, \
         {relief:.0} m of relief)",
        scene.stats.lanelets,
        scene.stats.line_strings,
        scene.stats.areas,
        scene.stats.points,
        scene.shapes.len(),
        loaded.coordinates.key(),
        loaded.projection,
    );
    // A tilted view of a map whose nodes have no `ele` is a tilted flat sheet, which
    // reads as a broken renderer rather than as a map that never had a third
    // dimension. Saying so costs one line and saves the question.
    if !view.is_plan() && !ll2_viz::worth_tilting(relief) {
        eprintln!(
            "note: every node in this map is at the same elevation — --3d will show a flat sheet"
        );
    }

    let page = SvgOptions {
        view,
        ..SvgOptions::default()
    };
    if let Err(error) = std::fs::write(positional[1], render_svg(&scene, &page)) {
        eprintln!("{}: {error}", positional[1]);
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
