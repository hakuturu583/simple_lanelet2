//! Renders a Lanelet2 `.osm` map to an SVG file.
//!
//! ```text
//! cargo run -p ll2-viz --example osm2svg -- map.osm map.svg [--light] [--centerlines]
//! ```

use std::process::ExitCode;

use ll2_viz::{LoadOptions, Scene, SvgOptions, Theme, VizOptions, load_osm_str, render_svg};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let positional: Vec<&String> = arguments.iter().filter(|a| !a.starts_with("--")).collect();
    if positional.len() != 2 {
        eprintln!("usage: osm2svg <map.osm> <out.svg> [--light] [--centerlines] [--points]");
        return ExitCode::FAILURE;
    }
    let has = |flag: &str| arguments.iter().any(|a| a == flag);

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
    eprintln!(
        "{} lanelets, {} linestrings, {} areas, {} points -> {} shapes ({} coordinates, {})",
        scene.stats.lanelets,
        scene.stats.line_strings,
        scene.stats.areas,
        scene.stats.points,
        scene.shapes.len(),
        loaded.coordinates.key(),
        loaded.projection,
    );

    if let Err(error) = std::fs::write(positional[1], render_svg(&scene, &SvgOptions::default())) {
        eprintln!("{}: {error}", positional[1]);
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
