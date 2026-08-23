//! Renders a Lanelet2 `.osm` map into a Rerun recording.
//!
//! ```text
//! cargo run -p ll2-rerun --example osm2rrd -- map.osm map.rrd [--light] [--centerlines] [--points]
//! cargo run -p ll2-rerun --example osm2rrd -- map.osm --spawn
//! ```

use std::process::ExitCode;

use ll2_rerun::{LoadOptions, MapLayers, MapOptions, Theme, VizOptions, load_osm_str};
use rerun::RecordingStreamBuilder;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let positional: Vec<&String> = arguments.iter().filter(|a| !a.starts_with("--")).collect();
    let has = |flag: &str| arguments.iter().any(|a| a == flag);
    let spawn = has("--spawn");

    // `--spawn` streams to a viewer, so it takes no output path; without it there
    // has to be one.
    if positional.len() != if spawn { 1 } else { 2 } {
        eprintln!("usage: osm2rrd <map.osm> <out.rrd> [--light] [--centerlines] [--points]");
        eprintln!("       osm2rrd <map.osm> --spawn [--light] [--centerlines] [--points]");
        return ExitCode::FAILURE;
    }

    let options = MapOptions {
        viz: VizOptions {
            theme: if has("--light") {
                Theme::Light
            } else {
                Theme::Dark
            },
            centerlines: has("--centerlines"),
            points: has("--points"),
            ..VizOptions::default()
        },
        ..MapOptions::default()
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

    // Built before the recording is opened, so a map that turns out to be empty is
    // reported instead of spawning a viewer to show nothing in.
    let layers = MapLayers::from_map(&loaded.map, &options);
    if layers.is_empty() {
        eprintln!("nothing to draw: every layer came out empty");
        return ExitCode::FAILURE;
    }

    let recording = if spawn {
        RecordingStreamBuilder::new(ll2_rerun::APPLICATION_ID).spawn()
    } else {
        RecordingStreamBuilder::new(ll2_rerun::APPLICATION_ID).save(positional[1])
    };
    let recording = match recording {
        Ok(recording) => recording,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };

    // `log_map` would do both of these, but it would also rebuild the layers that
    // have already been built above to check that there were any.
    let logged = layers.log_to(&recording, ll2_rerun::ROOT).and_then(|()| {
        ll2_rerun::blueprint::spatial3d(ll2_rerun::ROOT)
            .send(&recording, rerun::blueprint::BlueprintActivation::default())
    });
    if let Err(error) = logged {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }
    // A recording flushes on drop, which is too late to say anything about: the
    // process is on its way out and a half-written `.rrd` would look like a
    // successful run.
    if let Err(error) = recording.flush_blocking() {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }

    eprintln!(
        "{} lanelets, {} linestrings, {} areas, {} points -> \
         {} triangles, {} lines, {} arrows ({} coordinates, {})",
        layers.stats.lanelets,
        layers.stats.line_strings,
        layers.stats.areas,
        layers.stats.points,
        layers.counts.triangles,
        layers.counts.strips,
        layers.counts.arrows,
        loaded.coordinates.key(),
        loaded.projection,
    );
    ExitCode::SUCCESS
}
