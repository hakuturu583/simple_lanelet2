//! Lanelet2 maps in [Rerun](https://rerun.io), in a Spatial3D view.
//!
//! [`ll2_viz`] draws a map flat, because that is what an SVG page and a `<canvas>`
//! can hold. A Lanelet2 map is not flat — every node carries an `ele` — and neither
//! is anything you would want to look at next to one: a lidar sweep, a vehicle pose,
//! a planned trajectory. This crate is the other end of that: the map as Rerun
//! archetypes, logged in metres, in the same 3D space as whatever else the recording
//! holds.
//!
//! Like the rest of the visualisation side, none of it is part of this project's
//! drop-in compatibility claim. Upstream has no Rerun integration to be
//! byte-identical to, there is no Python binding, and nothing in the wheel imports
//! it.
//!
//! The shortest path from a file to a viewer:
//!
//! ```no_run
//! let text = std::fs::read_to_string("map.osm").unwrap();
//! ll2_rerun::spawn_osm(&text, &ll2_rerun::MapOptions::default()).unwrap();
//! ```
//!
//! or to a file you can open later, mail, or attach to a bug report:
//!
//! ```no_run
//! let text = std::fs::read_to_string("map.osm").unwrap();
//! ll2_rerun::save_osm(&text, "map.rrd", &ll2_rerun::MapOptions::default()).unwrap();
//! ```
//!
//! and the same thing with the steps visible, which is what you want as soon as the
//! map is one thing in a recording rather than the whole of it:
//!
//! ```no_run
//! use ll2_rerun::{LoadOptions, MapLayers, MapOptions, load_osm_str};
//!
//! let text = std::fs::read_to_string("map.osm").unwrap();
//! let loaded = load_osm_str(&text, &LoadOptions::default()).unwrap();
//! for problem in &loaded.errors {
//!     eprintln!("{problem}");
//! }
//!
//! let rec = rerun::RecordingStreamBuilder::new("my_stack").spawn().unwrap();
//! let layers = MapLayers::from_map(&loaded.map, &MapOptions::default());
//! layers.log_to(&rec, "world/map").unwrap();
//! // ... and now log the ego pose, the sensors and the trajectory alongside it.
//! ```
//!
//! That last form deliberately does not send a blueprint. [`log_map`] and
//! [`log_map_at`] do, because a recording that holds only a map wants a view built
//! for it; a recording that holds a map *and* a robot has a layout of its own, and
//! overwriting it would be the wrong kind of helpful.
//!
//! # What the map looks like
//!
//! Eight entities under one root, named by [`VizLayer`] — the same eight layers, in
//! the same painter's order, as the SVG writer's groups and the web demo's toggles.
//! Colours come from the same tag-to-style table, so a `line_thin`/`dashed` boundary
//! is the same hairline here as it is there, and a `stop_line` is the same red bar.
//!
//! Two things do not survive the crossing, both because 3D has no use for them:
//! dash patterns, which Rerun's line renderer has no equivalent for, and painter's
//! z-order, which becomes real separation along z instead — see
//! [`MapOptions::layer_lift`].
//!
//! Re-exported here so a caller needs one crate rather than three: the loader
//! ([`load_osm_str`], [`LoadOptions`], [`LoadedMap`], [`CoordinateSource`]) and the
//! layer and theme vocabulary ([`VizLayer`], [`VizOptions`], [`Theme`]).

pub mod blueprint;
pub mod geometry;
pub mod map;

pub use ll2_viz::{
    CoordinateSource, LoadOptions, LoadedMap, MapStats, Theme, VizLayer, VizOptions, load_osm_str,
};
pub use map::{LayerCounts, MapLayers, MapOptions, entity_path};

use std::path::PathBuf;

use ll2_core::map::LaneletMap;
use rerun::{RecordingStream, RecordingStreamBuilder};

/// Where a map is logged unless a caller says otherwise.
pub const ROOT: &str = "map";

/// The application id the one-call helpers record under.
///
/// Rerun keys a viewer's saved layout to this, so a fixed value is what makes the
/// blueprint you arranged last time still be there next time.
pub const APPLICATION_ID: &str = "lanelet2";

/// Loading a map failed, or logging one did.
#[derive(Debug)]
pub enum Error {
    /// The `.osm` file could not be turned into a drawable map. Carries the same
    /// message [`load_osm_str`] would have returned.
    Load(String),
    /// Rerun refused the recording — no viewer to connect to, an unwritable path.
    Rerun(rerun::RecordingStreamError),
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Load(message) => formatter.write_str(message),
            Error::Rerun(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Load(_) => None,
            Error::Rerun(error) => Some(error),
        }
    }
}

impl From<rerun::RecordingStreamError> for Error {
    fn from(error: rerun::RecordingStreamError) -> Error {
        Error::Rerun(error)
    }
}

/// Logs a map at [`ROOT`], with a Spatial3D view to look at it through.
pub fn log_map(
    rec: &RecordingStream,
    map: &LaneletMap,
    options: &MapOptions,
) -> Result<MapLayers, Error> {
    log_map_at(rec, ROOT, map, options)
}

/// [`log_map`], at an entity path of your choosing.
///
/// Sends a blueprint as well as the geometry, and so takes over the viewer's layout.
/// To put a map into a recording that already has one, build the layers yourself and
/// call [`MapLayers::log_to`].
pub fn log_map_at(
    rec: &RecordingStream,
    root: &str,
    map: &LaneletMap,
    options: &MapOptions,
) -> Result<MapLayers, Error> {
    let layers = MapLayers::from_map(map, options);
    layers.log_to(rec, root)?;
    blueprint::spatial3d(root).send(rec, rerun::blueprint::BlueprintActivation::default())?;
    Ok(layers)
}

/// Reads OSM XML and writes it to an `.rrd` file, ready to open in the viewer.
///
/// Everything it decides about the file — the origin, the projection, whether the
/// `local_x`/`local_y` tags are the real coordinates — is [`load_osm_str`]'s doing
/// and is described there. Non-fatal parse errors are swallowed; load the map
/// yourself when you need to see them.
pub fn save_osm(
    text: &str,
    path: impl Into<PathBuf>,
    options: &MapOptions,
) -> Result<MapLayers, Error> {
    let loaded = load_osm_str(text, &LoadOptions::default()).map_err(Error::Load)?;
    let rec = RecordingStreamBuilder::new(APPLICATION_ID).save(path)?;
    log_map(&rec, &loaded.map, options)
}

/// [`save_osm`], into a viewer it starts for you.
///
/// Needs the `rerun` binary on `PATH` — `pip install rerun-sdk` or `cargo install
/// rerun-cli` puts one there. An already-running viewer is reused rather than
/// duplicated.
pub fn spawn_osm(text: &str, options: &MapOptions) -> Result<MapLayers, Error> {
    let loaded = load_osm_str(text, &LoadOptions::default()).map_err(Error::Load)?;
    let rec = RecordingStreamBuilder::new(APPLICATION_ID).spawn()?;
    log_map(&rec, &loaded.map, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example_map() -> String {
        std::fs::read_to_string("../../tests/data/mapping_example.osm").unwrap()
    }

    /// The whole pipeline, into an in-memory sink: a real map, every default layer,
    /// the blueprint, and no viewer or filesystem anywhere.
    #[test]
    fn the_example_map_logs_into_a_recording() {
        let (rec, storage) = RecordingStreamBuilder::new("ll2_rerun_test")
            .memory()
            .unwrap();
        let loaded = load_osm_str(&example_map(), &LoadOptions::default()).unwrap();
        let layers = log_map(&rec, &loaded.map, &MapOptions::default()).unwrap();

        assert!(layers.stats.lanelets > 50);
        assert!(layers.counts.triangles > 100);
        assert!(layers.counts.strips > 100);
        assert!(layers.counts.arrows > 50);

        rec.flush_blocking().unwrap();
        assert!(
            !storage.take().is_empty(),
            "the recording came out with no messages in it"
        );
    }

    #[test]
    fn a_file_that_is_not_a_map_is_an_error_rather_than_an_empty_recording() {
        // `MapLayers` holds Rerun archetypes and is not `Debug`, so the error has to
        // be matched out rather than unwrapped.
        let Err(error) = save_osm("hello", "/dev/null", &MapOptions::default()) else {
            panic!("a file that is not a map produced a recording");
        };
        assert!(matches!(error, Error::Load(_)), "{error}");
        assert!(!error.to_string().is_empty());
    }

    #[test]
    fn the_map_can_be_logged_somewhere_other_than_the_default_root() {
        let (rec, _storage) = RecordingStreamBuilder::new("ll2_rerun_test")
            .memory()
            .unwrap();
        let loaded = load_osm_str(&example_map(), &LoadOptions::default()).unwrap();
        let layers = MapLayers::from_map(&loaded.map, &MapOptions::default());
        layers.log_to(&rec, "world/map").unwrap();
        rec.flush_blocking().unwrap();
    }
}
